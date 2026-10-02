//! Native2 camera-linear Smart Preview boundary.
#[path = "smart_preview_codec.rs"]
mod codec;
use crate::{Image, LensContext, ResolvedLens};
pub use codec::{DecodedSmartPreview, SmartPreviewEncoding};
use engine_api::{
    EngineError, EngineResult,
    recipe::{
        DevelopSettings, ProcessFamily, ProcessVersion,
        settings::{
            DecodeSettings, DemosaicSettings, DenoiseMethod, DenoiseSettings, LensProfileSource,
            LensSettings, LinearizeSettings,
        },
    },
};
use raw_decode::{CfaImage, RawMetadata};

/// Explicit pre-edit spatial quality tier; neither tier changes the color pipeline.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum SmartPreviewTier {
    Compact2048,
    #[default]
    Detail2560,
}
impl SmartPreviewTier {
    pub const fn max_edge(self) -> u32 {
        match self {
            Self::Compact2048 => 2048,
            Self::Detail2560 => 2560,
        }
    }
}

/// Owned camera-linear active-area pixels, before camera matrices and WB.
///
/// Generation bakes sensor corrections, reconstruction, demosaic and CA in their
/// original order. Lens resolution is captured once using original sensor pixels.
/// Profile vignette and common geometry remain in the shared render tail. No
/// correction is re-estimated on reduced pixels. Original metadata never serves
/// as the reduced image's addressing metadata. EXIF orientation remains caller-
/// owned, as for the CFA reference renderer; these planes are sensor-oriented.
///
/// Immutable fields make cloning an exact in-memory snapshot/round trip. This
/// type persists through a separately versioned, bounded camera-linear codec
/// with payload/container digests and an explicit original-source assertion.
#[derive(Clone, Debug)]
pub struct CameraLinearProxy {
    external_dng: bool,
    pixels: Image,
    metadata: RawMetadata,
    correction: ResolvedLens,
    decode: DecodeSettings,
    linearize: LinearizeSettings,
    demosaic: DemosaicSettings,
    denoise: DenoiseSettings,
    lens: LensSettings,
    original_content_digest: [u8; 32],
    scale: u32,
    tier: SmartPreviewTier,
}
impl CameraLinearProxy {
    /// DNG camera channels are a source, not a cached Tessera RAW prefix.
    /// They may use either process family and remain exportable at their own size.
    pub fn from_dng(mut dng: raw_decode::lossy_dng::LossyDng) -> EngineResult<Self> {
        if dng.metadata.opcode_lists.iter().any(Option::is_some) {
            return Err(EngineError::Unsupported {
                what: "LinearRaw DNG contains unconsumed correction opcodes".into(),
            });
        }
        dng.metadata.baseline_exposure = dng.baseline_exposure;
        let planes = (0..3)
            .map(|c| dng.pixels.iter().map(|p| p[c]).collect())
            .collect();
        let pixels = Image::new(dng.width as u32, dng.height as u32, planes)?;
        let s = DevelopSettings::default();
        let correction = crate::resolve_lens(
            &pixels,
            &s.lens,
            Some(&dng.metadata),
            &LensContext::default(),
        )?;
        Ok(Self {
            external_dng: true,
            pixels,
            metadata: dng.metadata,
            correction,
            decode: s.decode,
            linearize: s.linearize,
            demosaic: s.demosaic,
            denoise: s.denoise,
            lens: s.lens,
            original_content_digest: [0; 32],
            scale: 1,
            tier: SmartPreviewTier::Detail2560,
        })
    }
    /// Use an absolute catalog orientation before masks/crop/Upright, overriding EXIF.
    pub fn with_catalog_orientation(mut self, orientation: u16) -> EngineResult<Self> {
        if !(1..=8).contains(&orientation) {
            return Err(EngineError::invalid("catalog orientation", "expected 1..8"));
        }
        self.metadata.catalog_orientation = Some(orientation);
        self.metadata.orientation = 1;
        Ok(self)
    }
    pub fn is_external_dng(&self) -> bool {
        self.external_dng
    }
    pub const GENERATOR_REVISION: u32 = 3;
    pub const MAX_EDGE: u32 = 2560;
    /// Caller supplies the verified original byte digest, not a path identity.
    /// Only Native revision 2 and raw denoise Off are admitted. A downstream
    /// recipe change does not mutate this prefix or its captured lens dependency.
    pub fn generate(
        image: &CfaImage,
        metadata: &RawMetadata,
        settings: &DevelopSettings,
        process: ProcessVersion,
        original_content_digest: [u8; 32],
        context: &LensContext<'_>,
    ) -> EngineResult<Self> {
        Self::generate_with_tier(
            image,
            metadata,
            settings,
            process,
            original_content_digest,
            context,
            SmartPreviewTier::Detail2560,
        )
    }

    /// Same immutable pre-edit boundary with an explicit spatial tier. Reduction
    /// uses complete demosaiced camera-linear pixels, before calibration and WB.
    pub fn generate_with_tier(
        image: &CfaImage,
        metadata: &RawMetadata,
        settings: &DevelopSettings,
        process: ProcessVersion,
        original_content_digest: [u8; 32],
        context: &LensContext<'_>,
        tier: SmartPreviewTier,
    ) -> EngineResult<Self> {
        if process.family != ProcessFamily::Native || process.revision != 2 {
            return Err(required("Native revision 2 required"));
        }
        if !matches!(settings.denoise.method, DenoiseMethod::Off) {
            return Err(required("raw denoise must be Off"));
        }
        // HDR/headroom select the host's presentation, not the immutable RAW
        // prefix. Validate all other controls strictly without mutating the
        // caller's recipe or weakening the legacy SDR renderer's validator.
        let mut prefix_settings = settings.clone();
        prefix_settings.output.hdr = false;
        prefix_settings.output.hdr_headroom_stops = 0.;
        crate::validate_settings(&prefix_settings)?;
        // Late sensor-coordinate operations cannot be replayed on crop-first
        // pixels without explicit original-sensor bin-centre transforms.
        let embedded = crate::embedded_lens::Embedded::parse(metadata)?;
        let use_embedded = matches!(
            settings.lens.profile,
            LensProfileSource::Auto | LensProfileSource::Embedded
        );
        if use_embedded && !embedded.stages[2].is_empty() {
            return Err(required(
                "OpcodeList3 sensor-coordinate corrections require original",
            ));
        }
        let (camera, correction, _, _) =
            crate::render::camera_linear_prefix(settings, image, metadata, context, None, None)?;
        let scale = metadata.default_crop[2]
            .max(metadata.default_crop[3])
            .div_ceil(tier.max_edge())
            .max(1);
        let pixels = camera.downsample_crop(metadata.default_crop, scale)?;
        // Image::new enforces finite payload even when an upstream operator
        // overflowed; retain signed/HDR values rather than clamp/quantize them.
        let pixels = Image::new(pixels.width(), pixels.height(), pixels.planes().to_vec())?;
        Ok(Self {
            external_dng: false,
            pixels,
            metadata: metadata.clone(),
            correction,
            decode: settings.decode.clone(),
            linearize: settings.linearize.clone(),
            demosaic: settings.demosaic.clone(),
            denoise: settings.denoise.clone(),
            lens: settings.lens.clone(),
            original_content_digest,
            scale,
            tier,
        })
    }
    /// Portable unbaked tail for camera-linear L0 rendering. Captured sensor,
    /// demosaic and lateral CA corrections are never replayed or re-estimated.
    /// None means the caller must retain the scalar camera-linear renderer.
    pub fn resident_tail_plan(
        &self,
        settings: &DevelopSettings,
    ) -> EngineResult<Option<crate::LensPlan>> {
        self.validate_prefix(settings)?;
        if self.external_dng || self.metadata.catalog_orientation.is_some() {
            return Ok(None);
        } // explicit CPU fallback: resolve DNG optics per recipe
        self.correction.camera_linear_tail_plan(
            settings,
            &self.metadata,
            [self.pixels.width(), self.pixels.height()],
        )
    }
    pub fn pixels(&self) -> &Image {
        &self.pixels
    }
    pub fn original_metadata(&self) -> &RawMetadata {
        &self.metadata
    }
    pub fn original_content_digest(&self) -> [u8; 32] {
        self.original_content_digest
    }
    pub fn tier(&self) -> SmartPreviewTier {
        self.tier
    }
    pub fn scale(&self) -> u32 {
        self.scale
    }
    pub(crate) fn correction(&self) -> &ResolvedLens {
        &self.correction
    }
    pub fn validate_prefix(&self, s: &DevelopSettings) -> EngineResult<()> {
        if self.external_dng {
            if !matches!(s.denoise.method, DenoiseMethod::Off) {
                return Err(required("mosaic denoise is unavailable for LinearRaw DNG"));
            }
            return Ok(());
        }
        if self.decode != s.decode
            || self.linearize != s.linearize
            || self.demosaic != s.demosaic
            || self.denoise != s.denoise
            || self.lens != s.lens
        {
            return Err(required("baked prefix changed; regenerate from original"));
        }
        Ok(())
    }
}
fn required(reason: &str) -> EngineError {
    EngineError::Unsupported {
        what: format!("smart preview: original required: {reason}"),
    }
}
