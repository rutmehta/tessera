#[path = "../../export/tests/support/gain_map_fixture.rs"]
mod gain_map_fixture;
use tessera_ffi::{Engine, ExportTarget, ImageQuery};
#[test]
fn ffi_gain_map_exports_and_validates_options() {
    let normalized =
        tessera_ffi::normalize_export_settings(r#"{"format":"jpeg","gain_map":true}"#.into())
            .unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&normalized).unwrap()["gain_map"],
        true
    );
    let legacy = tessera_ffi::normalize_export_settings("{}".into()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&legacy).unwrap()["gain_map"],
        false
    );
    for bad in [
        r#"{"format":"png","gain_map":true}"#,
        r#"{"format":"original","gain_map":true}"#,
        r#"{"format":"jpeg","gain_map":true,"hdr":"pq"}"#,
        r#"{"gain_map":true,"color_space":"p3"}"#,
        r#"{"gain_map":true,"upscale":2}"#,
        r#"{"gain_map":true,"bit_depth":16}"#,
    ] {
        assert!(
            tessera_ffi::normalize_export_settings(bad.into()).is_err(),
            "{bad}"
        );
    }
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(80, 16, image::Rgb([255; 3]))
        .save(&source)
        .unwrap();
    let engine = Engine::open(dir.path().join("app").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(dir.path().to_string_lossy().into_owned())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let mut recipe: engine_api::recipe::Recipe =
        serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
    recipe
        .edit(engine_api::recipe::EditMeta::user("HDR", 0), |s| {
            s.output.hdr = true;
            s.output.hdr_headroom_stops = 2.;
        })
        .unwrap();
    engine
        .set_recipe_json(id.clone(), serde_json::to_string(&recipe).unwrap())
        .unwrap();
    let out = dir.path().join("out");
    let settings =
        serde_json::json!({"format":"jpeg","gain_map":true,"destination":out,"naming":"{name}"})
            .to_string();
    let report = engine
        .export_batch(
            ExportTarget::Images {
                image_ids: vec![id],
            },
            settings,
            None,
            None,
        )
        .unwrap();
    assert_eq!((report.exported, report.failed), (1, 0), "{report:?}");
    gain_map_fixture::check(&out.join("source.jpg"));
}
