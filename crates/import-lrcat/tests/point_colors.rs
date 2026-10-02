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
fn point_color_lua_maps_shifts_and_all_feathers_with_exact_source() {
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
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
    r.validate().unwrap();
}
#[test]
fn point_color_xmp_numeric_sequence_matches_lua() {
    let (r, _) = xmp::parse(&packet(CSV), "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1);
    let (lua, _) = lua_develop::parse(LUA, "15.4").unwrap();
    assert_eq!(r.settings, lua.settings);
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
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
    let native =
        sidecar::XmpPacket::from_recipe(&r, &Default::default(), &sidecar::MarkPreset::lightroom())
            .unwrap();
    assert_eq!(
        native
            .to_recipe()
            .unwrap()
            .recipe
            .settings
            .color
            .point_colors,
        r.settings.color.point_colors
    );
    let multiple = packet(&format!("{CSV}</rdf:li><rdf:li>{CSV}"));
    let (r, _) = xmp::parse(&multiple, "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 2);
}

#[test]
fn point_color_list_extensions_and_mixed_invalid_items_are_retained() {
    let text = packet(CSV).replace("</rdf:Seq>", "<crs:Future>1</crs:Future></rdf:Seq>");
    let (r, _) = xmp::parse(&text, "15.4").unwrap();
    assert!(r.settings.color.point_colors.is_empty());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
    let text = packet(&format!("{CSV}</rdf:li><rdf:li>unsupported"));
    let (r, _) = xmp::parse(&text, "15.4").unwrap();
    assert!(r.settings.color.point_colors.is_empty());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
}

#[test]
fn point_color_explicit_lua_array_indices_translate_without_losing_order() {
    let text = LUA.replace("PointColors = { {", "PointColors = { [1] = {");
    let (r, _) = lua_develop::parse(&text, "15.4").unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1);
    assert_eq!(
        r.settings,
        lua_develop::parse(LUA, "15.4").unwrap().0.settings
    );
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
}

#[test]
fn point_color_resource_wrapper_extensions_are_not_discarded() {
    let mut resource = String::from(
        r#"<rdf:Description crs:SrcHue="0" crs:SrcSat="0.5" crs:SrcLum="0.5" crs:HueShift="0.5">"#,
    );
    for name in ["HueRange", "SatRange", "LumRange"] {
        resource += &format!(
            r#"<crs:{name}><rdf:Description crs:LowerNone="0" crs:LowerFull="0.25" crs:UpperFull="0.75" crs:UpperNone="1"/></crs:{name}>"#
        );
    }
    resource += "</rdf:Description>";
    let (valid, _) = xmp::parse(&packet(&resource), "15.4").unwrap();
    assert_eq!(valid.settings.color.point_colors.len(), 1);
    resource += "<crs:Future>1</crs:Future>";
    let (r, _) = xmp::parse(&packet(&resource), "15.4").unwrap();
    assert!(r.settings.color.point_colors.is_empty());
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
}

#[test]
fn point_color_success_has_no_unsupported_warning() {
    let (_, warnings) = lua_develop::parse(LUA, "15.4").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
}
#[test]
fn point_color_defaults_missing_ranges() {
    let (r, warnings) = lua_develop::parse(
        "s = { PointColors = {{ SrcHue=0, SrcSat=0.9, SrcLum=0.5, HueShift=0.5 }} }",
        "15.4",
    )
    .unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1, "{warnings:?}");
    assert_eq!(
        r.settings.color.point_colors[0]
            .selection
            .as_ref()
            .unwrap()
            .saturation,
        [0., 0.25, 0.75, 1.]
    );
}
#[test]
fn point_color_skips_placeholder_among_real_records() {
    let placeholder = vec!["-1"; 19].join(",");
    let (r, warnings) = xmp::parse(
        &packet(&format!(
            "{placeholder}</rdf:li><rdf:li>{CSV}</rdf:li><rdf:li>{placeholder}"
        )),
        "15.4",
    )
    .unwrap();
    assert_eq!(r.settings.color.point_colors.len(), 1, "{warnings:?}");
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
}
#[test]
fn point_color_requires_pv3_or_later() {
    let (r, warnings) = lua_develop::parse(LUA, "5.7").unwrap();
    assert!(r.settings.color.point_colors.is_empty());
    assert!(
        warnings
            .iter()
            .any(|w| w.contains("PointColors") && w.contains("PV3")),
        "{warnings:?}"
    );
    assert!(r.unknown["lrcat_develop_source"]["properties"]["PointColors"].is_string());
}

#[test]
fn lr1c_shared_approximation_diagnostics_and_single_combined_import() {
    let lua = "s = { PointColors={{SrcHue=0,SrcSat=0.5,SrcLum=0.5,HueShift=0.5}}, ConvertToGrayscale=true, GrayMixerRed=20, PerspectiveUpright=1, UprightTransform_1='1,0,0,0,1,0,0.2,0,1' }";
    let xml = packet(CSV).replace("<crs:PointColors>", "<crs:ConvertToGrayscale>True</crs:ConvertToGrayscale><crs:GrayMixerRed>20</crs:GrayMixerRed><crs:PerspectiveUpright>1</crs:PerspectiveUpright><crs:UprightTransform_1>1,0,0,0,1,0,0.2,0,1</crs:UprightTransform_1><crs:PointColors>");
    for (r, warnings) in [
        lua_develop::parse(lua, "15.4").unwrap(),
        xmp::parse(&xml, "15.4").unwrap(),
    ] {
        assert!(warnings.is_empty(), "{warnings:?}");
        let entries = import_lrcat::diagnostics::entries(&r);
        let point = entries
            .get("PointColors")
            .expect("shared PointColors diagnostic");
        assert_eq!(point.len(), 1);
        assert_eq!(point[0].level, "info");
        assert_eq!(point[0].status, "approximate");
        assert_eq!(point[0].lane, "LR-1");
        assert_eq!(
            point[0].field.as_deref(),
            Some("/settings/color/point_colors")
        );
        for key in ["PointColors", "ConvertToGrayscale", "UprightTransform_1"] {
            assert!(entries.contains_key(key), "missing {key}");
            assert!(r.unknown["lrcat_develop_source"]["properties"][key].is_string());
        }
        assert!(!r.unknown.contains_key("tessera_import_info"));
        assert_eq!(r.history.entries.len(), 1);
        assert!(matches!(
            r.history.entries[0].meta.author,
            engine_api::recipe::Author::Import { .. }
        ));
        assert_eq!(r.history.state_at(r.history.head).unwrap(), r.settings);
        r.validate().unwrap();
    }
}
