//! Decoded raw sources the renderer can pull tiles from.

use std::path::Path;
use std::sync::Arc;

use engine_api::id::ImageId;
use engine_api::tile::{Extent, Pyramid};
use engine_api::{EngineError, EngineResult};
use raw_decode::{CfaImage, RawMetadata, RawSource};

/// A linearized CFA plane plus the metadata the pipeline needs. Cheap to
/// clone; the samples are shared.
#[derive(Clone)]
pub struct RawImage {
    id: ImageId,
    cfa: Arc<CfaImage>,
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
        Ok(Self { id, cfa, metadata })
    }

    /// Opens and decodes a raw file.
    pub fn open(id: ImageId, path: impl AsRef<Path>) -> EngineResult<Self> {
        let mut source = RawSource::open(path)?;
        let cfa = source.decode_cfa()?;
        let metadata = source.metadata();
        Self::new(id, Arc::new(cfa), Arc::new(metadata))
    }

    /// The same shared samples under another identity and metadata (for
    /// example a smaller `default_crop` window). Validated like [`Self::new`];
    /// the id must differ from the original's so memo keys never alias.
    pub fn with_metadata(&self, id: ImageId, metadata: Arc<RawMetadata>) -> EngineResult<Self> {
        Self::new(id, self.cfa.clone(), metadata)
    }

    /// Image identity used in memo keys.
    pub fn id(&self) -> ImageId {
        self.id
    }

    /// Linearized sensor samples.
    pub fn cfa(&self) -> &CfaImage {
        &self.cfa
    }

    /// Sensor metadata.
    pub fn metadata(&self) -> &RawMetadata {
        &self.metadata
    }

    /// Full sensor extent (the frame CFA phase is indexed in).
    pub fn sensor_extent(&self) -> Extent {
        self.cfa.pyramid().extent()
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
