use engine_api::recipe::{DevelopSettings, settings::NormalizedRect};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

#[test]
fn m2_render_controls_and_crop_are_wired() {
    let image = Image::new(
        32,
        24,
        vec![(0..768).map(|i| 0.1 + (i % 31) as f32 / 40.0).collect(); 3],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.tone.texture = 70.0;
    s.color.grading.global.saturation = 30.0;
    s.detail.sharpening.amount = 80.0;
    s.effects.grain.amount = 20.0;
    s.geometry.crop.rect = NormalizedRect {
        left: 0.25,
        top: 0.25,
        right: 0.75,
        bottom: 0.75,
    };
    s.geometry.crop.angle = 3.0;
    let a = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert_eq!((a.width(), a.height()), (16, 12));
    assert!(a.planes().iter().flatten().all(|v| v.is_finite()));
    let b = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert_eq!(a.planes(), b.planes());
}
