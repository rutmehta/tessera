//! LR-SCHEMA: ordinary fixtures remain schema 3; imports using an LR-7
//! feature require and write schema 4.
mod common;

use engine_api::recipe::{required_schema_version, v4_features_used};

#[test]
fn imported_recipes_require_schema_3() {
    let dir = tempfile::tempdir().unwrap();
    let synthetic = common::write(dir.path(), 300);
    let fixture = import_lrcat::fixture::write(&dir.path().join("fx")).unwrap();
    let mut seen = 0;
    for catalog in [synthetic, fixture.catalog] {
        for image in import_lrcat::import(&catalog).unwrap().images {
            let recipe = &image.recipe;
            // Both known fixtures contain zero schema-4 features from any lane.
            assert!(v4_features_used(recipe).is_empty());
            let expected = 3;
            assert_eq!(required_schema_version(recipe), expected);
            let written: serde_json::Value =
                serde_json::from_slice(&recipe.to_json().unwrap()).unwrap();
            assert_eq!(written["schema_version"], expected);
            seen += 1;
        }
    }
    assert!(seen > 300);
}

#[test]
fn lr7d_synthetic_feature_imports_write_v4() {
    for (row, version) in [
        ("s = { ChromaticAberrationR = 35 }", "5.7"),
        ("s = { ChromaticAberrationB = -25 }", "5.7"),
        (
            "s = { PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0.2,0,1' }",
            "15.4",
        ),
    ] {
        let (r, _) = import_lrcat::lua_develop::parse(row, version).unwrap();
        assert_eq!(required_schema_version(&r), 4);
        let value: serde_json::Value = serde_json::from_slice(&r.to_json().unwrap()).unwrap();
        assert_eq!(value["schema_version"], 4);
    }
}

#[test]
fn lr2f_synthetic_feature_imports_write_v4() {
    for (row, version, feature) in [
        ("s = { ConvertToGrayscale = true }", "15.4", "monochrome"),
        (
            "s = { ConvertToGrayscale = false, GrayMixerRed = 25 }",
            "15.4",
            "monochrome",
        ),
        (
            "s = { HDREditMode = 1, ExtendedToneCurvePV2012 = {0,0,255,300,510,600} }",
            "15.4",
            "curves_extended",
        ),
        ("s = { Exposure = 1 }", "5.7", "legacy_pv2010"),
    ] {
        let (recipe, _) = import_lrcat::lua_develop::parse(row, version).unwrap();
        assert_eq!(v4_features_used(&recipe), vec![feature]);
        assert_eq!(required_schema_version(&recipe), 4);
        let value: serde_json::Value = serde_json::from_slice(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(value["schema_version"], 4);
    }
}
