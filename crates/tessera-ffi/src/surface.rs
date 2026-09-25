//! IOSurface pixel writer for the develop viewport (docs/11 §1.2).
//!
//! Swift allocates the surfaces and presents them through its `CAMetalLayer`;
//! Rust only writes pixels through the surface's CPU mapping. The contract is
//! **RGBA8, display-encoded sRGB, straight alpha = 255**, 4 bytes per element
//! (`kCVPixelFormatType_32RGBA`, `'RGBA'`). Metal imports it as
//! `.rgba8Unorm_srgb`, so sampling yields linear sRGB and the layer's colour
//! space (`extendedLinearSRGB`) lets Core Animation convert to the display.
//! RGBA8 was chosen over RGBA16F because the Output stage already produces
//! 8-bit display values: half floats would double the bytes written per frame
//! for no additional information. An EDR path would switch to RGBA16F with
//! the scene-linear output, and only this module and the Swift pixel format
//! would change.
//!
//! Surfaces keep the sensor orientation; the Metal presenter applies the EXIF
//! orientation when sampling, so writes stay row-contiguous.

use engine_api::tile::{TILE_SIZE, Tile};
use std::ffi::c_void;

/// `'RGBA'`.
pub const PIXEL_FORMAT_RGBA8: u32 = u32::from_be_bytes(*b"RGBA");

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
}

// SAFETY: IOSurfaceRef is a thread-safe CoreFoundation object; pixel access is
// bracketed by IOSurfaceLock/Unlock on the writing thread.
unsafe impl Send for Surface {}
unsafe impl Sync for Surface {}

impl Surface {
    /// Looks up a surface created in this process (or a global one) by id and
    /// checks it matches the RGBA8 contract and the expected size.
    #[cfg(target_os = "macos")]
    pub fn lookup(id: u32, width: u32, height: u32) -> Result<Self, String> {
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
            };
            if IOSurfaceGetBytesPerElement(raw) != 4 {
                return Err("IOSurface must have 4 bytes per element (RGBA8)".into());
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

    /// Writes a display tile (`U8`, three planes) at its level position.
    pub fn write_tile(&self, tile: &Tile) -> Result<(), String> {
        self.with_pixels(|pixels, stride| write_rgba8(pixels, stride, 0, self.width, tile))?
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

/// Test support: creates an RGBA8 IOSurface in this process.
#[cfg(target_os = "macos")]
pub mod testing {
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

    /// An RGBA8 surface; returns its id. The surface is intentionally kept
    /// alive (leaked) for the life of the test process.
    pub fn create_rgba8(width: u32, height: u32) -> u32 {
        const K_CF_NUMBER_SINT64: isize = 4;
        // SAFETY: standard CF object construction; values outlive the call.
        unsafe {
            let values: [i64; 4] = [
                i64::from(width),
                i64::from(height),
                4,
                i64::from(PIXEL_FORMAT_RGBA8),
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
            let surface = IOSurfaceCreate(dict);
            CFRelease(dict);
            for n in numbers {
                CFRelease(n);
            }
            assert!(!surface.is_null(), "IOSurfaceCreate failed");
            IOSurfaceGetID(surface)
        }
    }
}
