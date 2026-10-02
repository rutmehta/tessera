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

#[test]
fn lr2b_legacy_and_hdr_reach_standalone_adobe_render() {
    use engine_api::recipe::settings::{Curve, CurvePoint, LegacyPv2010, ToneCurves};
    let s = DevelopSettings::default();
    let source = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let baseline = render_linear_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
    let mut legacy = s.clone();
    legacy.tone.legacy_pv2010 = Some(LegacyPv2010 {
        exposure: Some(1.),
        ..Default::default()
    });
    let result = render_linear_scaled(&legacy, &RenderSource::Rgb(&source), 1).unwrap();
    assert!(result.planes()[0][0] > baseline.planes()[0][0]);
    let mut hdr = s.clone();
    hdr.tone.curves_extended = Some(ToneCurves {
        rgb: Curve(vec![
            CurvePoint { x: 0., y: 0. },
            CurvePoint { x: 2., y: 3. },
        ]),
        ..Default::default()
    });
    let source = Image::new(4, 4, vec![vec![2f32.powf(2.2); 16]; 3]).unwrap();
    let result = render_linear_scaled(&hdr, &RenderSource::Rgb(&source), 1).unwrap();
    for x in result.planes().iter().flatten() {
        assert!((*x - 3f32.powf(2.2)).abs() < 0.0001, "{x}");
    }
}

#[test]
fn lr2c_channel_curve_toning_survives_monochrome_conversion() {
    use engine_api::recipe::settings::{Curve, CurvePoint, MonochromeSettings};
    let source = Image::new(4, 4, vec![vec![0.2; 16]; 3]).unwrap();
    let mut s = DevelopSettings::default();
    s.color.monochrome = Some(MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    s.tone.curves.red = Curve(vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 1., y: 0.5 },
    ]);
    let result = render_linear_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
    assert!((result.planes()[0][0] - result.planes()[1][0]).abs() > 0.01);
    let result = pipeline_cpu::render_linear_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
    assert!((result.planes()[0][0] - result.planes()[1][0]).abs() > 0.01);
}

#[test]
fn lr2e_hdr_curves_keep_parametric_sliders() {
    use engine_api::recipe::settings::{Curve, CurvePoint, ToneCurves};
    let source = Image::new(4, 4, vec![vec![0.18; 16]; 3]).unwrap();
    for active in [false, true] {
        let mut s = DevelopSettings::default();
        s.tone.curves_extended = Some(ToneCurves {
            rgb: if active {
                Curve(vec![
                    CurvePoint { x: 0., y: 0. },
                    CurvePoint { x: 2., y: 2.3 },
                ])
            } else {
                Curve::default()
            },
            ..Default::default()
        });
        let before = render_linear_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
        s.tone.curves.parametric.darks = 60.;
        s.tone.curves.parametric.lights = 60.;
        let after = render_linear_scaled(&s, &RenderSource::Rgb(&source), 1).unwrap();
        assert!(
            after.planes()[0][0] > before.planes()[0][0] + 0.001,
            "active={active}"
        );
    }
}
