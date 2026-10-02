use import_lrcat::lua_develop;
use serde_json::json;

#[test]
fn solved_upright_is_approximate_and_retained() {
    let (r, w) = lua_develop::parse("s = { PerspectiveUpright = 1, UprightTransform_1 = '1,0,0,0,1,0,0.2,0,1', PerspectiveVertical = 20, PerspectiveHorizontal = -10, PerspectiveRotate = 2, PerspectiveScale = 110, PerspectiveAspect = 5, PerspectiveX = 3, PerspectiveY = -4 }", "15.4").unwrap();
    r.validate().unwrap();
    let restored = engine_api::recipe::Recipe::from_json(&r.to_json().unwrap()).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.settings.geometry, r.settings.geometry);
    let g = serde_json::to_value(&r.settings.geometry).unwrap();
    assert_eq!(
        g["upright"]["homography"],
        json!([[1., 0., 0.], [0., 1., 0.], [0.2, 0., 1.]])
    );
    assert_eq!(
        g["transform"],
        json!({"vertical":20.,"horizontal":-10.,"rotate":2.,"scale":110.,"aspect":5.,"offset_x":3.,"offset_y":-4.})
    );
    assert!(r.unknown.contains_key("lrcat_develop_source"));
    assert!(!w.iter().any(|w| w.contains("UprightTransform_1")), "{w:?}");
}

#[test]
fn guided_segments_translate_atomically() {
    let (r, _) = lua_develop::parse("s = { PerspectiveUpright = 5, UprightFourSegmentsCount = 2, UprightFourSegments_0 = '0.1,0.1,0.2,0.9', UprightFourSegments_1 = '0.9,0.1,0.8,0.9' }", "15.4").unwrap();
    assert_eq!(r.settings.geometry.upright.guides.len(), 2);
    assert_eq!(r.settings.geometry.upright.guides[0].end, [0.2, 0.9]);
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn cloud_only_and_invalid_geometry_explain_missing_rendering() {
    let (r, w) = lua_develop::parse("s = { EnableDistractionRemoval = true, FilterList = {{What='synthetic-filter'}}, UprightTransform_1 = '1,0,0,0,0,0,0,0,1', PerspectiveUpright = 1 }", "15.4").unwrap();
    assert!(
        import_lrcat::diagnostics::entries(&r)
            .values()
            .flatten()
            .any(|note| note.status == "ignored"
                && note
                    .reason
                    .contains("requires Adobe cloud; not translatable"))
    );
    assert!(
        w.iter()
            .any(|w| w.contains("UprightTransform_1") && w.contains("singular")),
        "{w:?}"
    );
    assert!(
        r.unknown["lrcat_develop_source"]["properties"]
            .get("UprightTransform_1")
            .is_some()
    );
}

#[test]
fn xmp_solved_upright_uses_same_mapping() {
    let (r, _) = import_lrcat::xmp::parse(r#"<rdf:Description xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:PerspectiveUpright="4" crs:UprightTransform_4="1,0,0,0,1,0,0.2,0,1"/>"#, "15.4").unwrap();
    assert!(
        serde_json::to_value(&r.settings.geometry).unwrap()["upright"]["homography"].is_array()
    );
    assert!(r.unknown.contains_key("lrcat_develop_source"));
}

#[test]
fn inactive_and_malformed_matrices_stay_exact() {
    let (r, _) = lua_develop::parse("s = { PerspectiveUpright = 1, UprightTransform_1 = 'NaN,0,0,0,1,0,0,0,1', UprightTransform_4 = '1,0,0,0,1,0,0,0,1' }", "15.4").unwrap();
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["UprightTransform_4"],
        "'1,0,0,0,1,0,0,0,1'"
    );
    assert_eq!(
        r.unknown["lrcat_develop_source"]["properties"]["UprightTransform_1"],
        "'NaN,0,0,0,1,0,0,0,1'"
    );
    assert!(
        serde_json::to_value(&r.settings.geometry).unwrap()["upright"]
            .get("homography")
            .is_none()
    );
}
