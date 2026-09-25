#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::recipe::DevelopSettings;
use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn synthetic_renderer_with_default_detail_and_transfer_accounting() {
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    for (cfa, cache_dem) in [
        (common::RGGB, true),
        (common::xtrans(), true),
        (common::RGGB, false),
        (common::xtrans(), false),
    ] {
        let image = common::synthetic(71, 700, 533, cfa, [3, 5, 690, 521]);
        let mut s = DevelopSettings::default();
        s.tone.exposure = 0.3;
        s.tone.shadows = 15.0;
        let mut config = RendererConfig::default();
        config.graph = config
            .graph
            .with_cacheable(engine_api::stage::StageId::Demosaic, cache_dem);
        let r = Renderer::with_ops(
            gpu.clone(),
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config.clone(),
        );
        let cpu = Renderer::new(config);
        let rect = PixelRect::full(image.level_extent(0));
        for output in [RenderOutput::SceneLinear, RenderOutput::Display] {
            r.cache().clear();
            cpu.cache().clear();
            let a = r.render_region_as(&image, &s, 0, rect, output).unwrap();
            let b = cpu.render_region_as(&image, &s, 0, rect, output).unwrap();
            assert_eq!(a.len(), b.len());
            for (a, b) in a.iter().zip(b.iter()) {
                if output == RenderOutput::SceneLinear {
                    let diff = a
                        .samples::<f32>()
                        .unwrap()
                        .iter()
                        .zip(b.samples::<f32>().unwrap())
                        .map(|(a, b)| (a - b).abs())
                        .fold(0.0f32, f32::max);
                    assert!(diff <= 1e-4, "{diff}");
                } else {
                    assert!(
                        a.samples::<u8>()
                            .unwrap()
                            .iter()
                            .zip(b.samples::<u8>().unwrap())
                            .all(|(a, b)| a.abs_diff(*b) <= 1)
                    );
                }
            }
        }
        s.tone.exposure += 0.3;
        let before = gpu.stats();
        let tiles = r.render_region(&image, &s, 0, rect).unwrap();
        let after = gpu.stats();
        assert_eq!(
            after.uploads - before.uploads,
            7 * tiles.len() as u64,
            "base tone plus detail/tone/curves/color/effects/output uploads"
        );
        assert_eq!(
            after.readbacks - before.readbacks,
            7 * tiles.len() as u64,
            "each default M2 tile pass has one readback"
        );
        assert_eq!(
            after.submissions - before.submissions,
            4 + 3 * tiles.len() as u64,
            "four point-op batches plus per-tile detail/effects/output submissions"
        );
    }
}
