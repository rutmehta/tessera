//! Camera raw image decoding backed by LibRaw.

use std::path::{Path, PathBuf};

use engine_api::{
    EngineError, EngineResult,
    tile::{Extent, Pyramid, TILE_SIZE, Tile, TileCoord, TileFormat, TileLayout},
};
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
    pub orientation: u16,
    pub width: u32,
    pub height: u32,
    pub cfa_pattern: [u8; 4],
    pub black_levels: [f32; 4],
    pub white_level: u32,
    pub as_shot_wb: [f32; 4],
    pub camera_to_xyz: [[f32; 3]; 3],
    pub default_crop: [u32; 4],
    pub has_gain_map: bool,
    pub has_opcode_list: bool,
}

impl From<FfiMetadata> for RawMetadata {
    fn from(m: FfiMetadata) -> Self {
        Self {
            make: m.make,
            model: m.model,
            lens: None,
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
            cfa_pattern: [0; 4],
            black_levels: [0.; 4],
            white_level: 0,
            as_shot_wb: [0.; 4],
            camera_to_xyz: [[0.; 3]; 3],
            default_crop: [0; 4],
            has_gain_map: false,
            has_opcode_list: false,
        }
    }
}

/// Decoded mosaic samples, packed row-major.
#[derive(Debug, Clone)]
pub struct CfaU16 {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u16>,
    pub cfa_pattern: [u8; 4],
}

/// Level-zero tiled CFA image.
pub struct CfaImage {
    pyramid: CfaPyramid,
}

impl CfaImage {
    pub fn pyramid(&self) -> &CfaPyramid {
        &self.pyramid
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
        let mut raw = self.raw.cfa_data();
        let _ = &mut raw;
        let mut metadata = RawMetadata::from(self.raw.metadata());
        let image = self.raw.cfa_data();
        metadata.width = image.width;
        metadata.height = image.height;
        metadata.cfa_pattern = image.cfa_pattern;
        metadata.black_levels = image.black;
        metadata.white_level = image.white;
        metadata.as_shot_wb = image.wb_coeffs;
        metadata.camera_to_xyz = image.color_matrix;
        metadata.default_crop = image.crop;
        metadata
    }

    pub fn embedded_preview(&mut self) -> Option<Vec<u8>> {
        self.raw.embedded_preview()
    }

    pub fn decode_cfa_u16(&mut self) -> EngineResult<CfaU16> {
        self.raw.unpack().map_err(|e| decode_error(e.to_string()))?;
        let image = self.raw.cfa_data();
        if image.data.len() != image.width as usize * image.height as usize {
            return Err(decode_error("LibRaw did not return a packed CFA plane"));
        }
        Ok(CfaU16 {
            width: image.width,
            height: image.height,
            data: image.data,
            cfa_pattern: image.cfa_pattern,
        })
    }

    pub fn decode_cfa(&mut self) -> EngineResult<CfaImage> {
        self.raw.unpack().map_err(|e| decode_error(e.to_string()))?;
        let image = self.raw.cfa_data();
        if image.data.len() != image.width as usize * image.height as usize {
            return Err(decode_error("LibRaw did not return a packed CFA plane"));
        }
        let pixels = image
            .data
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                let channel = image.cfa_pattern[((i as u32 % image.width) & 1) as usize
                    + (((i as u32 / image.width) & 1) as usize) * 2]
                    as usize;
                let black = image.black[channel.min(3)];
                ((v as f32 - black) / (image.white as f32 - black).max(1.0)).clamp(0.0, 1.2)
            })
            .collect();
        Ok(CfaImage {
            pyramid: CfaPyramid {
                extent: Extent::new(image.width, image.height),
                pixels,
            },
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn decode_error(message: impl Into<String>) -> EngineError {
    EngineError::Decode {
        format: "raw".into(),
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_source_returns_engine_error() {
        assert!(RawSource::open("/definitely/not/a/raw/file.dng").is_err());
    }

    #[test]
    fn fixtures_decode() {
        let root = Path::new("../../fixtures/raw");
        if !root.exists() {
            eprintln!("skipping RAW fixture tests: fixtures/raw is absent");
            return;
        }
        let entries: Vec<_> = std::fs::read_dir(root)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file())
            .collect();
        for path in entries {
            let mut source =
                RawSource::open(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            let metadata = source.metadata();
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
            if let Some(jpeg) = source.embedded_preview() {
                assert!(
                    jpeg.starts_with(&[0xff, 0xd8]) && jpeg.ends_with(&[0xff, 0xd9]),
                    "{}",
                    path.display()
                );
            }
        }
    }
}
