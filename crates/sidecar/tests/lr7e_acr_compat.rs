use sidecar::XmpPacket;
#[test]
fn lr7e_acr_packets_match_main() {
    for (name, xml) in [
        (
            "original",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="15.4" crs:PerspectiveUpright="0" crs:UprightCenterMode="0" crs:UprightCenterNormX="0.5" crs:UprightCenterNormY="0.5" crs:UprightFocalMode="0" crs:UprightFocalLength35mm="35"/></rdf:RDF>"#,
        ),
        (
            "extended",
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="15.4" crs:PerspectiveUpright="0" crs:UprightCenterMode="0" crs:UprightCenterNormX="0.5" crs:UprightCenterNormY="0.5" crs:UprightFocalMode="0" crs:UprightFocalLength35mm="35" crs:UprightVersion="151388160" crs:UprightPreview="false" crs:UprightTransformCount="6" crs:UprightTransform_0="1,0,0,0,1,0,0,0,1" crs:ChromaticAberrationR="0" crs:ChromaticAberrationB="0"/></rdf:RDF>"#,
        ),
    ] {
        let recipe = XmpPacket::parse(xml).unwrap().to_recipe().unwrap().recipe;
        let bytes = recipe.to_json().unwrap();
        eprintln!("{name}: {} {}", bytes.len(), blake3::hash(&bytes));
        let expected = match name {
            "original" => "bd82c6ac0009c1f11342a8d837117c3c6f598591f43feb7096b1217d8cca0dbb",
            "extended" => "9b94b7dff899062b78c11e353b61aa95735145585db9a59e4396782c92c41de9",
            _ => unreachable!(),
        };
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), expected, "{name}");
    }
}
