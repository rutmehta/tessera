//! Invented fixtures only; no catalog data.
use import_lrcat::lua_develop;
use serde_json::json;

fn row(mask: &str) -> String {
    format!(
        r#"s = {{ ProcessVersion = "15.4", MaskGroupBasedCorrections = {{ {{ LocalExposure2012 = 1, CorrectionMasks = {{ {mask} }} }} }} }}"#
    )
}
#[test]
fn lr4_disabled_component_maps_without_losing_geometry() {
    let (r, _) = lua_develop::parse(&row(r#"{ What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0, MaskActive = false }"#), "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    let v = serde_json::to_value(&r.settings.locals.adjustments[0]).unwrap();
    assert_eq!(v["components"][0]["enabled"], false);
    assert_eq!(v["components"][0]["start"], json!([0., 0.]));
}
#[test]
fn lr4_nested_group_preserves_order_and_operators() {
    let (r, _) = lua_develop::parse(&row(r#"{ What = "Mask/Group", MaskBlendMode = 1, MaskInverted = true, Masks = {
        { What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0 },
        { What = "Mask/CircularGradient", Left = 0.25, Right = 0.75, Top = 0.25, Bottom = 0.75, MaskBlendMode = 2 }
    } }"#), "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    let v = serde_json::to_value(&r.settings.locals.adjustments[0]).unwrap();
    assert_eq!(v["components"][0]["kind"], "brush");
    assert_eq!(v["components"][0]["group"][1]["combine"], "intersect");
    assert_eq!(v["components"][0]["combine"], "subtract");
    assert_eq!(v["components"][0]["invert"], true);
}
#[test]
fn lr4_opaque_brush_color_and_image_are_retained_atomically() {
    for mask in [
        r#"{ What = "Mask/Paint", Dabs = { "opaque" } }"#,
        r#"{ What = "Mask/Image", MaskSyncID = "synthetic" }"#,
        r#"{ What = "Mask/Range", CorrectionRangeMask = { Type = "Color", ColorAmount = 0.5, AreaModels = { "opaque" } } }"#,
    ] {
        let (r, w) = lua_develop::parse(&row(mask), "15.4").unwrap();
        assert!(r.settings.locals.adjustments.is_empty());
        assert!(
            r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"]
                .is_string()
        );
        assert!(!w.is_empty());
    }
}

#[test]
fn lr4_explicit_luminance_and_depth_bounds_translate() {
    for (fields, kind) in [("LumMin = 0.25, LumMax = 0.75, LumFeather = 0", "luminance_range"), ("DepthMin = 0.25, DepthMax = 0.75, DepthFeather = 0", "depth")] {
        let (r, _) = lua_develop::parse(&row(&format!(r#"{{ What = "Mask/Range", CorrectionRangeMask = {{ {fields} }} }}"#)), "15.4").unwrap();
        assert_eq!(r.settings.locals.adjustments.len(), 1);
        let v = serde_json::to_value(&r.settings.locals.adjustments[0]).unwrap();
        assert_eq!(v["components"][0]["kind"], kind);
        assert_eq!(v["components"][0]["range"], json!([0.25, 0.75]));
        assert!(r.unknown["lrcat_develop_source"]["properties"]["MaskGroupBasedCorrections"].is_null());
    }
}
#[test]
fn lr4_group_range_intersects_the_whole_union() {
    let lua = r#"s = { ProcessVersion = "15.4", MaskGroupBasedCorrections = {{ LocalExposure2012 = 1,
        CorrectionMasks = {{ What = "Mask/Gradient", FullX = 0, FullY = 0, ZeroX = 1, ZeroY = 0 }},
        CorrectionRangeMask = { LumMin = 0.25, LumMax = 0.75, LumFeather = 0 }
    }} }"#;
    let (r, _) = lua_develop::parse(lua, "15.4").unwrap();
    assert_eq!(r.settings.locals.adjustments.len(), 1);
    let v = serde_json::to_value(&r.settings.locals.adjustments[0]).unwrap();
    assert_eq!(v["components"][1]["combine"], "intersect");
    assert_eq!(v["components"][1]["kind"], "luminance_range");
}
