use crate::{NoiseModelHint, denoise_with};
use anyhow::{Result, ensure};
use engine_api::id::ModelRef;
use ml_runtime::{ModelRegistry, PartitionReport, Session, SessionOptions, Tensor};

pub const DENOISE_MODEL_ID: &str = "enhance/drunet-color";
pub const DENOISE_VERSION: &str = "a2b9fccfa27b197f44a3876c567f5e48970c44a7";
pub const DENOISE_SHA256: &str = "2ae3ab5eb15daac2ee79be984d584b908ce7f0f60b27be87d005f728c2aa0087";
pub const DENOISE_ADAPTER_VERSION: &str = "linear-srgb-v1-sigma25";
pub const DENOISE_SIGMA: f32 = 25.0 / 255.0;

/// Pinned RGB DRUNet, not a CFA or camera-linear denoiser.
///
/// Nonzero inference requires bounded linear sRGB (D65/sRGB primaries, [0,1]).
/// Adapter v1 encodes the sRGB transfer function, appends a constant 25/255
/// sigma plane, executes RGB restoration, clamps display output, then decodes.
/// Amount and masks blend in linear light, not in the model's display domain.
/// Sensor read/shot hints are validated but deliberately NOT used: no calibrated
/// sensor-to-display noise conversion is available. Amount controls only blend.
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

    fn apply(
        &mut self,
        input: &Tensor,
        amount: f32,
        hint: Option<NoiseModelHint>,
        mask: Option<&[f32]>,
    ) -> Result<Tensor> {
        denoise_with(input, amount, hint, mask, |linear| {
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
                    conditioned.resize(4 * ph * pw, DENOISE_SIGMA);
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
        })
    }

    /// Finalizes executed-provider profiling; call after representative inference.
    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        self.session.partition_report()
    }
}
