use engine_api::recipe::{Decision, Grade, RECIPE_SCHEMA_VERSION, Recipe};
use sidecar::{RecipeDocument, Sidecar};

#[test]
fn legacy_envelope_upgrades_recipe_without_changing_sync_metadata() {
    let mut document = RecipeDocument::default();
    document.record_write("workstation", 1234).unwrap();
    document.recipe.schema_version = 1;
    document.recipe.selection.decision = Decision::Reject;
    document.recipe.selection.grade = Some(Grade::Three);
    let bytes = serde_json::to_vec(&document).unwrap();
    let expected_recipe =
        Recipe::from_json(&serde_json::to_vec(&document.recipe).unwrap()).unwrap();
    let path = std::env::temp_dir().join(format!(
        "tessera-schema-upgrade-{}.json",
        std::process::id()
    ));
    std::fs::write(&path, &bytes).unwrap();
    let result = Sidecar::read_recipe(&path);
    std::fs::remove_file(&path).unwrap();
    let loaded = result.unwrap();
    assert_eq!(loaded.recipe.schema_version, RECIPE_SCHEMA_VERSION);
    assert_eq!(loaded.recipe, expected_recipe);
    assert_eq!(loaded.vector_clock, document.vector_clock);
    assert_eq!(loaded.last_writer, document.last_writer);
    assert_eq!(loaded.recipe.selection.grade, None);
    loaded.recipe.validate().unwrap();

    let decoded: RecipeDocument = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(decoded, loaded);
}
