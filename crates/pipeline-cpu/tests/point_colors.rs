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
    // Gamma-encoded working RGB HSL: C=.5, m=.25; hue 0 -> 30 degrees gives X=.25.
    // At S=.125, smoothstep(.5)=.5 on the saturation feather: hue -> 15 degrees.
    let mut max_error = 0.0_f32;
    for (rgb, expected) in [
        ([0.75, 0.25, 0.25], [0.75, 0.5, 0.25]),
        ([0.5625, 0.4375, 0.4375], [0.5625, 0.46875, 0.4375]),
        ([0.25, 0.75, 0.75], [0.25, 0.75, 0.75]),
    ] {
        let out = render(rgb.map(linear), &settings());
        for (a, b) in out.into_iter().zip(expected.map(linear)) {
            max_error = max_error.max((a - b).abs());
            assert!((a - b).abs() < 2e-6, "{out:?} != {expected:?}");
        }
    }
    println!("Point Color three-swatch max absolute error: {max_error:e} (tolerance 2e-6)");
}
#[test]
fn point_color_noop_is_bit_exact_and_settings_are_accepted() {
    let mut s = settings();
    s.point_colors[0].hue_shift = 0.;
    assert_eq!(render([0.75, 0.25, 0.25], &s), [0.75, 0.25, 0.25]);
    let develop = engine_api::recipe::DevelopSettings {
        color: settings(),
        ..Default::default()
    };
    pipeline_cpu::validate_settings(&develop).unwrap();
}
#[test]
fn point_color_shifts_saturation_luminance_and_wraps_hue() {
    let mut s = settings();
    s.point_colors[0].hue_shift = -30.;
    s.point_colors[0].saturation_shift = -50.;
    s.point_colors[0].luminance_shift = 20.;
    // H=330, S=.25, L=.6 => C=.2, X=.1, m=.5.
    let out = render([0.75, 0.25, 0.25].map(linear), &s);
    for (a, b) in out.into_iter().zip([0.7, 0.5, 0.6].map(linear)) {
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

#[test]
fn point_color_range_width_luminance_feather_and_hue_seam() {
    let s = settings();
    // Lum=.125 lies halfway through its left feather, so shift is 15 degrees.
    let out = render([0.1875, 0.0625, 0.0625].map(linear), &s);
    for (a, b) in out.into_iter().zip([0.1875, 0.09375, 0.0625].map(linear)) {
        assert!((a - b).abs() < 2e-6);
    }
    // Narrowing range to 25 maps S=.125 outside its support around source S=.5.
    let mut narrow = s.clone();
    narrow.point_colors[0].range = 25.;
    assert_eq!(
        render([0.5625, 0.4375, 0.4375].map(linear), &narrow),
        [0.5625, 0.4375, 0.4375].map(linear)
    );
    // A hue on the far side of the red seam must still receive full weight.
    let out = render([0.75, 0.25, 0.5].map(linear), &s); // H=330 -> 0.
    for (a, b) in out.into_iter().zip([0.75, 0.25, 0.25].map(linear)) {
        assert!((a - b).abs() < 2e-6);
    }
    for input in [[0.5, 0.5, 0.5], [2., 2., 2.]] {
        assert_eq!(render(input, &s), input);
    }
}

#[test]
fn native_point_color_keeps_oklch_semantics_and_signed_headroom() {
    let settings: ColorSettings = serde_json::from_value(serde_json::json!({"point_colors":[{
        "source_lch":[0.5,0.1,0.], "range":100., "hue_shift":90.,
        "saturation_shift":50., "luminance_shift":-20.
    }]}))
    .unwrap();
    assert!(
        serde_json::to_value(&settings).unwrap()["point_colors"][0]
            .get("selection")
            .is_none()
    );
    // Independent evaluation of the published inverse OkLab matrices:
    // [.5,.1,0] -> [.4,0,.15]; the negative blue result must not be clipped.
    let output = render([0.20274383, 0.08162952, 0.1170534], &settings);
    for (a, b) in output
        .into_iter()
        .zip([0.09972682, 0.05478067, -0.01778171])
    {
        assert!((a - b).abs() < 2e-6, "native output {output:?}");
    }
}

fn linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
#[test]
fn sampled_colour_always_gets_full_weight() {
    for (sat, lum) in [(0.9, 0.1), (0.9, 0.5), (0.95, 0.85), (0.1, 0.2), (0.5, 0.5)] {
        let mut s = settings();
        s.point_colors[0].selection.as_mut().unwrap().source_hsl = [0., sat, lum];
        let c = (1.0_f32 - (2.0_f32 * lum - 1.).abs()) * sat;
        let lo = lum - c / 2.;
        let input = [lo + c, lo, lo].map(linear);
        let expected = [lo + c, lo + c / 2., lo].map(linear);
        let output = render(input, &s);
        for (a, b) in output.into_iter().zip(expected) {
            assert!(
                (a - b).abs() < 2e-6,
                "S={sat} L={lum}: {output:?} != {expected:?}"
            );
        }
    }
}
#[test]
fn point_membership_uses_original_pixel() {
    let input = [0.75, 0.25, 0.25].map(linear);
    let mut s = settings();
    s.point_colors[0].hue_shift = 60.;
    let expected = render(input, &s);
    let mut second = s.point_colors[0].clone();
    second.selection.as_mut().unwrap().source_hsl[0] = 60.;
    second.selection.as_mut().unwrap().hue = [0.45, 0.49, 0.51, 0.55];
    s.point_colors.push(second);
    assert_eq!(render(input, &s), expected);
}
#[test]
fn point_color_is_continuous_across_sdr_ceiling() {
    let mut s = settings();
    let selection = s.point_colors[0].selection.as_mut().unwrap();
    selection.saturation = [0., 0., 1., 1.];
    selection.luminance = [0., 0., 1., 1.];
    let below = render([0.999, 0.2, 0.2], &s);
    let above = render([1.001, 0.2, 0.2], &s);
    assert!(
        (below[1] - above[1]).abs() < 0.003,
        "{below:?} vs {above:?}"
    );
    assert!((above[1] - 0.2).abs() > 0.05);
}

#[test]
fn lr1c_selected_point_preserves_negative_channel_residual() {
    let mut s = settings();
    let selection = s.point_colors[0].selection.as_mut().unwrap();
    selection.saturation = [0., 0., 1., 1.];
    selection.luminance = [0., 0., 1., 1.];
    let out = render([linear(0.75), 0., -0.125], &s);
    assert!((out[0] - linear(0.75)).abs() < 2e-6);
    assert!((out[1] - linear(0.375)).abs() < 2e-6);
    assert!((out[2] + 0.125).abs() < 2e-6);
}

#[test]
fn lr1c_point_selection_precedes_monochrome_in_operator_and_render() {
    let mut s = settings();
    s.monochrome = Some(engine_api::recipe::settings::MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    let input = [0.75, 0.25, 0.25].map(linear);
    // Full-weight red -> orange, then neutral Rec.2020 luminance.
    let expected = 0.2627 * linear(0.75) + 0.6780 * linear(0.5) + 0.0593 * linear(0.25);
    for sample in render(input, &s) {
        assert!((sample - expected).abs() < 2e-6, "{sample} vs {expected}");
    }
    let image = Image::new(1, 1, input.map(|v| vec![v]).to_vec()).unwrap();
    let develop = engine_api::recipe::DevelopSettings {
        color: s,
        ..Default::default()
    };
    let out =
        pipeline_cpu::render_linear_scaled(&develop, &pipeline_cpu::RenderSource::Rgb(&image), 1)
            .unwrap();
    for plane in out.planes() {
        assert!((plane[0] - expected).abs() < 2e-6);
    }
}

#[test]
fn lr1c_point_edit_invalidates_pre_curve_monochrome_cache() {
    let mut s = engine_api::recipe::DevelopSettings {
        color: settings(),
        ..Default::default()
    };
    s.color.monochrome = Some(engine_api::recipe::settings::MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    let mut changed = s.clone();
    changed.color.point_colors[0].hue_shift += 10.;
    assert_eq!(
        s.first_dirty_stage(&changed),
        Some(engine_api::stage::StageId::Tone)
    );
}

#[test]
fn lr1c_inactive_monochrome_keeps_point_only_noop_bit_exact() {
    let mut s = settings();
    s.point_colors[0].hue_shift = 0.;
    s.monochrome = Some(Default::default());
    let input = [0.20274383, 0.08162952, -0.1170534];
    assert_eq!(render(input, &s), input);
}
