use engine_api::recipe::{Recipe, SourceKind};
use serde_json::json;

#[test]
fn legacy_source_kind_is_promoted_and_normalized() {
    let legacy = include_bytes!("fixtures/recipe-1.2.json");
    let recipe = Recipe::from_json(legacy).unwrap();
    recipe.validate().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&recipe.to_json().unwrap()).unwrap();
    assert_eq!(value["source_kind"], "raw");
    assert_eq!(value["schema_version"], 3);
    for kind in ["raw", "rgb", "Raw", "Rgb"] {
        let bytes = serde_json::to_vec(&json!({"schema_version":2,"source_kind":kind,"future":17}))
            .unwrap();
        let recipe = Recipe::from_json(&bytes).unwrap();
        assert!(!recipe.unknown.contains_key("source_kind"));
        let value: serde_json::Value = serde_json::from_slice(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(value["source_kind"], kind.to_lowercase());
        assert_eq!(value["future"], 17);
        assert_eq!(
            Recipe::from_json(&recipe.to_json().unwrap()).unwrap(),
            recipe
        );
    }
}

#[test]
fn source_kind_separates_render_caches() {
    let raw = Recipe::from_json(br#"{"source_kind":"raw"}"#).unwrap();
    let rgb = Recipe::from_json(br#"{"source_kind":"rgb"}"#).unwrap();
    assert_ne!(raw.recipe_hash(), rgb.recipe_hash());
    assert_ne!(raw.stage_chain(), rgb.stage_chain());
}

#[test]
fn source_kind_schema_and_invalid_values() {
    for (kind, value) in [
        (SourceKind::Raw, json!("raw")),
        (SourceKind::Rgb, json!("rgb")),
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), value);
        assert_eq!(serde_json::from_value::<SourceKind>(value).unwrap(), kind);
    }
    for value in [json!(null), json!("cmyk"), json!(1), json!({})] {
        assert!(serde_json::from_value::<SourceKind>(value.clone()).is_err());
        assert!(
            Recipe::from_json(&serde_json::to_vec(&json!({"source_kind":value})).unwrap()).is_err()
        );
    }
}
