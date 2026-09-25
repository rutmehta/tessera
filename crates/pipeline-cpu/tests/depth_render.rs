use engine_api::recipe::{
    mask::{LocalAdjustment, MaskComponent, MaskKind},
    settings::DevelopSettings,
};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled_with_depth};
#[test]
fn supplied_depth_reaches_local_depth_masks() {
    let image = Image::new(2, 1, vec![vec![0.2; 2]; 3]).unwrap();
    let mut s = DevelopSettings::default();
    let mut local = LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Depth {
            range: [0.9, 1.],
            feather: 0.,
            model: None,
        })],
        ..Default::default()
    };
    local.params.exposure = 1.;
    s.locals.adjustments.push(local);
    let out = render_linear_scaled_with_depth(
        &s,
        &RenderSource::Rgb(&image),
        1,
        &Default::default(),
        &[0., 1.],
        Default::default(),
    )
    .unwrap();
    assert!((out.planes()[0][1] / out.planes()[0][0] - 2.).abs() < 0.01);
}
