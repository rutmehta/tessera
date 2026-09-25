use engine_api::recipe::Recipe;
use serde_json::json;
use sidecar::{MarkPreset, Metadata, XmpPacket};

#[allow(dead_code)]
#[path = "../src/xml.rs"]
mod xml;
fn resource<'a>(tree: &'a xml::Tree, n: &'a xml::Node) -> &'a xml::Node {
    n.children
        .iter()
        .map(|i| &tree.nodes[*i])
        .find(|n| n.ns == xml::RDF && n.local == "Description")
        .unwrap_or(n)
}
#[path = "../src/structures.rs"]
mod structures;

fn fixture() -> Recipe {
    let mut r = Recipe::default();
    let model = json!({"id":"depth & <model> \"α\"", "version":"  v2\n<&>  "});
    r.settings.color.point_colors = serde_json::from_value(json!([
        {"source_lch":[0.65,0.21,312.3],"hue_shift":-23.4,"saturation_shift":17.2,"luminance_shift":-9.3,"range":43.2},
        {"source_lch":[0.12,0.05,12.4],"hue_shift":3.1,"saturation_shift":-4.2,"luminance_shift":8.7,"range":12.5}
    ])).unwrap();
    r.settings.effects.lens_blur = serde_json::from_value(json!({"amount":63.2,"focus_range":[0.13,0.72],"bokeh":"  custom <&> \"花\"\n ","depth_model":model})).unwrap();
    let area = json!({"kind":"area","components":[
        {"kind":"brush","combine":"subtract","invert":true,"strokes":[{"points":[[0.1,0.2,0.7],[0.31,0.42,0.9]],"radius":0.023,"feather":34.2,"flow":65.7,"erase":true}]},
        {"kind":"object","combine":"intersect","invert":false,"prompt":"  remove <lamp> & \"wire\"\n花 ","region":{"left":0.1,"top":0.2,"right":0.8,"bottom":0.9},"points":[[0.23,0.34],[0.67,0.78]],"model":model},
        {"kind":"radial","center":[0.3,0.4],"radii":[0.1,0.2],"angle":23.2,"feather":48.3}
    ]});
    r.settings.locals.retouch = serde_json::from_value(json!([
        {"id":4,"kind":{"kind":"heal","source_offset":[-0.23,0.17]},"target":area,"opacity":72.3,"feather":23.4,"enabled":false},
        {"id":8,"kind":{"kind":"clone","source_offset":[0.12,-0.32]},"target":area,"opacity":48.2,"feather":11.3,"enabled":true},
        {"id":12,"kind":{"kind":"remove","model":model},"target":{"kind":"implicit"},"opacity":87.1,"feather":5.2,"enabled":true},
        {"id":16,"kind":{"kind":"skin","person":7,"strength":43.7},"target":{"kind":"mask","mask":0},"opacity":93.2,"feather":17.4,"enabled":true}
    ])).unwrap();
    let settings = r.settings.clone();
    r = Recipe::default();
    r.edit(Default::default(), |s| *s = settings).unwrap();
    r
}

#[test]
fn codec_round_trips_every_structure_independently() {
    use engine_api::recipe::CrsKey;
    let r = fixture();
    let doc = serde_json::to_value(&r).unwrap();
    for key in [
        CrsKey::PointColors,
        CrsKey::LensBlur,
        CrsKey::RetouchAreas,
        CrsKey::RetouchInfo,
    ] {
        let v = doc.pointer(key.recipe_path().unwrap()).unwrap();
        let body = structures::encode(key, v).unwrap();
        let tree = xml::Tree::parse(&xml::packet(&body)).unwrap();
        let decoded = structures::decode(key, &tree).unwrap();
        assert_eq!(&decoded, v, "{key}");
    }
}

#[test]
fn nonempty_structures_round_trip_without_source_packet() {
    let r = fixture();
    let p = XmpPacket::from_recipe(&r, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    let xml = p.serialize();
    assert!(xml.contains("crs:BlurAmount"));
    assert!(xml.contains("crs:Opacity"));
    assert!(!xml.contains("crs:SourceLch"));
    let imported = p.to_recipe().unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.recipe.settings, r.settings);
    let mut next = imported.recipe;
    assert_eq!(next.allocate_retouch_id().0, 17);
}

#[test]
fn codec_empty_and_legacy_lens_blur_are_disabled() {
    use engine_api::recipe::CrsKey;
    for body in [
        "<crs:LensBlur><rdf:Seq/></crs:LensBlur>",
        "<crs:LensBlur rdf:parseType=\"Resource\"/>",
        "<crs:LensBlur crs:Active=\"False\"/>",
    ] {
        let tree = xml::Tree::parse(&xml::packet(body)).unwrap();
        assert_eq!(
            structures::decode(CrsKey::LensBlur, &tree).unwrap(),
            json!(null)
        );
    }
    for (key, v) in [
        (CrsKey::LensBlur, json!(null)),
        (CrsKey::PointColors, json!([])),
        (CrsKey::RetouchAreas, json!([])),
        (CrsKey::RetouchInfo, json!([])),
    ] {
        let tree = xml::Tree::parse(&xml::packet(&structures::encode(key, &v).unwrap())).unwrap();
        assert_eq!(structures::decode(key, &tree).unwrap(), v);
    }
}

#[test]
fn codec_foreign_structures_are_not_misrepresented() {
    use engine_api::recipe::CrsKey;
    for (key, body) in [
        (
            CrsKey::PointColors,
            "<crs:PointColors><rdf:Seq><rdf:li>opaque Adobe values</rdf:li></rdf:Seq></crs:PointColors>",
        ),
        (
            CrsKey::RetouchInfo,
            "<crs:RetouchInfo><rdf:Seq><rdf:li>opaque legacy spot</rdf:li></rdf:Seq></crs:RetouchInfo>",
        ),
        (
            CrsKey::RetouchAreas,
            "<crs:RetouchAreas><rdf:Seq><rdf:li crs:SpotType=\"heal\"/></rdf:Seq></crs:RetouchAreas>",
        ),
        (
            CrsKey::LensBlur,
            "<crs:LensBlur crs:Active=\"True\" crs:FocalRange=\"-48 32 64 144\"/>",
        ),
        (
            CrsKey::LensBlur,
            "<crs:LensBlur crs:Active=\"True\" crs:BlurAmount=\"NaN\"/>",
        ),
    ] {
        let tree = xml::Tree::parse(&xml::packet(body)).unwrap();
        assert!(structures::decode(key, &tree).is_err(), "{key}");
    }
}

#[test]
fn codec_empty_legacy_alias_does_not_clear_retouch() {
    use engine_api::recipe::CrsKey;
    let v = serde_json::to_value(fixture().settings.locals.retouch).unwrap();
    let body = structures::encode(CrsKey::RetouchAreas, &v).unwrap()
        + "<crs:RetouchInfo><rdf:Seq/></crs:RetouchInfo>";
    let tree = xml::Tree::parse(&xml::packet(&body)).unwrap();
    assert_eq!(structures::decode(CrsKey::RetouchInfo, &tree).unwrap(), v);
}
