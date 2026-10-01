use import_lrcat::lua_develop::parse;

#[test]
fn extended_curves_normalize_all_channels_and_remove_source() {
    for (suffix, field) in [
        ("", "rgb"),
        ("Red", "red"),
        ("Green", "green"),
        ("Blue", "blue"),
    ] {
        let key = format!("ExtendedToneCurvePV2012{suffix}");
        let (r, _) = parse(
            &format!("s = {{ {key} = {{0,0,128,160,255,255}} }}"),
            "15.4",
        )
        .unwrap();
        let v = serde_json::to_value(&r).unwrap();
        let p = &v["settings"]["tone"]["curves"][field][1];
        assert!((p["x"].as_f64().unwrap_or(-1.) - 128. / 255.).abs() < 1e-6);
        assert!((p["y"].as_f64().unwrap_or(-1.) - 160. / 255.).abs() < 1e-6);
        assert!(v["unknown"]["lrcat_develop_source"]["properties"][&key].is_null());
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
            assert!(
                v["unknown"]["lrcat_develop_source"]["properties"][format!("GrayMixer{key}")]
                    .is_null()
            );
        }
    }
}

#[test]
fn legacy_approximation_is_version_gated_and_modern_values_win() {
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
        (1.5, 25., 30., -20., -10.)
    );
    assert!(w.iter().any(|s| s.contains("approximation")));
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
    assert_eq!(both.settings.tone.exposure, 0.25);
}

#[test]
fn metadata_and_unrepresentable_curves_are_explicit_and_lossless() {
    let (r,w) = parse("s = { AutoToneDigest='opaque', AutoToneDigestNoSat='other', DepthMapInfo={Version=1}, ExtendedToneCurvePV2012={0,0,300,400} }", "15.4").unwrap();
    for key in [
        "AutoToneDigest",
        "AutoToneDigestNoSat",
        "DepthMapInfo",
        "ExtendedToneCurvePV2012",
    ] {
        assert!(!r.unknown["lrcat_develop_source"]["properties"][key].is_null());
        assert!(w.iter().any(|s| s.contains(key)), "{w:?}");
    }
    assert!(r.settings.tone.curves.rgb.0.is_empty());
}

#[test]
fn malformed_curves_and_grayscale_remain_retained() {
    for value in [
        "{0,0,128}",
        "{0,0,0,1,255,255}",
        "{0,0,255,300}",
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
    assert_eq!(xmp.settings.tone.exposure, 1.);
    assert!(xmp.unknown["lrcat_develop_source"]["properties"]["GrayMixerBlue"].is_null());
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
fn monochrome_warns_about_adobe_profile_dependent_fidelity() {
    let (_, warnings) = parse("s = { ConvertToGrayscale=true }", "15.4").unwrap();
    assert!(warnings.iter().any(|w| w.starts_with("B&W approximation:")));
}
