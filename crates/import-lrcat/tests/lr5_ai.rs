use engine_api::recipe::Recipe;

#[test]
fn lr5_ai_categories_are_approximate_and_source_is_retained() {
    for category in ["Subject", "Sky", "Background", "Object"] {
        let source = format!(
            "s = {{ MaskGroupBasedCorrections = {{ {{ LocalExposure2012 = 1, CorrectionMasks = {{ {{ What = 'Mask/Image', MaskType = '{category}', MaskDigest = 'opaque-id', Left = 0, Top = 0, Right = 1, Bottom = 1 }} }} }} }} }}"
        );
        let (recipe, warnings) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert!(warnings.is_empty(), "{category}: {warnings:?}");
        assert_eq!(recipe.settings.locals.adjustments.len(), 1, "{category}");
        let json = serde_json::to_value(&recipe).unwrap();
        assert_eq!(
            json["settings"]["locals"]["adjustments"][0]["components"][0]["adobe_ai"]["resource_id"],
            "opaque-id"
        );
        assert!(recipe.unknown.contains_key("lrcat_develop_source"));
        assert!(
            import_lrcat::diagnostics::entries(&recipe)
                .values()
                .flatten()
                .all(|e| !e.reason.contains("regenerated"))
        );
        let restored = Recipe::from_json(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(restored.settings, recipe.settings);
        assert_eq!(restored.schema_version, 4);
    }
}

#[test]
fn lr5_numeric_subtypes_and_unknown_byte_retention() {
    for (subtype, part, category) in [(1, 0, "Subject"), (2, 0, "Sky"), (0, 0, "Object")] {
        let source = format!(
            "s = {{ MaskGroupBasedCorrections = {{ {{ LocalExposure2012=1, CorrectionMasks={{ {{ What='Mask/Image', MaskSubType={subtype}, MaskSubCategoryID={part}, ReferencePoint='0.25 0.5', MaskDigest='synthetic', WholeImageArea='0/1,0/1,8/1,8/1', Origin='0,0', InputDigest='invented', ModelVersion=1 }} }} }} }} }}"
        );
        let (r, w) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(
            r.settings.locals.adjustments[0].components[0]
                .adobe_ai
                .as_ref()
                .unwrap()
                .category,
            category
        );
    }
    let source = "s = { MaskGroupBasedCorrections = { { CorrectionMasks={ { What='Mask/Image', MaskSubType=999, MaskDigest='exact-source' } } } } }";
    let (r, w) = import_lrcat::develop(1, source, "15.4").unwrap();
    assert!(r.settings.locals.adjustments.is_empty());
    assert!(!w.is_empty());
    assert!(import_lrcat::diagnostics::entries(&r).is_empty());
    assert!(
        String::from_utf8(r.to_json().unwrap())
            .unwrap()
            .contains("exact-source")
    );
}

#[test]
fn lr5b_person_parts_and_specific_people_are_unsupported() {
    for fields in [
        "MaskType='Hair'",
        "MaskType='Lips'",
        "MaskType='Teeth'",
        "MaskSubType=3,MaskSubCategoryID=4",
        "MaskType='People',PersonID=2",
        "MaskType='Person'",
    ] {
        let source = format!(
            "s={{MaskGroupBasedCorrections={{{{LocalExposure2012=1,CorrectionMasks={{{{What='Mask/Image',{fields}}}}}}}}}}}"
        );
        let (r, warnings) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert!(r.settings.locals.adjustments.is_empty(), "{fields}");
        assert!(!warnings.is_empty(), "{fields}");
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}
