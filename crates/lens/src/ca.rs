use crate::{Estimate, RgbImage};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromaticAberration {
    pub red: [f64; 3],
    pub blue: [f64; 3],
}
/// Fit per-channel radial edge alignment (scale and r² term) relative to green.
/// Requires shared, unsaturated edge texture; max_shift is a fractional radial bound.
pub fn estimate_ca(image: &RgbImage, max_shift: f64) -> Option<Estimate<ChromaticAberration>> {
    if !max_shift.is_finite() || max_shift <= 0. || max_shift > 0.2 {
        return None;
    }
    let green = &image.channels[1];
    let w = green.width;
    let h = green.height;
    let gradients: Vec<Vec<[f64; 2]>> = image
        .channels
        .iter()
        .map(|im| {
            let mut g = vec![[0.; 2]; w * h];
            for y in 1..h - 1 {
                for x in 1..w - 1 {
                    g[y * w + x] = im.gradient(x, y)
                }
            }
            g
        })
        .collect();
    let mut samples = Vec::new();
    for y in (2..h - 2).step_by(2) {
        for x in (2..w - 2).step_by(2) {
            let p = green.point(x, y);
            let g = gradients[1][y * w + x];
            if p[0].abs() < 0.8
                && p[1].abs() < 0.8
                && p[0] * p[0] + p[1] * p[1] > 0.05
                && g[0].hypot(g[1]) > 1e-5
            {
                samples.push((p, g));
            }
        }
    }
    if samples.len() < 24 {
        return None;
    }
    let cost = |channel: usize, s: f64, k: f64| {
        let mut dot = 0.;
        let mut aa = 0.;
        let mut bb = 0.;
        for (p, g) in &samples {
            let r = p[0] * p[0] + p[1] * p[1];
            let scale = s + k * r;
            let x = (p[0] * scale + 1.) * 0.5 * (w - 1) as f64;
            let y = (p[1] * scale + 1.) * 0.5 * (h - 1) as f64;
            if x < 1. || y < 1. || x >= (w - 2) as f64 || y >= (h - 2) as f64 {
                return f64::INFINITY;
            }
            let ix = x as usize;
            let iy = y as usize;
            let fx = x - ix as f64;
            let fy = y - iy as f64;
            let mut a = [0.; 2];
            for (j, t) in [
                (iy * w + ix, (1. - fx) * (1. - fy)),
                (iy * w + ix + 1, fx * (1. - fy)),
                ((iy + 1) * w + ix, (1. - fx) * fy),
                ((iy + 1) * w + ix + 1, fx * fy),
            ] {
                a[0] += gradients[channel][j][0] * t;
                a[1] += gradients[channel][j][1] * t;
            }
            dot += a[0] * g[0] + a[1] * g[1];
            aa += a[0] * a[0] + a[1] * a[1];
            bb += g[0] * g[0] + g[1] * g[1];
        }
        if aa * bb < 1e-16 {
            return f64::INFINITY;
        }
        1. - dot / (aa * bb).sqrt()
    };
    let mut values = [[1., 0., 0.]; 2];
    let mut residual = 0.;
    let mut baseline = 0.;
    for (index, c) in [0, 2].iter().enumerate() {
        let mut s = 1.;
        let mut k = 0.;
        for _ in 0..8 {
            s = crate::calibration::minimize(|s| cost(*c, s, k), [1. - max_shift, 1. + max_shift])
                .0;
            k = crate::calibration::minimize(|k| cost(*c, s, k), [-max_shift, max_shift]).0;
        }
        values[index] = [s, k, 0.];
        residual += cost(*c, s, k);
        baseline += cost(*c, 1., 0.);
    }
    if !residual.is_finite() {
        return None;
    }
    Some(Estimate {
        value: ChromaticAberration {
            red: values[0],
            blue: values[1],
        },
        residual: residual * 0.5,
        confidence: if baseline > 1e-10 {
            (1. - residual / baseline).clamp(0., 1.)
        } else {
            1.
        },
    })
}
