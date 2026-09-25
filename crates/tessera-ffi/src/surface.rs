//! IOSurface pixel writer for the develop viewport (docs/11 §1.2).
//!
//! Swift allocates the surfaces and presents them through its `CAMetalLayer`;
//! Rust writes through a GPU-imported texture or the CPU fallback mapping.
//! Two presentation contracts, chosen by the host per ring (M2-22):
//!
//! - **SDR: RGBA8, display-encoded sRGB, straight alpha = 255**, 4 bytes per
//!   element (`kCVPixelFormatType_32RGBA`, `'RGBA'`). Metal imports it as
//!   `.rgba8Unorm_srgb`, so sampling yields linear sRGB and the layer's
//!   colour space (`extendedLinearSRGB`) lets Core Animation convert to the
//!   display. This is the Output stage's 8-bit rendition, byte for byte.
//! - **EDR: RGBA16F, display-linear extended sRGB, alpha = 1.0**, 8 bytes
//!   per element (`kCVPixelFormatType_64RGBAHalf`, `'RGhA'`). Values run
//!   `0..=headroom`, where 1.0 is SDR white and `headroom` is the EDR
//!   multiple the frame was tone-mapped for
//!   ([`pipeline_cpu::display_linear`]); Metal imports it as `.rgba16Float`
//!   for an `extendedLinearSRGB` layer with EDR enabled. Hosts allocate it
//!   only on an EDR-capable screen with the recipe's HDR toggle on; SDR
//!   screens keep the RGBA8 ring, so their path is unchanged.
//!
//! Surfaces keep the sensor orientation; the Metal presenter applies the EXIF
//! orientation when sampling, so writes stay row-contiguous.

use engine_api::tile::{TILE_SIZE, Tile};
use std::ffi::c_void;

/// `'RGBA'`.
pub const PIXEL_FORMAT_RGBA8: u32 = u32::from_be_bytes(*b"RGBA");
/// `'RGhA'` (`kCVPixelFormatType_64RGBAHalf`): the EDR viewport contract.
pub const PIXEL_FORMAT_RGBA16F: u32 = u32::from_be_bytes(*b"RGhA");

/// Pixel contract of a looked-up surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceKind {
    /// SDR viewport: RGBA8 display-encoded sRGB.
    Rgba8,
    /// EDR viewport: RGBA16F display-linear extended sRGB.
    Rgba16Float,
    /// Mask overlay alpha plane.
    R8,
}
/// `'L008'` (`kCVPixelFormatType_OneComponent8`): the mask overlay's alpha
/// plane, one byte per pixel, imported by Metal as `.r8Unorm`.
pub const PIXEL_FORMAT_R8: u32 = u32::from_be_bytes(*b"L008");

type IOSurfaceRef = *mut c_void;

#[cfg(target_os = "macos")]
#[link(name = "IOSurface", kind = "framework")]
unsafe extern "C" {
    fn IOSurfaceLookup(csid: u32) -> IOSurfaceRef;
    fn IOSurfaceLock(buffer: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceUnlock(buffer: IOSurfaceRef, options: u32, seed: *mut u32) -> i32;
    fn IOSurfaceGetBaseAddress(buffer: IOSurfaceRef) -> *mut c_void;
    fn IOSurfaceGetBytesPerRow(buffer: IOSurfaceRef) -> usize;
    fn IOSurfaceGetBytesPerElement(buffer: IOSurfaceRef) -> usize;
    fn IOSurfaceGetPixelFormat(buffer: IOSurfaceRef) -> u32;
    fn IOSurfaceGetWidth(buffer: IOSurfaceRef) -> usize;
    fn IOSurfaceGetHeight(buffer: IOSurfaceRef) -> usize;
    fn IOSurfaceGetID(buffer: IOSurfaceRef) -> u32;
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(cf: *const c_void);
}

/// A retained IOSurface the engine may write. Dropping releases it.
pub struct Surface {
    raw: IOSurfaceRef,
    id: u32,
    width: u32,
    height: u32,
    kind: SurfaceKind,
}

// SAFETY: IOSurfaceRef is a thread-safe CoreFoundation object; pixel access is
// bracketed by IOSurfaceLock/Unlock on the writing thread.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Creates an owned, temporary presentation target for backend calibration.
    #[cfg(target_os = "macos")]
    pub(crate) fn create_rgba8(width: u32, height: u32) -> Result<Self, String> {
        allocation::create(width, height, 4, PIXEL_FORMAT_RGBA8, SurfaceKind::Rgba8)
    }

    #[cfg(not(target_os = "macos"))]
    pub(crate) fn create_rgba8(_width: u32, _height: u32) -> Result<Self, String> {
        Err("IOSurface requires macOS".into())
    }

    /// Looks up a surface created in this process (or a global one) by id and
    /// checks it matches the RGBA8 contract and the expected size.
    #[cfg(target_os = "macos")]
    pub fn lookup(id: u32, width: u32, height: u32) -> Result<Self, String> {
        Self::lookup_bytes(id, width, height, 4)
    }

    /// A viewport surface of either contract: RGBA8 (SDR) or `'RGhA'`
    /// RGBA16F (EDR), told apart by pixel format and element size.
    #[cfg(target_os = "macos")]
    pub fn lookup_presentation(id: u32, width: u32, height: u32) -> Result<Self, String> {
        Self::lookup_bytes(id, width, height, 4).or_else(|e| {
            Self::lookup_bytes(id, width, height, 8)
                .map_err(|_| format!("{e}, or RGBA16F ('RGhA', 8 bytes per element) for EDR"))
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn lookup_presentation(_id: u32, _width: u32, _height: u32) -> Result<Self, String> {
        Err("IOSurface requires macOS".into())
    }

    /// [`Surface::lookup`] for a one-byte-per-pixel (R8) mask overlay surface.
    #[cfg(target_os = "macos")]
    pub fn lookup_r8(id: u32, width: u32, height: u32) -> Result<Self, String> {
        Self::lookup_bytes(id, width, height, 1)
    }

    #[cfg(not(target_os = "macos"))]
    pub fn lookup_r8(_id: u32, _width: u32, _height: u32) -> Result<Self, String> {
        Err("IOSurface requires macOS".into())
    }

    #[cfg(target_os = "macos")]
    fn lookup_bytes(id: u32, width: u32, height: u32, bytes: usize) -> Result<Self, String> {
        // SAFETY: plain CF calls; a null return is handled, the reference is
        // released in Drop.
        unsafe {
            let raw = IOSurfaceLookup(id);
            if raw.is_null() {
                return Err(format!("IOSurface {id} not found"));
            }
            let surface = Self {
                raw,
                id: IOSurfaceGetID(raw),
                width: IOSurfaceGetWidth(raw) as u32,
                height: IOSurfaceGetHeight(raw) as u32,
                kind: match bytes {
                    1 => SurfaceKind::R8,
                    8 => SurfaceKind::Rgba16Float,
                    _ => SurfaceKind::Rgba8,
                },
            };
            if IOSurfaceGetBytesPerElement(raw) != bytes
                || (bytes == 8 && IOSurfaceGetPixelFormat(raw) != PIXEL_FORMAT_RGBA16F)
            {
                return Err(format!(
                    "IOSurface must have {bytes} byte(s) per element{}",
                    match bytes {
                        4 => " (RGBA8)",
                        8 => " ('RGhA' RGBA16F)",
                        _ => " (R8)",
                    }
                ));
            }
            if surface.width != width || surface.height != height {
                return Err(format!(
                    "IOSurface {id} is {}x{}, expected {width}x{height}",
                    surface.width, surface.height
                ));
            }
            Ok(surface)
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn lookup(_id: u32, _width: u32, _height: u32) -> Result<Self, String> {
        Err("IOSurface requires macOS".into())
    }

    pub fn id(&self) -> u32 {
        self.id
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn kind(&self) -> SurfaceKind {
        self.kind
    }
    /// The EDR (RGBA16F) contract.
    pub fn is_float(&self) -> bool {
        self.kind == SurfaceKind::Rgba16Float
    }

    /// Runs `f` with the locked pixel memory and its row stride.
    #[cfg(target_os = "macos")]
    pub fn with_pixels<R>(&self, f: impl FnOnce(&mut [u8], usize) -> R) -> Result<R, String> {
        // SAFETY: the surface is retained; between Lock and Unlock the base
        // address maps `bytes_per_row * height` bytes.
        unsafe {
            if IOSurfaceLock(self.raw, 0, std::ptr::null_mut()) != 0 {
                return Err("IOSurfaceLock failed".into());
            }
            let stride = IOSurfaceGetBytesPerRow(self.raw);
            let base = IOSurfaceGetBaseAddress(self.raw) as *mut u8;
            let len = stride * self.height as usize;
            let result = f(std::slice::from_raw_parts_mut(base, len), stride);
            IOSurfaceUnlock(self.raw, 0, std::ptr::null_mut());
            Ok(result)
        }
    }

    #[cfg(not(target_os = "macos"))]
    pub fn with_pixels<R>(&self, _f: impl FnOnce(&mut [u8], usize) -> R) -> Result<R, String> {
        Err("IOSurface requires macOS".into())
    }

    /// Writes a display tile at its level position: `U8` planes into an
    /// RGBA8 surface, display-linear `F32` planes into an RGBA16F surface.
    pub fn write_tile(&self, tile: &Tile) -> Result<(), String> {
        let float = self.is_float();
        self.with_pixels(|pixels, stride| {
            write_display(pixels, stride, 0, self.width, float, tile)
        })?
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        // SAFETY: balances the retain from IOSurfaceLookup.
        unsafe {
            CFRelease(self.raw as *const c_void)
        }
    }
}

/// Writes a display tile into rows of either contract: [`write_rgba8`] for
/// RGBA8 surfaces, [`write_rgba16f`] for RGBA16F surfaces. A tile of the
/// other contract's sample type is an error, never a reinterpretation.
pub fn write_display(
    pixels: &mut [u8],
    stride: usize,
    first_row: u32,
    width: u32,
    float: bool,
    tile: &Tile,
) -> Result<(), String> {
    if float {
        write_rgba16f(pixels, stride, first_row, width, tile)
    } else {
        write_rgba8(pixels, stride, first_row, width, tile)
    }
}

/// Clips `tile` to a band of whole rows starting at surface row `first_row`
/// of a `width`-wide surface: `(y0, y1, visible width)`, or `None`.
fn clip(
    tile: &Tile,
    pixels: usize,
    stride: usize,
    first_row: u32,
    width: u32,
) -> Option<(u32, u32, usize)> {
    let layout = tile.layout();
    let (tw, th) = (layout.extent.width, layout.extent.height);
    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
    let band_rows = (pixels / stride.max(1)) as u32;
    let y0 = oy.max(first_row);
    let y1 = (oy + th).min(first_row + band_rows);
    if ox >= width || y0 >= y1 {
        return None;
    }
    Some((y0, y1, tw.min(width - ox) as usize))
}

/// Interleaves a planar display-linear `F32` RGB tile into RGBA16F rows
/// (alpha 1.0). Values above 1.0 (EDR headroom) are stored as they are;
/// half floats represent them up to 65504. Pure, tested without an IOSurface.
pub fn write_rgba16f(
    pixels: &mut [u8],
    stride: usize,
    first_row: u32,
    width: u32,
    tile: &Tile,
) -> Result<(), String> {
    let layout = tile.layout();
    let data = tile
        .samples::<f32>()
        .map_err(|_| "EDR surfaces take display-linear F32 tiles".to_string())?;
    if layout.channels != 3 || layout.halo != 0 {
        return Err("display tile must have three planes and no halo".into());
    }
    let Some((y0, y1, w)) = clip(tile, pixels.len(), stride, first_row, width) else {
        return Ok(());
    };
    let n = layout.plane_len();
    let tw = layout.extent.width;
    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
    let (r, rest) = data.split_at(n);
    let (g, b) = rest.split_at(n);
    const ONE: [u8; 2] = half::f16::ONE.to_le_bytes();
    for y in y0..y1 {
        let src = ((y - oy) * tw) as usize;
        let dst = (y - first_row) as usize * stride + ox as usize * 8;
        let row = &mut pixels[dst..dst + w * 8];
        for (x, px) in row.as_chunks_mut::<8>().0.iter_mut().enumerate() {
            let i = src + x;
            px[0..2].copy_from_slice(&half::f16::from_f32(r[i]).to_le_bytes());
            px[2..4].copy_from_slice(&half::f16::from_f32(g[i]).to_le_bytes());
            px[4..6].copy_from_slice(&half::f16::from_f32(b[i]).to_le_bytes());
            px[6..8].copy_from_slice(&ONE);
        }
    }
    Ok(())
}

/// Interleaves a planar `U8` RGB tile into RGBA8 rows. `pixels` holds whole
/// rows of a `width`-wide surface starting at surface row `first_row` (a
/// band); the tile is clipped to the band and the width. Pure, so it is
/// tested without an IOSurface.
pub fn write_rgba8(
    pixels: &mut [u8],
    stride: usize,
    first_row: u32,
    width: u32,
    tile: &Tile,
) -> Result<(), String> {
    let layout = tile.layout();
    let data = tile.samples::<u8>().map_err(|e| e.to_string())?;
    if layout.channels != 3 || layout.halo != 0 {
        return Err("display tile must have three planes and no halo".into());
    }
    let n = layout.plane_len();
    let (tw, th) = (layout.extent.width, layout.extent.height);
    let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
    let band_rows = (pixels.len() / stride.max(1)) as u32;
    let y0 = oy.max(first_row);
    let y1 = (oy + th).min(first_row + band_rows);
    if ox >= width || y0 >= y1 {
        return Ok(());
    }
    let w = tw.min(width - ox) as usize;
    let (r, rest) = data.split_at(n);
    let (g, b) = rest.split_at(n);
    for y in y0..y1 {
        let src = ((y - oy) * tw) as usize;
        let dst = (y - first_row) as usize * stride + ox as usize * 4;
        let row = &mut pixels[dst..dst + w * 4];
        for (x, px) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            px[0] = r[src + x];
            px[1] = g[src + x];
            px[2] = b[src + x];
            px[3] = 255;
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
mod allocation {
    use super::*;

    type CFTypeRef = *const c_void;
    #[repr(C)]
    struct Callbacks([u8; 0]);

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        static kCFTypeDictionaryKeyCallBacks: Callbacks;
        static kCFTypeDictionaryValueCallBacks: Callbacks;
        fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            count: isize,
            key_callbacks: *const Callbacks,
            value_callbacks: *const Callbacks,
        ) -> CFTypeRef;
        fn CFNumberCreate(allocator: CFTypeRef, kind: isize, value: *const c_void) -> CFTypeRef;
    }
    #[link(name = "IOSurface", kind = "framework")]
    unsafe extern "C" {
        static kIOSurfaceWidth: CFTypeRef;
        static kIOSurfaceHeight: CFTypeRef;
        static kIOSurfaceBytesPerElement: CFTypeRef;
        static kIOSurfacePixelFormat: CFTypeRef;
        fn IOSurfaceCreate(properties: CFTypeRef) -> IOSurfaceRef;
    }

    pub(super) fn create(
        width: u32,
        height: u32,
        bytes_per_element: i64,
        format: u32,
        kind: SurfaceKind,
    ) -> Result<Surface, String> {
        if width == 0 || height == 0 {
            return Err("IOSurface dimensions must be nonzero".into());
        }
        const K_CF_NUMBER_SINT64: isize = 4;
        // SAFETY: standard CF object construction; values outlive the call.
        unsafe {
            let values: [i64; 4] = [
                i64::from(width),
                i64::from(height),
                bytes_per_element,
                i64::from(format),
            ];
            let numbers: Vec<CFTypeRef> = values
                .iter()
                .map(|v| {
                    CFNumberCreate(
                        std::ptr::null(),
                        K_CF_NUMBER_SINT64,
                        v as *const i64 as *const c_void,
                    )
                })
                .collect();
            if numbers.iter().any(|n| n.is_null()) {
                for n in numbers.into_iter().filter(|n| !n.is_null()) {
                    CFRelease(n);
                }
                return Err("CFNumberCreate failed".into());
            }
            let keys = [
                kIOSurfaceWidth,
                kIOSurfaceHeight,
                kIOSurfaceBytesPerElement,
                kIOSurfacePixelFormat,
            ];
            let dict = CFDictionaryCreate(
                std::ptr::null(),
                keys.as_ptr(),
                numbers.as_ptr(),
                4,
                &kCFTypeDictionaryKeyCallBacks,
                &kCFTypeDictionaryValueCallBacks,
            );
            let surface = if dict.is_null() {
                std::ptr::null_mut()
            } else {
                let surface = IOSurfaceCreate(dict);
                CFRelease(dict);
                surface
            };
            for n in numbers {
                CFRelease(n);
            }
            if surface.is_null() {
                return Err("IOSurfaceCreate failed".into());
            }
            Ok(Surface {
                raw: surface,
                id: IOSurfaceGetID(surface),
                width,
                height,
                kind,
            })
        }
    }
}

/// Test support: creates an RGBA8 IOSurface in this process.
#[cfg(target_os = "macos")]
pub mod testing {
    /// Intentionally retains the surface for the life of the test process.
    pub fn create_rgba8(width: u32, height: u32) -> u32 {
        let surface = super::Surface::create_rgba8(width, height).expect("IOSurfaceCreate failed");
        let id = surface.id();
        std::mem::forget(surface);
        id
    }

    /// An R8 (mask overlay) IOSurface, retained for the life of the process.
    pub fn create_r8(width: u32, height: u32) -> u32 {
        let surface = super::allocation::create(
            width,
            height,
            1,
            super::PIXEL_FORMAT_R8,
            super::SurfaceKind::R8,
        )
        .expect("IOSurfaceCreate failed");
        let id = surface.id();
        std::mem::forget(surface);
        id
    }

    /// An EDR (`'RGhA'` RGBA16F) viewport IOSurface, retained for the life
    /// of the process.
    pub fn create_rgba16f(width: u32, height: u32) -> u32 {
        let surface = super::allocation::create(
            width,
            height,
            8,
            super::PIXEL_FORMAT_RGBA16F,
            super::SurfaceKind::Rgba16Float,
        )
        .expect("IOSurfaceCreate failed");
        let id = surface.id();
        std::mem::forget(surface);
        id
    }
}

#[cfg(all(test, target_os = "macos"))]
mod gpu_tests {
    use super::*;
    #[test]
    fn calibration_surface_is_owned_and_released() {
        assert!(Surface::create_rgba8(0, 3).is_err());
        assert!(Surface::create_rgba8(3, 0).is_err());
        let surface = Surface::create_rgba8(17, 19).unwrap();
        let id = surface.id();
        surface.with_pixels(|pixels, _| pixels.fill(57)).unwrap();
        let retained = Surface::lookup(id, 17, 19).unwrap();
        drop(surface);
        retained
            .with_pixels(|pixels, _| assert!(pixels.iter().all(|&p| p == 57)))
            .unwrap();
        drop(retained);
        assert!(Surface::lookup(id, 17, 19).is_err());
    }

    use engine_api::{
        jobs::CancellationToken,
        stage::StageId,
        tile::{Extent, TileCoord, TileLayout},
    };
    use image_core::{Op, StageOp};
    use std::sync::Arc;

    #[test]
    fn resident_iosurface_roundtrip() {
        let gpu = pipeline_gpu::GpuStageOp::new(Arc::new(pipeline_gpu::GpuContext::new().unwrap()));
        let id = testing::create_rgba8(7, 5);
        let surface = Surface::lookup(id, 7, 5).unwrap();
        let tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(7, 5),
                halo: 0,
                channels: 3,
            },
            vec![0.18_f32; 105],
        )
        .unwrap();
        let op = Op::Display {
            gamut: Default::default(),
            headroom: None,
        };
        let expected = image_core::CpuStageOp
            .run(StageId::Output, &op, tile.clone())
            .unwrap();
        let before = gpu.stats();
        let mut batch = gpu.begin_resident().unwrap();
        let t = batch.upload(&tile).unwrap();
        let t = batch.run(&op, &t).unwrap();
        assert!(
            batch
                .finish(
                    vec![t],
                    true,
                    Some(image_core::resident::SurfaceTarget {
                        id,
                        histogram: false
                    }),
                    &CancellationToken::new()
                )
                .unwrap()
                .tiles
                .is_empty()
        );
        assert_eq!(gpu.stats().readbacks, before.readbacks);
        surface
            .with_pixels(|pixels, stride| {
                for y in 0..5 {
                    for x in 0..7 {
                        for c in 0..3 {
                            assert!(
                                pixels[y * stride + x * 4 + c].abs_diff(
                                    expected.samples::<u8>().unwrap()[c * 35 + y * 7 + x]
                                ) <= 1
                            );
                        }
                        assert_eq!(pixels[y * stride + x * 4 + 3], 255);
                    }
                }
            })
            .unwrap();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod histogram_tests {
    use super::*;
    use engine_api::{
        jobs::CancellationToken,
        tile::{Extent, TileCoord, TileLayout},
    };
    use image_core::{StageOp, resident::SurfaceTarget};
    use std::sync::Arc;

    #[test]
    fn fused_surface_preserves_offsets_and_resets_histogram_between_frames() {
        let gpu = pipeline_gpu::GpuStageOp::new(Arc::new(pipeline_gpu::GpuContext::new().unwrap()));
        let id = testing::create_rgba8(273, 275);
        let surface = Surface::lookup(id, 273, 275).unwrap();
        surface.with_pixels(|pixels, _| pixels.fill(91)).unwrap();
        for frame in [0, 1, 0] {
            let mut expected = [[0u32; 256]; 4];
            let mut batch = gpu.begin_resident().unwrap();
            let mut resident = Vec::new();
            let mut tiles = Vec::new();
            for (x, y, width, height) in [(0, 0, 256, 256), (1, 1, 17, 19)] {
                let layout = TileLayout {
                    extent: Extent::new(width, height),
                    halo: 0,
                    channels: 3,
                };
                let n = layout.plane_len();
                let values: Vec<_> = (0..layout.len())
                    .map(|i| ((i * 17 + i / n * 31 + frame * 73) % 256) as f32)
                    .collect();
                for i in 0..n {
                    let r = values[i] as usize;
                    let g = values[n + i] as usize;
                    let b = values[2 * n + i] as usize;
                    expected[0][r] += 1;
                    expected[1][g] += 1;
                    expected[2][b] += 1;
                    expected[3][(54 * r + 183 * g + 19 * b) >> 8] += 1;
                }
                let tile = Tile::from_samples(TileCoord::new(2, x, y), layout, values).unwrap();
                resident.push(batch.upload(&tile).unwrap());
                tiles.push(tile);
            }
            let result = batch
                .finish(
                    resident,
                    true,
                    Some(SurfaceTarget {
                        id,
                        histogram: true,
                    }),
                    &CancellationToken::new(),
                )
                .unwrap();
            assert_eq!(result.histogram, Some(expected));
            assert!(result.tiles.is_empty());
            assert_eq!(gpu.stats().last_resident_dispatches, 3);
            surface
                .with_pixels(|pixels, stride| {
                    for tile in &tiles {
                        let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
                        let layout = tile.layout();
                        let n = layout.plane_len();
                        let values = tile.samples::<f32>().unwrap();
                        for y in 0..layout.extent.height as usize {
                            for x in 0..layout.extent.width as usize {
                                let i = y * layout.stride() + x;
                                let dst = (oy as usize + y) * stride + (ox as usize + x) * 4;
                                assert_eq!(
                                    &pixels[dst..dst + 4],
                                    &[
                                        values[i] as u8,
                                        values[n + i] as u8,
                                        values[2 * n + i] as u8,
                                        255
                                    ]
                                );
                            }
                        }
                    }
                    // Neither tile covers the top-right or bottom-left gaps.
                    assert_eq!(&pixels[256 * 4..273 * 4], &[91; 17 * 4]);
                    assert_eq!(
                        &pixels[274 * stride..274 * stride + 256 * 4],
                        &[91; 256 * 4]
                    );
                })
                .unwrap();
        }
        assert_eq!(gpu.stats().pixel_readback_bytes, 0);
        assert_eq!(gpu.stats().histogram_readbacks, 3);
    }

    #[test]
    fn surface_histogram_matches_pixels_without_pixel_readback() {
        let gpu = pipeline_gpu::GpuStageOp::new(Arc::new(pipeline_gpu::GpuContext::new().unwrap()));
        let id = testing::create_rgba8(256, 65);
        let surface = Surface::lookup(id, 256, 65).unwrap();
        let n = 256 * 65;
        let tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(256, 65),
                halo: 0,
                channels: 3,
            },
            (0..3 * n)
                .map(|i| ((i * 17 + i / n * 31) % 256) as f32)
                .collect(),
        )
        .unwrap();
        let mut expected = [[0u32; 256]; 4];
        let samples = tile.samples::<f32>().unwrap();
        for i in 0..n {
            let r = samples[i] as usize;
            let g = samples[n + i] as usize;
            let b = samples[2 * n + i] as usize;
            expected[0][r] += 1;
            expected[1][g] += 1;
            expected[2][b] += 1;
            expected[3][(54 * r + 183 * g + 19 * b) >> 8] += 1;
        }
        let before = gpu.stats();
        let mut batch = gpu.begin_resident().unwrap();
        let t = batch.upload(&tile).unwrap();
        let out = batch
            .finish(
                vec![t],
                true,
                Some(SurfaceTarget {
                    id,
                    histogram: true,
                }),
                &CancellationToken::new(),
            )
            .unwrap();
        assert!(out.tiles.is_empty());
        assert_eq!(out.histogram, Some(expected));
        let after = gpu.stats();
        assert_eq!(after.readbacks, before.readbacks);
        assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
        assert_eq!(after.histogram_readbacks - before.histogram_readbacks, 1);
        assert_eq!(after.submissions - before.submissions, 1);
        // One clear and one fused surface/histogram dispatch, not two pixel passes.
        assert_eq!(after.last_resident_dispatches, 2);
        surface
            .with_pixels(|pixels, stride| {
                for y in 0..65 {
                    for x in 0..256 {
                        for c in 0..3 {
                            assert_eq!(
                                pixels[y * stride + x * 4 + c],
                                samples[c * n + y * 256 + x] as u8
                            );
                        }
                    }
                }
            })
            .unwrap();
    }
}
