//! Decoded CFA or working-space RGB sources the renderer can pull tiles from.

use std::path::Path;
use std::sync::Arc;

use engine_api::id::ImageId;
use engine_api::tile::{Extent, Pyramid};
use engine_api::{EngineError, EngineResult};
use raw_decode::{CfaImage, RawMetadata, RawSource};

/// Shared decoded source. The historical name is retained for callers, but
/// this may contain a CFA plane or upright working-space RGB. RGB has no
/// camera calibration; its metadata describes a D65 working-space identity.
#[derive(Clone)]
pub struct RawImage {
    id: ImageId,
    cfa: Option<Arc<CfaImage>>,
    rgb: Option<Arc<crate::RgbSource>>,
    metadata: Arc<RawMetadata>,
}

impl RawImage {
    /// Wraps a decoded image. Its extent must match `metadata.width/height`
    /// and `metadata.default_crop` must be a non-empty rectangle inside it.
    pub fn new(id: ImageId, cfa: Arc<CfaImage>, metadata: Arc<RawMetadata>) -> EngineResult<Self> {
        let e = cfa.pyramid().extent();
        if e.width != metadata.width || e.height != metadata.height {
            return Err(EngineError::invalid(
                "metadata",
                "dimensions do not match CFA",
            ));
        }
        let [left, top, w, h] = metadata.default_crop;
        if w == 0
            || h == 0
            || u64::from(left) + u64::from(w) > u64::from(e.width)
            || u64::from(top) + u64::from(h) > u64::from(e.height)
        {
            return Err(EngineError::invalid(
                "default_crop",
                "active area must be a non-empty rectangle inside the sensor",
            ));
        }
        Ok(Self {
            id,
            cfa: Some(cfa),
            rgb: None,
            metadata,
        })
    }

    /// Opens and decodes a RAW, JPEG, PNG, TIFF or feature-enabled HEIC file.
    pub fn open(id: ImageId, path: impl AsRef<Path>) -> EngineResult<Self> {
        if crate::RgbSource::recognizes(&path) {
            return Self::from_rgb(id, crate::RgbSource::open(path)?);
        }
        let mut source = RawSource::open(path)?;
        let cfa = source.decode_cfa()?;
        let metadata = source.metadata();
        Self::new(id, Arc::new(cfa), Arc::new(metadata))
    }

    /// The same shared samples under another identity and metadata (for
    /// example a smaller `default_crop` window). Validated like [`Self::new`];
    /// the id must differ from the original's so memo keys never alias.
    pub fn with_metadata(&self, id: ImageId, metadata: Arc<RawMetadata>) -> EngineResult<Self> {
        let cfa = self
            .cfa
            .clone()
            .ok_or_else(|| EngineError::invalid("source", "RGB sources have no camera metadata"))?;
        Self::new(id, cfa, metadata)
    }

    /// Wrap upright working-space RGB without allocating a synthetic CFA plane.
    pub fn from_rgb(id: ImageId, rgb: crate::RgbSource) -> EngineResult<Self> {
        let (width, height) = (rgb.pixels().width(), rgb.pixels().height());
        let xyz = engine_api::color::WorkingSpace::LinearRec2020.to_xyz();
        let inverse = xyz.inverse()?;
        let metadata = RawMetadata {
            make: String::new(),
            model: String::new(),
            lens: None,
            iso: 0.,
            shutter_s: 0.,
            aperture: 0.,
            focal_mm: 0.,
            capture_time: 0,
            orientation: 1,
            width,
            height,
            cfa_layout: raw_decode::CfaLayout::Unsupported,
            black_levels: [0.; 4],
            white_level: 1,
            as_shot_wb: [1.; 4],
            camera_to_xyz: xyz,
            cam_xyz: std::array::from_fn(|r| {
                if r < 3 {
                    inverse.0[r].map(|v| v as f32)
                } else {
                    [0.; 3]
                }
            }),
            rgb_cam: [[0.; 4]; 3],
            default_crop: [0, 0, width, height],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        };
        Ok(Self {
            id,
            cfa: None,
            rgb: Some(Arc::new(rgb)),
            metadata: Arc::new(metadata),
        })
    }

    pub fn rgb(&self) -> Option<&crate::RgbSource> {
        self.rgb.as_deref()
    }

    pub fn source_kind(&self) -> &'static str {
        if self.rgb.is_some() { "rgb" } else { "raw" }
    }

    /// Image identity used in memo keys.
    pub fn id(&self) -> ImageId {
        self.id
    }

    /// Linearized sensor samples. Only valid when [`Self::rgb`] is None.
    /// Panics if called on a rendered RGB source.
    pub fn cfa(&self) -> &CfaImage {
        self.cfa
            .as_deref()
            .expect("CFA requested for an RGB source")
    }

    /// Sensor metadata.
    pub fn metadata(&self) -> &RawMetadata {
        &self.metadata
    }

    /// Full sensor extent (the frame CFA phase is indexed in).
    pub fn sensor_extent(&self) -> Extent {
        Extent::new(self.metadata.width, self.metadata.height)
    }

    /// Active-area (default crop) extent: level 0 of the output pyramid.
    pub fn active_extent(&self) -> Extent {
        let [_, _, w, h] = self.metadata.default_crop;
        Extent::new(w, h)
    }

    /// Output-pyramid extent at `level` (`ceil(active / 2^level)`).
    pub fn level_extent(&self, level: u8) -> Extent {
        self.active_extent().at_level(level)
    }
}

impl std::fmt::Debug for RawImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RawImage")
            .field("id", &self.id)
            .field("sensor", &self.sensor_extent())
            .field("crop", &self.metadata.default_crop)
            .finish_non_exhaustive()
    }
}
