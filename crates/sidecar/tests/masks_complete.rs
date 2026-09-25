use engine_api::recipe::{LocalAdjustment, Recipe};
use serde_json::json;
use sidecar::{MarkPreset, Metadata, XmpPacket};

fn roundtrip(kind: serde_json::Value) -> String {
    let mut local: LocalAdjustment = serde_json::from_value(json!({
        "id": 73, "name": "Mask <&> λ", "enabled": false,
        "amount": 137.12346, "invert": true,
        "components": [kind],
        "params": {"exposure": 0.12345679, "temperature": -13.765432,
                   "color_overlay": [217.12346, 63.234566]}
    }))
    .unwrap();
    local.components[0].invert = true;
    local.components[0].combine = engine_api::recipe::mask::MaskCombine::Intersect;
    let mut recipe = Recipe::default();
    recipe
        .edit(Default::default(), |s| s.locals.adjustments = vec![local])
        .unwrap();
    recipe.ids.next_mask = 74;
    let packet =
        XmpPacket::from_recipe(&recipe, &Metadata::default(), &MarkPreset::default()).unwrap();
    let imported = packet.to_recipe().unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        imported.recipe.settings.locals.adjustments,
        recipe.settings.locals.adjustments
    );
    packet.xml
}

#[test]
fn linear_identity_combine_overlay_exact() {
    roundtrip(
        json!({"kind":"linear", "start":[0.12345679,0.7654321], "end":[0.9876543,0.2345679]}),
    );
}

// Compile/exercise the standalone translator before the parent wires develop.rs.
#[path = "../src/masks.rs"]
mod masks;
#[allow(dead_code)]
#[path = "../src/xml.rs"]
mod xml;

fn direct(kind: serde_json::Value) -> String {
    let mut locals = Vec::new();
    for (i, combine) in ["add", "subtract", "intersect"].iter().enumerate() {
        let mut c = kind.clone();
        c["combine"] = json!(combine);
        c["invert"] = json!(i != 0);
        let l: LocalAdjustment = serde_json::from_value(json!({
            "id": 31 + i, "name":"native & foreign < >", "enabled": i == 1,
            "amount": 137.12346, "invert":i != 0, "components":[c],
            "params":{"exposure":1.2345679,"contrast":-17.12345,"highlights":23.12345,
                "shadows":-43.12345,"whites":23.98765,"blacks":-13.98765,
                "temperature":-31.12345,"tint":13.98765,"hue":175.12346,
                "saturation":54.98765,"texture":-26.12345,"clarity":34.98765,
                "dehaze":-54.12345,"sharpness":76.98765,"noise":41.12345,
                "moire":12.98765,"defringe":43.12345,
                "color_overlay":if i == 0 {serde_json::Value::Null} else {json!([217.12346,63.98765])}}
        })).unwrap();
        locals.push(l);
    }
    let out = masks::export_masks(&serde_json::to_value(&locals).unwrap()).unwrap();
    let t = xml::Tree::parse(&xml::packet(&out)).unwrap();
    let back: Vec<LocalAdjustment> =
        serde_json::from_value(masks::import_masks(&t).unwrap()).unwrap();
    assert_eq!(back, locals);
    assert!(!out.contains("&quot;kind&quot;"));
    out
}

#[test]
fn direct_linear_all_combines_and_local_params() {
    direct(json!({"kind":"linear","start":[0.12345679,0.7654321],"end":[0.9876543,0.2345679]}));
}
#[test]
fn direct_radial_geometry_exact() {
    let x = direct(
        json!({"kind":"radial","center":[0.12345679,0.7654321],"radii":[0.03214568,0.2345679],"angle":-73.12345,"feather":32.98765}),
    );
    assert!(x.contains("crs:Left"));
    direct(
        json!({"kind":"radial","center":[0.5,0.5],"radii":[1e-30,1e-32],"angle":0.12345679,"feather":99.12345}),
    );
}
#[test]
fn direct_brush_multiple_strokes_pressure_erase() {
    direct(json!({"kind":"brush","strokes":[
        {"points":[[0.12345679,0.2345679,0.7654321],[0.3456789,0.456789,0.9876543]],"radius":0.012345679,"feather":43.12345,"flow":87.65432,"erase":false},
        {"points":[[0.654321,0.54321,0.4321]],"radius":0.03214568,"feather":12.98765,"flow":45.12345,"erase":true}
    ]}));
}
#[test]
fn direct_luminance_range() {
    let x = direct(
        json!({"kind":"luminance_range","range":[0.12345679,0.8765432],"smoothness":53.12345}),
    );
    assert!(x.contains("crs:LumMin"));
}
#[test]
fn direct_oklab_color_range() {
    let x = direct(
        json!({"kind":"color_range","samples":[[0.654321,-0.12345679,0.2345679],[0.8765432,0.14325678,-0.3214568]],"amount":67.12345}),
    );
    assert!(x.contains("crs:ColorAmount"));
    assert!(x.contains("ts:samples"));
}
#[test]
fn direct_ai_all_kinds_models_and_prompts() {
    let model = json!({"id":"segment/<&>","version":"v2.3 & test"});
    for kind in ["subject", "sky", "background"] {
        direct(json!({"kind":kind,"model":model}));
        direct(json!({"kind":kind,"model":null}));
    }
    direct(
        json!({"kind":"person","person":9007199254740993u64,"parts":["face_skin","hair","clothing"],"model":model}),
    );
    direct(
        json!({"kind":"object","prompt":"red <bicycle> & rider","region":{"left":0.12345679,"top":0.2345679,"right":0.8765432,"bottom":0.9876543},"points":[[0.3456789,0.456789],[0.654321,0.7654321]],"model":model}),
    );
    direct(json!({"kind":"object","prompt":null,"region":null,"points":[],"model":null}));
    direct(json!({"kind":"landscape","class":"natural_ground","model":model}));
    direct(json!({"kind":"depth","range":[0.2345679,0.8765432],"feather":34.98765,"model":model}));
}
fn foreign(component: &str) -> String {
    xml::packet(&format!(
        "<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:CorrectionMasks><rdf:Seq>{component}</rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>"
    ))
}
#[test]
fn direct_foreign_radial_rdf_description_attributes() {
    let t = xml::Tree::parse(&foreign(r#"<rdf:li><rdf:Description crs:What="Mask/CircularGradient" crs:Left="0.1" crs:Right="0.9" crs:Top="0.2" crs:Bottom="0.8" crs:Angle="23.5" crs:Feather="42.25" crs:MaskBlendMode="1" crs:MaskInverted="True"/></rdf:li>"#)).unwrap();
    let v = masks::import_masks(&t).unwrap();
    assert_eq!(v[0]["components"][0]["combine"], "subtract");
    assert_eq!(v[0]["components"][0]["center"], json!([0.5, 0.5]));
    assert_eq!(v[0]["components"][0]["invert"], true);
}
#[test]
fn direct_foreign_opaque_and_invalid_fail_atomically() {
    for c in [
        r#"<rdf:li crs:What="Mask/Future"/>"#,
        r#"<rdf:li crs:What="Mask/Paint"><crs:Dabs><rdf:Seq><rdf:li>opaque</rdf:li></rdf:Seq></crs:Dabs></rdf:li>"#,
        r#"<rdf:li crs:What="Mask/Gradient" crs:MaskActive="False"/>"#,
        r#"<rdf:li crs:What="Mask/Gradient" crs:MaskBlendMode="999"/>"#,
        r#"<rdf:li crs:What="Mask/Gradient" crs:FullX="NaN"/>"#,
    ] {
        assert!(masks::import_masks(&xml::Tree::parse(&foreign(c)).unwrap()).is_err());
    }
}
#[test]
fn foreign_unsupported_source_retained_on_export() {
    let source = foreign(r#"<rdf:li crs:What="Mask/Future" crs:MaskDigest="do-not-drop"/>"#);
    let p = XmpPacket::parse(source).unwrap();
    let r = p.to_recipe().unwrap();
    assert!(!r.warnings.is_empty());
    let p = XmpPacket::from_imported_recipe(&r.recipe, &MarkPreset::default()).unwrap();
    assert!(p.xml.contains("do-not-drop"));
}

#[test]
fn direct_radial_foreign_edit_does_not_use_stale_geometry() {
    let out = direct(
        json!({"kind":"radial","center":[0.5,0.5],"radii":[0.25,0.25],"angle":17.0,"feather":31.0}),
    );
    let changed = out.replace(
        "<crs:Right>0.75</crs:Right>",
        "<crs:Right>0.875</crs:Right>",
    );
    let back = masks::import_masks(&xml::Tree::parse(&xml::packet(&changed)).unwrap()).unwrap();
    assert_eq!(back[0]["components"][0]["center"][0], json!(0.5625));
    assert_eq!(back[0]["components"][0]["radii"][0], json!(0.3125));
}
#[test]
fn pipeline_all_mask_kinds() {
    for c in [
        json!({"kind":"radial","center":[0.2345679,0.654321],"radii":[0.12345679,0.2345679],"angle":37.12345,"feather":54.98765}),
        json!({"kind":"brush","strokes":[{"points":[[0.2345679,0.3456789,0.8765432]],"radius":0.023456789,"feather":34.12345,"flow":87.98765,"erase":true}]}),
        json!({"kind":"luminance_range","range":[0.12345679,0.8765432],"smoothness":53.12345}),
        json!({"kind":"color_range","samples":[[0.654321,-0.12345679,0.2345679]],"amount":67.12345}),
        json!({"kind":"subject","model":{"id":"segment/subject","version":"2"}}),
        json!({"kind":"sky","model":null}),
        json!({"kind":"background","model":null}),
        json!({"kind":"person","person":98765,"parts":["hair","iris"],"model":null}),
        json!({"kind":"object","prompt":"chair & <table>","points":[[0.12345679,0.7654321]],"model":null}),
        json!({"kind":"landscape","class":"water","model":null}),
        json!({"kind":"depth","range":[0.12345679,0.9876543],"feather":43.12345,"model":{"id":"depth/metric","version":"v1.2"}}),
    ] {
        roundtrip(c);
    }
}
