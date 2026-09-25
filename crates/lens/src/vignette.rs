use crate::{Estimate, GrayImage};
/// Robust radial illumination regression over low-gradient pixels.
/// This is a scene-dependent suggestion, not proof of lens falloff. Values describe
/// illumination (1 + v0*r² + v1*r⁴ + v2*r⁶); correction is its reciprocal.
pub fn estimate_vignette(image: &GrayImage) -> Option<Estimate<[f64; 3]>> {
    let mut samples = Vec::new();
    let stride = (image.width.min(image.height) / 128).max(1);
    for y in (1..image.height - 1).step_by(stride) {
        for x in (1..image.width - 1).step_by(stride) {
            let p = image.point(x, y);
            let r = p[0] * p[0] + p[1] * p[1];
            let v = image.data[y * image.width + x];
            let g = image.gradient(x, y);
            if v > 1e-8 {
                samples.push((r, v, g[0].hypot(g[1]) / v));
            }
        }
    }
    if samples.len() < 32 {
        return None;
    }
    let mut gradients: Vec<_> = samples.iter().map(|s| s.2).collect();
    gradients.sort_by(f64::total_cmp);
    let threshold = gradients[gradients.len() * 3 / 4].max(1e-5);
    samples.retain(|s| s.2 <= threshold);
    let mut weights = vec![1.; samples.len()];
    let mut coeff = [0.; 4];
    for _ in 0..4 {
        let mut a = [[0.; 4]; 4];
        let mut b = [0.; 4];
        for ((r, v, _), w) in samples.iter().zip(&weights) {
            let t = [1., *r, r * r, r * r * r];
            for i in 0..4 {
                b[i] += w * t[i] * v;
                for j in 0..4 {
                    a[i][j] += w * t[i] * t[j]
                }
            }
        }
        coeff = solve(a, b)?;
        let mut errors: Vec<_> = samples
            .iter()
            .map(|(r, v, _)| {
                (v - (coeff[0] + r * (coeff[1] + r * (coeff[2] + r * coeff[3])))).abs()
            })
            .collect();
        errors.sort_by(f64::total_cmp);
        let sigma = (errors[errors.len() / 2] * 1.4826).max(1e-8);
        for (w, (r, v, _)) in weights.iter_mut().zip(&samples) {
            let e = (v - (coeff[0] + r * (coeff[1] + r * (coeff[2] + r * coeff[3])))).abs();
            *w = (1.5 * sigma / e.max(1e-12)).min(1.);
        }
    }
    if coeff[0] <= 1e-8 {
        return None;
    }
    let value = [
        coeff[1] / coeff[0],
        coeff[2] / coeff[0],
        coeff[3] / coeff[0],
    ];
    for i in 0..=20 {
        let r = i as f64 * 0.1;
        if 1. + r * (value[0] + r * (value[1] + r * value[2])) <= 0.05 {
            return None;
        }
    }
    let residual = samples
        .iter()
        .map(|(r, v, _)| {
            ((v - (coeff[0] + r * (coeff[1] + r * (coeff[2] + r * coeff[3])))) / coeff[0]).powi(2)
        })
        .sum::<f64>()
        / samples.len() as f64;
    Some(Estimate {
        value,
        residual,
        confidence: (1. - residual.sqrt() * 10.).clamp(0., 1.),
    })
}
pub(crate) fn solve<const N: usize>(mut a: [[f64; N]; N], mut b: [f64; N]) -> Option<[f64; N]> {
    for i in 0..N {
        let pivot = (i..N).max_by(|j, k| a[*j][i].abs().total_cmp(&a[*k][i].abs()))?;
        if a[pivot][i].abs() < 1e-12 {
            return None;
        }
        a.swap(i, pivot);
        b.swap(i, pivot);
        let d = a[i][i];
        for value in a[i].iter_mut().skip(i) {
            *value /= d;
        }
        b[i] /= d;
        for k in 0..N {
            if k == i {
                continue;
            }
            let d = a[k][i];
            let pivot_row = a[i];
            for (value, pivot) in a[k].iter_mut().zip(pivot_row).skip(i) {
                *value -= d * pivot;
            }
            b[k] -= d * b[i];
        }
    }
    if b.iter().all(|x| x.is_finite()) {
        Some(b)
    } else {
        None
    }
}
