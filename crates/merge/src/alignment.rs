//! Downsampled luminance phase correlation, small-angle search and tile residuals.
//! Reference-to-source maps; limited to +/-3 degrees and 25% image translation.
use crate::{LinearImage, Result};
use rustfft::{FftPlanner, num_complex::Complex};

#[derive(Clone, Debug)]
pub struct Alignment {
    pub translation: [f64; 2],
    pub rotation_radians: f64,
    /// Residual translations at tile centers, interpolated continuously.
    pub tile_offsets: Vec<[f64; 2]>,
    pub tile_size: usize,
    width: usize,
    height: usize,
}
impl Alignment {
    pub fn identity(image: &LinearImage) -> Self {
        Self {
            translation: [0.; 2],
            rotation_radians: 0.,
            tile_offsets: Vec::new(),
            tile_size: 32,
            width: image.width,
            height: image.height,
        }
    }
    fn global(&self, x: f64, y: f64) -> [f64; 2] {
        let cx = (self.width as f64 - 1.) / 2.;
        let cy = (self.height as f64 - 1.) / 2.;
        let (s, c) = self.rotation_radians.sin_cos();
        [
            c * (x - cx) - s * (y - cy) + cx + self.translation[0],
            s * (x - cx) + c * (y - cy) + cy + self.translation[1],
        ]
    }
    pub fn map(&self, x: f64, y: f64) -> [f64; 2] {
        let mut p = self.global(x, y);
        if self.tile_offsets.is_empty() {
            return p;
        }
        let nx = self.width.div_ceil(self.tile_size);
        let ny = self.height.div_ceil(self.tile_size);
        let tx = (x / self.tile_size as f64 - 0.5).clamp(0., (nx - 1) as f64);
        let ty = (y / self.tile_size as f64 - 0.5).clamp(0., (ny - 1) as f64);
        let (ix, iy) = (tx as usize, ty as usize);
        let (fx, fy) = (tx - ix as f64, ty - iy as f64);
        for (dx, wx) in [(0, 1. - fx), (1, fx)] {
            for (dy, wy) in [(0, 1. - fy), (1, fy)] {
                let t = self.tile_offsets[(iy + dy).min(ny - 1) * nx + (ix + dx).min(nx - 1)];
                p[0] += t[0] * wx * wy;
                p[1] += t[1] * wx * wy;
            }
        }
        p
    }
}
fn luminance(p: [f32; 3]) -> f64 {
    (p[0] as f64 + p[1] as f64 * 2. + p[2] as f64) / 4.
}
fn fft2(v: &mut [Complex<f64>], w: usize, h: usize, inverse: bool) {
    let mut planner = FftPlanner::new();
    let row = if inverse {
        planner.plan_fft_inverse(w)
    } else {
        planner.plan_fft_forward(w)
    };
    for r in v.chunks_mut(w) {
        row.process(r);
    }
    let col = if inverse {
        planner.plan_fft_inverse(h)
    } else {
        planner.plan_fft_forward(h)
    };
    let mut tmp = vec![Complex::default(); h];
    for x in 0..w {
        for y in 0..h {
            tmp[y] = v[y * w + x];
        }
        col.process(&mut tmp);
        for y in 0..h {
            v[y * w + x] = tmp[y];
        }
    }
}
fn plane(im: &LinearImage, t: &Alignment, w: usize, h: usize, step: f64) -> Vec<Complex<f64>> {
    let mut v = vec![Complex::default(); w * h];
    let mut sum = 0.;
    let mut count = 0;
    for y in 0..h {
        for x in 0..w {
            let p = t.global(x as f64 * step, y as f64 * step);
            if let Some(rgb) = im.sample(p[0], p[1]) {
                // Log removes multiplicative exposure; mean subtraction removes DC.
                let l = luminance(rgb).max(0.0001).ln();
                v[y * w + x].re = l;
                sum += l;
                count += 1;
            }
        }
    }
    let mean = sum / count.max(1) as f64;
    for y in 0..h {
        for x in 0..w {
            let p = t.global(x as f64 * step, y as f64 * step);
            let window = (std::f64::consts::PI * x as f64 / (w - 1).max(1) as f64)
                .sin()
                .powi(2)
                * (std::f64::consts::PI * y as f64 / (h - 1).max(1) as f64)
                    .sin()
                    .powi(2);
            v[y * w + x].re = if im.sample(p[0], p[1]).is_some() {
                (v[y * w + x].re - mean) * window
            } else {
                0.
            };
        }
    }
    fft2(&mut v, w, h, false);
    v
}
fn phase(a: &[Complex<f64>], mut b: Vec<Complex<f64>>, w: usize, h: usize) -> [f64; 2] {
    for (b, a) in b.iter_mut().zip(a) {
        let cross = *b * a.conj();
        let norm = cross.norm();
        *b = if norm > 1e-10 {
            cross / norm
        } else {
            Complex::default()
        };
    }
    fft2(&mut b, w, h, true);
    let signed = |i: usize, n: usize| {
        if i > n / 2 {
            i as f64 - n as f64
        } else {
            i as f64
        }
    };
    let peak = (0..b.len())
        .filter(|i| {
            signed(i % w, w).abs() <= w as f64 / 4. && signed(i / w, h).abs() <= h as f64 / 4.
        })
        .max_by(|&i, &j| b[i].re.total_cmp(&b[j].re))
        .unwrap_or(0);
    [signed(peak % w, w), signed(peak / w, h)]
}
// Robust exposure-invariant log residual. A small outlier fraction (motion)
// cannot dominate alignment; clipping and insufficient overlap are excluded.
fn score(
    a: &LinearImage,
    b: &LinearImage,
    t: &Alignment,
    ratio: f64,
    rect: [usize; 4],
    stride: usize,
) -> f64 {
    let mut errors = Vec::new();
    let mut available = 0;
    for y in (rect[1]..rect[3]).step_by(stride) {
        for x in (rect[0]..rect[2]).step_by(stride) {
            let av = luminance(a.pixels[y * a.width + x]);
            if !(0.01..0.97).contains(&av) {
                continue;
            }
            available += 1;
            let p = t.global(x as f64, y as f64);
            if let Some(rgb) = b.sample(p[0], p[1]) {
                let bv = luminance(rgb);
                if (0.01..0.97).contains(&bv) {
                    errors.push((bv / av / ratio).ln());
                }
            }
        }
    }
    if errors.len() < 16 || errors.len() * 3 < available {
        return f64::INFINITY;
    }
    errors.sort_by(f64::total_cmp);
    let median = errors[errors.len() / 2];
    // EXIF bias is a constant log offset, not geometric displacement.
    for e in &mut errors {
        *e = (*e - median).abs();
    }
    errors.sort_by(f64::total_cmp);
    let n = (errors.len() * 4 / 5).max(1);
    errors[..n].iter().map(|v| v * v).sum::<f64>() / n as f64
}
pub fn align(a: &LinearImage, b: &LinearImage, ratio: f64) -> Result<Alignment> {
    a.validate()?;
    b.validate()?;
    if a.width != b.width || a.height != b.height || !ratio.is_finite() || ratio <= 0. {
        return Err("incompatible alignment inputs".into());
    }
    let mut best = Alignment::identity(a);
    if a.width < 16 || a.height < 16 {
        return Ok(best);
    }
    let mean = a.pixels.iter().map(|p| luminance(*p)).sum::<f64>() / a.pixels.len() as f64;
    let variance = a
        .pixels
        .iter()
        .map(|p| (luminance(*p) - mean).powi(2))
        .sum::<f64>()
        / a.pixels.len() as f64;
    if variance < 1e-10 {
        return Ok(best);
    }
    let step = (a.width.max(a.height) as f64 / 128.).max(1.);
    let w = (a.width as f64 / step).floor() as usize;
    let h = (a.height as f64 / step).floor() as usize;
    if w < 8 || h < 8 {
        return Err("image too thin for two-dimensional phase alignment".into());
    }
    let spectrum = plane(a, &best, w, h, step);
    let rect = [0, 0, a.width, a.height];
    let stride = (a.width.max(a.height) / 100).max(1);
    let mut cost = score(a, b, &best, ratio, rect, stride);
    for angle in -12..=12 {
        let mut candidate = Alignment::identity(a);
        candidate.rotation_radians = (angle as f64 * 0.25).to_radians();
        let delta = phase(&spectrum, plane(b, &candidate, w, h, step), w, h);
        let (s, c) = candidate.rotation_radians.sin_cos();
        candidate.translation = [
            step * (c * delta[0] - s * delta[1]),
            step * (s * delta[0] + c * delta[1]),
        ];
        let value = score(a, b, &candidate, ratio, rect, stride);
        if value < cost {
            cost = value;
            best = candidate;
        }
    }
    // Coordinate descent makes FFT-grid estimates subpixel and subdegree.
    for scale in [1_f64, 0.5, 0.25, 0.1, 0.04] {
        for _ in 0..12 {
            let previous = cost;
            for axis in 0..3 {
                for sign in [-1., 1.] {
                    let mut candidate = best.clone();
                    if axis == 2 {
                        candidate.rotation_radians += (sign * scale * 0.15).to_radians();
                    } else {
                        candidate.translation[axis] += sign * scale * step;
                    }
                    if candidate.rotation_radians.abs() > 3_f64.to_radians()
                        || candidate.translation[0].abs() > a.width as f64 / 4.
                        || candidate.translation[1].abs() > a.height as f64 / 4.
                    {
                        continue;
                    }
                    let value = score(a, b, &candidate, ratio, rect, stride);
                    if value < cost {
                        cost = value;
                        best = candidate;
                    }
                }
            }
            if cost >= previous {
                break;
            }
        }
    }
    if !cost.is_finite() || cost > 0.03 {
        return Err("insufficient consistent HDR overlap".into());
    }
    let nx = a.width.div_ceil(best.tile_size);
    let ny = a.height.div_ceil(best.tile_size);
    let mut offsets = Vec::with_capacity(nx * ny);
    for ty in 0..ny {
        for tx in 0..nx {
            let rect = [
                tx * best.tile_size,
                ty * best.tile_size,
                ((tx + 1) * best.tile_size).min(a.width),
                ((ty + 1) * best.tile_size).min(a.height),
            ];
            let mut tile_cost = score(a, b, &best, ratio, rect, 2);
            let initial = tile_cost;
            let mut offset = [0.; 2];
            for dy in -4..=4 {
                for dx in -4..=4 {
                    let mut candidate = best.clone();
                    candidate.translation[0] += dx as f64 * 0.25;
                    candidate.translation[1] += dy as f64 * 0.25;
                    let value = score(a, b, &candidate, ratio, rect, 2);
                    if value < tile_cost {
                        tile_cost = value;
                        offset = [dx as f64 * 0.25, dy as f64 * 0.25];
                    }
                }
            }
            // Refuse weak/noisy improvements and large residuals that are motion.
            if !initial.is_finite() || tile_cost > initial * 0.8 || tile_cost > 0.005 {
                offset = [0.; 2];
            }
            offsets.push(offset);
        }
    }
    best.tile_offsets = offsets;
    Ok(best)
}
