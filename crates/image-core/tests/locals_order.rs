mod common;
use common::*;
use engine_api::recipe::{DevelopSettings, LocalAdjustment, LocalParams};
use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig};

#[test]
fn empty_masks_are_identity_and_groups_add_deltas_from_prelocal_color() {
    let raw = synthetic(910, 32, 24, RGGB, [0, 0, 32, 24]);
    let r = Renderer::new(RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    });
    let rect = PixelRect::full(raw.active_extent());
    let render = |s: &DevelopSettings| {
        r.render_region_as(&raw, s, 0, rect, RenderOutput::SceneLinear)
            .unwrap()[0]
            .samples::<f32>()
            .unwrap()
            .to_vec()
    };
    let mut s = DevelopSettings::default();
    s.color.saturation = 25.;
    let before = render(&s);
    let group = LocalAdjustment {
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    };
    s.locals.adjustments.push(group.clone());
    assert_eq!(before, render(&s), "empty component stack selects nothing");
    let full = vec![engine_api::recipe::MaskComponent::new(
        engine_api::recipe::MaskKind::Radial {
            center: [0.5, 0.5],
            radii: [1., 1.],
            angle: 0.,
            feather: 0.,
        },
    )];
    s.locals.adjustments[0].components = full.clone();
    let second = LocalAdjustment {
        components: full,
        params: LocalParams {
            exposure: -1.,
            ..Default::default()
        },
        ..group
    };
    s.locals.adjustments.push(second);
    let forward = render(&s);
    // +1 EV contributes +base; -1 EV contributes -0.5*base: total 1.5*base.
    for (a, b) in forward.iter().zip(&before) {
        assert!((a - b * 1.5).abs() <= 1e-5);
    }
    s.locals.adjustments.reverse();
    let reverse = render(&s);
    for (a, b) in forward.iter().zip(reverse) {
        assert!((a - b).abs() <= 1e-6);
    }
    s.locals
        .adjustments
        .iter_mut()
        .for_each(|g| g.enabled = false);
    assert_eq!(before, render(&s));
}
