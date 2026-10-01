use engine_api::recipe::{Recipe, max_writable_schema_version};
use sidecar::{RecipeDocument, Sidecar};
use std::collections::BTreeMap;

#[test]
fn recipe_atomic_roundtrip_and_schema_guard() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/.scratch-recipe");
    let paths = Sidecar::paths(dir.join("photo.CR3"));
    assert_eq!(paths.xmp, dir.join("photo.CR3.xmp"));
    assert_eq!(paths.recipe, dir.join(".edits/photo.json"));
    let mut doc = RecipeDocument {
        recipe: Recipe::default(),
        vector_clock: BTreeMap::from([("mac".into(), 7)]),
        last_writer: Default::default(),
    };
    doc.recipe
        .unknown
        .insert("future_member".into(), serde_json::json!({"a": [1,2]}));
    doc.recipe
        .edit(engine_api::recipe::EditMeta::user("CA", 1), |s| {
            s.lens.legacy_ca_red = Some(35.)
        })
        .unwrap();
    Sidecar::write_recipe(&paths.recipe, &doc).unwrap();
    let first = std::fs::read(&paths.recipe).unwrap();
    let read = Sidecar::read_recipe(&paths.recipe).unwrap();
    assert_eq!(read.recipe.schema_version, 4);
    let mut expected = doc.clone();
    expected.recipe.schema_version = read.recipe.schema_version;
    assert_eq!(expected, read);
    Sidecar::write_recipe(&paths.recipe, &read).unwrap();
    assert_eq!(first, std::fs::read(&paths.recipe).unwrap());
    doc.recipe.schema_version = max_writable_schema_version() + 1;
    assert!(Sidecar::write_recipe(&paths.recipe, &doc).is_err());
    assert_eq!(first, std::fs::read(&paths.recipe).unwrap());
    std::fs::remove_dir_all(dir).unwrap();
}
