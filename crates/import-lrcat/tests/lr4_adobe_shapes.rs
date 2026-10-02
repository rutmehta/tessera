use import_lrcat::lua_develop;
#[test]
fn lr4_adobe_aggregate_and_range_mask_spelling() {
    let lua = r#"s = { ProcessVersion = "15.4", MaskGroupBasedCorrections = {{ CorrectionMasks = {{
        What = "Mask/Aggregate", Masks = {{ What = "Mask/RangeMask", CorrectionRangeMask = { LumMin = 0.2, LumMax = 0.8 } }}
    }} }} }"#;
    let (r, _) = lua_develop::parse(lua, "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    let c = &r.settings.locals.adjustments[0].components[0];
    assert!(matches!(
        c.group.as_ref().unwrap()[0].kind,
        engine_api::recipe::MaskKind::LuminanceRange {
            luminance_domain: _,
            range: [0.2, 0.8],
            ..
        }
    ));
    assert!(r.unknown.contains_key("lrcat_develop_source"));
    assert!(!import_lrcat::diagnostics::entries(&r).is_empty());
}
#[test]
fn lr4_unverified_type_and_feather_keep_exact_source() {
    for range in [
        "Type = 9, LumMin = 0.2, LumMax = 0.8",
        "LumMin = 0.2, LumMax = 0.8, LumFeather = 0.5",
    ] {
        let lua = format!(
            r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{{{ CorrectionMasks = {{{{ What = "Mask/Range", CorrectionRangeMask = {{ {range} }} }}}} }}}} }}"#
        );
        let (r, _) = lua_develop::parse(&lua, "15.4").unwrap();
        assert!(r.unknown.contains_key("lrcat_develop_source"));
        if range.starts_with("Type") {
            assert!(r.settings.locals.adjustments.is_empty());
        }
    }
}
#[test]
fn lr4_unrenderable_local_params_are_not_source_promoted() {
    for local in [
        "LocalDefringe = 1",
        "LocalToningHue = 30, LocalToningSaturation = 0.5",
    ] {
        let lua = format!(
            r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{{{ {local}, CorrectionMasks = {{{{ What = "Mask/Gradient", MaskActive = false }}}} }}}} }}"#
        );
        let (r, _) = lua_develop::parse(&lua, "15.4").unwrap();
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}
