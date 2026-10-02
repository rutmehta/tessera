use engine_api::recipe::{LocalAdjustment, MaskComponent, MaskKind, mask::MaskCombine};
use pipeline_cpu::Image;
#[test]
fn lr4b_external_composition_keeps_four_bounds_and_inverts_once() {
    let mut c = MaskComponent::new(MaskKind::LuminanceRange {
        luminance_domain: Default::default(),
        range: [0.25, 0.5],
        smoothness: 0.,
    });
    c.luminance_bounds = Some([0., 0.25, 0.5, 1.]);
    c.combine = MaskCombine::Intersect;
    c.invert = true;
    let g = LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Subject { model: None }), c],
        ..Default::default()
    };
    let input = Image::new(
        4,
        1,
        vec![vec![0.014349875, 0.05087609, 0.21404114, 0.52252155]; 3],
    )
    .unwrap();
    let plane = mask_ai::compose(&input, &g, |_, _, _| Ok(vec![0.5; 4].into())).unwrap();
    for (got, want) in plane.iter().zip([0.25, 0., 0., 0.25]) {
        assert!((got - want).abs() < 1e-6, "{got} != {want}");
    }
}
