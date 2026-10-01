use import_lrcat::lua_develop::parse;

#[test]
fn lr2c_inactive_fields_preserve_settings_history_and_hash() {
    let (base, _) = parse("s={Exposure2012=0.5}", "15.4").unwrap();
    let (r, _) = parse("s={Exposure2012=0.5,ConvertToGrayscale=false,GrayMixerRed=0,ExtendedToneCurvePV2012={0,0,255,255}}", "15.4").unwrap();
    assert_eq!(r.settings, base.settings);
    assert_eq!(r.history, base.history);
    assert_eq!(r.stage_chain(), base.stage_chain());
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["ConvertToGrayscale"],
        "false"
    );
}

#[test]
fn lr2c_extended_curves_never_replace_normal_and_require_hdr() {
    for hdr in [0, 1] {
        let (r, _) = parse(&format!("s={{HDREditMode={hdr},ToneCurvePV2012={{0,0,128,160,255,255}},ExtendedToneCurvePV2012={{0,0,255,255}}}}"), "15.4").unwrap();
        assert_eq!(r.settings.tone.curves.rgb.0.len(), 3);
        assert!(r.settings.tone.curves_extended.is_none());
        let (r, _) = parse(&format!("s={{HDREditMode={hdr},ToneCurvePV2012={{0,0,128,160,255,255}},ExtendedToneCurvePV2012={{0,0,255,300,510,600}}}}"), "15.4").unwrap();
        assert_eq!(r.settings.tone.curves.rgb.0.len(), 3);
        assert_eq!(r.settings.tone.curves_extended.is_some(), hdr == 1);
    }
}

#[test]
fn lr2c_approximation_is_info_only_and_source_is_exact() {
    for (pv, key, value) in [
        ("15.4", "ConvertToGrayscale", "true"),
        ("15.4", "GrayMixerRed", "25"),
        ("5.7", "Brightness", "75"),
        ("5.7", "Exposure", "1"),
    ] {
        let (r, w) = parse(&format!("s={{{key}={value}}}"), pv).unwrap();
        assert!(w.is_empty(), "{key}: {w:?}");
        assert_eq!(r.unknown["lrcat_develop_source"]["properties"][key], value);
        assert!(
            r.unknown["lrcat_develop_diagnostics"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d["key"] == key
                    && d["level"] == "info"
                    && d["message"].as_str().unwrap().starts_with("approximate: "))
        );
    }
}

#[test]
fn lr2c_legacy_controls_win_and_import_has_one_replayable_edit() {
    let (mut r, w) = parse(
        "s={Exposure=1,Brightness=50,Shadows=5,Exposure2012=2,Blacks2012=-20}",
        "5.7",
    )
    .unwrap();
    let legacy = r.settings.tone.legacy_pv2010.as_ref().unwrap();
    assert_eq!(legacy.exposure, Some(1.));
    assert_eq!(legacy.brightness, Some(50.));
    assert_eq!(legacy.blacks, Some(5.));
    assert_eq!(r.settings.tone.exposure, 0.);
    assert_eq!(r.settings.tone.blacks, 0.);
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(r.history.entries.len(), 1);
    r.validate().unwrap();
    let settings = r.settings.clone();
    assert!(r.undo().unwrap());
    assert!(r.redo().unwrap());
    assert_eq!(r.settings, settings);
}

#[test]
fn lr2c_digest_never_changes_legacy_row_settings_or_history() {
    let (base, _) = parse("s={Exposure2012=0.25}", "5.7").unwrap();
    let (r, w) = parse("s={Exposure2012=0.25,AutoToneDigest='cache'}", "5.7").unwrap();
    assert_eq!(r.settings, base.settings);
    assert_eq!(r.history, base.history);
    assert_eq!(r.recipe_hash(), base.recipe_hash());
    assert!(!w.iter().any(|w| w.contains("AutoToneDigest")));
}
