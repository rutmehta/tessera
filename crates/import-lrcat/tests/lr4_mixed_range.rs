use import_lrcat::lua_develop;
#[test]
fn lr4_unused_range_family_fields_remain_retained() {
    for fields in [
        "DepthMin = 0.2, DepthMax = 0.8, LumMin = 0.1",
        "LumMin = 0.2, LumMax = 0.8, DepthFeather = 0",
    ] {
        let lua = format!("s = {{ ProcessVersion = '15.4', MaskGroupBasedCorrections = {{{{ CorrectionMasks = {{{{ What = 'Mask/Range', CorrectionRangeMask = {{ {fields} }} }}}} }}}} }}");
        let (r, _) = lua_develop::parse(&lua, "15.4").unwrap();
        assert!(r.unknown.contains_key("lrcat_develop_source"));
    }
}
