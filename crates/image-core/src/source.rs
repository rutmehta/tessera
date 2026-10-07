//! Decoded CFA or working-space RGB sources the renderer can pull tiles from.

use std::path::Path;
use std::sync::Arc;

use engine_api::id::ImageId;
use engine_api::tile::{Extent, Pyramid};
use engine_api::{EngineError, EngineResult};
use raw_decode::{CfaImage, RawMetadata, RawSource};

struct EmbeddedProfile {
    bytes: Result<Option<Vec<u8>>, String>,
}

/// Shared decoded source. The historical name is retained for callers, but
/// this may contain a CFA plane or upright working-space RGB. RGB has no
/// camera calibration; its metadata describes a D65 working-space identity.
#[derive(Clone)]
pub struct RawImage {
    id: ImageId,
    recipe_owner: ImageId,
    camera_linear_proxy: Option<Arc<pipeline_cpu::CameraLinearProxy>>,
    cfa: Option<Arc<CfaImage>>,
    rgb: Option<Arc<crate::RgbSource>>,
    metadata: Arc<RawMetadata>,
    embedded_profile: Option<Arc<EmbeddedProfile>>,
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
            recipe_owner: id,
            embedded_profile: None,
            camera_linear_proxy: None,
            cfa: Some(cfa),
            rgb: None,
            metadata,
        })
    }

    /// Opens and decodes a RAW, JPEG, PNG, TIFF or feature-enabled HEIC file.
    pub fn open(id: ImageId, path: impl AsRef<Path>) -> EngineResult<Self> {
        let path = path.as_ref();
        let mut file = std::fs::File::open(path).map_err(|e| EngineError::io_at(path, &e))?;
        let embedded_profile = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dng"))
            .then(|| {
                Arc::new(EmbeddedProfile {
                    // Keep a snapshot alongside decoded pixels. Bad profile metadata
                    // becomes a non-fatal substitution note, never a decode failure.
                    bytes: pipeline_adobe::dcp::read_embedded_profile(&mut file),
                })
            });
        let decode_error = |e: std::io::Error| EngineError::Decode {
            format: "LinearRaw DNG".into(),
            message: e.to_string(),
        };
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("dng"))
            && let Some(dng) = raw_decode::lossy_dng::read(&mut file).map_err(decode_error)?
        {
            let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng)?;
            return Ok(Self {
                id,
                recipe_owner: id,
                embedded_profile,
                metadata: Arc::new(proxy.original_metadata().clone()),
                camera_linear_proxy: Some(Arc::new(proxy)),
                cfa: None,
                rgb: None,
            });
        }
        if raw_decode::linear_dng::is_linear_dng(&mut file).map_err(decode_error)? {
            let dng = raw_decode::linear_dng::read(&mut file).map_err(decode_error)?;
            return Self::from_rgb(id, crate::RgbSource::from_linear_dng(dng)?);
        }
        if crate::RgbSource::recognizes(path) {
            return Self::from_rgb(id, crate::RgbSource::open(path)?);
        }
        let mut source = RawSource::open(path)?;
        let cfa = source.decode_cfa()?;
        let metadata = source.metadata();
        let mut image = Self::new(id, Arc::new(cfa), Arc::new(metadata))?;
        image.embedded_profile = embedded_profile;
        Ok(image)
    }

    pub(crate) fn embedded_dcp(&self) -> EngineResult<Option<&[u8]>> {
        self.embedded_profile.as_ref().map_or_else(
            || {
                Ok(self
                    .camera_linear_proxy
                    .as_ref()
                    .and_then(|p| p.embedded_profile()))
            },
            |profile| {
                profile
                    .bytes
                    .as_ref()
                    .map(|bytes| bytes.as_deref())
                    .map_err(|error| EngineError::invalid("embedded DNG profile", error.clone()))
            },
        )
    }

    /// Open in the catalog's absolute frame. RAW reconstruction remains sensor-
    /// aligned, while its common render tail orients before normalized edits.
    pub fn open_with_catalog_orientation(
        id: ImageId,
        path: impl AsRef<Path>,
        orientation: Option<u16>,
    ) -> EngineResult<Self> {
        let Some(orientation) = orientation else {
            return Self::open(id, path);
        };
        if !(1..=8).contains(&orientation) {
            return Err(EngineError::invalid("catalog orientation", "expected 1..8"));
        }
        let path = path.as_ref();
        if crate::RgbSource::recognizes(path) {
            return Self::from_rgb(
                id,
                crate::RgbSource::open_with_orientation(path, Some(orientation))?,
            );
        }
        let mut image = Self::open(id, path)?;
        if image.rgb.is_some() {
            // Working-space linear DNGs also consume orientation in their decoder.
            return Self::from_rgb(
                id,
                crate::RgbSource::open_with_orientation(path, Some(orientation))?,
            );
        }
        let metadata = Arc::make_mut(&mut image.metadata);
        metadata.catalog_orientation = Some(orientation);
        metadata.orientation = 1;
        if let Some(proxy) = &mut image.camera_linear_proxy {
            *proxy = Arc::new(
                proxy
                    .as_ref()
                    .clone()
                    .with_catalog_orientation(orientation)?,
            );
        }
        Ok(image)
    }

    /// The same shared samples under another identity and metadata (for
    /// example a smaller `default_crop` window). Validated like [`Self::new`];
    /// the id must differ from the original's so memo keys never alias.
    pub fn with_metadata(&self, id: ImageId, metadata: Arc<RawMetadata>) -> EngineResult<Self> {
        let cfa = self
            .cfa
            .clone()
            .ok_or_else(|| EngineError::invalid("source", "RGB sources have no camera metadata"))?;
        let mut image = Self::new(id, cfa, metadata)?;
        image.embedded_profile = self.embedded_profile.clone();
        Ok(image)
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
            catalog_orientation: None,
            baseline_exposure: 0.,
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
            recipe_owner: id,
            embedded_profile: None,
            camera_linear_proxy: None,
            cfa: None,
            rgb: Some(Arc::new(rgb)),
            metadata: Arc::new(metadata),
        })
    }

    /// Attach an immutable camera-linear preview to its original RAW recipe.
    /// `render_id` must identify the verified representation, generator, payload
    /// and scale, and must differ from the original recipe owner. The caller
    /// owns persistence verification and must not reuse it for another payload.
    pub fn from_camera_linear_proxy(
        recipe_owner: ImageId,
        render_id: ImageId,
        proxy: Arc<pipeline_cpu::CameraLinearProxy>,
    ) -> EngineResult<Self> {
        if recipe_owner == render_id {
            return Err(EngineError::invalid(
                "render_id",
                "proxy and original identities must differ",
            ));
        }
        Ok(Self {
            id: render_id,
            recipe_owner,
            embedded_profile: None,
            metadata: Arc::new(proxy.original_metadata().clone()),
            camera_linear_proxy: Some(proxy),
            cfa: None,
            rgb: None,
        })
    }

    pub fn camera_linear_proxy(&self) -> Option<&pipeline_cpu::CameraLinearProxy> {
        self.camera_linear_proxy.as_deref()
    }

    /// Keep a stable recipe identity while a relinked source uses its own render
    /// identity. Render caches must never alias proxy and original payloads.
    pub fn with_recipe_owner(mut self, owner: ImageId) -> Self {
        self.recipe_owner = owner;
        self
    }

    /// Catalog/recipe identity, independent of the decoded representation.
    pub fn recipe_owner(&self) -> ImageId {
        self.recipe_owner
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

    /// Linearized sensor samples. Only valid for an original CFA source.
    /// Panics on RGB and camera-linear proxy sources.
    pub fn cfa(&self) -> &CfaImage {
        self.cfa
            .as_deref()
            .expect("CFA requested for a non-CFA source")
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
        let (w, h) = if let Some(proxy) = self.camera_linear_proxy() {
            (proxy.pixels().width(), proxy.pixels().height())
        } else {
            let [_, _, w, h] = self.metadata.default_crop;
            (w, h)
        };
        if self.metadata.catalog_orientation.is_some_and(|o| o >= 5) {
            Extent::new(h, w)
        } else {
            Extent::new(w, h)
        }
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
