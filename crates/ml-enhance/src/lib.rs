//! On-device image enhancement.
mod cfa;
mod packing;
pub use cfa::{CfaDenoiser, CfaNoise, denoise_cfa_with};
pub use packing::BayerPacking;
mod denoise;
pub use denoise::{
    DENOISE_ADAPTER_VERSION, DENOISE_MODEL_ID, DENOISE_SHA256, DENOISE_SIGMA, DENOISE_VERSION,
    Denoiser,
};
mod sr;
mod tiling;
pub use sr::{SR_VERSION, SR_X2_SHA256, SR_X4_SHA256, SuperResolution};
pub use tiling::{SpatialContract, Tiling, run_tiled};

use anyhow::{Result, ensure};
use ml_runtime::Tensor;

/// Advisory sensor variance: variance(signal) = read + shot * signal.
/// DRUNet adapter v1 validates but does not consume this hint; no calibrated
/// sensor-to-display sigma mapping is available.
#[derive(Clone, Copy, Debug)]
pub struct NoiseModelHint {
    pub read: [f32; 3],
    pub shot: [f32; 3],
}

/// Packed RGGB denoise extension point. The three-colour hint shares G1/G2
/// calibration; use CfaDenoiser::apply for independent green-site noise models.
pub trait CfaDenoise {
    fn denoise_cfa(&mut self, packed_cfa: &Tensor, noise: NoiseModelHint) -> Result<Tensor>;
}

/// Blend an inferred RGB result in linear light. Amount is 0..=100.
/// Mask is an M2-08 raster in the input image's coordinate frame, 0..=1.
/// The callback permits zero-amount/mask bypass before loading any weights.
pub fn denoise_with(
    linear_rgb: &Tensor,
    amount: f32,
    noise_model_hint: Option<NoiseModelHint>,
    mask: Option<&[f32]>,
    infer: impl FnOnce(&Tensor) -> Result<Tensor>,
) -> Result<Tensor> {
    validate_rgb(linear_rgb)?;
    ensure!(
        amount.is_finite() && (0.0..=100.0).contains(&amount),
        "amount must be 0..=100"
    );
    if let Some(noise) = noise_model_hint {
        ensure!(
            noise
                .read
                .iter()
                .chain(&noise.shot)
                .all(|v| v.is_finite() && *v >= 0.0),
            "invalid noise model"
        );
    }
    let [_, _, h, w] = linear_rgb.shape();
    if let Some(mask) = mask {
        ensure!(
            mask.len() == h * w
                && mask
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid denoise mask"
        );
    }
    if amount == 0.0 || mask.is_some_and(|m| m.iter().all(|v| *v == 0.0)) {
        return Ok(linear_rgb.clone());
    }
    let output = infer(linear_rgb)?;
    ensure!(
        output.shape() == linear_rgb.shape(),
        "denoiser must preserve RGB shape"
    );
    validate_rgb(&output)?;
    let result = linear_rgb
        .data()
        .iter()
        .zip(output.data())
        .enumerate()
        .map(|(i, (&src, &dst))| {
            let alpha = amount / 100.0 * mask.map_or(1.0, |m| m[i % (h * w)]);
            // Preserve even signed zero in unselected regions.
            if alpha == 0.0 {
                src
            } else if alpha == 1.0 {
                dst
            } else {
                src * (1.0 - alpha) + dst * alpha
            }
        })
        .collect();
    Tensor::new(3, h, w, result)
}

fn validate_rgb(input: &Tensor) -> Result<()> {
    ensure!(
        input.shape()[1] == 3 && input.data().iter().all(|v| v.is_finite()),
        "finite three-channel RGB required"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ml_runtime::Tensor;

    #[test]
    fn zero_amount_is_bit_exact_and_does_not_run_model() {
        let values = vec![-0.0, 1.0, -2.0, 20.0, 0.1, 0.8];
        let input = Tensor::new(3, 1, 2, values.clone()).unwrap();
        let output = denoise_with(&input, 0.0, None, None, |_| {
            panic!("zero amount must not run inference")
        })
        .unwrap();
        assert_eq!(
            output
                .data()
                .iter()
                .map(|x| x.to_bits())
                .collect::<Vec<_>>(),
            values.iter().map(|x| x.to_bits()).collect::<Vec<_>>()
        );
    }
}
