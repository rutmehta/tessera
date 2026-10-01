use engine_api::recipe::Author;
use import_lrcat::lua_develop;

#[test]
fn approximate_retains_exact_source_without_warnings_and_one_import_edit() {
    let (r,w) = lua_develop::parse("s = { ChromaticAberrationR = 35, PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0.2,0,1' }", "5.7").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["ChromaticAberrationR"],
        "35"
    );
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["UprightTransform_1"],
        "'1,0,0,0,1,0,0.2,0,1'"
    );
    for key in ["ChromaticAberrationR", "UprightTransform_1"] {
        let d = &r.unknown["translation_diagnostics"][key];
        assert_eq!(d["level"], "info");
        assert!(d["message"].as_str().unwrap().starts_with("approximate: "));
    }
    assert_eq!(r.history.entries.len(), 1);
    assert!(matches!(
        r.history.entries[0].meta.author,
        Author::Import { .. }
    ));
    r.validate().unwrap();
}

#[test]
fn legacy_ca_is_gated_and_zero_does_not_create_history() {
    for version in ["5.0", "5.7", "6.7", "15.4"] {
        let (base, _) = lua_develop::parse("s = {}", version).unwrap();
        let (r, _) = lua_develop::parse(
            "s = { ChromaticAberrationR = 0, ChromaticAberrationB = 0 }",
            version,
        )
        .unwrap();
        assert_eq!(r.settings, base.settings);
        assert_eq!(r.history.entries.len(), base.history.entries.len());
        if version == "6.7" || version == "15.4" {
            let (r, _) = lua_develop::parse("s = { ChromaticAberrationR = 35 }", version).unwrap();
            assert!(r.settings.lens.legacy_ca_red.is_none());
        }
    }
}

#[test]
fn center_focal_frame_is_used_and_mode_is_tagged() {
    let (r,w) = lua_develop::parse("s = { PerspectiveUpright = 1, UprightCenterMode = 1, UprightCenterNormX = 0.25, UprightCenterNormY = 0.75, UprightFocalMode = 1, UprightFocalLength35mm = 70, UprightTransform_1 = '0,-1,0,1,0,0,0,0,1' }","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let j = serde_json::to_value(r.settings.geometry.upright).unwrap();
    assert_eq!(j["homography_mode"], "auto");
    assert_eq!(
        j["homography"],
        serde_json::json!([[0., -1., 1.], [1., 0., 0.5], [0., 0., 1.]])
    );
}
