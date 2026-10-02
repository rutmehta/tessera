use engine_api::{
    recipe::settings::ColorSettings,
    tile::{Extent, Tile, TileCoord, TileLayout},
};

fn pixel(rgb: [f32; 3], settings: &ColorSettings) -> [f32; 3] {
    let mut tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        },
        rgb.to_vec(),
    )
    .unwrap();
    pipeline_cpu::color(&mut tile, settings).unwrap();
    tile.samples::<f32>().unwrap().try_into().unwrap()
}

#[test]
fn grayscale_swatch_reference_and_inactive_identity() {
    let s: ColorSettings =
        serde_json::from_value(serde_json::json!({"monochrome":{"enabled":true,"mixer":{}}}))
            .unwrap();
    // Neutral mixer uses linear Rec.2020 luminance, with absolute error <= 1e-6.
    for (rgb, y) in [
        ([1., 0., 0.], 0.2627),
        ([0., 1., 0.], 0.6780),
        ([0., 0., 1.], 0.0593),
        ([0.18; 3], 0.18),
    ] {
        let out = pixel(rgb, &s);
        for v in out {
            assert!((v - y).abs() < 1e-6, "{out:?} expected {y}");
        }
    }
    let off: ColorSettings = serde_json::from_value(
        serde_json::json!({"monochrome":{"enabled":false,"mixer":{"red":100}}}),
    )
    .unwrap();
    assert_eq!(pixel([0.3, 0.1, 0.9], &off), [0.3, 0.1, 0.9]);
}

#[test]
fn mixer_changes_colored_swatches_but_not_neutrals() {
    let s: ColorSettings = serde_json::from_value(serde_json::json!({"monochrome":{"enabled":true,"mixer":{"red":50,"green":50,"blue":50,"orange":50,"yellow":50,"aqua":50,"purple":50,"magenta":50}}})).unwrap();
    // All bands +50 => 1.5x luminance on fully saturated RGB; neutral unaffected.
    for (rgb, y) in [
        ([1., 0., 0.], 0.39405),
        ([0., 1., 0.], 1.017),
        ([0., 0., 1.], 0.08895),
        ([0.18; 3], 0.18),
    ] {
        for v in pixel(rgb, &s) {
            assert!((v - y).abs() < 1e-6, "{v} expected {y}");
        }
    }
}

#[test]
fn lr2e_extended_curves_compose_with_ordinary_parametric() {
    use engine_api::recipe::settings::{Curve, CurvePoint, ToneCurves, ToneSettings};
    let input = pipeline_cpu::Image::new(3, 1, vec![vec![0.08, 0.18, 0.4]; 3]).unwrap();
    for active in [false, true] {
        let mut s = ToneSettings::default();
        s.curves.parametric.darks = 55.;
        s.curves.parametric.lights = 35.;
        let param_only = pipeline_cpu::tone_extra_image(&input, &s).unwrap();
        let extended = ToneCurves {
            rgb: if active {
                Curve(vec![
                    CurvePoint { x: 0., y: 0. },
                    CurvePoint { x: 2., y: 2.3 },
                ])
            } else {
                Curve::default()
            },
            ..Default::default()
        };
        s.curves_extended = Some(extended.clone());
        let actual = pipeline_cpu::tone_extra_image(&input, &s).unwrap();
        let expected = pipeline_cpu::tone_extra_image(
            &param_only,
            &ToneSettings {
                curves_extended: Some(extended),
                ..Default::default()
            },
        )
        .unwrap();
        for (a, b) in actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
        {
            assert!((a - b).abs() < 1e-6, "active={active}: {a} vs {b}");
        }
        assert_ne!(actual.planes(), input.planes());
    }
}
