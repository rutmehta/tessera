use import_lrcat::{lua_develop, xmp};
const LUA: &str = include_str!("data/point-color.lua");
const CSV: &str =
    "0, 0.5, 0.5, 0.5, 0, 0, 0.5, 0, 0.25, 0.75, 1, 0, 0.25, 0.75, 1, 0, 0.25, 0.75, 1";
fn packet(csv: &str) -> String {
    format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><crs:PointColors><rdf:Seq><rdf:li>{csv}</rdf:li></rdf:Seq></crs:PointColors></rdf:Description></rdf:RDF>"#
    )
}
#[test]
fn point_color_lua_maps_shifts_and_all_feathers_without_pending_source() {
    let (r, _) = lua_develop::parse(LUA, "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1);
    let p = &r.settings.color.point_colors[0];
    assert_eq!(p.hue_shift, 30.);
    assert_eq!(p.range, 50.);
    let j = serde_json::to_value(p).unwrap();
    assert_eq!(
        j["selection"]["source_hsl"],
        serde_json::json!([0., 0.5, 0.5])
    );
    for key in ["hue", "saturation", "luminance"] {
        assert_eq!(j["selection"][key], serde_json::json!([0., 0.25, 0.75, 1.]));
    }
    assert!(r.unknown.get("lrcat_develop_source").is_none());
    r.validate().unwrap();
}
#[test]
fn point_color_xmp_numeric_sequence_matches_lua() {
    let (r, _) = xmp::parse(&packet(CSV), "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1);
    let (lua, _) = lua_develop::parse(LUA, "15.4").unwrap();
    assert_eq!(r.settings, lua.settings);
    assert!(r.unknown.get("lrcat_develop_source").is_none());
}
#[test]
fn point_color_partial_or_unknown_shape_stays_atomic_and_retained() {
    for text in [
        LUA.replace("SrcSat = 0.5,", ""),
        LUA.replace("SatScale = 0", "SatScale = 2"),
        LUA.replace("SrcHue = 0", "Future = 1, SrcHue = 0"),
        LUA.replace("LowerFull = 0.25", "LowerFull = 0.9"),
    ] {
        let (r, warnings) = lua_develop::parse(&text, "15.4").unwrap();
        assert!(r.settings.color.point_colors.is_empty());
        assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
        assert!(!warnings.is_empty());
    }
    for csv in ["0, 0.5", "NaN, 0.5", "opaque Adobe data"] {
        let (r, _) = xmp::parse(&packet(csv), "15.4").unwrap();
        assert!(r.settings.color.point_colors.is_empty());
        assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
    }
}
#[test]
fn point_color_units_multiple_points_and_native_roundtrip() {
    let text = LUA
        .replace("SrcHue = 0", "SrcHue = 2")
        .replace("SatScale = 0", "SatScale = -0.25")
        .replace("LumScale = 0", "LumScale = 0.4");
    let (r, _) = lua_develop::parse(&text, "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1);
    let p = &r.settings.color.point_colors[0];
    assert_eq!(p.saturation_shift, -25.);
    assert_eq!(p.luminance_shift, 40.);
    assert_eq!(
        serde_json::to_value(p).unwrap()["selection"]["source_hsl"][0],
        serde_json::json!(120.)
    );
    let bytes = r.to_json().unwrap();
    assert_eq!(
        engine_api::recipe::Recipe::from_json(&bytes)
            .unwrap()
            .settings,
        r.settings
    );
    let multiple = packet(&format!("{CSV}</rdf:li><rdf:li>{CSV}"));
    let (r, _) = xmp::parse(&multiple, "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 2);
}
