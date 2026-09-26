//! Large-sigma area-reduction / separable blur / bilinear reconstruction.
use crate::{Buffer, checkpoint, convolve, gaussian_kernel};
use engine_api::EngineResult;
use std::sync::atomic::AtomicBool;
pub(crate) fn factor(sigma: f32) -> usize {
    if sigma <= 32.0 {
        1
    } else if sigma < 64.0 {
        2
    } else if sigma < 128.0 {
        4
    } else {
        8
    }
}
pub(crate) fn padding(sigma: f32, f: usize) -> usize {
    ((3.0 * sigma).ceil() as usize + 2 * f).div_ceil(f) * f
}
pub(crate) fn gaussian(src: &Buffer, sigma: f32, cancel: &AtomicBool) -> EngineResult<Buffer> {
    let f = factor(sigma);
    if f == 1 {
        return convolve(src, &gaussian_kernel(sigma), cancel);
    }
    let pad = padding(sigma, f);
    let w = (src.w + 2 * pad).div_ceil(f);
    let h = (src.h + 2 * pad).div_ceil(f);
    let mut small = Buffer {
        w,
        h,
        pixels: vec![[0.0; 4]; w * h],
    };
    for y in 0..h {
        checkpoint(cancel)?;
        for x in 0..w {
            let mut sum = [0.0; 4];
            for dy in 0..f {
                for dx in 0..f {
                    let q = src.at(
                        (x * f + dx) as i32 - pad as i32,
                        (y * f + dy) as i32 - pad as i32,
                    );
                    for c in 0..4 {
                        sum[c] += q[c] / (f * f) as f32;
                    }
                }
            }
            small.pixels[y * w + x] = sum;
        }
    }
    let small = convolve(&small, &gaussian_kernel(sigma / f as f32), cancel)?;
    let mut out = src.clone();
    for y in 0..src.h {
        checkpoint(cancel)?;
        for x in 0..src.w {
            let u = (x as f32 + pad as f32 - (f - 1) as f32 * 0.5) / f as f32;
            let v = (y as f32 + pad as f32 - (f - 1) as f32 * 0.5) / f as f32;
            let tx = u - u.floor();
            let ty = v - v.floor();
            let ix = u.floor() as i32;
            let iy = v.floor() as i32;
            let a = small.at(ix, iy);
            let b = small.at(ix + 1, iy);
            let c = small.at(ix, iy + 1);
            let d = small.at(ix + 1, iy + 1);
            for ch in 0..4 {
                let top = a[ch] + tx * (b[ch] - a[ch]);
                let bot = c[ch] + tx * (d[ch] - c[ch]);
                out.pixels[y * src.w + x][ch] = top + ty * (bot - top);
            }
        }
    }
    Ok(out)
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Buffer, convolve, gaussian_kernel};
    use std::sync::atomic::AtomicBool;
    #[test]
    fn large_radius_matches_exact_on_edges_impulses_and_texture() {
        let cancel = AtomicBool::new(false);
        for sigma in [32.01, 33.0, 63.9, 64.0, 127.9, 128.0, 250.0] {
            for kind in 0..4 {
                let w = 41;
                let h = 27;
                let src = Buffer {
                    w,
                    h,
                    pixels: (0..w * h)
                        .map(|i| {
                            let x = i % w;
                            let y = i / w;
                            let v = match kind {
                                0 => {
                                    if x == 0 && y == 0 {
                                        1.0
                                    } else {
                                        0.0
                                    }
                                }
                                1 => {
                                    if x > w / 2 {
                                        1.0
                                    } else {
                                        0.0
                                    }
                                }
                                2 => ((x * 13 + y * 7) % 31) as f32 / 31.0,
                                _ => 0.37,
                            };
                            [v; 4]
                        })
                        .collect(),
                };
                let exact = convolve(&src, &gaussian_kernel(sigma), &cancel).unwrap();
                let fast = gaussian(&src, sigma, &cancel).unwrap();
                let error = exact
                    .pixels
                    .iter()
                    .flatten()
                    .zip(fast.pixels.iter().flatten())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0, f32::max);
                assert!(error < 0.035, "sigma={sigma},kind={kind},error={error}");
            }
        }
    }
    #[test]
    fn separable_coefficient_bound_is_content_independent() {
        // L1 distance of 1-D weights bounds the 2-D max absolute error for
        // [0,1] inputs (tensor-product telescoping; each kernel sums to one).
        // Clamping image edges only merges coefficients, decreasing L1.
        for sigma in (321..=2500).map(|n| n as f32 / 10.0) {
            let f = factor(sigma);
            let pad = padding(sigma, f);
            let k = gaussian_kernel(sigma / f as f32);
            let kr = (k.len() / 2) as i32;
            let exact = gaussian_kernel(sigma);
            let er = (exact.len() / 2) as i32;
            for x in 0..f {
                let u = (x as f32 + pad as f32 - (f - 1) as f32 * 0.5) / f as f32;
                let lo = u.floor() as i32;
                let t = u - u.floor();
                let mut weights = std::collections::BTreeMap::<i32, f32>::new();
                for (base, mix) in [(lo, 1.0 - t), (lo + 1, t)] {
                    for (j, v) in k.iter().enumerate() {
                        for q in 0..f {
                            let pos = (base + j as i32 - kr) * f as i32 + q as i32
                                - pad as i32
                                - x as i32;
                            *weights.entry(pos).or_default() += v * mix / f as f32;
                        }
                    }
                }
                for (j, v) in exact.iter().enumerate() {
                    *weights.entry(j as i32 - er).or_default() -= v;
                }
                let bound: f32 = weights.values().map(|v| v.abs()).sum();
                assert!(bound < 0.035, "sigma={sigma},phase={x},bound={bound}");
            }
        }
    }
}
