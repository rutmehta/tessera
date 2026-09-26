//! Opt-in CFA adapter. Calibration is explicit, never inferred from ISO alone.
use engine_api::{
    EngineError, EngineResult,
    id::ModelRef,
    recipe::settings::{DenoiseMethod, DenoiseSettings},
    stage::{ParamHash, StageId},
};
use ml_enhance::{BayerPacking, CfaDenoiser, CfaNoise, Tiling};
use ml_runtime::{ModelRegistry, SessionOptions};
use pipeline_cpu::{Image, PostDemosaicDenoise};
use raw_decode::CfaLayout;
use std::sync::{Arc, Mutex};

pub struct MlCfaDenoise {
    registry: Arc<ModelRegistry>,
    options: SessionOptions,
    model: ModelRef,
    noise: CfaNoise,
    session: Mutex<Option<CfaDenoiser>>,
    fallback: crate::MlPostDemosaicDenoise,
    mask: Option<Image>,
    revision: String,
}
impl MlCfaDenoise {
    /// Noise arrays are in canonical RGGB site order, in normalized sensor units.
    /// Caller must supply a measured camera calibration, not display-domain sigma.
    pub fn new(
        registry: Arc<ModelRegistry>,
        options: SessionOptions,
        model: ModelRef,
        noise: CfaNoise,
    ) -> Self {
        let revision = format!(
            "cfa-unet-v1/{}/{}/{}/{}",
            model.id,
            model.version,
            ParamHash::of(StageId::Denoise, &(noise.shot, noise.read)),
            pipeline_cpu::POST_DENOISE_ADAPTER
        );
        Self {
            fallback: crate::MlPostDemosaicDenoise::new(registry.clone(), options.clone()),
            registry,
            options,
            model,
            noise,
            session: Mutex::new(None),
            mask: None,
            revision,
        }
    }
    /// M2-08 raster sampled at full sensor pixel centres before crop/lens warp.
    /// Every Bayer site retains its own mask coverage. No 2x2 averaging.
    pub fn with_mask(mut self, width: u32, height: u32, samples: Vec<f32>) -> EngineResult<Self> {
        if samples.iter().any(|v| !(0.0..=1.0).contains(v)) {
            return Err(EngineError::invalid("CFA mask", "coverage must be 0..=1"));
        }
        let hash = ParamHash::of(StageId::Denoise, &(width, height, &samples));
        self.fallback = self.fallback.with_mask(width, height, samples.clone())?;
        self.mask = Some(Image::new(width, height, vec![samples])?);
        self.revision = format!("{}/mask-{hash}", self.revision);
        Ok(self)
    }
}
impl PostDemosaicDenoise for MlCfaDenoise {
    fn adapter_revision(&self) -> &str {
        &self.revision
    }
    fn denoise(&self, input: &Image, amount: f32) -> EngineResult<Image> {
        self.fallback.denoise(input, amount)
    }
    fn denoise_raw(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
    ) -> EngineResult<Image> {
        pipeline_cpu::validate_denoise(settings)?;
        if !pipeline_cpu::denoise_active(settings) {
            return Ok(input.clone());
        }
        if !matches!(&settings.method, DenoiseMethod::Neural { model, joint_demosaic: false } if model == &self.model)
        {
            return Err(EngineError::invalid(
                "CFA model",
                "recipe/adapter model mismatch",
            ));
        }
        if input.planes().len() != 1 || !matches!(cfa, CfaLayout::Bayer(_)) {
            return Err(EngineError::invalid(
                "CFA model",
                "single-plane Bayer required",
            ));
        }
        let colors = [
            cfa.channel_at(0, 0),
            cfa.channel_at(1, 0),
            cfa.channel_at(0, 1),
            cfa.channel_at(1, 1),
        ];
        let colors = colors.map(|c| if c == 3 { 1 } else { c });
        let turns = match colors {
            [0, 1, 1, 2] => 0,
            [1, 0, 2, 1] => 1,
            [2, 1, 1, 0] => 2,
            [1, 2, 0, 1] => 3,
            _ => {
                return Err(EngineError::invalid(
                    "CFA model",
                    "unsupported Bayer pattern",
                ));
            }
        };
        let err = |e: String| EngineError::invalid("CFA runtime", e);
        let packing = BayerPacking::new(input.width() as usize, input.height() as usize, turns)
            .map_err(|e| err(e.to_string()))?;
        let packed = packing
            .pack(&input.planes()[0])
            .map_err(|e| err(e.to_string()))?;
        let mask = if let Some(mask) = &self.mask {
            if mask.width() != input.width() || mask.height() != input.height() {
                return Err(EngineError::invalid("CFA mask", "sensor extent mismatch"));
            }
            if mask.planes()[0].iter().all(|v| *v == 0.0) {
                return Ok(input.clone());
            }
            Some(
                packing
                    .pack(&mask.planes()[0])
                    .map_err(|e| err(e.to_string()))?,
            )
        } else {
            None
        };
        let mut guard = self
            .session
            .lock()
            .map_err(|_| EngineError::internal("CFA session poisoned"))?;
        if guard.is_none() {
            *guard = Some(
                CfaDenoiser::load(&self.registry, &self.model, self.options.clone())
                    .map_err(|e| err(e.to_string()))?,
            );
        }
        let output = guard
            .as_mut()
            .expect("loaded")
            .apply(
                &packed,
                self.noise,
                settings.amount,
                mask.as_ref().map(|m| m.data()),
                Tiling {
                    tile_size: 128,
                    halo: 16,
                },
            )
            .map_err(|e| err(e.to_string()))?;
        Image::new(
            input.width(),
            input.height(),
            vec![packing.unpack(&output).map_err(|e| err(e.to_string()))?],
        )
    }
}
