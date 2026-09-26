//! Select and Mask: edge refinement and colour decontamination.

use engine_api::{EngineError, EngineResult};

use crate::filter::{box_mean, gaussian, guided_filter, signed_distance, sobel};
use crate::mask::{Image, Mask};

/// Global refinements (spec 02 §3 "Select and Mask").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RefineParams {
    /// Edge detection radius, pixels: the band around the edge where alpha
    /// is re-estimated from the image with a guided filter.
    pub radius: f32,
    /// Narrow the band where the image edge is crisp and widen it where it
    /// is soft (hair, fur).
    pub smart_radius: bool,
    /// Outline smoothing σ, pixels.
    pub smooth: f32,
    /// Feather σ, pixels.
    pub feather: f32,
    /// Contrast `0..=1` (1 = hard edge).
    pub contrast: f32,
    /// Shift edge, pixels (positive grows).
    pub shift_edge: f32,
    /// Guided-filter regularization.
    pub epsilon: f32,
}

impl Default for RefineParams {
    fn default() -> Self {
        Self {
            radius: 0.0,
            smart_radius: false,
            smooth: 0.0,
            feather: 0.0,
            contrast: 0.0,
            shift_edge: 0.0,
            epsilon: 1e-3,
        }
    }
}

/// Refines `mask` against `img`: smooth and shift the outline (signed
/// distance), re-estimate alpha in the edge band with a luminance-guided
/// filter, then feather and contrast.
pub fn refine_edge(mask: &Mask, img: &Image, p: &RefineParams) -> EngineResult<Mask> {
    if mask.width() != img.width || mask.height() != img.height {
        return Err(EngineError::invalid(
            "refine",
            "mask and image sizes differ",
        ));
    }
    let vals = [
        p.radius,
        p.smooth,
        p.feather,
        p.contrast,
        p.shift_edge,
        p.epsilon,
    ];
    if vals.iter().any(|v| !v.is_finite()) || p.radius < 0.0 || p.epsilon <= 0.0 {
        return Err(EngineError::invalid("refine", "invalid parameters"));
    }
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    let mut sdf = signed_distance(mask.data(), w, h);
    if p.smooth > 0.0 {
        sdf = gaussian(&sdf, w, h, p.smooth);
    }
    for s in &mut sdf {
        *s -= p.shift_edge;
    }
    let hard: Vec<f32> = sdf.iter().map(|s| (0.5 - s).clamp(0.0, 1.0)).collect();
    let mut a = hard.clone();
    if p.radius > 0.0 {
        let lum = img.luminance();
        let r = ((p.radius * 0.5).round() as usize).max(1);
        let q = guided_filter(&hard, &lum, w, h, r, p.epsilon);
        let band: Vec<f32> = if p.smart_radius {
            // Edge crispness from the absolute luminance slope: ≥ 0.2 per
            // pixel counts as a hard edge (band shrinks to a quarter).
            let g = box_mean(&sobel(&lum, w, h), w, h, 1);
            g.iter()
                .map(|v| p.radius * (1.0 - 0.75 * (v / 0.2).clamp(0.0, 1.0)))
                .collect()
        } else {
            vec![p.radius; w * h]
        };
        for i in 0..w * h {
            let t = sdf[i].abs() / band[i].max(1e-3);
            if t < 1.0 {
                // Fade back to the hard mask at the band's rim.
                let k = ((t - 0.7) / 0.3).clamp(0.0, 1.0);
                let k = k * k * (3.0 - 2.0 * k);
                a[i] = q[i].clamp(0.0, 1.0) * (1.0 - k) + hard[i] * k;
            }
        }
    }
    if p.feather > 0.0 {
        a = gaussian(&a, w, h, p.feather);
    }
    if p.contrast > 0.0 {
        let k = 1.0 / (1.0 - p.contrast.clamp(0.0, 1.0) * 0.99);
        a.iter_mut()
            .for_each(|v| *v = ((*v - 0.5) * k + 0.5).clamp(0.0, 1.0));
    }
    Mask::from_vec(mask.width(), mask.height(), a)
}

/// Decontaminate Colors (placeholder quality): in the partially selected
/// band, pulls each colour towards the mean colour of nearby fully selected
/// pixels, by `amount · (1 − α)`. A full matting-equation foreground
/// estimate is future work.
pub fn decontaminate(img: &Image, mask: &Mask, amount: f32, radius: usize) -> EngineResult<Image> {
    if mask.width() != img.width || mask.height() != img.height {
        return Err(EngineError::invalid(
            "decontaminate",
            "mask and image sizes differ",
        ));
    }
    let (w, h) = (img.width as usize, img.height as usize);
    let fg: Vec<f32> = mask
        .data()
        .iter()
        .map(|a| f32::from(u8::from(*a > 0.95)))
        .collect();
    let wsum = box_mean(&fg, w, h, radius.max(1));
    let mut out = img.clone();
    let mut sums = Vec::new();
    for c in 0..3 {
        let ch: Vec<f32> = img.data.iter().zip(&fg).map(|(p, f)| p[c] * f).collect();
        sums.push(box_mean(&ch, w, h, radius.max(1)));
    }
    for i in 0..w * h {
        let a = mask.data()[i];
        if a <= 0.0 || a >= 0.95 || wsum[i] <= 0.0 {
            continue;
        }
        let k = amount.clamp(0.0, 1.0) * (1.0 - a);
        for c in 0..3 {
            let f = sums[c][i] / wsum[i];
            out.data[i][c] += (f - out.data[i][c]) * k;
        }
    }
    Ok(out)
}
