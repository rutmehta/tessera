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

#[derive(Default)]
struct Runtime {
    model: Option<CfaDenoiser>,
    memo: Option<(ParamHash, crate::cfa::PackedCfa)>,
}

pub struct MlCfaDenoise {
    registry: Arc<ModelRegistry>,
    options: SessionOptions,
    model: ModelRef,
    noise: CfaNoise,
    session: Mutex<Runtime>,
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
            "cfa-unet-packed-v2/{}/{}/{}/{}/{:?}",
            model.id,
            model.version,
            ParamHash::of(StageId::Denoise, &(noise.shot, noise.read)),
            pipeline_cpu::POST_DENOISE_ADAPTER,
            options
        );
        Self {
            fallback: crate::MlPostDemosaicDenoise::new(registry.clone(), options.clone()),
            registry,
            options,
            model,
            noise,
            session: Mutex::new(Runtime::default()),
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
        crate::cfa::CfaDenoise::infer(self, input, cfa, settings)?
            .blend_cpu(input, settings.amount / 100.0)
    }
}

impl crate::cfa::CfaDenoise for MlCfaDenoise {
    fn supports(&self, cfa: CfaLayout, settings: &DenoiseSettings) -> bool {
        crate::cfa::bayer_turns(cfa).is_some()
            && matches!(&settings.method, DenoiseMethod::Neural { model, joint_demosaic: false } if model == &self.model)
    }
    fn infer(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
    ) -> EngineResult<crate::cfa::PackedCfa> {
        self.infer_with_identity(input, cfa, settings, None)
    }
    fn infer_keyed(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
        identity: engine_api::stage::MemoKey,
    ) -> EngineResult<crate::cfa::PackedCfa> {
        self.infer_with_identity(input, cfa, settings, Some(identity))
    }
}
impl MlCfaDenoise {
    fn infer_with_identity(
        &self,
        input: &Image,
        cfa: CfaLayout,
        settings: &DenoiseSettings,
        identity: Option<engine_api::stage::MemoKey>,
    ) -> EngineResult<crate::cfa::PackedCfa> {
        use crate::cfa::CfaDenoise;
        pipeline_cpu::validate_denoise(settings)?;
        if !self.supports(cfa, settings) || input.planes().len() != 1 {
            return Err(EngineError::invalid(
                "CFA inference",
                "unsupported input/model",
            ));
        }
        let turns = crate::cfa::bayer_turns(cfa).expect("supported Bayer");
        // Content addresses the upstream sensor for direct legacy callers, who
        // have no ImageId. Renderer memoization additionally includes ImageId
        // and the upstream graph key. Neither identity contains Amount/tone.
        let key = ParamHash::of(
            StageId::Denoise,
            &(
                input.width(),
                input.height(),
                turns,
                &input.planes()[0],
                &self.revision,
                identity,
            ),
        );
        let mut guard = self
            .session
            .lock()
            .map_err(|_| EngineError::internal("CFA session poisoned"))?;
        if let Some((cached, output)) = &guard.memo
            && *cached == key
        {
            return Ok(output.clone());
        }
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
            Some(
                packing
                    .pack(&mask.planes()[0])
                    .map_err(|e| err(e.to_string()))?,
            )
        } else {
            None
        };
        if mask
            .as_ref()
            .is_some_and(|m| m.data().iter().all(|v| *v == 0.0))
        {
            return crate::cfa::PackedCfa::from_tensors(
                engine_api::tile::Extent::new(input.width(), input.height()),
                turns,
                packed,
                mask,
            );
        }
        if guard.model.is_none() {
            guard.model = Some(
                CfaDenoiser::load(&self.registry, &self.model, self.options.clone())
                    .map_err(|e| err(e.to_string()))?,
            );
        }
        let output = guard
            .model
            .as_mut()
            .expect("loaded")
            .apply(
                &packed,
                self.noise,
                100.0,
                None,
                Tiling {
                    tile_size: 128,
                    halo: 16,
                },
            )
            .map_err(|e| err(e.to_string()))?;
        let output = crate::cfa::PackedCfa::from_tensors(
            engine_api::tile::Extent::new(input.width(), input.height()),
            turns,
            output,
            mask,
        )?;
        guard.memo = Some((key, output.clone()));
        Ok(output)
    }
}
