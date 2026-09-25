use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;
#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[test]
fn local_recipe_falls_back_from_resident_surface_and_matches_cpu() {
    use engine_api::{
        jobs::CancellationToken,
        recipe::{DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind},
    };
    use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(0)),
        RendererConfig::default(),
    );
    let raw = common::synthetic(911, 64, 48, common::RGGB, [0, 0, 64, 48]);
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Linear {
            start: [0., 0.],
            end: [1., 0.],
        })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    assert!(
        !renderer
            .render_to_surface(&raw, &s, 0, 0, &CancellationToken::new())
            .unwrap()
    );
    assert!(
        renderer
            .render_surface(&raw, &s, 0, 0, &CancellationToken::new())
            .unwrap()
            .is_none()
    );
    assert_eq!(gpu.stats().submissions, 0);
    let rect = PixelRect::full(raw.active_extent());
    let got = renderer
        .render_region_as(&raw, &s, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    let cpu = Renderer::new(RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    });
    let expected = cpu
        .render_region_as(&raw, &s, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    let max = got[0]
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(expected[0].samples::<f32>().unwrap())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max);
    assert!(max <= 2e-3, "{max}");
    assert!(gpu.stats().submissions > 0);
    assert_eq!(renderer.mask_cache().stats().inserts, 0); // zero budget
}

#[test]
fn oversized_local_blend_uses_cpu_instead_of_invalid_dispatch() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let n = 2048 * 1024;
    let base = Image::new(2048, 1024, vec![vec![0.2; n]; 3]).unwrap();
    let adjusted = Image::new(2048, 1024, vec![vec![0.8; n]; 3]).unwrap();
    let got = gpu.blend_local(&base, &adjusted, &vec![0.5; n]).unwrap();
    assert!((got.planes()[0][n - 1] - 0.5).abs() <= 1e-6);
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn local_blend_computes_on_gpu_with_linear_tolerance() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let n = 257 * 3;
    let base = Image::new(
        257,
        3,
        (0..3)
            .map(|c| {
                (0..n)
                    .map(|i| i as f32 / n as f32 * 4. - c as f32)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let adjusted = Image::new(
        257,
        3,
        base.planes()
            .iter()
            .map(|p| p.iter().map(|v| v * 1.7 + 0.1).collect())
            .collect(),
    )
    .unwrap();
    let mask: Vec<_> = (0..n).map(|i| (i % 11) as f32 / 10.).collect();
    let got = gpu.blend_local(&base, &adjusted, &mask).unwrap();
    let reference = pipeline_cpu::blend_local(&base, &adjusted, &mask).unwrap();
    for (actual, expected) in got
        .planes()
        .iter()
        .flatten()
        .zip(reference.planes().iter().flatten())
    {
        assert!((actual - expected).abs() <= 1e-4);
    }
    let mut max_error = 0f32;
    for c in 0..3 {
        for (i, alpha) in mask.iter().enumerate() {
            let expected =
                base.planes()[c][i] + alpha * (adjusted.planes()[c][i] - base.planes()[c][i]);
            max_error = max_error.max((got.planes()[c][i] - expected).abs());
        }
    }
    assert!(max_error <= 1e-4, "{max_error}");
    assert_eq!(gpu.stats().submissions, 1);
    assert_eq!(gpu.stats().readbacks, 1);
    assert_eq!(gpu.stats().uploads, 3);
    assert!(gpu.blend_local(&base, &adjusted, &[0.5]).is_err());
    let mut invalid = mask;
    invalid[0] = f32::NAN;
    assert!(gpu.blend_local(&base, &adjusted, &invalid).is_err());
    assert_eq!(gpu.stats().submissions, 1);
}
