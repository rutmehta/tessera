use engine_api::{
    id::ModelRef,
    recipe::{DevelopSettings, settings::DenoiseMethod},
    stage::StageId,
};
mod common;
use image_core::{PipelineGraph, PixelRect, Renderer, RendererConfig};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct Counting(AtomicUsize);
impl pipeline_cpu::PostDemosaicDenoise for Counting {
    fn adapter_revision(&self) -> &str {
        "test-identity-v1"
    }
    fn denoise(
        &self,
        input: &pipeline_cpu::Image,
        _: f32,
    ) -> engine_api::EngineResult<pipeline_cpu::Image> {
        self.0.fetch_add(1, Ordering::SeqCst);
        assert!(
            input
                .planes()
                .iter()
                .flatten()
                .all(|v| (0.0..=1.0).contains(v))
        );
        Ok(input.clone())
    }
}
#[test]
fn full_image_inference_cached_at_demosaic() {
    let image = common::synthetic(19, 270, 32, common::RGGB, [0, 0, 270, 32]);
    let backend = Arc::new(Counting(AtomicUsize::new(0)));
    let renderer =
        Renderer::new(RendererConfig::default()).with_post_demosaic_denoise(backend.clone());
    let mut s = enabled();
    let rect = PixelRect::full(image.level_extent(0));
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    s.tone.exposure = 1.0;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    s.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    s.denoise.amount = 25.0;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 2);
    s.denoise.amount = 0.0;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    s.denoise.method = DenoiseMethod::Off;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 2);
}

fn enabled() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.denoise.method = DenoiseMethod::Neural {
        model: ModelRef {
            id: pipeline_cpu::POST_DENOISE_MODEL_ID.into(),
            version: pipeline_cpu::POST_DENOISE_VERSION.into(),
        },
        joint_demosaic: false,
    };
    s
}
#[test]
fn denoise_dirties_demosaic_not_reserved_raw_stage() {
    assert_eq!(
        PipelineGraph::earliest_dirty_stage(&DevelopSettings::default(), &enabled()),
        Some(StageId::Demosaic)
    );
}
