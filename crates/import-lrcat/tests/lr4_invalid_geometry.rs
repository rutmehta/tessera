use import_lrcat::lua_develop;

#[test]
fn lr4_unrenderable_geometry_and_amount_keep_source() {
    for (local, geometry) in [
        ("", "What = 'Mask/Gradient', FullX = 0, FullY = 0, ZeroX = 0, ZeroY = 0"),
        ("", "What = 'Mask/CircularGradient', Left = 0, Right = 0, Top = 0, Bottom = 1"),
        ("CorrectionAmount = -1,", "What = 'Mask/Gradient', FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0"),
    ] {
        let lua = format!("s = {{ ProcessVersion = '15.4', MaskGroupBasedCorrections = {{{{ {local} CorrectionMasks = {{{{ What = 'Mask/Group', Masks = {{{{ {geometry} }}}} }}}} }}}} }}");
        let (r, _) = lua_develop::parse(&lua, "15.4").unwrap();
        assert!(r.unknown.contains_key("lrcat_develop_source"), "{local} {geometry}");
    }
}
