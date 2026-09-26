//! Whole-frame restoration only. Face restoration is license-blocked, not
//! approximated by a blur or silently replaced with another model.
use crate::{Cancel, NeuralFilter, ParamSchema, Params, unit, validate};
use anyhow::{Context, Result, ensure};
use compositor::raster::Raster;
use ml_enhance::Denoiser;
use ml_runtime::{ModelRegistry, PartitionReport, SessionOptions};
use std::sync::Mutex;

pub struct PhotoRestoration {
    denoiser: Option<Mutex<Denoiser>>,
}
pub const SCHEMA: &[ParamSchema] = &[
    ParamSchema {
        name: "Photo enhancement",
        min: 0.0,
        max: 1.0,
        default: 0.5,
    },
    ParamSchema {
        name: "Enhance face (unavailable)",
        min: 0.0,
        max: 0.0,
        default: 0.0,
    },
    ParamSchema {
        name: "Scratch reduction (not implemented)",
        min: 0.0,
        max: 0.0,
        default: 0.0,
    },
];
impl PhotoRestoration {
    pub fn unloaded() -> Self {
        Self { denoiser: None }
    }
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
impl NeuralFilter for PhotoRestoration {
    fn name(&self) -> &'static str {
        "Photo Restoration (no face model)"
    }
    fn params_schema(&self) -> &'static [ParamSchema] {
        SCHEMA
    }
    fn requires_weights(&self) -> bool {
        true
    }
    fn apply(&self, input: &Raster, p: &Params, cancel: &Cancel) -> Result<Raster> {
        validate(input, cancel)?;
        unit(p.photo_enhancement)?;
        ensure!(
            p.enhance_face == 0.0,
            "GFPGAN excluded: StyleGAN2/NVIDIA non-commercial lineage (MODELS.md)"
        );
        ensure!(
            p.scratch_reduction == 0.0,
            "scratch reduction not implemented: no approved model selected"
        );
        if p.photo_enhancement == 0.0 {
            return Ok(input.clone());
        }
        crate::jpeg::denoise(
            input,
            p.photo_enhancement * 100.0,
            cancel,
            self.denoiser.as_ref().context("DRUNet not loaded")?,
        )
    }
}
