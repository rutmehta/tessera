//! Explicit managed SDR output. The legacy `render` path remains golden-stable.
use crate::{RenderSource, SigmoidSettings};
use color_mgmt::{Builtin, GamutWarning, Profile, Registry, Transform, TransformOptions};
use engine_api::{EngineError, EngineResult, recipe::DevelopSettings};

/// A selected monitor profile or an RGB document/export profile, never inferred
/// from machine-global state. An export target must not bake in a soft proof.
#[derive(Clone, Copy)]
pub enum OutputTarget<'a> {
    Display(&'a Profile),
    Export(&'a Profile),
}

/// Caller owns the registry and profile lifetimes. Select OS display profiles
/// with `Registry::display_profiles`, or load a document/printer ICC explicitly.
pub struct OutputContext<'a> {
    pub registry: &'a mut Registry,
    pub target: OutputTarget<'a>,
    /// Resolved profile corresponding to OutputSettings::proof_profile.
    pub proof: Option<&'a Profile>,
    pub options: TransformOptions,
}

pub struct ManagedOutput {
    /// Target-encoded SDR float RGB; no intermediate sRGB8 quantization.
    pub pixels: image::Rgb32FImage,
    /// Row-major flags, evaluated before output gamut mapping, never painted in.
    pub gamut_warnings: Vec<GamutWarning>,
}

/// Full CPU render through Geometry followed by ICC-managed Output. Scale is
/// applied in scene-linear light. The output uses the existing sigmoid tone
/// mapper, then the shared CMM from linear Rec.2020 directly to the target.
pub fn render_managed_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &mut OutputContext<'_>,
) -> EngineResult<ManagedOutput> {
    let transform = context.resolve(settings)?;
    render_with_transform(settings, source, scale, &transform)
}

impl OutputContext<'_> {
    /// Resolve and validate output/proof identity for either CPU or GPU output.
    pub fn resolve(&mut self, settings: &DevelopSettings) -> EngineResult<Transform> {
        let context = self;
        let target = match context.target {
            OutputTarget::Display(p) | OutputTarget::Export(p) => p,
        };
        let working = context
            .registry
            .builtin(Builtin::LinearRec2020)
            .map_err(color_error)?;
        let proof = match (settings.output.proof_profile, context.proof) {
            (Some(handle), Some(profile))
                if handle
                    == engine_api::color::IccProfileHandle::from_profile_bytes(
                        profile.icc_bytes(),
                    ) =>
            {
                Some(profile)
            }
            (None, None) => None,
            _ => {
                return Err(color_error(
                    "missing, unexpected or mismatched proof profile",
                ));
            }
        };
        if proof.is_some() && matches!(context.target, OutputTarget::Export(_)) {
            return Err(color_error(
                "soft proof is display-only; clear proof state for export",
            ));
        }
        let transform = match proof {
            Some(proof) => Transform::proof(&working, target, proof, context.options),
            None => Transform::new(&working, target, context.options),
        }
        .map_err(color_error)?;
        Ok(transform)
    }
}

fn render_with_transform(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    transform: &Transform,
) -> EngineResult<ManagedOutput> {
    // Only the resolved proof control is consumed here. All other validation,
    // including rejecting unsupported HDR, remains in the reference renderer.
    let mut linear_settings = settings.clone();
    linear_settings.output.proof_profile = None;
    let rgb = crate::render_linear_scaled(&linear_settings, source, scale)?;
    let mut pixels = image::Rgb32FImage::new(rgb.width(), rgb.height());
    let mut gamut_warnings = Vec::with_capacity(pixels.as_raw().len() / 3);
    for (i, pixel) in pixels.pixels_mut().enumerate() {
        let v = std::array::from_fn(|c| rgb.planes()[c][i]);
        let y = crate::luminance(v);
        let toned = if y <= 0.0 {
            [0.0; 3]
        } else {
            v.map(|c| c * crate::sigmoid(y, SigmoidSettings::default()) / y)
        };
        gamut_warnings.push(transform.gamut_warning(toned));
        let mut encoded = transform.apply(toned);
        if settings.output.gamut_mapping == engine_api::recipe::settings::GamutMapping::Perceptual
            && encoded.iter().any(|v| !(0.0..=1.0).contains(v))
        {
            // Compress along a constant working-luminance ray, testing the
            // actual ICC destination boundary (not an assumed sRGB gamut).
            let grey = crate::luminance(toned).clamp(0.0, 1.0);
            let mut lo = 0.0;
            let mut hi = 1.0;
            encoded = transform.apply([grey; 3]);
            for _ in 0..18 {
                let chroma = (lo + hi) * 0.5;
                let candidate = transform.apply(toned.map(|v| grey + chroma * (v - grey)));
                if candidate.iter().all(|v| (0.0..=1.0).contains(v)) {
                    lo = chroma;
                    encoded = candidate;
                } else {
                    hi = chroma;
                }
            }
        }
        *pixel = image::Rgb(encoded.map(|v| v.clamp(0.0, 1.0)));
    }
    Ok(ManagedOutput {
        pixels,
        gamut_warnings,
    })
}

fn color_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("output", e.to_string())
}
