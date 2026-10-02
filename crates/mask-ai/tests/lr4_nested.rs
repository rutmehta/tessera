use engine_api::recipe::mask::{LocalAdjustment, MaskCombine, MaskComponent, MaskKind};
use pipeline_cpu::Image;

#[test]
fn lr4_external_rasters_compose_inside_nested_groups() {
    let mut off = MaskComponent::new(MaskKind::Sky { model: None });
    off.enabled = false;
    let mut gradient = MaskComponent::new(MaskKind::Linear {
        start: [0., 0.],
        end: [1., 0.],
    });
    gradient.combine = MaskCombine::Intersect;
    let mut wrapper = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
    wrapper.group = Some(vec![
        off,
        MaskComponent::new(MaskKind::Subject { model: None }),
        gradient,
    ]);
    let group = LocalAdjustment {
        components: vec![wrapper],
        ..Default::default()
    };
    let input = Image::new(2, 1, vec![vec![0.25; 2]; 3]).unwrap();
    let mut calls = 0;
    let plane = mask_ai::compose(&input, &group, |kind, _, _| {
        assert!(matches!(kind, MaskKind::Subject { .. }));
        calls += 1;
        Ok(vec![0.5, 1.].into())
    })
    .unwrap();
    assert_eq!(plane, vec![0.375, 0.25]);
    assert_eq!(calls, 1);
}
