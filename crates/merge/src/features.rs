//! Deterministic FAST-9 / BRIEF and normalized projective RANSAC.
// Indexed small-matrix algebra is clearer than nested mutable iterators here.
#![allow(clippy::needless_range_loop)]
use crate::{LinearImage, Result};
pub(crate) type H = [[f64; 3]; 3];
pub(crate) const ID: H = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
pub(crate) fn apply(h: H, x: f64, y: f64) -> [f64; 2] {
    let d = h[2][0] * x + h[2][1] * y + h[2][2];
    [
        (h[0][0] * x + h[0][1] * y + h[0][2]) / d,
        (h[1][0] * x + h[1][1] * y + h[1][2]) / d,
    ]
}
pub(crate) fn multiply(a: H, b: H) -> H {
    std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum()))
}
pub(crate) fn inverse(h: H) -> Option<H> {
    let mut c = [[0.; 3]; 3];
    for i in 0..3 {
        for j in 0..3 {
            c[j][i] = h[(i + 1) % 3][(j + 1) % 3] * h[(i + 2) % 3][(j + 2) % 3]
                - h[(i + 1) % 3][(j + 2) % 3] * h[(i + 2) % 3][(j + 1) % 3];
        }
    }
    let d = (0..3).map(|j| h[0][j] * c[j][0]).sum::<f64>();
    if !d.is_finite() || d.abs() < 1e-12 {
        return None;
    }
    for row in &mut c {
        for v in row {
            *v /= d;
        }
    }
    Some(c)
}
fn gray(p: [f32; 3]) -> f64 {
    (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.
}
fn rng(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}
struct Feature {
    p: [f64; 2],
    bits: [u64; 4],
}
fn detect(im: &LinearImage) -> Vec<Feature> {
    let (w, h) = (im.width, im.height);
    if w < 25 || h < 25 {
        return vec![];
    }
    let g: Vec<f64> = im.pixels.iter().map(|p| gray(*p)).collect();
    let ring: [(isize, isize); 16] = [
        (0, -3),
        (1, -3),
        (2, -2),
        (3, -1),
        (3, 0),
        (3, 1),
        (2, 2),
        (1, 3),
        (0, 3),
        (-1, 3),
        (-2, 2),
        (-3, 1),
        (-3, 0),
        (-3, -1),
        (-2, -2),
        (-1, -3),
    ];
    let mut scores = vec![0.; w * h];
    let lo = g.iter().copied().fold(f64::INFINITY, f64::min);
    let hi = g.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let threshold = ((hi - lo) * 0.035).max(0.001);
    for y in 12..h - 12 {
        for x in 12..w - 12 {
            let c = g[y * w + x];
            let d: Vec<f64> = ring
                .iter()
                .map(|(dx, dy)| {
                    g[((y as isize + dy) as usize) * w + (x as isize + dx) as usize] - c
                })
                .collect();
            let mut score: f64 = 0.;
            for start in 0..16 {
                for sign in [-1., 1.] {
                    let s = (0..9)
                        .map(|k| sign * d[(start + k) % 16])
                        .fold(f64::INFINITY, f64::min);
                    score = score.max(s);
                }
            }
            if score > threshold {
                scores[y * w + x] = score;
            }
        }
    }
    let mut points = Vec::new();
    for y in 12..h - 12 {
        for x in 12..w - 12 {
            let s = scores[y * w + x];
            if s > 0.
                && (y - 1..=y + 1).all(|yy| (x - 1..=x + 1).all(|xx| scores[yy * w + xx] <= s))
            {
                points.push((s, x, y));
            }
        }
    }
    points.sort_by(|a, b| b.0.total_cmp(&a.0));
    points.truncate(2400);
    let mut seed = 0x987654321u64;
    let pairs: Vec<(isize, isize, isize, isize)> = (0..256)
        .map(|_| {
            (
                (rng(&mut seed) % 19) as isize - 9,
                (rng(&mut seed) % 19) as isize - 9,
                (rng(&mut seed) % 19) as isize - 9,
                (rng(&mut seed) % 19) as isize - 9,
            )
        })
        .collect();
    points
        .into_iter()
        .map(|(_, x, y)| {
            let mut bits = [0; 4];
            for (i, (ax, ay, bx, by)) in pairs.iter().enumerate() {
                let a = g[(y as isize + ay) as usize * w + (x as isize + ax) as usize];
                let b = g[(y as isize + by) as usize * w + (x as isize + bx) as usize];
                if a < b {
                    bits[i / 64] |= 1 << (i % 64);
                }
            }
            Feature {
                p: [x as f64, y as f64],
                bits,
            }
        })
        .collect()
}
fn distance(a: &Feature, b: &Feature) -> u32 {
    a.bits
        .iter()
        .zip(b.bits)
        .map(|(x, y)| (x ^ y).count_ones())
        .sum()
}
pub(crate) fn solve(mut a: [[f64; 8]; 8], mut b: [f64; 8]) -> Option<[f64; 8]> {
    for k in 0..8 {
        let p = (k..8).max_by(|i, j| a[*i][k].abs().total_cmp(&a[*j][k].abs()))?;
        if a[p][k].abs() < 1e-12 {
            return None;
        }
        a.swap(k, p);
        b.swap(k, p);
        let d = a[k][k];
        for j in k..8 {
            a[k][j] /= d;
        }
        b[k] /= d;
        for i in 0..8 {
            if i != k {
                let d = a[i][k];
                for j in k..8 {
                    a[i][j] -= d * a[k][j];
                }
                b[i] -= d * b[k];
            }
        }
    }
    Some(b)
}
fn fit(pairs: &[([f64; 2], [f64; 2])]) -> Option<H> {
    // Normalize coordinates for stable normal equations, including large sensors.
    let mut center = [[0.; 2]; 2];
    for (a, b) in pairs {
        for k in 0..2 {
            center[0][k] += a[k] / pairs.len() as f64;
            center[1][k] += b[k] / pairs.len() as f64;
        }
    }
    let mut scale = [0.; 2];
    for (a, b) in pairs {
        for k in 0..2 {
            scale[0] += (a[k] - center[0][k]).powi(2);
            scale[1] += (b[k] - center[1][k]).powi(2);
        }
    }
    for s in &mut scale {
        *s = (*s / pairs.len() as f64).sqrt();
        if *s < 1e-6 {
            return None;
        }
    }
    let mut aa = [[0.; 8]; 8];
    let mut bb = [0.; 8];
    for (a, b) in pairs {
        let x = (a[0] - center[0][0]) / scale[0];
        let y = (a[1] - center[0][1]) / scale[0];
        let u = (b[0] - center[1][0]) / scale[1];
        let v = (b[1] - center[1][1]) / scale[1];
        for (r, t) in [
            ([x, y, 1., 0., 0., 0., -u * x, -u * y], u),
            ([0., 0., 0., x, y, 1., -v * x, -v * y], v),
        ] {
            for i in 0..8 {
                bb[i] += r[i] * t;
                for j in 0..8 {
                    aa[i][j] += r[i] * r[j];
                }
            }
        }
    }
    let q = solve(aa, bb)?;
    let h = [[q[0], q[1], q[2]], [q[3], q[4], q[5]], [q[6], q[7], 1.]];
    let a = [
        [1. / scale[0], 0., -center[0][0] / scale[0]],
        [0., 1. / scale[0], -center[0][1] / scale[0]],
        [0., 0., 1.],
    ];
    let b = [
        [scale[1], 0., center[1][0]],
        [0., scale[1], center[1][1]],
        [0., 0., 1.],
    ];
    let mut h = multiply(b, multiply(h, a));
    let d = h[2][2];
    for row in &mut h {
        for v in row {
            *v /= d;
        }
    }
    Some(h)
}
pub(crate) fn ransac(pairs: &[([f64; 2], [f64; 2])]) -> Result<H> {
    if pairs.len() < 8 {
        return Err("insufficient feature matches / disconnected panorama".into());
    }
    let mut seed = 39127;
    let mut best = Vec::new();
    for _ in 0..3000 {
        let mut ids = Vec::new();
        while ids.len() < 4 {
            let i = rng(&mut seed) as usize % pairs.len();
            if !ids.contains(&i) {
                ids.push(i);
            }
        }
        let sample: Vec<_> = ids.iter().map(|i| pairs[*i]).collect();
        if let Some(h) = fit(&sample) {
            if inverse(h).is_none() {
                continue;
            }
            let inliers: Vec<_> = pairs
                .iter()
                .copied()
                .filter(|(a, b)| {
                    let p = apply(h, a[0], a[1]);
                    (p[0] - b[0]).powi(2) + (p[1] - b[1]).powi(2) < 4.
                })
                .collect();
            if inliers.len() > best.len() {
                best = inliers;
            }
        }
    }
    if best.len() < 8 || best.len() * 5 < pairs.len() {
        return Err("degenerate or disconnected panorama".into());
    }
    fit(&best).ok_or_else(|| "degenerate homography".into())
}
pub(crate) fn register(source: &LinearImage, target: &LinearImage) -> Result<H> {
    let a = detect(source);
    let b = detect(target);
    let mut pairs = Vec::new();
    for (i, f) in a.iter().enumerate() {
        let mut order: Vec<_> = b
            .iter()
            .enumerate()
            .map(|(j, g)| (distance(f, g), j))
            .collect();
        order.sort_unstable();
        if order.len() < 2 || order[0].0 > 70 || order[0].0 * 100 >= order[1].0 * 78 {
            continue;
        }
        let j = order[0].1;
        let reverse = a
            .iter()
            .enumerate()
            .min_by_key(|(_, g)| distance(g, &b[j]))
            .map(|(k, _)| k);
        if reverse == Some(i) {
            pairs.push((f.p, b[j].p));
        }
    }
    let h = ransac(&pairs)?;
    Ok(refine(source, target, h))
}
fn refine(src: &LinearImage, dst: &LinearImage, mut h: H) -> H {
    // Robust direct alignment after RANSAC removes integer FAST localization bias.
    let s = src.width.max(src.height) as f64;
    let mut q = [
        h[0][0],
        h[0][1],
        h[0][2] / s,
        h[1][0],
        h[1][1],
        h[1][2] / s,
        h[2][0] * s,
        h[2][1] * s,
    ];
    for _ in 0..20 {
        let mut aa = [[0.; 8]; 8];
        let mut bb = [0.; 8];
        let mut count = 0;
        for y in (3..src.height.saturating_sub(3)).step_by(3) {
            for x in (3..src.width.saturating_sub(3)).step_by(3) {
                let xn = x as f64 / s;
                let yn = y as f64 / s;
                let d = q[6] * xn + q[7] * yn + 1.;
                let u = s * (q[0] * xn + q[1] * yn + q[2]) / d;
                let v = s * (q[3] * xn + q[4] * yn + q[5]) / d;
                let Some(c) = dst.sample(u, v) else {
                    continue;
                };
                let (Some(l), Some(r), Some(t), Some(b)) = (
                    dst.sample(u - 0.5, v),
                    dst.sample(u + 0.5, v),
                    dst.sample(u, v - 0.5),
                    dst.sample(u, v + 0.5),
                ) else {
                    continue;
                };
                let residual = gray(c) - gray(src.pixels[y * src.width + x]);
                let gx = gray(r) - gray(l);
                let gy = gray(b) - gray(t);
                let j = [
                    gx * s * xn / d,
                    gx * s * yn / d,
                    gx * s / d,
                    gy * s * xn / d,
                    gy * s * yn / d,
                    gy * s / d,
                    -(gx * u + gy * v) * xn / d,
                    -(gx * u + gy * v) * yn / d,
                ];
                let weight = 1. / (1. + (residual / 0.05).powi(2));
                for i in 0..8 {
                    bb[i] -= weight * j[i] * residual;
                    for k in 0..8 {
                        aa[i][k] += weight * j[i] * j[k];
                    }
                }
                count += 1;
            }
        }
        if count < 64 {
            break;
        }
        let Some(delta) = solve(aa, bb) else {
            break;
        };
        if delta.iter().any(|v| !v.is_finite() || v.abs() > 0.1) {
            break;
        }
        for i in 0..8 {
            q[i] += delta[i];
        }
        if delta.iter().map(|v| v * v).sum::<f64>() < 1e-14 {
            break;
        }
    }
    h = [
        [q[0], q[1], q[2] * s],
        [q[3], q[4], q[5] * s],
        [q[6] / s, q[7] / s, 1.],
    ];
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ransac_rejects_sixty_percent_outliers_and_collinear_points() {
        let truth = [
            [1.02, 0.015, 80.],
            [-0.008, 0.99, 4.],
            [0.00015, -0.00007, 1.],
        ];
        let mut pairs = Vec::new();
        let mut seed = 77;
        for i in 0..100 {
            let a = [(rng(&mut seed) % 400) as f64, (rng(&mut seed) % 250) as f64];
            let b = if i < 40 {
                apply(truth, a[0], a[1])
            } else {
                [(rng(&mut seed) % 400) as f64, (rng(&mut seed) % 250) as f64]
            };
            pairs.push((a, b));
        }
        let h = ransac(&pairs).unwrap();
        for (a, b) in &pairs[..40] {
            let p = apply(h, a[0], a[1]);
            assert!((p[0] - b[0]).hypot(p[1] - b[1]) < 0.01);
        }
        let line: Vec<_> = (0..30)
            .map(|i| ([i as f64, 0.], [i as f64 + 5., 0.]))
            .collect();
        assert!(ransac(&line).is_err());
    }
}
