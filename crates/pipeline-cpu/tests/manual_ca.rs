use engine_api::recipe::{DevelopSettings, settings::LensProfileSource};
use pipeline_cpu::{
    Image, LensContext, ManualCaSettings, RenderSource, render_linear_scaled_with_lens,
};

#[test]
fn manual_ca_maps_crs_keys_independently() {
    let mapped = ManualCaSettings::from_crs([
        ("crs:ChromaticAberrationR", 25.),
        ("crs:ChromaticAberrationB", -10.),
        ("crs:AutoLateralCA", 0.),
    ])
    .unwrap();
    assert_eq!(mapped.red_cyan, 25.);
    assert_eq!(mapped.blue_yellow, -10.);
    assert!(ManualCaSettings::from_crs([("crs:ChromaticAberrationR", f32::NAN)]).is_err());
}

#[test]
fn nonfinite_manual_ca_rejected() {
    let image = Image::new(8, 8, vec![vec![0.1; 64]; 3]).unwrap();
    let context = LensContext {
        manual_ca: ManualCaSettings {
            red_cyan: f32::NAN,
            blue_yellow: 0.,
        },
        ..Default::default()
    };
    assert!(
        render_linear_scaled_with_lens(
            &DevelopSettings::default(),
            &RenderSource::Rgb(&image),
            1,
            &context
        )
        .is_err()
    );
}

#[test]
fn independent_manual_ca_works_without_auto_ca_or_profile() {
    let image = Image::new(
        32,
        24,
        vec![(0..768).map(|i| (i % 32) as f32 / 64.).collect(); 3],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.lens.chromatic_aberration_scale = 0.;
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let run = |manual_ca| {
        render_linear_scaled_with_lens(
            &s,
            &RenderSource::Rgb(&image),
            1,
            &LensContext {
                manual_ca,
                ..Default::default()
            },
        )
        .unwrap()
    };
    let baseline = run(ManualCaSettings::default());
    let red = run(ManualCaSettings {
        red_cyan: 100.,
        blue_yellow: 0.,
    });
    let blue = run(ManualCaSettings {
        red_cyan: 0.,
        blue_yellow: -100.,
    });
    assert_ne!(red.planes()[0], baseline.planes()[0]);
    assert!(
        red.planes()[1]
            .iter()
            .zip(&baseline.planes()[1])
            .all(|(a, b)| (a - b).abs() < 1e-7)
    );
    assert!(
        red.planes()[2]
            .iter()
            .zip(&baseline.planes()[2])
            .all(|(a, b)| (a - b).abs() < 1e-7)
    );
    assert_ne!(blue.planes()[2], baseline.planes()[2]);
    assert!(
        blue.planes()[0]
            .iter()
            .zip(&baseline.planes()[0])
            .all(|(a, b)| (a - b).abs() < 1e-7)
    );
    assert!(
        blue.planes()[1]
            .iter()
            .zip(&baseline.planes()[1])
            .all(|(a, b)| (a - b).abs() < 1e-7)
    );
}
