use engine_api::{recipe::settings::ColorSettings, tile::TileCoord};
use pipeline_cpu::{Image, color};
fn settings() -> ColorSettings {
    serde_json::from_value(serde_json::json!({"point_colors":[{
        "hue_shift":30., "range":50., "selection":{
            "source_hsl":[0.,0.5,0.5], "hue":[0.,0.25,0.75,1.],
            "saturation":[0.,0.25,0.75,1.], "luminance":[0.,0.25,0.75,1.]
        }
    }]}))
    .unwrap()
}
fn render(rgb: [f32; 3], s: &ColorSettings) -> [f32; 3] {
    let mut t = Image::new(1, 1, rgb.map(|v| vec![v]).to_vec())
        .unwrap()
        .tile(TileCoord::new(0, 0, 0), 0, 1)
        .unwrap();
    color(&mut t, s).unwrap();
    t.samples::<f32>().unwrap().try_into().unwrap()
}
#[test]
fn point_color_reference_swatch_full_half_feather_and_excluded() {
    // Linear working RGB HSL: C=.5, m=.25; hue 0 -> 30 degrees gives X=.25.
    // At S=.125, smoothstep(.5)=.5 on the saturation feather: hue -> 15 degrees.
    for (rgb, expected) in [
        ([0.75, 0.25, 0.25], [0.75, 0.5, 0.25]),
        ([0.5625, 0.4375, 0.4375], [0.5625, 0.46875, 0.4375]),
        ([0.25, 0.75, 0.75], [0.25, 0.75, 0.75]),
    ] {
        let out = render(rgb, &settings());
        for (a, b) in out.into_iter().zip(expected) {
            assert!((a - b).abs() < 2e-6, "{out:?} != {expected:?}");
        }
    }
}
#[test]
fn point_color_noop_is_bit_exact_and_settings_are_accepted() {
    let mut s = settings();
    s.point_colors[0].hue_shift = 0.;
    assert_eq!(render([0.75, 0.25, 0.25], &s), [0.75, 0.25, 0.25]);
    let mut develop = engine_api::recipe::DevelopSettings::default();
    develop.color = settings();
    pipeline_cpu::validate_settings(&develop).unwrap();
}
#[test]
fn point_color_shifts_saturation_luminance_and_wraps_hue() {
    let mut s = settings();
    s.point_colors[0].hue_shift = -30.;
    s.point_colors[0].saturation_shift = -50.;
    s.point_colors[0].luminance_shift = 20.;
    // H=330, S=.25, L=.6 => C=.2, X=.1, m=.5.
    let out = render([0.75, 0.25, 0.25], &s);
    for (a, b) in out.into_iter().zip([0.7, 0.5, 0.6]) {
        assert!((a - b).abs() < 2e-6);
    }
}
#[test]
fn point_color_rejects_invalid_ranges_without_mutating_tile() {
    let mut v = serde_json::to_value(settings()).unwrap();
    v["point_colors"][0]["selection"]["hue"] = serde_json::json!([0., 0.9, 0.25, 1.]);
    let s: ColorSettings = serde_json::from_value(v).unwrap();
    let mut t = Image::new(1, 1, vec![vec![0.75], vec![0.25], vec![0.25]])
        .unwrap()
        .tile(TileCoord::new(0, 0, 0), 0, 1)
        .unwrap();
    let before = t.samples::<f32>().unwrap().to_vec();
    assert!(color(&mut t, &s).is_err());
    assert_eq!(t.samples::<f32>().unwrap(), before);
}
