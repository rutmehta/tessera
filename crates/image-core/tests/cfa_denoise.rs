mod common;
use engine_api::{
    id::ModelRef,
    recipe::{
        DevelopSettings,
        settings::{DemosaicMethod, DenoiseMethod, DenoiseSettings},
    },
    stage::StageId,
};
use image_core::{PipelineGraph, PixelRect, Renderer, RendererConfig};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
struct RawCounting(AtomicUsize);
impl pipeline_cpu::PostDemosaicDenoise for RawCounting {
    fn adapter_revision(&self) -> &str {
        "raw-counting-v1"
    }
    fn denoise(
        &self,
        _: &pipeline_cpu::Image,
        _: f32,
    ) -> engine_api::EngineResult<pipeline_cpu::Image> {
        panic!("Bayer must not use RGB")
    }
    fn denoise_raw(
        &self,
        input: &pipeline_cpu::Image,
        _: raw_decode::CfaLayout,
        _: &DenoiseSettings,
    ) -> engine_api::EngineResult<pipeline_cpu::Image> {
        assert_eq!(input.planes().len(), 1);
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(input.clone())
    }
}
#[test]
fn cfa_denoise_is_cached_before_demosaic() {
    assert!(PipelineGraph::m2().node(StageId::Denoise).implemented);
    let image = common::synthetic(20, 270, 32, common::RGGB, [0, 0, 270, 32]);
    let backend = Arc::new(RawCounting(AtomicUsize::new(0)));
    let renderer =
        Renderer::new(RendererConfig::default()).with_post_demosaic_denoise(backend.clone());
    let mut settings = DevelopSettings::default();
    settings.denoise.method = DenoiseMethod::Neural {
        model: ModelRef {
            id: "enhance/cfa-unet-fp32".into(),
            version: "a".repeat(64),
        },
        joint_demosaic: false,
    };
    assert_eq!(
        PipelineGraph::earliest_dirty_stage(&DevelopSettings::default(), &settings),
        Some(StageId::Denoise)
    );
    let rect = PixelRect::full(image.level_extent(0));
    renderer.render_region(&image, &settings, 0, rect).unwrap();
    assert_eq!(backend.0.load(Ordering::SeqCst), 1);
    settings.demosaic.method = DemosaicMethod::Bilinear;
    renderer.render_region(&image, &settings, 0, rect).unwrap();
    assert_eq!(
        backend.0.load(Ordering::SeqCst),
        1,
        "demosaic edits reuse raw denoise"
    );
    settings.denoise.amount = 20.0;
    renderer.render_region(&image, &settings, 0, rect).unwrap();
    assert_eq!(
        backend.0.load(Ordering::SeqCst),
        1,
        "Amount edits reuse full-strength inference"
    );
}
