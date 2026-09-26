//! Deterministic inference boundary; never used as a real-model benchmark.
use engine_api::{EngineResult, recipe::settings::DenoiseSettings, tile::Extent};
use image_core::cfa::{CfaDenoise, PackedCfa, bayer_turns};
use pipeline_cpu::{Image, PostDemosaicDenoise};
use raw_decode::CfaLayout;
use std::sync::atomic::{AtomicUsize, Ordering};
pub struct Inference {
    pub calls: AtomicUsize,
    pub revision: String,
    pub cancel: Option<engine_api::jobs::CancellationToken>,
}
impl Default for Inference {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            revision: "test/noise-1/mask-sites-v1".into(),
            cancel: None,
        }
    }
}
impl PostDemosaicDenoise for Inference {
    fn adapter_revision(&self) -> &str {
        &self.revision
    }
    fn denoise(&self, input: &Image, _: f32) -> EngineResult<Image> {
        Ok(input.clone())
    }
    fn denoise_raw(
        &self,
        input: &Image,
        cfa: CfaLayout,
        s: &DenoiseSettings,
    ) -> EngineResult<Image> {
        self.infer(input, cfa, s)?
            .blend_cpu(input, s.amount / 100.0)
    }
}
impl CfaDenoise for Inference {
    fn supports(&self, cfa: CfaLayout, s: &DenoiseSettings) -> bool {
        bayer_turns(cfa).is_some() && pipeline_cpu::cfa_denoise_selected(s)
    }
    fn infer(&self, input: &Image, cfa: CfaLayout, _: &DenoiseSettings) -> EngineResult<PackedCfa> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if let Some(cancel) = &self.cancel {
            cancel.cancel();
        }
        let turns = bayer_turns(cfa).unwrap();
        let packing =
            ml_enhance::BayerPacking::new(input.width() as usize, input.height() as usize, turns)
                .unwrap();
        let full: Vec<f32> = input.planes()[0].iter().map(|v| v * 0.8 + 0.035).collect();
        let mask: Vec<f32> = (0..full.len()).map(|i| [0.0, 0.25, 1.0][i % 3]).collect();
        PackedCfa::new(
            Extent::new(input.width(), input.height()),
            turns,
            packing.pack(&full).unwrap().data().to_vec(),
            Some(packing.pack(&mask).unwrap().data().to_vec()),
        )
    }
}
pub fn settings() -> engine_api::recipe::DevelopSettings {
    let mut s = engine_api::recipe::DevelopSettings::default();
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
        model: engine_api::id::ModelRef {
            id: "enhance/cfa-unet-fp32".into(),
            version: "a".repeat(64),
        },
        joint_demosaic: false,
    };
    s
}
