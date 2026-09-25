//! Optional real DRUNet adapter; no global registry or implicit construction I/O.
use engine_api::{EngineError, EngineResult};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};
use pipeline_cpu::{Image, PostDemosaicDenoise};
use std::sync::{Arc, Mutex};

/// Lazy session with caller-owned registry and explicit provider configuration.
/// First nonzero invocation may download the pinned model. Off/zero never load
/// a session. Concurrent callers serialize inference through this instance.
pub struct MlPostDemosaicDenoise {
    registry: Arc<ModelRegistry>,
    options: SessionOptions,
    session: Mutex<Option<ml_enhance::Denoiser>>,
    mask: Option<Image>,
    revision: String,
}
impl MlPostDemosaicDenoise {
    pub fn new(registry: Arc<ModelRegistry>, options: SessionOptions) -> Self {
        Self {
            registry,
            options,
            session: Mutex::new(None),
            mask: None,
            revision: pipeline_cpu::POST_DENOISE_ADAPTER.into(),
        }
    }

    /// Attach an M2-08 raster in full level-0 sensor coordinates. Rasterize
    /// against the complete sensor image, not a cropped preview. The immutable
    /// samples and extent become part of Demosaic's cache identity.
    pub fn with_mask(mut self, width: u32, height: u32, samples: Vec<f32>) -> EngineResult<Self> {
        if samples.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(EngineError::invalid(
                "denoise mask",
                "samples must be 0..=1",
            ));
        }
        let mask = Image::new(width, height, vec![samples])?;
        let hash = engine_api::stage::ParamHash::of(
            engine_api::stage::StageId::Demosaic,
            &(width, height, &mask.planes()[0]),
        );
        self.revision = format!("{}/sensor-mask-{hash}", pipeline_cpu::POST_DENOISE_ADAPTER);
        self.mask = Some(mask);
        Ok(self)
    }
}
impl PostDemosaicDenoise for MlPostDemosaicDenoise {
    fn adapter_revision(&self) -> &str {
        &self.revision
    }
    fn denoise(&self, input: &Image, amount: f32) -> EngineResult<Image> {
        if !amount.is_finite() || !(0.0..=100.0).contains(&amount) {
            return Err(EngineError::invalid("denoise", "amount must be 0..=100"));
        }
        if amount == 0.0 {
            return Ok(input.clone());
        }
        if let Some(mask) = &self.mask {
            if mask.width() != input.width() || mask.height() != input.height() {
                return Err(EngineError::invalid(
                    "denoise mask",
                    "sensor extent mismatch",
                ));
            }
            if mask.planes()[0].iter().all(|v| *v == 0.0) {
                return Ok(input.clone());
            }
        }
        if input.planes().len() != 3
            || input
                .planes()
                .iter()
                .flatten()
                .any(|v| !(0.0..=1.0).contains(v))
        {
            return Err(EngineError::invalid(
                "denoise",
                "bounded linear sRGB required",
            ));
        }
        let err =
            |e: &dyn std::fmt::Display| EngineError::invalid("denoise runtime", e.to_string());
        let tensor = Tensor::new(
            3,
            input.height() as usize,
            input.width() as usize,
            input.planes().iter().flatten().copied().collect(),
        )
        .map_err(|e| err(&e))?;
        let mut session = self
            .session
            .lock()
            .map_err(|_| EngineError::internal("denoise session poisoned"))?;
        if session.is_none() {
            *session = Some(
                ml_enhance::Denoiser::load(&self.registry, self.options.clone())
                    .map_err(|e| err(&e))?,
            );
        }
        // No calibrated sensor-to-display noise mapping: use the pinned sigma.
        let model = session.as_mut().expect("loaded above");
        let result = if let Some(mask) = &self.mask {
            model.denoise_masked(&tensor, amount, None, &mask.planes()[0])
        } else {
            model.denoise(&tensor, amount, None)
        }
        .map_err(|e| err(&e))?;
        let n = input.width() as usize * input.height() as usize;
        Image::new(
            input.width(),
            input.height(),
            result.data().chunks_exact(n).map(|p| p.to_vec()).collect(),
        )
    }
}
