//! Catalog adapter for the dependency-neutral sidecar decoder.
pub(crate) fn apply<'a>(
    recipe: &mut engine_api::recipe::Recipe,
    warnings: &mut Vec<String>,
    properties: impl Iterator<Item = (&'a str, &'a str)>,
) -> engine_api::EngineResult<()> {
    for entry in sidecar::apply_adobe_geometry(recipe, warnings, properties)? {
        crate::diagnostics::push_approximate(
            recipe,
            &entry.adobe_key,
            &entry.field,
            "LR-7",
            &entry.reason,
        );
    }
    Ok(())
}

pub(crate) fn finish(recipe: &mut engine_api::recipe::Recipe) -> engine_api::EngineResult<()> {
    use engine_api::recipe::{Author, EditMeta};
    recipe.history.record(
        &recipe.history.base.clone(),
        &recipe.settings,
        EditMeta {
            label: "Import XMP".into(),
            author: Author::Import {
                source: "xmp".into(),
            },
            ..EditMeta::default()
        },
    )?;
    Ok(())
}
