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
        let d = import_lrcat::diagnostics::entries(&r);
        assert_eq!(d[key][0].level, "info");
        assert!(d[key][0].reason.starts_with("approximate: "));
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

#[test]
fn focal_scale_conjugates_translation() {
    let (r,_) = lua_develop::parse("s = { PerspectiveUpright = 1, UprightCenterNormX = 0.25, UprightCenterNormY = 0.75, UprightFocalLength35mm = 70, UprightTransform_1 = '1,0,0.1,0,1,0.2,0,0,1' }","15.4").unwrap();
    let h = r.settings.geometry.upright.homography.unwrap();
    assert!((h[0][2] - 0.2).abs() < 1e-12);
    assert!((h[1][2] - 0.4).abs() < 1e-12);
}

#[test]
fn catalog_process_version_gates_ca_even_when_xmp_claims_legacy() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="5.7" crs:ChromaticAberrationR="35"/></rdf:RDF>"#;
    let (r, _) = import_lrcat::xmp::parse(xml, "15.4").unwrap();
    assert!(r.settings.lens.legacy_ca_red.is_none());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["ChromaticAberrationR"].is_string());
    r.validate().unwrap();
}

#[test]
fn invalid_frame_metadata_is_not_silently_defaulted() {
    for pair in [
        "UprightFocalLength35mm = -1",
        "UprightCenterNormX = 'NaN'",
        "UprightCenterNormY = 2",
    ] {
        let (r, w) = lua_develop::parse(
            &format!(
                "s = {{ PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0,0,1', {pair} }}"
            ),
            "15.4",
        )
        .unwrap();
        assert!(r.settings.geometry.upright.homography.is_none());
        assert!(
            w.iter().any(|w| w.contains("invalid center/focal frame")),
            "{w:?}"
        );
    }
}

#[test]
fn framed_matrix_poles_are_checked_in_the_actual_image_domain() {
    // The pole qx=2/3 is outside the centered [-0.5,0.5] source frame,
    // even though it lies in the unframed [0,1] square.
    let (r,w)=lua_develop::parse("s = { PerspectiveUpright = 1, UprightCenterNormX = 0.5, UprightCenterNormY = 0.5, UprightTransform_1 = '1,0,0,0,1,0,-1.5,0,1' }","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert_eq!(
        r.settings.geometry.upright.homography,
        Some([[0.25, 0., 0.375], [-0.75, 1., 0.375], [-1.5, 0., 1.75]])
    );
    r.validate().unwrap();
}

#[test]
fn unknown_frame_family_members_do_not_change_the_known_mapping() {
    let (r,w)=lua_develop::parse("s = { PerspectiveUpright = 1, UprightCenterFuture = 'future', UprightTransform_1 = '0,-1,1,1,0,0,0,0,1' }","15.4").unwrap();
    assert_eq!(
        r.settings.geometry.upright.homography,
        Some([[0., -1., 1.], [1., 0., 0.], [0., 0., 1.]])
    );
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["UprightCenterFuture"],
        "'future'"
    );
    assert!(w.iter().any(|w| w.contains("UprightCenterFuture")));
}

#[test]
fn lr7d_pv2012_ignored_ca_is_info_not_warning() {
    let (r, w) = lua_develop::parse("s = { ChromaticAberrationR = 35 }", "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    let entries = import_lrcat::diagnostics::entries(&r);
    assert!(
        entries["ChromaticAberrationR"][0]
            .reason
            .contains("ignored (PV2012+)")
    );
    assert!(r.settings.lens.legacy_ca_red.is_none());
}

#[test]
fn lr7d_hooks_finish_one_replayable_import_transaction() {
    let (r,_) = lua_develop::parse("s = { Exposure2012 = 0.5, ChromaticAberrationR = 35, PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0.2,0,1' }", "5.7").unwrap();
    assert_eq!(r.history.entries.len(), 1);
    assert_eq!(r.history.state_at(r.history.head).unwrap(), r.settings);
}
