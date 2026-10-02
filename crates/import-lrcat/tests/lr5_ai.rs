use engine_api::recipe::Recipe;

#[test]
fn lr5_ai_categories_are_approximate_and_source_is_retained() {
    for category in ["Subject", "Sky", "Background", "People", "Hair", "FaceSkin", "Object"] {
        let source = format!("s = {{ MaskGroupBasedCorrections = {{ {{ LocalExposure2012 = 1, CorrectionMasks = {{ {{ What = 'Mask/Image', MaskType = '{category}', MaskDigest = 'opaque-id', Left = 0, Top = 0, Right = 1, Bottom = 1 }} }} }} }} }}");
        let (recipe, warnings) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert!(warnings.is_empty(), "{category}: {warnings:?}");
        assert_eq!(recipe.settings.locals.adjustments.len(), 1, "{category}");
        let json = serde_json::to_value(&recipe).unwrap();
        assert_eq!(json["settings"]["locals"]["adjustments"][0]["components"][0]["adobe_ai"]["resource_id"], "opaque-id");
        assert!(recipe.unknown.contains_key("lrcat_develop_source"));
        assert!(import_lrcat::diagnostics::entries(&recipe).values().flatten().any(|e| e.reason.contains("regenerated")));
        let restored = Recipe::from_json(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(restored.settings, recipe.settings);
        assert_eq!(restored.schema_version, 4);
    }
}
