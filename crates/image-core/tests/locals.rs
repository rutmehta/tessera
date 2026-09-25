mod common;
use common::*;
use engine_api::{
    recipe::{DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind},
    stage::StageId,
};
use image_core::{PipelineGraph, PixelRect, RenderOutput, Renderer, RendererConfig};

#[test]
fn locals_stage_applies_masked_exposure_after_color() {
    assert!(PipelineGraph::m2().node(StageId::Locals).implemented);
    let raw = synthetic(908, 32, 24, RGGB, [0, 0, 32, 24]);
    let renderer = Renderer::new(RendererConfig::default());
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.;
    let rect = PixelRect::full(raw.active_extent());
    let before = renderer
        .render_region_as(&raw, &settings, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    settings.locals.adjustments.push(LocalAdjustment {
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
    let after = renderer
        .render_region_as(&raw, &settings, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    let a = before[0].samples::<f32>().unwrap();
    let b = after[0].samples::<f32>().unwrap();
    assert!(b[0] > a[0] * 1.8);
    assert!((b[31] - a[31]).abs() < a[31].abs() * 0.04 + 1e-5);
}

#[test]
fn graph_reuses_raster_for_sliders_but_not_upstream_or_mask_edits() {
    let raw = synthetic(909, 32, 24, RGGB, [0, 0, 32, 24]);
    let renderer = Renderer::new(RendererConfig::default());
    let rect = PixelRect::full(raw.active_extent());
    let mut s = DevelopSettings::default();
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    assert_eq!(renderer.mask_cache().stats().inserts, 0);
    renderer.cache().clear();
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Linear {
            start: [0., 0.],
            end: [1., 0.],
        })],
        params: LocalParams {
            exposure: 0.5,
            ..Default::default()
        },
        ..Default::default()
    });
    // The first masked frame starts cold; slider-only edits must reuse its raster.
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    let before = renderer.mask_cache().stats();
    s.locals.adjustments[0].params.exposure = 1.5;
    s.locals.adjustments[0].amount = 75.;
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    assert_eq!(renderer.mask_cache().stats().inserts, before.inserts);
    assert_eq!(renderer.mask_cache().stats().hits, before.hits + 1);
    s.tone.exposure = 0.25;
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    assert_eq!(renderer.mask_cache().stats().inserts, before.inserts + 1);
    s.locals.adjustments[0].invert = true;
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    assert_eq!(renderer.mask_cache().stats().inserts, before.inserts + 2);
    s.locals.adjustments.clear();
    renderer.render_region(&raw, &s, 0, rect).unwrap();
    assert_eq!(renderer.mask_cache().stats().inserts, before.inserts + 2);
}
