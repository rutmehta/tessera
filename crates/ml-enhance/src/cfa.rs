//! Phase 2a: packed, normalized RGGB in linear sensor units. No demosaic.
use anyhow::{Result, ensure};
use engine_api::id::ModelRef;
use ml_runtime::{ModelRegistry, PartitionReport, Session, SessionOptions, Tensor};

pub struct CfaDenoiser {
    session: Session,
}

impl CfaDenoiser {
    pub fn load(
        registry: &ModelRegistry,
        model: &ModelRef,
        options: SessionOptions,
    ) -> Result<Self> {
        let handle = registry.resolve_ref(model)?;
        ensure!(
            model.version == handle.spec().sha256,
            "CFA version must equal model digest"
        );
        ensure!(handle.spec().task == "cfa-denoise", "not a CFA model");
        ensure!(
            matches!(
                model.id.as_str(),
                "enhance/cfa-unet-fp32" | "enhance/cfa-unet-fp16"
            ),
            "unknown CFA spatial contract"
        );
        Ok(Self {
            session: Session::load(handle.path(), options)?,
        })
    }

    /// Host-tensor reference path, not a GPU-resident buffer interchange API.
    pub fn apply(
        &mut self,
        input: &Tensor,
        noise: CfaNoise,
        amount: f32,
        mask: Option<&[f32]>,
        tiling: crate::Tiling,
    ) -> Result<Tensor> {
        denoise_cfa_with(input, noise, amount, mask, |_| {
            crate::run_tiled(
                input,
                crate::SpatialContract {
                    scale: 1,
                    radius: 16,
                    alignment: 2,
                },
                tiling,
                |patch| {
                    denoise_cfa_with(patch, noise, 100.0, None, |conditioned| {
                        self.session.run(conditioned)
                    })
                },
            )
        })
    }

    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        self.session.partition_report()
    }
}

impl crate::CfaDenoise for CfaDenoiser {
    fn denoise_cfa(&mut self, input: &Tensor, noise: crate::NoiseModelHint) -> Result<Tensor> {
        let noise = CfaNoise {
            shot: [noise.shot[0], noise.shot[1], noise.shot[1], noise.shot[2]],
            read: [noise.read[0], noise.read[1], noise.read[1], noise.read[2]],
        };
        self.apply(
            input,
            noise,
            100.0,
            None,
            crate::Tiling {
                tile_size: 128,
                halo: 16,
            },
        )
    }
}

/// Separate green sites remain distinct. Variance = shot * signal + read.
#[derive(Clone, Copy, Debug)]
pub struct CfaNoise {
    pub shot: [f32; 4],
    pub read: [f32; 4],
}

impl CfaNoise {
    /// Dark-frame-free estimate in normalized, canonical RGGB sensor units.
    /// Nonoverlapping 8x8 same-site patches use a mixed second difference
    /// (a-b-c+d)/2, which cancels constant and linear scene gradients and has
    /// unit noise energy. The flattest three quarters reject textured patches.
    /// A nonnegative variance-versus-mean fit separates shot/read noise only
    /// when the surviving signal range exceeds 0.05; otherwise read variance
    /// represents the observed noise at that signal. This is an estimate, not
    /// a camera calibration, and may overestimate noise in textured scenes.
    pub fn estimate(packed: &Tensor) -> Result<Self> {
        let [_, channels, h, w] = packed.shape();
        ensure!(
            channels == 4 && w >= 8 && h >= 8,
            "noise estimate needs four 8x8 CFA planes"
        );
        ensure!(
            packed.data().iter().all(|v| v.is_finite()),
            "nonfinite CFA samples"
        );
        let mut result = Self {
            shot: [0.; 4],
            read: [0.; 4],
        };
        for c in 0..4 {
            let plane = &packed.data()[c * w * h..(c + 1) * w * h];
            let mut patches = Vec::new();
            for y in (0..h - 7).step_by(8) {
                for x in (0..w - 7).step_by(8) {
                    let (mut sum, mut variance, mut gradient) = (0.0f64, 0.0f64, 0.0f64);
                    for dy in (0..8).step_by(2) {
                        for dx in (0..8).step_by(2) {
                            let i = (y + dy) * w + x + dx;
                            let [a, b, c, d] =
                                [plane[i], plane[i + 1], plane[i + w], plane[i + w + 1]]
                                    .map(f64::from);
                            sum += a + b + c + d;
                            variance += (a - b - c + d).powi(2) / 4.0;
                            gradient += (a + b - c - d).powi(2) + (a + c - b - d).powi(2);
                        }
                    }
                    patches.push((gradient, sum / 64.0, variance / 16.0));
                }
            }
            patches.sort_by(|a, b| a.0.total_cmp(&b.0));
            patches.truncate((patches.len() * 3 / 4).max(1));
            let n = patches.len() as f64;
            let mean = patches.iter().map(|p| p.1).sum::<f64>() / n;
            let variance = patches.iter().map(|p| p.2).sum::<f64>() / n;
            let spread = patches.iter().map(|p| (p.1 - mean).powi(2)).sum::<f64>();
            let lo = patches.iter().map(|p| p.1).fold(f64::INFINITY, f64::min);
            let hi = patches
                .iter()
                .map(|p| p.1)
                .fold(f64::NEG_INFINITY, f64::max);
            let shot = if hi - lo > 0.05 && spread > 0.0 {
                (patches
                    .iter()
                    .map(|p| (p.1 - mean) * (p.2 - variance))
                    .sum::<f64>()
                    / spread)
                    .max(0.0)
                    .min(variance / mean.max(1e-12))
            } else {
                0.0
            };
            result.shot[c] = shot.min(f32::MAX as f64) as f32;
            result.read[c] = (variance - shot * mean).clamp(0.0, f32::MAX as f64) as f32;
        }
        Ok(result)
    }
}

/// Blend in packed sensor coordinates. The optional mask is FOUR planes,
/// packed/rotated exactly like the sensor, not one averaged 2x2 mask sample.
/// The callback receives eight planes: R,G1,G2,B and their estimated sigmas.
pub fn denoise_cfa_with(
    input: &Tensor,
    noise: CfaNoise,
    amount: f32,
    mask: Option<&[f32]>,
    infer: impl FnOnce(&Tensor) -> Result<Tensor>,
) -> Result<Tensor> {
    let [_, c, h, w] = input.shape();
    ensure!(
        c == 4 && input.data().iter().all(|v| v.is_finite()),
        "finite packed CFA required"
    );
    ensure!(
        amount.is_finite() && (0.0..=100.0).contains(&amount),
        "amount must be 0..=100"
    );
    ensure!(
        noise
            .shot
            .iter()
            .chain(&noise.read)
            .all(|v| v.is_finite() && *v >= 0.0),
        "invalid CFA noise"
    );
    if let Some(mask) = mask {
        ensure!(
            mask.len() == input.data().len() && mask.iter().all(|v| (0.0..=1.0).contains(v)),
            "invalid packed mask"
        );
    }
    if amount == 0.0 || mask.is_some_and(|m| m.iter().all(|v| *v == 0.0)) {
        return Ok(input.clone());
    }
    let mut data = input.data().to_vec();
    for (i, &v) in input.data().iter().enumerate() {
        let ch = i / (h * w);
        let variance = noise.shot[ch] * v.max(0.0) + noise.read[ch];
        ensure!(variance.is_finite(), "noise variance overflow");
        data.push(variance.sqrt());
    }
    let out = infer(&Tensor::new(8, h, w, data)?)?;
    ensure!(
        out.shape() == input.shape() && out.data().iter().all(|v| v.is_finite()),
        "invalid CFA output"
    );
    // The resident handoff needs the owned full-strength output, without a
    // second full tensor allocation just to blend at the exact endpoint.
    if amount == 100.0 && mask.is_none() {
        return Ok(out);
    }
    Tensor::new(
        4,
        h,
        w,
        input
            .data()
            .iter()
            .zip(out.data())
            .enumerate()
            .map(|(i, (&a, &b))| {
                let alpha = amount / 100.0 * mask.map_or(1.0, |m| m[i]);
                if alpha == 0.0 {
                    a
                } else if alpha == 1.0 {
                    b
                } else {
                    a * (1.0 - alpha) + b * alpha
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod automatic_noise_tests {
    use super::*;

    #[test]
    fn estimate_recovers_independent_flat_site_variances() {
        let (w, h) = (64, 64);
        let mut seed = 7u32;
        let mut data = Vec::new();
        for c in 0..4 {
            let amplitude = 0.002 * (c + 1) as f32;
            for _ in 0..w * h {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let u = (seed >> 8) as f32 / 16777216.0;
                data.push(0.25 + amplitude * (2.0 * u - 1.0));
            }
        }
        let input = Tensor::new(4, h, w, data).unwrap();
        let estimated = CfaNoise::estimate(&input).unwrap();
        for c in 0..4 {
            let expected = (0.002 * (c + 1) as f32).powi(2) / 3.0;
            let actual = estimated.read[c] + estimated.shot[c] * 0.25;
            assert!(
                (actual / expected - 1.0).abs() < 0.3,
                "site {c}: {actual} vs {expected}"
            );
        }
    }

    #[test]
    fn estimate_rejects_invalid_input_and_keeps_noiseless_flat_zero() {
        assert!(CfaNoise::estimate(&Tensor::new(3, 16, 16, vec![0.2; 768]).unwrap()).is_err());
        let noise = CfaNoise::estimate(&Tensor::new(4, 16, 16, vec![0.2; 1024]).unwrap()).unwrap();
        assert_eq!(noise.shot, [0.; 4]);
        assert_eq!(noise.read, [0.; 4]);
    }
}
