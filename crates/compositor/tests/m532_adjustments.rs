mod common;
use common::*;
use compositor::{Adjustment, Depth, Layer, LayerKind};
use engine_api::tile::Extent;

fn render_adjustment(a: Adjustment, c: [f32; 4], extent: Extent) -> Vec<f32> {
    let mut d = doc(extent, Depth::F32);
    add(
        &mut d,
        None,
        layer_fn("input", extent, Depth::F32, |_, _| c),
    );
    add(&mut d, None, Layer::new("adjust", LayerKind::Adjustment(a)));
    render(&d)
}

#[test]
fn lookup_dither_is_spatial_repeatable_bounded_and_preserves_alpha() {
    let a: Adjustment = serde_json::from_value(json!({
        "kind":"color_lookup", "size":2, "data":vec![[0.5; 3]; 8], "dither":true
    }))
    .unwrap();
    let extent = Extent::new(260, 2);
    let out = render_adjustment(a.clone(), [0.2, 0.3, 0.4, 0.75], extent);
    assert_eq!(
        out,
        render_adjustment(a.clone(), [0.2, 0.3, 0.4, 0.75], extent)
    );
    assert_ne!(px(&out, 260, 0, 0), px(&out, 260, 1, 0));
    assert_ne!(px(&out, 260, 0, 0), px(&out, 260, 256, 0));
    assert_ne!(px(&out, 260, 0, 0), px(&out, 260, 0, 1));
    for p in out.as_chunks::<4>().0 {
        for v in &p[..3] {
            assert!((v - 0.5).abs() <= 0.5 / 255.0 + 1e-7);
        }
        assert_eq!(p[3], 0.75);
    }
    let mut value = serde_json::to_value(a).unwrap();
    value["dither"] = json!(false);
    let out = render_adjustment(
        serde_json::from_value(value).unwrap(),
        [0.2, 0.3, 0.4, 0.75],
        extent,
    );
    for p in out.as_chunks::<4>().0 {
        assert_eq!(p, &out[..4]);
        assert_close(p, &[0.5, 0.5, 0.5, 0.75], 1e-6, "undithered LUT");
    }
}
use serde_json::json;

#[test]
fn match_color_neutralize_removes_cast_without_changing_lightness() {
    let cast = [0.7, 0.4, 0.2];
    let a = Adjustment::match_color_from_pixels(compositor::LayerId(1), &[cast], &[cast]).unwrap();
    let mut value = serde_json::to_value(&a).unwrap();
    value["neutralize"] = json!(true);
    let neutral: Adjustment = serde_json::from_value(value.clone()).unwrap();
    let out = render_adjustment(neutral, opaque(cast), Extent::new(1, 1));
    assert!(
        (out[0] - out[1]).abs() < 2e-5 && (out[1] - out[2]).abs() < 2e-5,
        "{out:?}"
    );
    let decode = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = |rgb: &[f32]| {
        0.2126729 * decode(rgb[0]) + 0.7151522 * decode(rgb[1]) + 0.072175 * decode(rgb[2])
    };
    assert!(
        (y(&out) - y(&cast)).abs() < 2e-5,
        "neutralize must preserve Lab lightness"
    );
    assert_eq!(serde_json::to_value(&a).unwrap()["neutralize"], false);
    let mut legacy = serde_json::to_value(&a).unwrap();
    legacy.as_object_mut().unwrap().remove("neutralize");
    assert_eq!(serde_json::from_value::<Adjustment>(legacy).unwrap(), a);
    value["fade"] = json!(100.0);
    let faded = render_adjustment(
        serde_json::from_value(value).unwrap(),
        opaque(cast),
        Extent::new(1, 1),
    );
    assert_close(&faded, &opaque(cast), 1e-6, "fade overrides neutralize");
}

#[test]
fn auto_clip_percentages_default_and_roundtrip_without_changing_frozen_output() {
    let old = json!({"kind":"auto", "mode":"tone", "black":vec![0.1_f32; 3], "white":vec![0.9_f32; 3], "gamma":vec![1.0; 3]});
    let a: Adjustment = serde_json::from_value(old.clone()).unwrap();
    let value = serde_json::to_value(&a).unwrap();
    assert_eq!(value["shadow_clip"], 0.5);
    assert_eq!(value["highlight_clip"], 0.5);
    let mut custom = old;
    custom["shadow_clip"] = json!(2.0);
    custom["highlight_clip"] = json!(3.0);
    let b: Adjustment = serde_json::from_value(custom.clone()).unwrap();
    assert_eq!(serde_json::to_value(&b).unwrap(), custom);
    assert_eq!(
        Adjustment::from_versioned_json(&b.to_versioned_json().unwrap()).unwrap(),
        b
    );
    assert_eq!(
        render_adjustment(a, [0.4; 4], Extent::new(1, 1)),
        render_adjustment(b, [0.4; 4], Extent::new(1, 1))
    );
    for (shadow, highlight) in [(-1.0, 0.5), (0.5, -1.0), (100.0, 0.0), (60.0, 40.0)] {
        custom["shadow_clip"] = json!(shadow);
        custom["highlight_clip"] = json!(highlight);
        let invalid: Adjustment = serde_json::from_value(custom.clone()).unwrap();
        assert!(invalid.validate().is_err());
        assert!(Adjustment::from_versioned_json(&invalid.to_versioned_json().unwrap()).is_err());
    }
}

#[test]
fn auto_analysis_uses_independent_percent_tails_in_every_mode() {
    use compositor::adjust::AutoMode;
    let h = [
        vec![1, 10, 50, 20, 19],
        vec![1, 10, 50, 20, 19],
        vec![1, 10, 50, 20, 19],
    ];
    for mode in [AutoMode::Tone, AutoMode::Contrast, AutoMode::Color] {
        let a = Adjustment::auto_from_histogram_with_clips(mode, &h, 1.0, 19.0).unwrap();
        a.validate().unwrap();
        let value = serde_json::to_value(&a).unwrap();
        assert_eq!(value["black"], json!([0.25, 0.25, 0.25]));
        assert_eq!(value["white"], json!([0.75, 0.75, 0.75]));
        assert_eq!(value["shadow_clip"], 1.0);
        assert_eq!(value["highlight_clip"], 19.0);
        let legacy = Adjustment::auto_from_histogram(mode, &h, 0.0).unwrap();
        assert_eq!(
            legacy,
            Adjustment::auto_from_histogram_with_clips(mode, &h, 0.0, 0.0).unwrap()
        );
        let empty = [vec![0; 5], vec![0; 5], vec![0; 5]];
        let neutral = Adjustment::auto_from_histogram_with_clips(mode, &empty, 0.5, 0.5).unwrap();
        let value = serde_json::to_value(neutral).unwrap();
        assert_eq!(value["black"], json!([0.0, 0.0, 0.0]));
        assert_eq!(value["white"], json!([1.0, 1.0, 1.0]));
        assert_eq!(value["gamma"], json!([1.0, 1.0, 1.0]));
    }
    for (s, hclip) in [
        (f32::NAN, 0.0),
        (0.0, f32::INFINITY),
        (-1.0, 0.0),
        (70.0, 30.0),
    ] {
        assert!(Adjustment::auto_from_histogram_with_clips(AutoMode::Tone, &h, s, hclip).is_err());
    }
}

#[test]
fn neutralize_preserves_color_variation_and_obeys_fade() {
    let samples = [[0.7, 0.4, 0.2], [0.6, 0.5, 0.3], [0.8, 0.3, 0.4]];
    let a =
        Adjustment::match_color_from_pixels(compositor::LayerId(1), &samples, &samples).unwrap();
    let mut value = serde_json::to_value(a).unwrap();
    value["neutralize"] = json!(true);
    let a: Adjustment = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(
        Adjustment::from_versioned_json(&a.to_versioned_json().unwrap()).unwrap(),
        a
    );
    let c = opaque(samples[2]);
    let corrected = render_adjustment(a, c, Extent::new(1, 1));
    assert!(
        (corrected[0] - corrected[1]).abs() > 0.05,
        "cast correction must not desaturate all pixels"
    );
    value["fade"] = json!(50.0);
    let faded = render_adjustment(serde_json::from_value(value).unwrap(), c, Extent::new(1, 1));
    for i in 0..3 {
        assert!((faded[i] - (corrected[i] + c[i]) * 0.5).abs() < 1e-6);
    }
}

#[test]
fn legacy_auto_fraction_retains_histogram_boundary_precision() {
    let h = [vec![1, 49, 49, 1], vec![1, 49, 49, 1], vec![1, 49, 49, 1]];
    let a = Adjustment::auto_from_histogram(compositor::adjust::AutoMode::Tone, &h, 0.01).unwrap();
    let value = serde_json::to_value(a).unwrap();
    // The original API promotes 0.01_f32 to f64 before multiplying by total.
    // Its cut is just below one, not exactly one as in the percentage API.
    assert_eq!(value["black"], json!([0.0, 0.0, 0.0]));
    assert_eq!(value["white"], json!([1.0, 1.0, 1.0]));
    assert_eq!(value["shadow_clip"], 1.0);
    assert_eq!(value["highlight_clip"], 1.0);
}

#[test]
fn lookup_metadata_roundtrips_and_old_payload_defaults() {
    let old = json!({"kind":"color_lookup", "size":2, "data":vec![[0.5; 3]; 8]});
    let a: Adjustment = serde_json::from_value(old.clone()).unwrap();
    let encoded = serde_json::to_value(a).unwrap();
    assert_eq!(encoded["dither"], false);
    assert_eq!(encoded["source_filename"], serde_json::Value::Null);
    let mut new = old;
    new["source_filename"] = json!("Looks/暖かい.cube");
    new["dither"] = json!(true);
    let a: Adjustment = serde_json::from_value(new.clone()).unwrap();
    a.validate().unwrap();
    assert_eq!(serde_json::to_value(&a).unwrap(), new);
    assert_eq!(
        Adjustment::from_versioned_json(&a.to_versioned_json().unwrap()).unwrap(),
        a
    );
}
