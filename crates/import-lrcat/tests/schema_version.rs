//! LR-SCHEMA: ordinary fixtures remain schema 3; imports using an LR-7
//! feature require and write schema 4.
mod common;

use engine_api::recipe::required_schema_version;

#[test]
fn imported_recipes_require_schema_3() {
    let dir = tempfile::tempdir().unwrap();
    let synthetic = common::write(dir.path(), 300);
    let fixture = import_lrcat::fixture::write(&dir.path().join("fx")).unwrap();
    let mut seen = 0;
    for catalog in [synthetic, fixture.catalog] {
        for image in import_lrcat::import(&catalog).unwrap().images {
            let recipe = &image.recipe;
            // Both known fixtures contain zero LR-7 schema-4 features.
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
