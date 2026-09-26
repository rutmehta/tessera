//! Frozen image-dependent controls. Call constructors again when analysis changes.
use super::{Adjustment, AutoMode};
use engine_api::{EngineError, EngineResult};

fn check_histogram(h: &[Vec<u64>; 3]) -> EngineResult<()> {
    if h.iter().any(|v| v.len() < 2 || v.len() != h[0].len()) {
        return Err(EngineError::invalid(
            "histogram",
            "three equal-length channels with at least two bins required",
        ));
    }
    Ok(())
}
fn endpoints(h: &[f64], clip: f64) -> (f32, f32) {
    let total = h.iter().sum::<f64>();
    if total == 0.0 {
        return (0.0, 1.0);
    }
    let cut = total * clip;
    let mut sum = 0.0;
    let mut lo = 0;
    for (i, &v) in h.iter().enumerate() {
        sum += v;
        if sum > cut {
            lo = i;
            break;
        }
    }
    sum = 0.0;
    let mut hi = h.len() - 1;
    for (i, &v) in h.iter().enumerate().rev() {
        sum += v;
        if sum > cut {
            hi = i;
            break;
        }
    }
    if hi <= lo {
        (0.0, 1.0)
    } else {
        (
            lo as f32 / (h.len() - 1) as f32,
            hi as f32 / (h.len() - 1) as f32,
        )
    }
}
fn lab_stats(pixels: &[[f32; 3]]) -> EngineResult<([f32; 3], [f32; 3])> {
    if pixels.is_empty() || pixels.iter().flatten().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid(
            "pixels",
            "nonempty finite RGB samples required",
        ));
    }
    let mut mean = [0.0_f64; 3];
    let mut m2 = [0.0_f64; 3];
    for (j, &p) in pixels.iter().enumerate() {
        let lab = super::color::lab(p);
        for i in 0..3 {
            let delta = lab[i] as f64 - mean[i];
            mean[i] += delta / (j + 1) as f64;
            m2[i] += delta * (lab[i] as f64 - mean[i]);
        }
    }
    Ok((
        mean.map(|v| v as f32),
        m2.map(|v| (v.max(0.0) / pixels.len() as f64).sqrt() as f32),
    ))
}
impl Adjustment {
    /// Freeze source statistics from a layer resolved in this snapshot's tree.
    /// Pixel layers and text proxies are supported; groups, fills and smart
    /// objects require explicit rendering and are rejected. Uses raw level-zero
    /// straight RGB, excluding zero-alpha pixels, with equal weight for all
    /// positive-alpha samples. Masks, opacity, effects, selection and visibility
    /// are not applied: this analyzes layer content, not its composite.
    /// Untagged (sRGB) documents only; tagged sources must be color-converted
    /// explicitly and passed to `match_color_from_pixels`. Target samples are
    /// caller-selected straight sRGB. Statistics remain frozen after edits.
    pub fn match_color_from_layer(
        doc: &crate::DocState,
        source_layer: engine_api::id::LayerId,
        target: &[[f32; 3]],
    ) -> EngineResult<Self> {
        if source_layer.is_root() {
            return Err(EngineError::invalid(
                "source_layer",
                "a real source layer is required",
            ));
        }
        let layer = doc
            .find(source_layer)
            .ok_or_else(|| EngineError::not_found("source_layer", source_layer.0))?;
        if doc.profile.is_some() {
            return Err(EngineError::invalid(
                "source_layer",
                "tagged source requires explicit sRGB conversion",
            ));
        }
        let raster = layer.raster().ok_or_else(|| {
            EngineError::invalid(
                "source_layer",
                "source must have its own raster; render groups and smart objects explicitly",
            )
        })?;
        if raster.channels() != 4 {
            return Err(EngineError::invalid(
                "source_layer",
                "straight RGBA raster required",
            ));
        }
        let mut source = Vec::new();
        for y in 0..raster.extent().height {
            for x in 0..raster.extent().width {
                let p = raster.pixel(x, y);
                if !p[3].is_finite() || p[3] < 0.0 {
                    return Err(EngineError::invalid(
                        "source_layer",
                        "finite nonnegative alpha required",
                    ));
                }
                if p[3] > 0.0 {
                    source.push([p[0], p[1], p[2]]);
                }
            }
        }
        Self::match_color_from_pixels(source_layer, &source, target)
    }

    /// Low-level conversion of source and destination RGB populations to frozen
    /// CIE Lab D65 statistics. The supplied ID is metadata only: this method
    /// cannot verify that samples belong to it. Prefer `match_color_from_layer`
    /// to resolve source identity in a document. Samples are straight sRGB;
    /// callers select/filter their own masked or transparent samples.
    pub fn match_color_from_pixels(
        source_layer: engine_api::id::LayerId,
        source: &[[f32; 3]],
        target: &[[f32; 3]],
    ) -> EngineResult<Self> {
        if source_layer.is_root() {
            return Err(EngineError::invalid(
                "source_layer",
                "a real source layer is required",
            ));
        }
        let (source_mean, source_std) = lab_stats(source)?;
        let (target_mean, target_std) = lab_stats(target)?;
        Ok(Self::MatchColor {
            source_layer: source_layer.0,
            source_mean,
            source_std,
            target_mean,
            target_std,
            luminance: 100.0,
            color_intensity: 100.0,
            fade: 0.0,
        })
    }

    /// Clip a fraction [0,0.5) from each tail. Tone stretches channels;
    /// Contrast uses a pooled histogram; Color additionally maps each stretched
    /// mean to neutral 0.5 with a per-channel gamma.
    pub fn auto_from_histogram(mode: AutoMode, h: &[Vec<u64>; 3], clip: f32) -> EngineResult<Self> {
        check_histogram(h)?;
        if !clip.is_finite() || !(0.0..0.5).contains(&clip) {
            return Err(EngineError::invalid(
                "clip",
                "expected a fraction in [0,0.5)",
            ));
        }
        let hs: [Vec<f64>; 3] = std::array::from_fn(|i| h[i].iter().map(|&v| v as f64).collect());
        let mut black = [0.0; 3];
        let mut white = [1.0; 3];
        let mut gamma = [1.0; 3];
        if mode == AutoMode::Contrast {
            let pooled: Vec<f64> = (0..h[0].len())
                .map(|i| hs[0][i] + hs[1][i] + hs[2][i])
                .collect();
            let (b, w) = endpoints(&pooled, clip as f64);
            black = [b; 3];
            white = [w; 3];
        } else {
            for i in 0..3 {
                (black[i], white[i]) = endpoints(&hs[i], clip as f64);
                if mode == AutoMode::Color {
                    let total = hs[i].iter().sum::<f64>();
                    let mean = hs[i]
                        .iter()
                        .enumerate()
                        .map(|(j, n)| {
                            n * ((j as f64 / (h[i].len() - 1) as f64 - black[i] as f64)
                                / (white[i] - black[i]) as f64)
                                .clamp(0.0, 1.0)
                        })
                        .sum::<f64>()
                        / total;
                    if mean > 0.0 && mean < 1.0 {
                        gamma[i] = (mean.ln() / 0.5_f64.ln()).clamp(0.1, 10.0) as f32;
                    }
                }
            }
        }
        Ok(Self::Auto {
            mode,
            black,
            white,
            gamma,
        })
    }

    /// CDF-min normalization, linear interpolation between bins. Empty/constant
    /// populations use identity, avoiding a divide-by-zero or a flat black image.
    pub fn equalize_from_histogram(h: &[Vec<u64>; 3]) -> EngineResult<Self> {
        check_histogram(h)?;
        let maps = std::array::from_fn(|i| {
            let total = h[i].iter().map(|&v| v as f64).sum::<f64>();
            let first = h[i].iter().find(|&&v| v > 0).copied().unwrap_or(0) as f64;
            let mut sum = 0.0;
            h[i].iter()
                .enumerate()
                .map(|(j, &v)| {
                    sum += v as f64;
                    if total <= first {
                        j as f32 / (h[i].len() - 1) as f32
                    } else {
                        ((sum - first) / (total - first)).clamp(0.0, 1.0) as f32
                    }
                })
                .collect()
        });
        Ok(Self::Equalize { maps })
    }
}
