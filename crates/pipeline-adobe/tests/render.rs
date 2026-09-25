use engine_api::recipe::{DevelopSettings, settings::DisplayTransform};
use pipeline_adobe::{Image, RenderSource, render_linear_scaled, render_scaled};

#[test]
fn standalone_render_and_linear_share_geometry_and_validate() {
    let source = Image::new(9, 7, vec![vec![0.18; 63]; 3]).unwrap();
    let mut s = DevelopSettings::default();
    s.tone.display_transform = DisplayTransform::AdobePv6Compat;
    let linear = render_linear_scaled(&s, &RenderSource::Rgb(&source), 4).unwrap();
    let display = render_scaled(&s, &RenderSource::Rgb(&source), 4).unwrap();
    assert_eq!((linear.width(), linear.height()), (3, 2));
    assert_eq!(display.dimensions(), (3, 2));
    assert!(display.pixels().all(|p| p[0] > 0 && p[0] < 255));
    assert!(render_scaled(&s, &RenderSource::Rgb(&source), 0).is_err());
    s.tone.exposure = f32::NAN;
    assert!(render_scaled(&s, &RenderSource::Rgb(&source), 1).is_err());
}

#[test]
fn exposure_changes_render_without_changing_input() {
    let source = Image::new(8, 8, vec![vec![0.18; 64]; 3]).unwrap();
    let s = DevelopSettings::default();
    let first = render_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
    let mut bright = s.clone();
    bright.tone.exposure = 1.;
    let second = render_scaled(&bright, &RenderSource::Rgb(&source), 1).unwrap();
    assert!(second.get_pixel(0, 0)[0] > first.get_pixel(0, 0)[0]);
    assert_eq!(source.planes()[0][0], 0.18);
}
