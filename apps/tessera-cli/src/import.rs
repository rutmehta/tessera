use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

/// Persist a lossless import bundle, never sidecars beside source originals.
/// Recipes are keyed by catalog id to retain virtual copies and duplicate stems.
pub fn apply(source: &Path, dest: &Path) -> Result<Value> {
    ensure!(
        !dest.try_exists()?,
        "destination already exists: {}",
        dest.display()
    );
    let plan = import_lrcat::import(source)?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    plan.library.write(staging.path().join("library.json"))?;
    std::fs::write(
        staging.path().join("import-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    let recipes = staging.path().join("recipes");
    std::fs::create_dir(&recipes)?;
    for image in &plan.images {
        sidecar::Sidecar::write_recipe(
            recipes.join(format!("{}.json", image.catalog_id)),
            &sidecar::RecipeDocument {
                recipe: image.recipe.clone(),
                ..Default::default()
            },
        )?;
    }
    // Exclusive reservation prevents replacing any pre-existing destination.
    std::fs::create_dir(dest).context("reserve import destination")?;
    if let Err(error) = std::fs::rename(staging.path(), dest) {
        let _ = std::fs::remove_dir(dest);
        return Err(error.into());
    }
    Ok(json!({"dest":dest,"images":plan.images.len(),"report":plan.report}))
}
