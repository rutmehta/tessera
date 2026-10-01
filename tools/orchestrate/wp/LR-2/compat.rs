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
        "29c byte compatibility: 5 synthetic recipes, {total} serialized bytes identical to 87ff1ff1"
    );
}
