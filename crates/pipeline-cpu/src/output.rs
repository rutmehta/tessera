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
    // Resolve (validate) before rendering, as before.
    let transform = context.resolve(settings)?;
    let pixels = render_output_linear_scaled(settings, source, scale)?;
    output_with_transforms(settings, pixels, transform, context, true)
}

/// [`render_managed_scaled`] without gamut warnings (file exports discard
/// them; the pixels are identical).
pub fn render_managed_scaled_pixels(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &mut OutputContext<'_>,
) -> EngineResult<image::Rgb32FImage> {
    let transform = context.resolve(settings)?;
    let pixels = render_output_linear_scaled(settings, source, scale)?;
    Ok(output_with_transforms(settings, pixels, transform, context, false)?.pixels)
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

/// [`render_managed_scaled`] with an already-resolved lens correction.
pub fn render_managed_scaled_resolved(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &mut OutputContext<'_>,
    resolved: &crate::ResolvedLens,
) -> EngineResult<ManagedOutput> {
    let transform = context.resolve(settings)?;
    let mut linear_settings = settings.clone();
    linear_settings.output.proof_profile = None;
    let rgb = crate::render_linear_scaled_resolved(&linear_settings, source, scale, resolved)?;
    output_with_transforms(settings, tone_map(rgb), transform, context, true)
}

/// Tone-mapped linear Rec.2020 floats, before output gamut mapping or encoding.
/// Allows enhancement in linear light before the final managed output stage.
pub fn render_output_linear_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<image::Rgb32FImage> {
    // Only the resolved proof control is consumed here. All other validation,
    // including rejecting unsupported HDR, remains in the reference renderer.
    let mut linear_settings = settings.clone();
    linear_settings.output.proof_profile = None;
    let rgb = crate::render_linear_scaled(&linear_settings, source, scale)?;
    Ok(tone_map(rgb))
}

fn tone_map(rgb: crate::Image) -> image::Rgb32FImage {
    let mut pixels = image::Rgb32FImage::new(rgb.width(), rgb.height());
    for (i, pixel) in pixels.pixels_mut().enumerate() {
        let v = std::array::from_fn(|c| rgb.planes()[c][i]);
        let y = crate::luminance(v);
        let toned = if y <= 0.0 {
            [0.0; 3]
        } else {
            v.map(|c| c * crate::sigmoid(y, SigmoidSettings::default()) / y)
        };
        *pixel = image::Rgb(toned);
    }
    pixels
}

/// Convert tone-mapped linear Rec.2020 to the selected output profile once.
pub fn output_managed_linear(
    settings: &DevelopSettings,
    pixels: image::Rgb32FImage,
    context: &mut OutputContext<'_>,
) -> EngineResult<ManagedOutput> {
    let transform = context.resolve(settings)?;
    output_with_transforms(settings, pixels, transform, context, true)
}

/// [`output_managed_linear`] without gamut warnings (file exports, print
/// and documents discard them; the pixels are identical).
pub fn output_managed_pixels(
    settings: &DevelopSettings,
    pixels: image::Rgb32FImage,
    context: &mut OutputContext<'_>,
) -> EngineResult<image::Rgb32FImage> {
    let transform = context.resolve(settings)?;
    Ok(output_with_transforms(settings, pixels, transform, context, false)?.pixels)
}

/// Rows per output-transform worker below which one thread does the frame.
const ROWS_PER_WORKER: usize = 64;

/// The per-pixel output transform over contiguous row bands, one band per
/// worker thread (ENG-10). A colour transform is not shareable between
/// threads, so every extra band resolves its own from the same context;
/// each pixel's arithmetic, and the row-major warning order, are unchanged,
/// so the result is identical to a single pass.
fn output_with_transforms(
    settings: &DevelopSettings,
    mut pixels: image::Rgb32FImage,
    first: Transform,
    context: &mut OutputContext<'_>,
    warnings: bool,
) -> EngineResult<ManagedOutput> {
    let width = pixels.width() as usize;
    let rows = pixels.height() as usize;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .min(rows / ROWS_PER_WORKER)
        .max(1);
    if workers == 1 || width == 0 {
        let gamut_warnings = output_band(settings, &mut pixels, &first, warnings);
        return Ok(ManagedOutput {
            pixels,
            gamut_warnings,
        });
    }
    let mut transforms = vec![first];
    for _ in 1..workers {
        transforms.push(context.resolve(settings)?);
    }
    let band = rows.div_ceil(workers) * width * 3;
    let bands: Vec<Vec<GamutWarning>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (*pixels)
            .chunks_mut(band)
            .zip(transforms)
            .map(|(samples, transform)| {
                scope.spawn(move || output_band(settings, samples, &transform, warnings))
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().expect("output transform worker panicked"))
            .collect()
    });
    Ok(ManagedOutput {
        pixels,
        gamut_warnings: bands.concat(),
    })
}

/// Interleaved RGB samples, in place; returns their gamut warnings in order
/// (none when `warnings` is false; the pixels do not depend on them).
fn output_band(
    settings: &DevelopSettings,
    samples: &mut [f32],
    transform: &Transform,
    warnings: bool,
) -> Vec<GamutWarning> {
    let mut gamut_warnings = Vec::with_capacity(if warnings { samples.len() / 3 } else { 0 });
    for pixel in samples.chunks_exact_mut(3) {
        let toned = [pixel[0], pixel[1], pixel[2]];
        if warnings {
            gamut_warnings.push(transform.gamut_warning(toned));
        }
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
        pixel.copy_from_slice(&encoded.map(|v| v.clamp(0.0, 1.0)));
    }
    gamut_warnings
}

fn color_error(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("output", e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::settings::GamutMapping;

    /// ENG-10: banded (multi-threaded) output equals one single-threaded
    /// pass bit for bit, warnings included, for both gamut mappings and
    /// colours far outside the target (the Perceptual chroma search); the
    /// warning-free variant gives the same pixels.
    #[test]
    fn eng10_banded_output_transform_equals_one_pass() {
        let (w, h) = (37u32, 5 * ROWS_PER_WORKER as u32 + 3);
        let pixels = image::Rgb32FImage::from_fn(w, h, |x, y| {
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            image::Rgb(match (x + y) % 3 {
                0 => [1.4 * fx, 0.02, 0.3 * fy],
                1 => [0.01, 0.9 * fy + 0.05, 0.02],
                _ => [fx * fy, 0.5 * fx, 1.2 * fy],
            })
        });
        let mut registry = Registry::new();
        let target = registry.builtin(Builtin::Srgb).unwrap();
        for mapping in [GamutMapping::Perceptual, GamutMapping::Clip] {
            let mut settings = DevelopSettings::default();
            settings.output.gamut_mapping = mapping;
            let mut context = OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            };
            let banded = output_managed_linear(&settings, pixels.clone(), &mut context).unwrap();
            let unwarned = output_managed_pixels(&settings, pixels.clone(), &mut context).unwrap();
            let transform = context.resolve(&settings).unwrap();
            let mut single = pixels.clone();
            let warnings = output_band(&settings, &mut single, &transform, true);
            assert_eq!(
                unwarned
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                single
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                "{mapping:?}: pixels without warnings"
            );
            assert_eq!(
                banded
                    .pixels
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                single
                    .as_raw()
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>(),
                "{mapping:?}"
            );
            assert_eq!(banded.gamut_warnings, warnings, "{mapping:?}");
            assert!(warnings.iter().any(|w| w.monitor), "fixture leaves sRGB");
        }
    }
}
