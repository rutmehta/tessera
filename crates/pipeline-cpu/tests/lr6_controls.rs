use engine_api::recipe::settings::LensBlur;
use pipeline_cpu::{Image, lens_blur};
use serde_json::json;

#[test]
fn each_optional_control_roundtrips_and_renders() {
    let image = Image::new(
        21,
        21,
        vec![
            (0..441)
                .map(|i| if i % 13 == 0 { 4. } else { 0.1 })
                .collect();
            3
        ],
    )
    .unwrap();
    let base = json!({"amount":100.,"focus_range":[0.,0.1],"bokeh":"5-blade", "adobe":{"highlights_boost":60.,"cat_eye_amount":60.}});
    let baseline: LensBlur = serde_json::from_value(base.clone()).unwrap();
    let reference = lens_blur(&image, &[1.; 441], &baseline, Default::default()).unwrap();
    for (field, value, changes_pixels) in [
        ("focal_range", json!([0., 0.1, 0.2, 1.]), true),
        ("bokeh_shape_detail", json!(80.), true),
        ("highlights_boost", json!(90.), true),
        ("highlights_threshold", json!(20.), true),
        ("cat_eye_amount", json!(0.), true),
        ("cat_eye_scale", json!(10.), true),
        ("bokeh_aspect", json!(80.), true),
        ("bokeh_rotation", json!(40.), true),
        ("spherical_aberration", json!(-80.), true),
        ("version", json!("1"), false),
        ("bokeh_shape", json!(2.), false),
        ("focal_range_source", json!(1.), false),
        ("sampled_area", json!("0.2 0.3 0.4 0.5"), false),
        ("sampled_range", json!("20 40"), false),
        ("subject_range", json!("10 80"), false),
        ("active", json!(true), false),
    ] {
        let mut document = base.clone();
        document["adobe"][field] = value;
        let setting: LensBlur = serde_json::from_value(document).unwrap();
        let restored: LensBlur =
            serde_json::from_slice(&serde_json::to_vec(&setting).unwrap()).unwrap();
        assert_eq!(restored, setting, "{field}");
        let rendered = lens_blur(&image, &[0.7; 441], &restored, Default::default()).unwrap();
        let same_depth_reference =
            lens_blur(&image, &[0.7; 441], &baseline, Default::default()).unwrap();
        let max_diff = rendered.planes()[0]
            .iter()
            .zip(&same_depth_reference.planes()[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max);
        if changes_pixels {
            assert!(max_diff > 1e-6, "{field}: {max_diff}");
        } else {
            assert_eq!(rendered.planes(), same_depth_reference.planes(), "{field}");
        }
    }
    for field in [
        "depth_source",
        "base_raw_depth_table",
        "base_raw_depth_input_digest",
        "base_raw_depth_version",
        "base_layered_depth_table",
        "base_layered_depth_input_digest",
        "base_layered_depth_version",
        "base_highlight_guide_table",
        "base_highlight_guide_input_digest",
        "base_highlight_guide_version",
        "mask_key",
        "regenerate",
    ] {
        let mut document = base.clone();
        document["depth"] = json!({field: match field { "mask_key" => json!([7;32].to_vec()), "regenerate" => json!(true), _ => json!("opaque") }});
        let setting: LensBlur = serde_json::from_value(document).unwrap();
        let restored: LensBlur =
            serde_json::from_slice(&serde_json::to_vec(&setting).unwrap()).unwrap();
        assert_eq!(restored, setting);
        assert_eq!(
            lens_blur(&image, &[1.; 441], &restored, Default::default())
                .unwrap()
                .planes(),
            reference.planes(),
            "{field}: bookkeeping alone must not change pixels"
        );
    }
}

#[test]
fn absent_extensions_preserve_native_bytes_and_disabled_blur_is_identity() {
    let original =
        br#"{"amount":50.0,"focus_range":[0.0,0.1],"bokeh":"circle","depth_model":null}"#;
    let mut setting: LensBlur = serde_json::from_slice(original).unwrap();
    assert_eq!(serde_json::to_vec(&setting).unwrap(), original);
    setting.adobe = Some(engine_api::recipe::settings::AdobeLensBlur {
        active: Some(false),
        ..Default::default()
    });
    let image = Image::new(2, 1, vec![vec![-0., 1.]; 3]).unwrap();
    assert_eq!(
        lens_blur(&image, &[1.; 2], &setting, Default::default())
            .unwrap()
            .planes()[0][0]
            .to_bits(),
        (-0_f32).to_bits()
    );
}
