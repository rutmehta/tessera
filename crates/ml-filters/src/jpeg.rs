use crate::{Cancel, NeuralFilter, ParamSchema, Params, render, unit, validate};
use anyhow::{Context, Result};
use compositor::raster::Raster;
use ml_enhance::Denoiser;
use ml_runtime::{ModelRegistry, PartitionReport, SessionOptions, Tensor};
use std::sync::Mutex;

pub struct JpegArtifactRemoval {
    denoiser: Option<Mutex<Denoiser>>,
}
pub const SCHEMA: &[ParamSchema] = &[ParamSchema {
    name: "Strength",
    min: 0.0,
    max: 1.0,
    default: 0.5,
}];
impl JpegArtifactRemoval {
    /// An unloaded instance supports zero-strength bypass, never a fake denoiser.
    pub fn unloaded() -> Self {
        Self { denoiser: None }
    }
    /// Explicit opt-in download, checked by the DRUNet adapter's pinned digest.
    pub fn load(registry: &ModelRegistry, options: SessionOptions) -> Result<Self> {
        Ok(Self {
            denoiser: Some(Mutex::new(Denoiser::load(registry, options)?)),
        })
    }
    pub fn partition_report(&self) -> Result<PartitionReport> {
        self.denoiser
            .as_ref()
            .context("DRUNet not loaded")?
            .lock()
            .map_err(|_| anyhow::anyhow!("DRUNet mutex poisoned"))?
            .partition_report()
    }
}
impl NeuralFilter for JpegArtifactRemoval {
    fn name(&self) -> &'static str {
        "JPEG Artifact Removal"
    }
    fn params_schema(&self) -> &'static [ParamSchema] {
        SCHEMA
    }
    fn requires_weights(&self) -> bool {
        true
    }
    fn apply(&self, input: &Raster, p: &Params, cancel: &Cancel) -> Result<Raster> {
        validate(input, cancel)?;
        unit(p.strength)?;
        if p.strength == 0.0 {
            return Ok(input.clone());
        }
        let quality = estimate_jpeg_quality(input, cancel)?;
        // Block evidence conditions blend strength, not DRUNet's fixed noise
        // channel. A modest floor also treats ringing without visible blocking.
        let amount = p.strength * (15.0 + 0.65 * (100.0 - quality));
        denoise(
            input,
            amount,
            cancel,
            self.denoiser.as_ref().context("DRUNet not loaded")?,
        )
    }
}

/// Heuristic, not recovery of the original encoder setting. Excess gradient at
/// 8-pixel boundaries versus interior gradients estimates block damage. Cropped
/// JPEGs with shifted grids and real grid-pattern subjects can fool this metric.
pub fn estimate_jpeg_quality(input: &Raster, cancel: &Cancel) -> Result<f32> {
    validate(input, cancel)?;
    let mut sums = [0.0f64; 2];
    let mut counts = [0usize; 2];
    for y in 0..input.extent().height {
        cancel.check()?;
        for x in 0..input.extent().width {
            let p = input.pixel(x, y);
            for (valid, xx, yy, boundary) in [
                (x > 0, x.saturating_sub(1), y, x % 8 == 0),
                (y > 0, x, y.saturating_sub(1), y % 8 == 0),
            ] {
                if !valid {
                    continue;
                }
                let q = input.pixel(xx, yy);
                let index = usize::from(boundary);
                sums[index] += (0..3).map(|c| (p[c] - q[c]).abs() as f64).sum::<f64>() / 3.0;
                counts[index] += 1;
            }
        }
    }
    let mean = |i: usize| sums[i] / counts[i].max(1) as f64;
    Ok((100.0 - 2000.0 * (mean(1) - mean(0)).max(0.0)).clamp(1.0, 100.0) as f32)
}

pub(crate) fn denoise(
    input: &Raster,
    amount: f32,
    cancel: &Cancel,
    model: &Mutex<Denoiser>,
) -> Result<Raster> {
    let w = input.extent().width as usize;
    let h = input.extent().height as usize;
    let mut data = vec![0.0; 3 * w * h];
    for y in 0..h {
        cancel.check()?;
        for x in 0..w {
            let p = input.pixel(x as u32, y as u32);
            for c in 0..3 {
                data[c * w * h + y * w + x] = decode(p[c]);
            }
        }
    }
    cancel.check()?;
    let out = model
        .lock()
        .map_err(|_| anyhow::anyhow!("DRUNet mutex poisoned"))?
        .denoise(&Tensor::new(3, h, w, data)?, amount, None)?;
    cancel.check()?;
    render(input, cancel, |x, y, p| {
        for (c, v) in p.iter_mut().take(3).enumerate() {
            *v = encode(out.data()[c * w * h + y as usize * w + x as usize]).clamp(0.0, 1.0);
        }
    })
}
fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
