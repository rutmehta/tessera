//! LR-SCHEMA: with no schema 4 feature registered, every imported recipe
//! (bundled fixture and synthetic catalog) requires and writes schema 3.
mod common;

use engine_api::recipe::{RECIPE_SCHEMA_VERSION, required_schema_version, v4_features_used};

#[test]
fn imported_recipes_require_schema_3() {
    let dir = tempfile::tempdir().unwrap();
    let synthetic = common::write(dir.path(), 300);
    let fixture = import_lrcat::fixture::write(&dir.path().join("fx")).unwrap();
    let mut seen = 0;
    for catalog in [synthetic, fixture.catalog] {
        for image in import_lrcat::import(&catalog).unwrap().images {
            let recipe = &image.recipe;
            assert_eq!(required_schema_version(recipe), RECIPE_SCHEMA_VERSION);
            assert!(v4_features_used(recipe).is_empty(), "{:?}", image.path);
            let written: serde_json::Value =
                serde_json::from_slice(&recipe.to_json().unwrap()).unwrap();
            assert_eq!(written["schema_version"], 3);
            seen += 1;
        }
    }
    assert!(seen > 300);
}
