use import_lrcat::lua_develop;
#[test]
fn lr4c_documented_keys_each_admit_a_renderable_approximation() {
    for (mask, correction) in [
        (r#"MaskSyncID="invented""#, ""),
        (r#"MaskName="invented""#, ""),
        ("MaskVersion=1", ""),
        ("MaskValue=0.5", ""),
        ("Midpoint=50", ""),
        ("Roundness=0", ""),
        ("", r#"CorrectionID="invented","#),
        ("", r#"CorrectionSyncID="invented","#),
        ("", "LocalToningHue=0,LocalToningSaturation=0,"),
    ] {
        let row = format!(
            r#"s={{MaskGroupBasedCorrections={{{{{correction}CorrectionMasks={{{{What="Mask/Gradient",FullX=0,FullY=0,ZeroX=1,ZeroY=0,{mask}}}}}}}}}}}"#
        );
        let (r, w) = lua_develop::parse(&row, "15.4").unwrap();
        assert!(w.is_empty(), "{mask} {correction}: {w:?}");
        assert_eq!(r.settings.locals.adjustments.len(), 1);
        assert!(!import_lrcat::diagnostics::entries(&r).is_empty());
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"]
                .is_string()
        );
    }
}

#[test]
fn lr4c_neutral_mask_value_keeps_the_legacy_flat_envelope() {
    let row = r#"s={MaskGroupBasedCorrections={{CorrectionMasks={{What="Mask/Gradient",FullX=0,FullY=0,ZeroX=1,ZeroY=0,MaskValue=1}}}}}"#;
    let (r, _) = lua_develop::parse(row, "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    assert!(import_lrcat::diagnostics::entries(&r).is_empty());
    assert!(
        r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"].is_string()
    );
}

#[test]
fn lr4d_nested_diagnostics_name_populated_fields_not_fallback_brushes() {
    let row = r#"s={MaskGroupBasedCorrections={{CorrectionMasks={{What="Mask/Group",Masks={{What="Mask/Paint",Radius=0.1,Flow=0.5,CenterWeight=0.5,MaskValue=1,Dabs={"d 0.5 0.5"}}}}}}}}"#;
    let (recipe, warnings) = lua_develop::parse(row, "15.4").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let notes = import_lrcat::diagnostics::entries(&recipe);
    let dabs = &notes["MaskGroupBasedCorrections/Mask/Paint/Dabs"];
    assert_eq!(dabs.len(), 1);
    assert_eq!(
        dabs[0].field,
        "/settings/locals/adjustments/0/components/0/group/0/strokes"
    );
    let json = serde_json::to_value(&recipe).unwrap();
    for note in notes.values().flatten() {
        let field = json.pointer(&note.field).expect("translated recipe field");
        assert!(!field.is_null());
        assert!(!field.as_array().is_some_and(Vec::is_empty));
    }
}
