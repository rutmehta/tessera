use import_lrcat::lua_develop;
#[test]
fn lr4c_documented_keys_each_admit_a_renderable_approximation() {
    for (mask, correction) in [
        (r#"MaskSyncID="invented""#, ""),
        (r#"MaskName="invented""#, ""),
        ("MaskVersion=1", ""),
        ("MaskValue=1", ""),
        ("Midpoint=50", ""),
        ("Roundness=0", ""),
        ("", r#"CorrectionID="invented","#),
        ("", r#"CorrectionSyncID="invented","#),
        ("", "LocalToningHue=0,LocalToningSaturation=0,"),
    ] {
        let row = format!(r#"s={{MaskGroupBasedCorrections={{{{{correction}CorrectionMasks={{{{What="Mask/Gradient",FullX=0,FullY=0,ZeroX=1,ZeroY=0,{mask}}}}}}}}}}}"#);
        let (r,w) = lua_develop::parse(&row,"15.4").unwrap();
        assert!(w.is_empty(), "{mask} {correction}: {w:?}");
        assert_eq!(r.settings.locals.adjustments.len(),1);
        assert!(r.unknown["lrcat_develop_diagnostics"].is_array());
        assert!(r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"].is_string());
    }
}
