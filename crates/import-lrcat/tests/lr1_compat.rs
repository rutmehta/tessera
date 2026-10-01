use engine_api::id::Digest;
use import_lrcat::{lua_develop, xmp};
#[test]
fn untranslated_recipe_bytes_match_pre_lr1() {
    let lua = [
        ("empty", "s = {}"),
        ("global", include_str!("data/lrc155/global.lua")),
        // LR-3f: this row now translates its explicit-source heal; its pin includes retouch.
        ("structures", include_str!("data/lrc155/structures.lua")),
        ("legacy", include_str!("data/lrc155/legacy.lua")),
        (
            "future",
            "s = { FutureColorThing = { Strength = 0.8 }, Exposure2012 = 0.5 }",
        ),
        ("nil-point", "s = { PointColors = nil }"),
        ("empty-point", "s = { PointColors = {} }"),
        ("opaque-point", "s = { PointColors = { 'opaque' } }"),
        (
            "pending",
            "s = { LensBlur = { Active = true }, RetouchInfo = { 'opaque' } }",
        ),
    ];
    let mut actual = String::new();
    for (name, text) in lua {
        let r = lua_develop::parse(text, "15.4").unwrap().0;
        let bytes = r.to_json().unwrap();
        actual += &format!(
            "{name} {} {}\n",
            bytes.len(),
            Digest::derive("LR-1 byte compatibility", &bytes)
        );
    }
    let text = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="0.5"><crs:PointColors><rdf:Seq><rdf:li>opaque Adobe data</rdf:li></rdf:Seq></crs:PointColors></rdf:Description></rdf:RDF>"#;
    let bytes = xmp::parse(text, "15.4").unwrap().0.to_json().unwrap();
    actual += &format!(
        "xmp-opaque {} {}\n",
        bytes.len(),
        Digest::derive("LR-1 byte compatibility", &bytes)
    );
    assert_eq!(actual, include_str!("data/point-color-compat.txt"));
}
