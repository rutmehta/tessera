//! ENG-7b: "Remove Chromatic Aberration" defaults to off (Lightroom's Adobe
//! Default for most cameras); an imported `crs:AutoLateralCA` is honoured.
use sidecar::XmpPacket;

fn import(attrs: &str) -> bool {
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" {attrs}/></rdf:RDF>"#
    );
    XmpPacket::parse(xml)
        .unwrap()
        .to_recipe()
        .unwrap()
        .recipe
        .settings
        .lens
        .remove_chromatic_aberration
}

#[test]
fn auto_lateral_ca_maps_exactly_and_absence_is_off() {
    assert!(import(r#"crs:AutoLateralCA="1""#));
    assert!(!import(r#"crs:AutoLateralCA="0""#));
    assert!(!import(r#"crs:Exposure2012="0.5""#));
}
