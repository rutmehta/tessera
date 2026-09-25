use lens::*;
#[test]
fn lcp_nested_samples_and_ca() {
    let xml = r#"<rdf:RDF xmlns:rdf="rdf" xmlns:crs="crs"><rdf:Description crs:Make="Acme" crs:LensPrettyName="Prime"><crs:CameraProfiles><rdf:Seq><rdf:li><rdf:Description crs:FocalLength="35" crs:ApertureValue="2.8" crs:FocusDistance="3"><crs:PerspectiveModel><rdf:Description crs:RadialDistortParam1="0.02" crs:TangentialDistortParam1="0.001" crs:ImageXCenter="0.5" crs:ImageYCenter="0.5"/></crs:PerspectiveModel><crs:ChromaticRedGreenModel><rdf:Description crs:ScaleFactor="1.001" crs:RadialDistortParam1="0.0002"/></crs:ChromaticRedGreenModel><crs:VignetteModel><rdf:Description crs:VignetteModelParam1="-0.2"/></crs:VignetteModel></rdf:Description></rdf:li></rdf:Seq></crs:CameraProfiles></rdf:Description></rdf:RDF>"#;
    let db = ProfileDatabase::from_lcp(xml).unwrap();
    let s = &db.profiles[0].samples[0];
    assert_eq!(s.focal, 35.);
    assert_eq!(s.distortion.k1, 0.02);
    assert_eq!(s.distortion.cx, 0.);
    assert_eq!(s.ca_red, [1.001, 0.0002, 0.]);
    assert_eq!(s.vignette[0], -0.2);
    assert!(ProfileDatabase::from_lcp("<x/>").is_err());
}
