#[path = "../../../../crates/import-lrcat/tests/common/mod.rs"]
mod common;
// Compare serialized recipes to the lane base on unrelated synthetic inputs.
fn main() {
    let rows = [
        "s = {}",
        "s = { Exposure2012=1.25, Contrast2012=-10, ToneCurvePV2012={0,0,128,140,255,255} }",
        "s = { FutureSetting={Nested='exact', Values={1,2,3}}, UprightFuture='opaque' }",
        "s = { AutoToneDigest='opaque', DepthMapInfo={Version=1}, Exposure=1, Brightness=75 }",
    ];
    let mut total = 0;
    for row in rows {
        let (before, _) = import_lrcat_baseline::lua_develop::parse(row, "15.4").unwrap();
        let (after, _) = import_lrcat::lua_develop::parse(row, "15.4").unwrap();
        let a = before.to_json().unwrap();
        let b = after.to_json().unwrap();
        assert_eq!(a, b, "{row}");
        total += a.len();
    }
    let packet = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="1.25" crs:AutoToneDigest="opaque"><crs:FutureSetting><rdf:Seq><rdf:li>exact</rdf:li></rdf:Seq></crs:FutureSetting></rdf:Description></rdf:RDF>"#;
    let (before, _) = import_lrcat_baseline::xmp::parse(packet, "15.4").unwrap();
    let (after, _) = import_lrcat::xmp::parse(packet, "15.4").unwrap();
    let a = before.to_json().unwrap();
    assert_eq!(a, after.to_json().unwrap());
    total += a.len();
    println!(
        "29c byte compatibility: 5 synthetic recipes, {total} serialized bytes identical to 26e5cb8a"
    );

    // Prove the changed bulk golden is confined to already-translated B&W
    // history, by compiling both the baseline importer and baseline sidecar.
    let dir = tempfile::tempdir().unwrap();
    let path = common::write(dir.path(), 2000);
    let before = import_lrcat_baseline::import(&path).unwrap();
    let after = import_lrcat::import(&path).unwrap();
    let mut changed = 0;
    let mut baseline_bytes = Vec::new();
    for (mut a, mut b) in before.images.into_iter().zip(after.images) {
        a.recipe.image_id = None;
        b.recipe.image_id = None;
        serde_json::to_writer(&mut baseline_bytes, &a).unwrap();
        baseline_bytes.push(b'\n');
        let av = serde_json::to_value(a).unwrap();
        let bv = serde_json::to_value(b).unwrap();
        if av != bv {
            changed += 1;
            assert_eq!(av["recipe"]["settings"], bv["recipe"]["settings"]);
            let mut normalized = av.clone();
            normalized["recipe"]["history"] = bv["recipe"]["history"].clone();
            assert_eq!(normalized, bv, "bulk golden changed outside history");
            assert!(!bv["recipe"]["settings"]["color"]["monochrome"].is_null());
        }
    }
    assert_eq!(
        engine_api::id::Digest::derive("B5-29c golden", &baseline_bytes).to_string(),
        "ad814642116dda65cf0a49494eee8a5cf034c2d3151cb8c35c69975f8cfd661a"
    );
    assert_eq!(changed, 1200);
    println!(
        "bulk golden audit: 1200 already-translated monochrome rows change history only; 800 rows remain byte-identical; old golden reproduced"
    );
}
