use import_lrcat::lua_develop::parse;

#[test]
fn extended_curves_normalize_all_channels_and_retain_approximate_source() {
    for (suffix, field) in [
        ("", "rgb"),
        ("Red", "red"),
        ("Green", "green"),
        ("Blue", "blue"),
    ] {
        let key = format!("ExtendedToneCurvePV2012{suffix}");
        let (r, _) = parse(
            &format!("s = {{ HDREditMode=1, {key} = {{0,0,128,160,255,255}} }}"),
            "15.4",
        )
        .unwrap();
        let v = serde_json::to_value(&r).unwrap();
        let p = &v["settings"]["tone"]["curves_extended"][field][1];
        assert!((p["x"].as_f64().unwrap_or(-1.) - 128. / 255.).abs() < 1e-6);
        assert!((p["y"].as_f64().unwrap_or(-1.) - 160. / 255.).abs() < 1e-6);
        assert_eq!(
            v["lrcat_develop_source"]["properties"][&key],
            "{0,0,128,160,255,255}"
        );
    }
}

#[test]
fn grayscale_preserves_eight_bands_and_inactive_state() {
    for enabled in [true, false] {
        let (r, _) = parse(&format!("s = {{ ConvertToGrayscale = {enabled}, GrayMixerRed=10, GrayMixerOrange=20, GrayMixerYellow=30, GrayMixerGreen=40, GrayMixerAqua=-10, GrayMixerBlue=-20, GrayMixerPurple=-30, GrayMixerMagenta=-40 }}"), "15.4").unwrap();
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["settings"]["color"]["monochrome"]["enabled"], enabled);
        for (key, band, n) in [
            ("Red", "red", 10.),
            ("Orange", "orange", 20.),
            ("Yellow", "yellow", 30.),
            ("Green", "green", 40.),
            ("Aqua", "aqua", -10.),
            ("Blue", "blue", -20.),
            ("Purple", "purple", -30.),
            ("Magenta", "magenta", -40.),
        ] {
            assert_eq!(v["settings"]["color"]["monochrome"]["mixer"][band], n);
            assert!(v["lrcat_develop_source"]["properties"][format!("GrayMixer{key}")].is_string());
        }
    }
}

#[test]
fn legacy_approximation_is_version_gated_and_legacy_values_win() {
    let row = "s = { Exposure=1, Brightness=75, Contrast=50, FillLight=30, HighlightRecovery=20, Shadows=10 }";
    let (r, w) = parse(row, "5.7").unwrap();
    assert_eq!(
        (
            r.settings.tone.exposure,
            r.settings.tone.contrast,
            r.settings.tone.shadows,
            r.settings.tone.highlights,
            r.settings.tone.blacks
        ),
        (0., 0., 0., 0., 0.)
    );
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["Brightness"],
        "75"
    );
    let (modern, _) = parse(row, "15.4").unwrap();
    assert_eq!(modern.settings.tone.exposure, 0.);
    let (both, _) = parse(
        "s = { Exposure=2, Brightness=75, Exposure2012=0.25 }",
        "5.7",
    )
    .unwrap();
    assert_eq!(both.settings.tone.exposure, 0.);
    assert_eq!(both.settings.tone.legacy_pv2010.unwrap().exposure, Some(2.));
}

#[test]
fn metadata_and_unrepresentable_curves_are_explicit_and_lossless() {
    let (r,w) = parse("s = { AutoToneDigest='opaque', AutoToneDigestNoSat='other', DepthMapInfo={Version=1}, ExtendedToneCurvePV2012={0,0,300,-1} }", "15.4").unwrap();
    for key in [
        "AutoToneDigest",
        "AutoToneDigestNoSat",
        "DepthMapInfo",
        "ExtendedToneCurvePV2012",
    ] {
        assert!(!r.unknown["lrcat_develop_source"]["properties"][key].is_null());
        assert_eq!(
            w.iter().any(|s| s.contains(key)),
            !key.starts_with("AutoToneDigest"),
            "{w:?}"
        );
    }
    assert!(r.settings.tone.curves.rgb.0.is_empty());
}

#[test]
fn malformed_curves_and_grayscale_remain_retained() {
    for value in [
        "{0,0,128}",
        "{0,0,0,1,255,255}",
        "{0,0,255,-1}",
        "{255,255,0,0}",
    ] {
        let (r, _) = parse(
            &format!("s = {{ ExtendedToneCurvePV2012={value} }}"),
            "15.4",
        )
        .unwrap();
        assert_eq!(
            r.unknown["lrcat_develop_source"]["properties"]["ExtendedToneCurvePV2012"],
            value
        );
        assert!(r.settings.tone.curves.rgb.0.is_empty());
    }
    let (r, _) = parse(
        "s = { ConvertToGrayscale='invalid', GrayMixerRed=101 }",
        "15.4",
    )
    .unwrap();
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["GrayMixerRed"],
        "101"
    );
}

#[test]
fn xmp_and_lua_lr2_settings_match() {
    let (lua,_) = parse("s = { ConvertToGrayscale=true, GrayMixerBlue=25, ExtendedToneCurvePV2012={0,0,128,160,255,255}, Exposure=1 }", "5.7").unwrap();
    let (xmp,_) = import_lrcat::xmp::parse(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ConvertToGrayscale="true" crs:GrayMixerBlue="25" crs:Exposure="1"><crs:ExtendedToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 160</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ExtendedToneCurvePV2012></rdf:Description></rdf:RDF>"#, "5.7").unwrap();
    assert_eq!(lua.settings, xmp.settings);
    assert_eq!(
        serde_json::to_value(&xmp.settings).unwrap()["tone"]["legacy_pv2010"]["exposure"],
        1.
    );
    assert!(
        xmp.unknown
            .get("lrcat_develop_source")
            .is_some_and(|source| source["properties"]["GrayMixerBlue"].is_string())
    );
}

#[test]
fn nonmonotone_extended_curve_is_retained_instead_of_breaking_render() {
    let (r, w) = parse(
        "s = { ExtendedToneCurvePV2012={0,0,128,220,255,180} }",
        "15.4",
    )
    .unwrap();
    assert!(r.settings.tone.curves.rgb.0.is_empty());
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["ExtendedToneCurvePV2012"],
        "{0,0,128,220,255,180}"
    );
    assert!(w.iter().any(|s| s.contains("ExtendedToneCurve")));
}

#[test]
fn monochrome_recipe_roundtrip_preserves_history_and_disabled_mixer() {
    let (r, _) = parse("s = { ConvertToGrayscale=false, GrayMixerRed=40 }", "15.4").unwrap();
    let bytes = r.to_json().unwrap();
    let back = engine_api::recipe::Recipe::from_json(&bytes).unwrap();
    assert_eq!(back.settings, r.settings);
    back.validate().unwrap();
    let v = serde_json::to_value(back.settings).unwrap();
    assert_eq!(v["color"]["monochrome"]["mixer"]["red"], 40.);
    let (unrelated, _) =
        parse("s = { Exposure2012=1, FutureValue={Keep='exact'} }", "15.4").unwrap();
    assert!(
        serde_json::to_value(unrelated.settings).unwrap()["color"]
            .get("monochrome")
            .is_none()
    );
}

#[test]
fn monochrome_diagnoses_adobe_fidelity_without_user_warning() {
    let (r, warnings) = parse("s = { ConvertToGrayscale=true }", "15.4").unwrap();
    assert!(warnings.is_empty());
    assert!(r.unknown.contains_key("lrcat_develop_diagnostics"));
}

#[test]
fn lr2b_legacy_and_hdr_roundtrip() {
    let (r, _) = parse("s={HDREditMode=1,Exposure=1,Brightness=75,Contrast=50,FillLight=30,Recovery=20,Blacks=10,ExtendedToneCurvePV2012={0,0,255,300,510,600}}", "5.7").unwrap();
    let v = serde_json::to_value(&r.settings).unwrap();
    assert_eq!(v["tone"]["legacy_pv2010"]["exposure"], 1.);
    assert_eq!(v["tone"]["legacy_pv2010"]["brightness"], 75.);
    assert_eq!(v["tone"]["legacy_pv2010"]["recovery"], 20.);
    assert_eq!(v["tone"]["curves_extended"]["rgb"][2]["x"], 2.);
    let back = engine_api::recipe::Recipe::from_json(&r.to_json().unwrap()).unwrap();
    assert_eq!(back.settings, r.settings);
    back.validate().unwrap();
    let (r, _) = parse(
        "s={Exposure=2,Brightness=75,Exposure2012=0.25,Contrast=50,Contrast2012=10}",
        "5.7",
    )
    .unwrap();
    let v = serde_json::to_value(&r.settings).unwrap();
    assert_eq!(v["tone"]["legacy_pv2010"]["exposure"], 2.);
    assert_eq!(v["tone"]["legacy_pv2010"]["brightness"], 75.);
    assert_eq!(v["tone"]["legacy_pv2010"]["contrast"], 50.);
}
#[test]
fn lr2b_digest_is_retained_but_not_a_user_warning() {
    let (r, w) = parse(
        "s={AutoToneDigest='opaque',AutoToneDigestNoSat='other'}",
        "15.4",
    )
    .unwrap();
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["AutoToneDigest"],
        "'opaque'"
    );
    assert!(!w.iter().any(|v| v.contains("AutoToneDigest")), "{w:?}");
}

#[test]
fn lr2b_hdr_channel_precedence_and_signed_knots_match_xmp() {
    let (lua, _) = parse("s={HDREditMode=1,ToneCurvePV2012Red={0,0,255,200},ExtendedToneCurvePV2012Red={-255,-128,510,600},ExtendedToneCurvePV2012Blue={0,0,255,220}}","15.4").unwrap();
    let (xmp,_) = import_lrcat::xmp::parse(r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:HDREditMode="1"><crs:ToneCurvePV2012Red><rdf:Seq><rdf:li>0,0</rdf:li><rdf:li>255,200</rdf:li></rdf:Seq></crs:ToneCurvePV2012Red><crs:ExtendedToneCurvePV2012Red><rdf:Seq><rdf:li>-255,-128</rdf:li><rdf:li>510,600</rdf:li></rdf:Seq></crs:ExtendedToneCurvePV2012Red><crs:ExtendedToneCurvePV2012Blue><rdf:Seq><rdf:li>0,0</rdf:li><rdf:li>255,220</rdf:li></rdf:Seq></crs:ExtendedToneCurvePV2012Blue></rdf:Description></rdf:RDF>"#,"15.4").unwrap();
    assert_eq!(lua.settings, xmp.settings);
    let extended = lua.settings.tone.curves_extended.as_ref().unwrap();
    assert_eq!(extended.red.0[0].x, -1.);
    assert!((extended.red.0[1].y - 600. / 255.).abs() < 1e-6);
    assert!((extended.blue.0[1].y - 220. / 255.).abs() < 1e-6);
}

#[test]
fn lr2b_legacy_modern_precedence_and_alias_order() {
    let (r,_) = parse("s={Exposure=2,Brightness=75,Contrast=50,FillLight=40,Recovery=10,HighlightRecovery=20,Blacks=5,Shadows=8,Exposure2012=0.25,Contrast2012=10,Shadows2012=15,Highlights2012=-30,Blacks2012=-5}","5.7").unwrap();
    assert_eq!(
        r.settings.tone.legacy_pv2010.as_ref().unwrap().exposure,
        Some(2.)
    );
    assert_eq!(r.settings.tone.exposure, 0.);
    let (r, _) = parse(
        "s={Recovery=10,HighlightRecovery=20,Blacks=5,Shadows=8}",
        "5.7",
    )
    .unwrap();
    let legacy = r.settings.tone.legacy_pv2010.unwrap();
    assert_eq!(legacy.recovery, Some(20.));
    assert_eq!(legacy.blacks, Some(8.));
    let (r, _) = parse("s={Exposure=1,Brightness=75,Contrast=50}", "15.4").unwrap();
    assert!(r.settings.tone.legacy_pv2010.is_none());
}
