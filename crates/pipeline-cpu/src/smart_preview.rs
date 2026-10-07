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
    external_profile: Option<std::sync::Arc<[u8]>>,
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
            external_profile: None,
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
    /// Use an absolute catalog orientation as the display orientation,
    /// replacing (never composing with) EXIF. Edits stay in the sensor frame
    /// and callers orient output exactly as for an ordinary RAW (LR-8m).
    pub fn with_catalog_orientation(mut self, orientation: u16) -> EngineResult<Self> {
        if !(1..=8).contains(&orientation) {
            return Err(EngineError::invalid("catalog orientation", "expected 1..8"));
        }
        self.metadata.catalog_orientation = Some(orientation);
        self.metadata.orientation = orientation;
        Ok(self)
    }
    /// Bounded profile metadata captured with an external DNG's decoded pixels.
    /// Native generated previews never carry this external camera profile.
    pub fn with_embedded_profile(mut self, bytes: Option<Vec<u8>>) -> Self {
        if self.external_dng {
            self.external_profile = bytes.map(Into::into);
        }
        self
    }
    pub fn embedded_profile(&self) -> Option<&[u8]> {
        self.external_profile.as_deref()
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
            external_profile: None,
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
        // These operators need caller-owned scalar resources; never admit a
        // tail that silently plans them away before checking GPU capability.
        if settings.effects.lens_blur.is_some() || !settings.locals.retouch.is_empty() {
            return Ok(None);
        }
        // Not needed for correctness since LR-8m (orientation is display-only);
        // kept until the rotated-proxy GPU tail is admitted in a performance
        // follow-up (export/tests/lrcat_jxl.rs pins the decline).
        if self.metadata.catalog_orientation.is_some_and(|o| o != 1) {
            return Ok(None);
        }
        let planned = self.render_plan(settings, false).0;
        let correction = if self.external_dng {
            crate::resolve_lens(
                &self.working_rgb(&planned)?,
                &planned.lens,
                Some(&self.metadata),
                &LensContext::default(),
            )?
        } else {
            self.correction.clone()
        };
        correction.camera_linear_tail_plan(
            &planned,
            &self.metadata,
            [self.pixels.width(), self.pixels.height()],
        )
    }
    pub(crate) fn working_rgb(&self, settings: &DevelopSettings) -> EngineResult<Image> {
        let camera_xyz =
            crate::camera_to_xyz(engine_api::color::ColorMatrix3(std::array::from_fn(|r| {
                self.metadata.cam_xyz[r].map(f64::from)
            })))?;
        let profile = engine_api::color::WorkingSpace::LinearRec2020
            .to_xyz()
            .inverse()?
            * camera_xyz;
        let wb = crate::white_balance_matrix(
            &settings.white_balance,
            camera_xyz,
            self.metadata.as_shot_wb,
        )?;
        let mut out = self.pixels.clone();
        for coord in out.coords() {
            let mut tile = out.tile(coord, 0, 1)?;
            crate::apply_matrix(&mut tile, profile)?;
            crate::apply_matrix(&mut tile, wb)?;
            out.put(&tile)?;
        }
        Ok(out)
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
    /// Render-only adaptation for external mosaic-free Lightroom sources.
    /// The persisted recipe is never changed. Notes contain field names only.
    pub fn render_plan(
        &self,
        settings: &DevelopSettings,
        mask_hooks: bool,
    ) -> (DevelopSettings, Vec<&'static str>) {
        self.render_plan_with_resources(settings, mask_hooks, false, false)
    }

    /// Keep dependency-backed operators when the caller can actually execute
    /// them. Providers must validate their depth extent and renderer results;
    /// this planning step never infers availability from the proxy's format.
    pub fn render_plan_with_resources(
        &self,
        settings: &DevelopSettings,
        mask_hooks: bool,
        depth: bool,
        retouch: bool,
    ) -> (DevelopSettings, Vec<&'static str>) {
        let mut drawn = settings.clone();
        let mut notes = Vec::new();
        if !self.external_dng {
            return (drawn, notes);
        }
        let defaults = DevelopSettings::default();
        if drawn.white_balance.mode == engine_api::recipe::settings::WhiteBalanceMode::Auto {
            drawn.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::AsShot;
            notes.push("/white_balance/mode");
        }
        macro_rules! omit {
            ($field:ident, $name:literal) => {
                if drawn.$field != defaults.$field {
                    notes.push($name);
                    drawn.$field = defaults.$field.clone();
                }
            };
        }
        omit!(decode, "/decode");
        omit!(linearize, "/linearize");
        omit!(demosaic, "/demosaic");
        omit!(denoise, "/denoise");
        if drawn.camera_profile.look != defaults.camera_profile.look {
            notes.push("/camera_profile/look");
            drawn.camera_profile.look = defaults.camera_profile.look;
        }
        // Headroom only parameterizes HDR presentation. With HDR output off it
        // changes nothing the user sees, so it is dropped without a note.
        if drawn.output.hdr {
            notes.push("/output/hdr");
        }
        drawn.output.hdr = false;
        drawn.output.hdr_headroom_stops = 0.;
        if matches!(drawn.lens.profile, LensProfileSource::Database { .. }) {
            notes.push("/lens/profile");
            drawn.lens.profile = LensProfileSource::None;
        }
        if drawn.effects.lens_blur.is_some() && !depth {
            notes.push("/effects/lens_blur");
            drawn.effects.lens_blur = None;
        }
        if !drawn.locals.retouch.is_empty() && !retouch {
            notes.push("/locals/retouch");
            drawn.locals.retouch.clear();
        }
        for group in &mut drawn.locals.adjustments {
            if group.enabled
                && group
                    .components
                    .iter()
                    .flat_map(engine_api::recipe::MaskComponent::active_leaves)
                    .any(|c| {
                        use engine_api::recipe::MaskKind;
                        !matches!(
                            c.kind,
                            MaskKind::Linear { .. }
                                | MaskKind::Radial { .. }
                                | MaskKind::Brush { .. }
                                | MaskKind::LuminanceRange { .. }
                                | MaskKind::ColorRange { .. }
                        ) && !(mask_hooks && c.kind.is_ai())
                    })
            {
                group.enabled = false;
                if !notes.contains(&"/locals/adjustments") {
                    notes.push("/locals/adjustments");
                }
            }
        }
        (drawn, notes)
    }

    pub fn validate_prefix(&self, s: &DevelopSettings) -> EngineResult<()> {
        if self.external_dng {
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
