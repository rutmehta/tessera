//! Camera raw image decoding backed by LibRaw.

use std::path::{Path, PathBuf};

pub mod dng;
pub mod linear_dng;

use engine_api::{
    EngineError, EngineResult,
    color::ColorMatrix3,
    tile::{Extent, Pyramid, TILE_SIZE, Tile, TileCoord, TileFormat, TileLayout},
};
pub use libraw_ffi::CfaLayout;
use libraw_ffi::{Metadata as FfiMetadata, RawFile};

/// Camera and sensor information exposed by LibRaw.
#[derive(Debug, Clone)]
pub struct RawMetadata {
    pub make: String,
    pub model: String,
    pub lens: Option<String>,
    pub iso: f32,
    pub shutter_s: f32,
    pub aperture: f32,
    pub focal_mm: f32,
    pub capture_time: i64,
    /// EXIF orientation 1–8. The raw plane itself is never rotated.
    pub orientation: u16,
    /// Full sensor dimensions, including masked margins.
    pub width: u32,
    pub height: u32,
    pub cfa_layout: CfaLayout,
    pub black_levels: [f32; 4],
    pub white_level: u32,
    pub as_shot_wb: [f32; 4],
    /// LibRaw camera-to-linear-sRGB composed with sRGB-to-XYZ (D65).
    /// Apply to white-balanced camera RGB, not unbalanced mosaic samples.
    pub camera_to_xyz: ColorMatrix3,
    /// Original LibRaw XYZ -> camera matrix (D65), not derived from rgb_cam.
    pub cam_xyz: [[f32; 3]; 4],
    /// Original LibRaw white-balanced camera -> linear sRGB matrix.
    pub rgb_cam: [[f32; 4]; 3],
    /// [left, top, width, height] in full sensor coordinates.
    pub default_crop: [u32; 4],
    /// DNG GainMap opcode presence. No gain map is applied during decode.
    pub has_gain_map: bool,
    pub has_opcode_list: bool,
    /// Owned raw OpcodeList1/2/3 bytes for the LibRaw-selected image IFD.
    /// Absence is not an identity calibration; proprietary maker-note corrections are not exposed.
    pub opcode_lists: [Option<Vec<u8>>; 3],
}

impl From<FfiMetadata> for RawMetadata {
    fn from(m: FfiMetadata) -> Self {
        Self {
            make: m.make,
            model: m.model,
            lens: m.lens,
            iso: m.iso,
            shutter_s: m.shutter,
            aperture: m.aperture,
            focal_mm: m.focal,
            capture_time: m.timestamp,
            orientation: if (1..=8).contains(&m.orientation) {
                m.orientation
            } else {
                1
            },
            width: 0,
            height: 0,
            cfa_layout: CfaLayout::Unsupported,
            black_levels: [0.; 4],
            white_level: 0,
            as_shot_wb: [0.; 4],
            camera_to_xyz: ColorMatrix3([[0.; 3]; 3]),
            cam_xyz: [[0.; 3]; 4],
            rgb_cam: [[0.; 4]; 3],
            default_crop: [0; 4],
            has_gain_map: m.has_gain_map,
            has_opcode_list: m.has_opcode_list,
            opcode_lists: m.opcode_lists,
        }
    }
}

/// Decoded mosaic samples, packed row-major.
#[derive(Debug, Clone)]
pub struct CfaU16 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
    pub cfa_layout: CfaLayout,
}

/// Level-zero tiled CFA image.
pub struct CfaImage {
    pyramid: CfaPyramid,
}

impl CfaImage {
    pub fn pyramid(&self) -> &CfaPyramid {
        &self.pyramid
    }

    /// Wraps already-linearized, row-major sensor samples (synthetic sources,
    /// tests, and callers that normalized samples themselves). Samples must be
    /// finite; no clamping or black/white normalization is applied.
    pub fn from_linear(width: u32, height: u32, pixels: Vec<f32>) -> EngineResult<Self> {
        if width == 0
            || height == 0
            || pixels.len() as u64 != u64::from(width) * u64::from(height)
            || pixels.iter().any(|v| !v.is_finite())
        {
            return Err(EngineError::invalid(
                "CFA samples",
                "nonempty finite width*height plane required",
            ));
        }
        Ok(Self {
            pyramid: CfaPyramid {
                extent: Extent::new(width, height),
                pixels,
            },
        })
    }
}

/// Single-plane, linearized CFA pyramid. Higher levels are intentionally absent.
pub struct CfaPyramid {
    extent: Extent,
    pixels: Vec<f32>,
}

impl Pyramid for CfaPyramid {
    fn extent(&self) -> Extent {
        self.extent
    }
    fn format(&self) -> TileFormat {
        TileFormat::F32Planar
    }
    fn channels(&self) -> u8 {
        1
    }
    fn halo(&self) -> u16 {
        0
    }
    fn level_count(&self) -> u8 {
        1
    }
    fn tile(&self, coord: TileCoord) -> EngineResult<Tile> {
        if coord.level != 0 || !self.contains(coord) {
            return Err(EngineError::invalid("coord", coord.to_string()));
        }
        let extent = self.tile_extent(coord);
        let layout = TileLayout {
            extent,
            halo: 0,
            channels: 1,
        };
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        let mut data = Vec::with_capacity((extent.width * extent.height) as usize);
        for y in oy..oy + extent.height {
            let start = (y * self.extent.width + ox) as usize;
            data.extend_from_slice(&self.pixels[start..start + extent.width as usize]);
        }
        Tile::from_samples(coord, layout, data)
    }
}

/// Opened camera raw source.
pub struct RawSource {
    path: PathBuf,
    raw: RawFile,
}

impl RawSource {
    pub fn open(path: impl AsRef<Path>) -> EngineResult<Self> {
        let raw = RawFile::open(&path).map_err(|e| decode_error(e.to_string()))?;
        Ok(Self {
            path: path.as_ref().to_path_buf(),
            raw,
        })
    }

    pub fn metadata(&self) -> RawMetadata {
        let mut metadata = RawMetadata::from(self.raw.metadata());
        let image = self.raw.sensor_info();
        metadata.width = image.width;
        metadata.height = image.height;
        metadata.cfa_layout = image.cfa_layout;
        metadata.black_levels = image.black;
        metadata.white_level = image.white;
        metadata.as_shot_wb = image.wb_coeffs;
        metadata.camera_to_xyz = ColorMatrix3(image.color_matrix.map(|row| row.map(f64::from)));
        metadata.cam_xyz = image.cam_xyz;
        metadata.rgb_cam = image.rgb_cam;
        metadata.default_crop = image.crop;
        metadata
    }

    pub fn embedded_preview(&mut self) -> Option<Vec<u8>> {
        self.raw.embedded_preview()
    }

    pub fn decode_cfa_u16(&mut self) -> EngineResult<CfaU16> {
        self.raw.unpack().map_err(|e| decode_error(e.to_string()))?;
        let image = self.raw.cfa_data();
        validate_plane(&image)?;
        Ok(CfaU16 {
            width: image.width,
            height: image.height,
            data: image.data,
            cfa_layout: image.cfa_layout,
        })
    }

    pub fn decode_cfa(&mut self) -> EngineResult<CfaImage> {
        self.raw.unpack().map_err(|e| decode_error(e.to_string()))?;
        let image = self.raw.cfa_data();
        linearize(image)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn linearize(image: libraw_ffi::CfaImage) -> EngineResult<CfaImage> {
    validate_plane(&image)?;
    if image
        .black
        .iter()
        .any(|&b| !b.is_finite() || b >= image.white as f32)
    {
        return Err(decode_error("invalid sensor black/white levels"));
    }
    let pixels = image
        .data
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let channel = image
                .cfa_layout
                .channel_at(i as u32 % image.width, i as u32 / image.width);
            let black = image.black[channel];
            ((v as f32 - black) / (image.white as f32 - black)).clamp(0.0, 1.2)
        })
        .collect();
    Ok(CfaImage {
        pyramid: CfaPyramid {
            extent: Extent::new(image.width, image.height),
            pixels,
        },
    })
}

fn validate_plane(image: &libraw_ffi::CfaImage) -> EngineResult<()> {
    if image.width == 0
        || image.height == 0
        || image.data.len() != image.width as usize * image.height as usize
    {
        return Err(decode_error("LibRaw did not return a packed CFA plane"));
    }
    let valid = match image.cfa_layout {
        CfaLayout::Bayer(p) => p.iter().flatten().all(|&c| c < 4),
        CfaLayout::XTrans(p) => p.iter().flatten().all(|&c| c < 3),
        CfaLayout::Unsupported => false,
    };
    if !valid {
        return Err(decode_error("unsupported CFA layout"));
    }
    Ok(())
}

fn decode_error(message: impl Into<String>) -> EngineError {
    EngineError::Decode {
        format: "raw".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests;

// Exercise the dependency-free parser even before the lens crate is integrated.
#[cfg(test)]
#[path = "../../lens/src/opcodes.rs"]
mod correction_opcode_tests;

#[cfg(test)]
mod fixture_tests {
    use super::*;

    #[test]
    fn metadata_preserves_opcode_payloads() {
        let m = FfiMetadata {
            make: String::new(),
            model: String::new(),
            lens: None,
            iso: 0.,
            shutter: 0.,
            aperture: 0.,
            focal: 0.,
            timestamp: 0,
            orientation: 1,
            has_opcode_list: true,
            has_gain_map: false,
            opcode_lists: [Some(vec![0; 4]), None, Some(vec![1, 2])],
        };
        let expected = m.opcode_lists.clone();
        assert_eq!(RawMetadata::from(m).opcode_lists, expected);
    }

    #[test]
    fn missing_source_returns_engine_error() {
        assert!(RawSource::open("/definitely/not/a/raw/file.dng").is_err());
    }

    #[test]
    fn fixtures_decode() {
        let root = std::env::var_os("RAW_DECODE_FIXTURES")
            .map(PathBuf::from)
            .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
        if !root.exists() {
            eprintln!("skipping RAW fixture tests: fixtures/raw is absent");
            return;
        }
        let mut entries: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|e| {
                        matches!(
                            e.to_string_lossy().to_ascii_lowercase().as_str(),
                            "cr3" | "arw" | "nef" | "raf" | "dng"
                        )
                    })
            })
            .collect();
        entries.sort();
        assert!(
            !entries.is_empty(),
            "fixture directory exists but contains no RAW files"
        );
        for extension in ["cr3", "arw", "nef", "raf", "dng"] {
            assert!(
                entries
                    .iter()
                    .any(|p| p.extension().unwrap().eq_ignore_ascii_case(extension)),
                "missing {extension} fixture"
            );
        }
        for path in entries {
            let mut source =
                RawSource::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let metadata = source.metadata();
            let original = RawFile::open(&path).unwrap().sensor_info();
            assert_eq!(metadata.cam_xyz, original.cam_xyz);
            assert_eq!(metadata.rgb_cam, original.rgb_cam);
            assert!(metadata.cam_xyz.iter().flatten().any(|&v| v != 0.0));
            assert!(
                metadata.camera_to_xyz.0.iter().flatten().any(|v| *v != 0.0),
                "missing matrix: {}",
                path.display()
            );
            assert_ne!(
                metadata.cfa_layout,
                CfaLayout::Unsupported,
                "missing CFA: {}",
                path.display()
            );
            assert!(
                (1..=8).contains(&metadata.orientation),
                "{}",
                path.display()
            );
            let decoded = source
                .decode_cfa()
                .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            assert_eq!(decoded.pyramid.extent.width, metadata.width);
            assert_eq!(decoded.pyramid.extent.height, metadata.height);
            let packed = source.decode_cfa_u16().expect("repeat decode as u16");
            assert_eq!(packed.width, metadata.width);
            assert_eq!(packed.height, metadata.height);
            assert_eq!(packed.cfa_layout, metadata.cfa_layout);
            assert_eq!(packed.data.len(), decoded.pyramid.pixels.len());
            assert_eq!(source.metadata().width, metadata.width);
            assert!(
                decoded
                    .pyramid
                    .pixels
                    .iter()
                    .all(|&v| v.is_finite() && (0.0..=1.2).contains(&v))
            );
            let after = source.metadata();
            for (i, &v) in packed.data.iter().enumerate().step_by(997) {
                let c = after
                    .cfa_layout
                    .channel_at(i as u32 % packed.width, i as u32 / packed.width);
                let b = after.black_levels[c];
                let expected =
                    ((f32::from(v) - b) / (after.white_level as f32 - b)).clamp(0.0, 1.2);
                assert_eq!(decoded.pyramid.pixels[i], expected);
            }
            let n = decoded.pyramid.pixels.len() as f64;
            let mean = decoded
                .pyramid
                .pixels
                .iter()
                .map(|&v| f64::from(v))
                .sum::<f64>()
                / n;
            assert!(
                (0.005..0.9).contains(&mean),
                "{} mean={mean}",
                path.display()
            );
            let preview = source.embedded_preview();
            eprintln!(
                "{}: {}x{}, mean={mean:.6}, JPEG={}, layout={:?}",
                path.display(),
                metadata.width,
                metadata.height,
                preview.as_ref().map_or(0, Vec::len),
                metadata.cfa_layout
            );
            if let Some(jpeg) = preview {
                assert!(
                    jpeg.starts_with(&[0xff, 0xd8]) && jpeg.ends_with(&[0xff, 0xd9]),
                    "{}",
                    path.display()
                );
                assert_eq!(source.embedded_preview().as_ref(), Some(&jpeg));
            } else {
                // Some DNGs contain only a bitmap thumbnail, not an embedded JPEG.
                assert!(
                    path.extension().unwrap().eq_ignore_ascii_case("dng"),
                    "missing JPEG: {}",
                    path.display()
                );
            }
        }
    }
}
