//! Synthetic LR-6 acceptance probes. No catalog or user image is read.
use engine_api::recipe::Recipe;

const LENS_BLUR: &str = r#"s = { LensBlur = {
    Version = 1, Active = true, BlurAmount = 37,
    FocalRange = "10 20 60 80", BokehShape = 0,
    BokehShapeDetail = 0, HighlightsBoost = 25, HighlightsThreshold = 50,
    CatEyeAmount = 0, CatEyeScale = 100,
    BokehAspect = 0, BokehRotation = 0, SphericalAberration = 0
} }"#;

fn assert_translated(recipe: &Recipe, warnings: &[String]) {
    let blur = recipe.settings.effects.lens_blur.as_ref().expect(
        "active Adobe LensBlur must populate the renderable recipe, not remain opaque source",
    );
    assert_eq!(blur.amount, 37.0);
    assert!(
        recipe
            .unknown
            .get("lrcat_develop_source")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.get("LensBlur"))
            .is_none(),
        "fully translated LensBlur must leave the per-key source bucket"
    );
    assert!(
        !warnings
            .iter()
            .any(|w| w.contains("LensBlur") && w.contains("cannot")),
        "translation still reports unsupported semantics: {warnings:?}"
    );
    recipe.validate().unwrap();
    let restored: Recipe = serde_json::from_slice(&serde_json::to_vec(recipe).unwrap()).unwrap();
    assert_eq!(restored, *recipe);
}

#[test]
fn lr6_lua_lens_blur_translates_without_opaque_source() {
    let (recipe, warnings) = import_lrcat::develop(1, LENS_BLUR, "15.4").unwrap();
    assert_translated(&recipe, &warnings);
}

#[test]
fn lr6_xmp_lens_blur_translates_without_opaque_source() {
    let source = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
        <rdf:Description><crs:LensBlur rdf:parseType="Resource"
            crs:Version="1" crs:Active="true" crs:BlurAmount="37"
            crs:FocalRange="10 20 60 80" crs:BokehShape="0"
            crs:BokehShapeDetail="0" crs:HighlightsBoost="25"
            crs:HighlightsThreshold="50" crs:CatEyeAmount="0"
            crs:CatEyeScale="100" crs:BokehAspect="0"
            crs:BokehRotation="0" crs:SphericalAberration="0"/>
        </rdf:Description></rdf:RDF>"#;
    let (recipe, warnings) = import_lrcat::develop(1, source, "15.4").unwrap();
    assert_translated(&recipe, &warnings);
}

#[test]
fn lr6_missing_adobe_depth_records_regeneration_in_recipe() {
    // No helper/resource is supplied to the pure develop decoder. This tests
    // bookkeeping only, not inference success or Adobe pixel equivalence.
    let source = r#"s = {
        LensBlur = { Active = true, BlurAmount = 37 },
        DepthMapInfo = { DepthSource = "synthetic-unavailable" }
    }"#;
    let (recipe, _) = import_lrcat::develop(1, source, "15.4").unwrap();
    assert!(
        serde_json::to_string(&recipe)
            .unwrap()
            .contains("regenerated depth"),
        "the regeneration marker must survive saving the recipe, not only an import report"
    );
}
