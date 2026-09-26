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
