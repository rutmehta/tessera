use crate::{NoiseModelHint, denoise_with};
use anyhow::{Result, ensure};
use engine_api::id::ModelRef;
use ml_runtime::{ModelRegistry, PartitionReport, Session, SessionOptions, Tensor};

pub const DENOISE_MODEL_ID: &str = "enhance/drunet-color";
pub const DENOISE_VERSION: &str = "a2b9fccfa27b197f44a3876c567f5e48970c44a7";
pub const DENOISE_SHA256: &str = "2ae3ab5eb15daac2ee79be984d584b908ce7f0f60b27be87d005f728c2aa0087";
pub const DENOISE_ADAPTER_VERSION: &str = "linear-srgb-v1-sigma25";
pub const DENOISE_SIGMA: f32 = 25.0 / 255.0;
/// Automatic path only; legacy fixed-sigma API keeps its v1 identity.
pub const DENOISE_AUTO_ADAPTER_VERSION: &str = "linear-srgb-v2-auto-m249-rms50";
/// Conservative cap on display-domain conditioning (50/255).
pub const DENOISE_AUTO_MAX_SIGMA: f32 = 50.0 / 255.0;

/// Estimate one frame-wide display-domain sigma from bounded linear sRGB.
/// Reuses M2-49's independent-plane shot/read estimator after demosaic. At
/// each channel mean mu, propagate variance with the sRGB derivative:
/// sigma = sqrt(mean_c(f'(mu_c)^2 * (read_c + shot_c * mu_c))).
/// Means avoid amplifying individual noisy dark pixels. A single RMS sigma
/// matches RGB DRUNet's AWGN conditioning and is shared by every tile. This is
/// a first-order approximation, not calibrated propagation through demosaic.
/// At least 8x8 pixels are required; smaller inputs return an error rather than
/// substituting an invented fixed noise level.
pub fn estimate_drunet_sigma(linear: &Tensor) -> Result<f32> {
    crate::validate_rgb(linear)?;
    ensure!(
        linear.data().iter().all(|v| (0.0..=1.0).contains(v)),
        "DRUNet requires bounded linear sRGB in [0,1], not camera RGB or HDR"
    );
    let noise = crate::CfaNoise::estimate_rgb(linear)?;
    let [_, _, h, w] = linear.shape();
    let mut variance = 0.0f64;
    for (c, plane) in linear.data().chunks_exact(h * w).enumerate() {
        let mean = plane.iter().map(|&v| f64::from(v)).sum::<f64>() / plane.len() as f64;
        let derivative = if mean <= 0.0031308 {
            12.92
        } else {
            (1.055 / 2.4) * mean.powf(1.0 / 2.4 - 1.0)
        };
        variance +=
            derivative.powi(2) * (f64::from(noise.read[c]) + f64::from(noise.shot[c]) * mean);
    }
    Ok(((variance / 3.0).sqrt() as f32).clamp(0.0, DENOISE_AUTO_MAX_SIGMA))
}

/// Automatic conditioning with the existing linear-light amount/mask contract.
/// The callback takes bounded linear RGB and one estimated display sigma, and
/// returns a full-strength linear RGB restoration. Zero amount/mask bypasses
/// estimation as well as inference. Use `Denoiser::denoise_automatic` for DRUNet.
pub fn denoise_automatic_with(
    input: &Tensor,
    amount: f32,
    mask: Option<&[f32]>,
    infer: impl FnOnce(&Tensor, f32) -> Result<Tensor>,
) -> Result<Tensor> {
    denoise_with(input, amount, None, mask, |linear| {
        let sigma = estimate_drunet_sigma(linear)?;
        if sigma == 0.0 {
            return Ok(linear.clone());
        }
        infer(linear, sigma)
    })
}

/// Pinned RGB DRUNet, not a CFA or camera-linear denoiser.
///
/// Nonzero inference requires bounded linear sRGB (D65/sRGB primaries, [0,1]).
/// Legacy `denoise`/`denoise_masked` encode the sRGB transfer function and
/// append a constant 25/255 sigma plane. `denoise_automatic` estimates it from
/// linear RGB instead. Both restore display RGB, clamp, then decode.
/// Amount and masks blend in linear light, not in the model's display domain.
/// Sensor read/shot hints are validated but deliberately NOT used: no calibrated
/// sensor-to-display noise conversion is available. The automatic API measures
/// the already-converted RGB instead of consuming a sensor hint. Amount is blend.
/// Negative/HDR input is rejected, except when inference is bypassed entirely.
/// Callers must convert camera/working-space primaries before invoking this API.
pub struct Denoiser {
    session: Session,
}

impl Denoiser {
    /// Explicitly resolves (and may download) the pinned fp32 model.
    pub fn load(registry: &ModelRegistry, options: SessionOptions) -> Result<Self> {
        let handle = registry.resolve_ref(&ModelRef {
            id: DENOISE_MODEL_ID.into(),
            version: DENOISE_VERSION.into(),
        })?;
        Self::from_handle(handle, options)
    }

    /// Strict cache-only load: never re-enters a downloading resolver.
    pub fn load_cached(registry: &ModelRegistry, options: SessionOptions) -> Result<Self> {
        let handle = registry
            .resolve_cached_ref(&ModelRef {
                id: DENOISE_MODEL_ID.into(),
                version: DENOISE_VERSION.into(),
            })?
            .ok_or_else(|| {
                anyhow::anyhow!("missing DRUNet weights (offline); enable model downloads")
            })?;
        Self::from_handle(handle, options)
    }

    fn from_handle(handle: ml_runtime::ModelHandle, options: SessionOptions) -> Result<Self> {
        ensure!(
            handle.spec().sha256 == DENOISE_SHA256,
            "unexpected DRUNet weights"
        );
        Ok(Self {
            session: Session::load(handle.path(), options)?,
        })
    }

    /// Restore NCHW linear sRGB, preserving shape. Amount is 0..=100.
    /// Zero amount bypasses inference and returns an exact clone.
    pub fn denoise(
        &mut self,
        linear_rgb: &Tensor,
        amount: f32,
        noise_model_hint: Option<NoiseModelHint>,
    ) -> Result<Tensor> {
        self.apply(linear_rgb, amount, noise_model_hint, None)
    }

    /// As `denoise`, with an H*W row-major mask in [0,1]. Zero mask pixels
    /// retain their original bits; all-zero masks bypass inference entirely.
    pub fn denoise_masked(
        &mut self,
        linear_rgb: &Tensor,
        amount: f32,
        noise_model_hint: Option<NoiseModelHint>,
        mask: &[f32],
    ) -> Result<Tensor> {
        self.apply(linear_rgb, amount, noise_model_hint, Some(mask))
    }

    /// Estimate conditioning from demosaiced bounded linear sRGB (at least 8x8).
    /// Amount and optional H*W mask only blend the full-strength restoration.
    pub fn denoise_automatic(
        &mut self,
        linear_rgb: &Tensor,
        amount: f32,
        mask: Option<&[f32]>,
    ) -> Result<Tensor> {
        denoise_automatic_with(linear_rgb, amount, mask, |linear, sigma| {
            self.restore(linear, sigma)
        })
    }

    fn apply(
        &mut self,
        input: &Tensor,
        amount: f32,
        hint: Option<NoiseModelHint>,
        mask: Option<&[f32]>,
    ) -> Result<Tensor> {
        denoise_with(input, amount, hint, mask, |linear| {
            self.restore(linear, DENOISE_SIGMA)
        })
    }

    fn restore(&mut self, linear: &Tensor, sigma: f32) -> Result<Tensor> {
        ensure!(
            linear.data().iter().all(|v| (0.0..=1.0).contains(v)),
            "DRUNet requires bounded linear sRGB in [0,1], not camera RGB or HDR"
        );
        let [_, _, h, w] = linear.shape();
        let display = Tensor::new(
            3,
            h,
            w,
            linear
                .data()
                .iter()
                .map(|&v| {
                    if v <= 0.0031308 {
                        12.92 * v
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                })
                .collect(),
        )?;
        // Local residual U-Net: 185-pixel dependency radius, rounded to
        // 192 so every patch retains the stride-8 sampling phase. Unlike
        // NAFNet, this graph has no global pooling or attention operations.
        let restored = crate::run_tiled(
            &display,
            crate::SpatialContract {
                scale: 1,
                radius: 185,
                alignment: 8,
            },
            crate::Tiling {
                tile_size: 128,
                halo: 192,
            },
            |patch| {
                let [_, _, ph, pw] = patch.shape();
                let mut conditioned = patch.data().to_vec();
                conditioned.resize(4 * ph * pw, sigma);
                self.session.run(&Tensor::new(4, ph, pw, conditioned)?)
            },
        )?;
        Tensor::new(
            3,
            h,
            w,
            restored
                .data()
                .iter()
                .map(|&v| {
                    // The display-domain model can overshoot. Clip before decoding.
                    let v = v.clamp(0.0, 1.0);
                    if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                })
                .collect(),
        )
    }

    /// Finalizes executed-provider profiling; call after representative inference.
    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        self.session.partition_report()
    }
}
