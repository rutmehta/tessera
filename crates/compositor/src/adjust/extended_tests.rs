use super::*;

#[test]
fn gradient_dither_is_spatial_repeatable_and_bounded() {
    let a = Adjustment::GradientMap {
        stops: vec![],
        dither: true,
        reverse: false,
        method: GradientMethod::Classic,
    };
    let compiled = a.compile();
    let c = [0.5; 3];
    let origin = compiled.apply(c);
    assert_eq!(origin, compiled.apply_at(c, 0, 0));
    let row: Vec<_> = (0..260).map(|x| compiled.apply_at(c, x, 0)).collect();
    assert!(row.windows(2).any(|p| p[0] != p[1]));
    assert_ne!(&row[..4], &row[256..260]);
    assert_ne!(origin, compiled.apply_at(c, 0, 1));
    // Reverse traversal and extreme coordinates exercise stable wrapping math.
    for (x, y) in (0..260).rev().map(|x| (x, 0)).chain([(u32::MAX, u32::MAX)]) {
        let out = compiled.apply_at(c, x, y);
        assert_eq!(out, compiled.apply_at(c, x, y));
        if y == 0 {
            assert_eq!(out, row[x as usize]);
        }
        for v in out {
            assert!((v - luma(c)).abs() <= 0.5 / 255.0);
        }
    }
}

#[test]
fn gradient_without_dither_ignores_position() {
    for method in [
        GradientMethod::Classic,
        GradientMethod::Linear,
        GradientMethod::Perceptual,
    ] {
        let a = Adjustment::GradientMap {
            stops: vec![[0.0, 0.1, 0.2, 0.3], [1.0, 0.8, 0.9, 1.0]],
            dither: false,
            reverse: true,
            method,
        };
        let compiled = a.compile();
        for c in [[0.5; 3], [0.1, 0.8, 0.2]] {
            for (x, y) in [(0, 0), (256, 257), (u32::MAX, u32::MAX)] {
                assert_eq!(compiled.apply(c), compiled.apply_at(c, x, y));
            }
        }
    }
}

#[test]
fn every_named_preset_round_trips() {
    let presets = [
        PhotoFilterPreset::Warming85,
        PhotoFilterPreset::WarmingLba,
        PhotoFilterPreset::Warming81,
        PhotoFilterPreset::Cooling80,
        PhotoFilterPreset::CoolingLbb,
        PhotoFilterPreset::Cooling82,
        PhotoFilterPreset::Red,
        PhotoFilterPreset::Orange,
        PhotoFilterPreset::Yellow,
        PhotoFilterPreset::Green,
        PhotoFilterPreset::Cyan,
        PhotoFilterPreset::Blue,
        PhotoFilterPreset::Violet,
        PhotoFilterPreset::Magenta,
        PhotoFilterPreset::Sepia,
        PhotoFilterPreset::DeepRed,
        PhotoFilterPreset::DeepBlue,
        PhotoFilterPreset::DeepEmerald,
        PhotoFilterPreset::DeepYellow,
        PhotoFilterPreset::Underwater,
    ];
    for p in presets {
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<PhotoFilterPreset>(&json).unwrap(), p);
        assert!(p.resolve(25.0, true).unwrap().validate().is_ok());
    }
    assert!(serde_json::from_str::<PhotoFilterPreset>(r#""unknown""#).is_err());
}
#[test]
fn match_layer_skips_transparent_pixels_and_rejects_empty() {
    use crate::{Depth, DocState, Layer, LayerKind, Raster, Rect};
    use engine_api::{id::LayerId, tile::Extent};
    let extent = Extent::new(2, 1);
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(extent), 1, |x, _, p| {
            *p = if x == 0 {
                [0.8, 0.3, 0.2, 1.0]
            } else {
                [1.0, 0.0, 1.0, 0.0]
            }
        })
        .unwrap();
    let mut layer = Layer::new("source", LayerKind::Pixel(raster));
    layer.id = LayerId(7);
    let mut doc = DocState::new(extent, Depth::F32);
    doc.root.push(std::sync::Arc::new(layer));
    let target = [[0.2; 3]];
    assert_eq!(
        Adjustment::match_color_from_layer(&doc, LayerId(7), &target).unwrap(),
        Adjustment::match_color_from_pixels(LayerId(7), &[[0.8, 0.3, 0.2]], &target).unwrap()
    );
    doc.layer_mut(LayerId(7), |l| {
        l.kind = LayerKind::Pixel(Raster::new(extent, 4, Depth::F32, 0.0))
    });
    assert!(Adjustment::match_color_from_layer(&doc, LayerId(7), &target).is_err());
}

#[test]
fn validate_rejects_nonfinite_direct_controls() {
    let cases = [
        Adjustment::Vibrance {
            vibrance: f32::NAN,
            saturation: 0.0,
        },
        Adjustment::PhotoFilter {
            color: [f32::INFINITY; 3],
            density: 10.0,
            preserve_luminosity: false,
        },
        Adjustment::GradientMap {
            stops: vec![[0.0, 0.0, f32::NEG_INFINITY, 0.0]],
            dither: false,
            reverse: false,
            method: GradientMethod::Classic,
        },
        Adjustment::ColorLookup {
            size: 2,
            data: vec![[f32::NAN; 3]; 8],
        },
        Adjustment::Exposure {
            exposure: 0.0,
            offset: 0.0,
            gamma: f32::NAN,
        },
    ];
    for a in cases {
        assert!(a.validate().is_err());
        assert!(Adjustment::from_versioned_json(&a.to_versioned_json().unwrap()).is_err());
    }
    for json in [
        r#"{"version":1,"adjustment":{"kind":"vibrance","vibrance":1e999,"saturation":0}}"#,
        r#"{"version":1,"adjustment":{"kind":"shadows_highlights","settings":{"black_clip":1}}}"#,
        r#"{"version":1,"adjustment":{"kind":"match_color","source_layer":0,"source_mean":[0,0,0],"source_std":[1,1,1],"target_mean":[0,0,0],"target_std":[1,1,1],"luminance":100,"color_intensity":100,"fade":0}}"#,
    ] {
        assert!(Adjustment::from_versioned_json(json).is_err());
    }
}

#[test]
fn match_color_from_layer_resolves_nested_source() {
    use crate::{Depth, DocState, GroupMode, Layer, LayerKind, Raster};
    use engine_api::{id::LayerId, tile::Extent};
    let extent = Extent::new(2, 1);
    let mut doc = DocState::new(extent, Depth::F32);
    let mut source = Layer::new(
        "source",
        LayerKind::Pixel(Raster::new(extent, 4, Depth::F32, 0.5)),
    );
    source.id = LayerId(42);
    doc.root.push(std::sync::Arc::new(
        Layer::group("nested", GroupMode::PassThrough).with_child(source),
    ));
    let target = [[0.2, 0.4, 0.6]];
    let a = Adjustment::match_color_from_layer(&doc, LayerId(42), &target).unwrap();
    assert_eq!(
        a,
        Adjustment::match_color_from_pixels(LayerId(42), &[[0.5; 3]; 2], &target).unwrap()
    );
    assert!(Adjustment::match_color_from_layer(&doc, LayerId(0), &target).is_err());
    assert!(Adjustment::match_color_from_layer(&doc, LayerId(99), &target).is_err());
    doc.layer_mut(LayerId(42), |l| {
        l.kind = LayerKind::Adjustment(Adjustment::Invert)
    });
    assert!(Adjustment::match_color_from_layer(&doc, LayerId(42), &target).is_err());
}

#[test]
fn preset_resolves_named_and_custom_colors() {
    let a = PhotoFilterPreset::Warming85.resolve(25.0, true).unwrap();
    assert_eq!(
        a,
        Adjustment::PhotoFilter {
            color: [236.0 / 255.0, 138.0 / 255.0, 0.0],
            density: 25.0,
            preserve_luminosity: true
        }
    );
    let p = PhotoFilterPreset::Custom([0.2, 0.4, 0.6]);
    let encoded = serde_json::to_string(&p).unwrap();
    assert_eq!(
        serde_json::from_str::<PhotoFilterPreset>(&encoded).unwrap(),
        p
    );
    assert_eq!(
        p.resolve(50.0, false).unwrap(),
        Adjustment::PhotoFilter {
            color: [0.2, 0.4, 0.6],
            density: 50.0,
            preserve_luminosity: false
        }
    );
    assert!(p.resolve(f32::NAN, false).is_err());
    assert!(p.resolve(101.0, false).is_err());
    assert!(
        PhotoFilterPreset::Custom([f32::INFINITY, 0.0, 0.0])
            .resolve(0.0, false)
            .is_err()
    );
}

#[test]
fn versioned_lookup_rejects_invalid_shape() {
    for (size, data) in [
        (0, vec![]),
        (1, vec![[0.0; 3]]),
        (2, vec![[0.0; 3]; 7]),
        (u32::MAX, vec![]),
    ] {
        let a = Adjustment::ColorLookup { size, data };
        assert!(a.validate().is_err());
        assert!(Adjustment::from_versioned_json(&a.to_versioned_json().unwrap()).is_err());
    }
}

#[test]
fn three_dl_loader_converts_blue_fastest_and_requires_scale() {
    let text = "0 1023\n0 0 0\n0 0 4095\n0 4095 0\n0 4095 4095\n4095 0 0\n4095 0 4095\n4095 4095 0\n4095 4095 4095\n";
    let a = Adjustment::color_lookup_from_3dl(text, 4095.0).unwrap();
    close(a.compile().apply([0.2, 0.4, 0.7]), [0.2, 0.4, 0.7]);
    assert!(Adjustment::color_lookup_from_3dl(text, 0.0).is_err());
    assert!(Adjustment::color_lookup_from_3dl("0 1023\n0 0 0", 4095.0).is_err());
}

#[test]
fn cube_loader_trilinear_axis_order_and_errors() {
    let text = "TITLE \"swap rb\"\nLUT_3D_SIZE 2\n0 0 0\n0 0 1\n0 1 0\n0 1 1\n1 0 0\n1 0 1\n1 1 0\n1 1 1\n";
    let a = Adjustment::color_lookup_from_cube(text).unwrap();
    close(a.compile().apply([0.2, 0.4, 0.8]), [0.8, 0.4, 0.2]);
    close(a.compile().apply([-1.0, 0.4, 2.0]), [1.0, 0.4, 0.0]);
    assert!(Adjustment::color_lookup_from_cube("LUT_3D_SIZE 2\n0 0 0").is_err());
    assert!(Adjustment::color_lookup_from_cube("LUT_1D_SIZE 2\n0 0 0\n1 1 1").is_err());
    assert!(Adjustment::color_lookup_from_cube("LUT_3D_SIZE 99999999").is_err());
}

#[test]
fn match_color_resolves_layer_lab_stats_and_fade() {
    let source = vec![[0.8, 0.3, 0.2]; 4];
    let target = vec![[0.2, 0.4, 0.6]; 4];
    let a =
        Adjustment::match_color_from_pixels(engine_api::id::LayerId(42), &source, &target).unwrap();
    close(a.compile().apply(target[0]), source[0]);
    let mut faded = a.clone();
    if let Adjustment::MatchColor {
        source_layer, fade, ..
    } = &mut faded
    {
        assert_eq!(*source_layer, 42);
        *fade = 100.0;
    }
    close(faded.compile().apply(target[0]), target[0]);
    assert!(
        Adjustment::match_color_from_pixels(engine_api::id::LayerId(42), &[], &target).is_err()
    );
}

#[test]
fn auto_resolves_percentiles_and_modes() {
    let h = [
        vec![0, 1, 2, 1, 0],
        vec![1, 1, 2, 0, 0],
        vec![0, 0, 2, 1, 1],
    ];
    let a = Adjustment::auto_from_histogram(AutoMode::Tone, &h, 0.0).unwrap();
    close(a.compile().apply([0.25, 0.0, 0.5]), [0.0; 3]);
    close(a.compile().apply([0.75, 0.5, 1.0]), [1.0; 3]);
    let a = Adjustment::auto_from_histogram(AutoMode::Contrast, &h, 0.0).unwrap();
    close(a.compile().apply([0.3, 0.4, 0.5]), [0.3, 0.4, 0.5]);
    let a = Adjustment::auto_from_histogram(AutoMode::Color, &h, 0.0).unwrap();
    if let Adjustment::Auto { gamma, .. } = a {
        assert_ne!(gamma, [1.0; 3]);
    } else {
        panic!();
    }
    assert!(Adjustment::auto_from_histogram(AutoMode::Tone, &h, 0.5).is_err());
}

#[test]
fn equalize_cdf_constructor_and_degenerate_histograms() {
    let h = [vec![0, 2, 1, 1], vec![0, 2, 1, 1], vec![0, 2, 1, 1]];
    let a = Adjustment::equalize_from_histogram(&h).unwrap();
    close(
        a.compile().apply([1.0 / 3.0, 2.0 / 3.0, 1.0]),
        [0.0, 0.5, 1.0],
    );
    let h = [vec![0, 4, 0], vec![0, 4, 0], vec![0, 4, 0]];
    close(
        Adjustment::equalize_from_histogram(&h)
            .unwrap()
            .compile()
            .apply([0.2, 0.5, 0.8]),
        [0.2, 0.5, 0.8],
    );
    assert!(Adjustment::equalize_from_histogram(&[vec![], vec![], vec![]]).is_err());
}

#[test]
fn gradient_methods_reverse_sort_and_dither() {
    let make = |method, reverse, dither| Adjustment::GradientMap {
        stops: vec![[1.0, 1.0, 1.0, 1.0], [0.0, 0.0, 0.0, 0.0]],
        method,
        reverse,
        dither,
    };
    close(
        make(GradientMethod::Classic, false, false)
            .compile()
            .apply([0.25; 3]),
        [0.25; 3],
    );
    close(
        make(GradientMethod::Classic, true, false)
            .compile()
            .apply([0.25; 3]),
        [0.75; 3],
    );
    close(
        make(GradientMethod::Linear, false, false)
            .compile()
            .apply([0.5; 3]),
        [0.735357; 3],
    );
    close(
        make(GradientMethod::Perceptual, false, false)
            .compile()
            .apply([0.5; 3]),
        [0.388573; 3],
    );
    let a = make(GradientMethod::Classic, false, true);
    let x = a.compile().apply([0.25; 3]);
    assert_ne!(x, [0.25; 3]);
    assert!((x[0] - 0.25).abs() < 1.0 / 255.0);
    assert_eq!(x, a.compile().apply([0.25; 3]));
}

#[test]
fn vibrance_chroma_and_neutral_invariants() {
    let gray = Adjustment::Vibrance {
        vibrance: 80.0,
        saturation: 0.0,
    };
    close(gray.compile().apply([0.4; 3]), [0.4; 3]);
    let a = Adjustment::Vibrance {
        vibrance: 0.0,
        saturation: 0.0,
    };
    close(a.compile().apply([0.4, 0.3, 0.6]), [0.4, 0.3, 0.6]);
    let a = Adjustment::Vibrance {
        vibrance: 0.0,
        saturation: -100.0,
    };
    let o = a.compile().apply([0.8, 0.2, 0.2]);
    assert!((o[0] - o[1]).abs() < 1e-5 && (o[1] - o[2]).abs() < 1e-5);
    let o = gray.compile().apply([0.4, 0.3, 0.6]);
    assert!(o[2] - o[1] > 0.3);
}

#[test]
fn replace_color_masks_distance_and_shifts_hsl() {
    let a = Adjustment::ReplaceColor {
        color: [1.0, 0.0, 0.0],
        fuzziness: 0.0,
        hue: 120.0,
        saturation: 0.0,
        lightness: 0.0,
    };
    close(a.compile().apply([1.0, 0.0, 0.0]), [0.0, 1.0, 0.0]);
    close(a.compile().apply([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0]);
    let b = Adjustment::ReplaceColor {
        color: [1.0, 0.0, 0.0],
        fuzziness: 100.0,
        hue: 120.0,
        saturation: 0.0,
        lightness: 0.0,
    };
    let o = b.compile().apply([0.9, 0.1, 0.0]);
    assert!(o[1] > 0.1 && o[0] < 0.9);
}

#[test]
fn selective_color_membership_absolute_and_relative() {
    let mut colors = [[0.0; 4]; 9];
    colors[0][0] = 20.0;
    let abs = Adjustment::SelectiveColor {
        colors,
        absolute: true,
    };
    let rel = Adjustment::SelectiveColor {
        colors,
        absolute: false,
    };
    close(abs.compile().apply([0.8, 0.2, 0.2]), [0.68, 0.2, 0.2]);
    close(rel.compile().apply([0.8, 0.2, 0.2]), [0.776, 0.2, 0.2]);
    close(abs.compile().apply([0.2, 0.8, 0.2]), [0.2, 0.8, 0.2]);
}

#[test]
fn black_white_hue_sliders_and_tint() {
    let a = Adjustment::BlackWhite {
        sliders: [40.0, 60.0, 40.0, 60.0, 20.0, 80.0],
        tint: None,
    };
    close(a.compile().apply([1.0, 0.0, 0.0]), [0.4; 3]);
    close(a.compile().apply([1.0, 1.0, 0.0]), [0.6; 3]);
    close(a.compile().apply([0.3; 3]), [0.3; 3]);
    let a = Adjustment::BlackWhite {
        sliders: [50.0; 6],
        tint: Some([0.8, 0.4, 0.0]),
    };
    close(a.compile().apply([1.0, 0.0, 0.0]), [1.0, 0.5, 0.0]);
}

#[test]
fn color_balance_tone_weights() {
    let a = Adjustment::ColorBalance {
        shadows: [20.0, 0.0, 0.0],
        midtones: [0.0, 20.0, 0.0],
        highlights: [0.0, 0.0, 20.0],
        preserve_luminosity: false,
    };
    close(a.compile().apply([0.5; 3]), [0.55, 0.6, 0.55]);
    close(a.compile().apply([0.0; 3]), [0.2, 0.0, 0.0]);
    let a = Adjustment::ColorBalance {
        shadows: [20.0, 0.0, 0.0],
        midtones: [0.0; 3],
        highlights: [0.0; 3],
        preserve_luminosity: true,
    };
    assert!((luma(a.compile().apply([0.4; 3])) - 0.4).abs() < 1e-5);
}

#[test]
fn photo_filter_multiplies_density_and_preserves_luma() {
    let a = Adjustment::PhotoFilter {
        color: [1.0, 0.5, 0.0],
        density: 50.0,
        preserve_luminosity: false,
    };
    close(a.compile().apply([0.4; 3]), [0.4, 0.3, 0.2]);
    let b = Adjustment::PhotoFilter {
        color: [1.0, 0.5, 0.0],
        density: 50.0,
        preserve_luminosity: true,
    };
    let o = b.compile().apply([0.4; 3]);
    assert!((0.299 * o[0] + 0.587 * o[1] + 0.114 * o[2] - 0.4).abs() < 1e-5);
}

#[test]
fn desaturate_uses_hsl_lightness_not_weighted_luma() {
    close(
        Adjustment::Desaturate.compile().apply([0.9, 0.2, 0.3]),
        [0.55; 3],
    );
}

#[test]
fn new_adjustments_round_trip_versioned_envelope() {
    use serde_json::json;
    let cases = vec![
        json!({"kind":"vibrance","vibrance":20.0,"saturation":10.0}),
        json!({"kind":"color_balance","shadows":[0,0,0],"midtones":[0,0,0],"highlights":[0,0,0],"preserve_luminosity":true}),
        json!({"kind":"black_white","sliders":[40,60,40,60,20,80],"tint":null}),
        json!({"kind":"photo_filter","color":[1,0.5,0],"density":25,"preserve_luminosity":true}),
        json!({"kind":"gradient_map","stops":[[0,0,0,0],[1,1,1,1]],"dither":true,"reverse":false,"method":"perceptual"}),
        json!({"kind":"selective_color","colors":[[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0],[0,0,0,0]],"absolute":true}),
        json!({"kind":"desaturate"}),
        json!({"kind":"equalize","maps":[[0,1],[0,1],[0,1]]}),
        json!({"kind":"auto","mode":"color","black":[0,0,0],"white":[1,1,1],"gamma":[1,1,1]}),
        json!({"kind":"match_color","source_layer":42,"source_mean":[0,0,0],"source_std":[1,1,1],"target_mean":[0,0,0],"target_std":[1,1,1],"luminance":100,"color_intensity":100,"fade":0}),
        json!({"kind":"replace_color","color":[1,0,0],"fuzziness":40,"hue":20,"saturation":0,"lightness":0}),
        json!({"kind":"color_lookup","size":2,"data":[[0,0,0],[1,0,0],[0,1,0],[1,1,0],[0,0,1],[1,0,1],[0,1,1],[1,1,1]]}),
        json!({"kind":"brightness_contrast","brightness":35,"contrast":10,"legacy":false}),
        json!({"kind":"shadows_highlights","settings":{}}),
    ];
    assert_eq!(cases.len(), 14);
    for value in cases {
        let a: Adjustment = serde_json::from_value(value).unwrap();
        let encoded = a.to_versioned_json().unwrap();
        assert_eq!(Adjustment::from_versioned_json(&encoded).unwrap(), a);
    }
    assert!(
        Adjustment::from_versioned_json(r#"{"version":99,"adjustment":{"kind":"invert"}}"#)
            .is_err()
    );
    assert_eq!(
        serde_json::from_str::<Adjustment>(r#"{"kind":"invert"}"#).unwrap(),
        Adjustment::Invert
    );
}

fn close(a: [f32; 3], b: [f32; 3]) {
    for i in 0..3 {
        assert!((a[i] - b[i]).abs() < 2e-4, "{a:?} != {b:?}");
    }
}
#[test]
fn brightness_preserves_endpoints_and_legacy_is_affine() {
    let modern = Adjustment::BrightnessContrast {
        brightness: 75.0,
        contrast: 40.0,
        legacy: false,
    };
    close(
        modern.compile().apply([0.0, 0.5, 1.0]),
        [
            0.0,
            {
                let x = 0.625_f32.powf(0.4_f32.exp2());
                x / (x + 0.375_f32.powf(0.4_f32.exp2()))
            },
            1.0,
        ],
    );
    let legacy = Adjustment::BrightnessContrast {
        brightness: 30.0,
        contrast: 0.0,
        legacy: true,
    };
    close(legacy.compile().apply([0.0, 0.5, 1.0]), [0.2, 0.7, 1.0]);
    let json = serde_json::to_string(&modern).unwrap();
    assert_eq!(serde_json::from_str::<Adjustment>(&json).unwrap(), modern);
}
