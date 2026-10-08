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
            // ENG-7b: re-pinned (was bd82c6ac…0dbb); Remove CA defaults to off
            // and neither packet sets crs:AutoLateralCA.
            "original" => "c1d516e43f369225c94d36ebcf28a77fef67c4351c57edcef4b1472c3525e22c",
            // ENG-7b: re-pinned (was 9b94b7df…1de9), same reason.
            "extended" => "4ae81080433d4cd97649810fffc50c5a88143c99d5d5e6ef6c02e739c81ced23",
            _ => unreachable!(),
        };
        assert_eq!(blake3::hash(&bytes).to_hex().as_str(), expected, "{name}");
    }
}
