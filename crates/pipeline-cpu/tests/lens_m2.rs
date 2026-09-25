use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};
#[test]
fn guided_upright_and_manual_transform_compose() {
    use engine_api::recipe::settings::{GuideLine, UprightMode};
    let image = image();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.geometry.upright.mode = UprightMode::Guided;
    s.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.],
            end: [0.1, 1.],
        },
        GuideLine {
            start: [0.8, 0.],
            end: [0.9, 1.],
        },
    ];
    s.geometry.transform.scale = 110.;
    let a = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert!(a.planes().iter().flatten().all(|v| v.is_finite()));
    s.geometry.upright.mode = UprightMode::Off;
    s.geometry.upright.guides.clear();
    let b = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert_ne!(a.planes(), b.planes());
}
fn image() -> Image {
    Image::new(
        32,
        24,
        vec![(0..768).map(|i| (i % 32) as f32 / 40.0 + 0.1).collect(); 3],
    )
    .unwrap()
}
#[test]
fn geometry_combines_manual_lens_and_transform() {
    let image = image();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.manual_distortion = 30.;
    s.geometry.transform.rotate = 3.;
    s.geometry.transform.vertical = 20.;
    s.geometry.crop.angle = 2.;
    s.geometry.crop.rect.left = 0.1;
    let out = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert_eq!(out.width(), 29);
    assert!(out.planes().iter().flatten().all(|x| x.is_finite()));
    assert_ne!(out.planes()[0][16], image.planes()[0][16]);
}
#[test]
fn defringe_hue_band_changes_edge_but_not_flat_colour() {
    let mut planes = vec![vec![0.05; 64 * 32]; 3];
    for y in 0..32 {
        for x in 32..64 {
            for (c, p) in planes.iter_mut().enumerate() {
                p[y * 64 + x] = if c == 1 { 0.1 } else { 1. };
            }
        }
    }
    let image = Image::new(64, 32, planes).unwrap();
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    let a = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    s.lens.defringe_purple.amount = 20.;
    let b = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    assert!(b.planes()[0][16 * 64 + 32] < a.planes()[0][16 * 64 + 32]);
    assert_eq!(b.planes()[0][16 * 64 + 50], a.planes()[0][16 * 64 + 50]);
}
#[test]
fn manual_vignette_runs_before_tone_and_off_is_exact() {
    let image = image();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    let source = RenderSource::Rgb(&image);
    let baseline = render_linear_scaled(&s, &source, 1).unwrap();
    let repeat = render_linear_scaled(&s, &source, 1).unwrap();
    assert_eq!(baseline.planes(), repeat.planes());
    s.lens.manual_vignetting = 50.;
    let corrected = render_linear_scaled(&s, &source, 1).unwrap();
    assert!(corrected.planes()[0][0] > baseline.planes()[0][0]);
}
