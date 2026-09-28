//! In-memory Native2 Smart Preview boundary. This is not a persistence format.
use crate::{Image, LensContext, ResolvedLens};
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
/// type deliberately has no deserializer: persisted snapshots need a separately
/// reviewed bounded schema, payload digest and dependency identity contract.
#[derive(Clone, Debug)]
pub struct CameraLinearProxy {
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
}
impl CameraLinearProxy {
    pub const GENERATOR_REVISION: u32 = 1;
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
        if process.family != ProcessFamily::Native || process.revision != 2 {
            return Err(required("Native revision 2 required"));
        }
        if !matches!(settings.denoise.method, DenoiseMethod::Off) {
            return Err(required("raw denoise must be Off"));
        }
        crate::validate_settings(settings)?;
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
            .div_ceil(Self::MAX_EDGE)
            .max(1);
        let pixels = camera.downsample_crop(metadata.default_crop, scale)?;
        // Image::new enforces finite payload even when an upstream operator
        // overflowed; retain signed/HDR values rather than clamp/quantize them.
        let pixels = Image::new(pixels.width(), pixels.height(), pixels.planes().to_vec())?;
        Ok(Self {
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
        })
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
    pub fn scale(&self) -> u32 {
        self.scale
    }
    pub(crate) fn correction(&self) -> &ResolvedLens {
        &self.correction
    }
    pub fn validate_prefix(&self, s: &DevelopSettings) -> EngineResult<()> {
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
