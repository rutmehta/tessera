use crate::{Cancel, NeuralFilter, ParamSchema, Params, render, unit, validate};
use anyhow::{Result, ensure};
use compositor::raster::Raster;

pub struct SkinSmoothing;
pub const SCHEMA: &[ParamSchema] = &[
    ParamSchema {
        name: "Blur",
        min: 0.0,
        max: 16.0,
        default: 4.0,
    },
    ParamSchema {
        name: "Smoothness",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
];
impl NeuralFilter for SkinSmoothing {
    fn name(&self) -> &'static str {
        "Skin Smoothing"
    }
    fn params_schema(&self) -> &'static [ParamSchema] {
        SCHEMA
    }
    fn requires_weights(&self) -> bool {
        false
    }
    fn apply(&self, input: &Raster, p: &Params, cancel: &Cancel) -> Result<Raster> {
        validate(input, cancel)?;
        unit(p.smoothness)?;
        ensure!(
            p.blur.is_finite() && (0.0..=16.0).contains(&p.blur),
            "blur must be 0..16 pixels"
        );
        ensure!(
            p.faces
                .iter()
                .all(|f| f.iter().all(|v| v.is_finite()) && f[2] > 0.0 && f[3] > 0.0),
            "invalid face box"
        );
        if p.smoothness == 0.0 || p.blur == 0.0 || p.faces.is_empty() {
            return Ok(input.clone());
        }
        let w = input.extent().width as usize;
        let h = input.extent().height as usize;
        let mut rgb = Vec::with_capacity(w * h);
        let mut mask = Vec::with_capacity(w * h);
        for y in 0..h {
            cancel.check()?;
            for x in 0..w {
                let px = input.pixel(x as u32, y as u32);
                rgb.push([px[0], px[1], px[2]]);
                let spatial = p
                    .faces
                    .iter()
                    .map(|f| {
                        let dx = (x as f32 + 0.5 - f[0]) / f[2] * 2.0 - 1.0;
                        let dy = (y as f32 + 0.5 - f[1]) / f[3] * 2.0 - 1.0;
                        ((1.0 - dx * dx - dy * dy) * 5.0).clamp(0.0, 1.0)
                    })
                    .fold(0.0f32, f32::max);
                let cb = 0.5 - 0.168736 * px[0] - 0.331264 * px[1] + 0.5 * px[2];
                let cr = 0.5 + 0.5 * px[0] - 0.418688 * px[1] - 0.081312 * px[2];
                let skin = (0.28..0.55).contains(&cb) && (0.52..0.72).contains(&cr);
                mask.push(if skin && (input.channels() == 3 || px[3] > 0.0) {
                    spatial
                } else {
                    0.0
                });
            }
        }
        // Keep the high-frequency residual exactly; smooth only the low band.
        // Bilateral range weights prevent mixing across strong edges.
        let low = bilateral(&rgb, w, h, 1, cancel)?;
        let smooth = bilateral(&low, w, h, p.blur.ceil() as usize, cancel)?;
        render(input, cancel, |x, y, px| {
            let i = y as usize * w + x as usize;
            for c in 0..3 {
                px[c] =
                    (px[c] + p.smoothness * mask[i] * (smooth[i][c] - low[i][c])).clamp(0.0, 1.0);
            }
        })
    }
}

fn bilateral(
    src: &[[f32; 3]],
    w: usize,
    h: usize,
    r: usize,
    cancel: &Cancel,
) -> Result<Vec<[f32; 3]>> {
    // Separable bilateral approximation: O(N*r), not O(N*r*r).
    let mut current = src.to_vec();
    for horizontal in [true, false] {
        let mut out = current.clone();
        for y in 0..h {
            cancel.check()?;
            for x in 0..w {
                let i = y * w + x;
                let mut sum = [0.0; 3];
                let mut weight = 0.0;
                for d in -(r as isize)..=r as isize {
                    let xx = if horizontal {
                        (x as isize + d).clamp(0, w as isize - 1) as usize
                    } else {
                        x
                    };
                    let yy = if horizontal {
                        y
                    } else {
                        (y as isize + d).clamp(0, h as isize - 1) as usize
                    };
                    let q = current[yy * w + xx];
                    let distance: f32 =
                        q.iter().zip(current[i]).map(|(a, b)| (a - b).powi(2)).sum();
                    let a = (-(d * d) as f32 / (2.0 * (r as f32).powi(2)) - distance / 0.02).exp();
                    weight += a;
                    for c in 0..3 {
                        sum[c] += a * q[c];
                    }
                }
                out[i] = sum.map(|v| v / weight);
            }
        }
        current = out;
    }
    Ok(current)
}
