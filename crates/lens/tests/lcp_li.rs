use lens::*;
#[test]
fn lcp_rdf_li_property_elements() {
    let xml = r#"<rdf:RDF xmlns:rdf="r" xmlns:crs="c"><rdf:Description><crs:Make>Acme &amp; Co</crs:Make><crs:LensPrettyName>Prime</crs:LensPrettyName><crs:CameraProfiles><rdf:Seq><rdf:li><crs:FocalLength>35</crs:FocalLength><crs:PerspectiveModel><crs:RadialDistortParam1>0.05</crs:RadialDistortParam1></crs:PerspectiveModel></rdf:li></rdf:Seq></crs:CameraProfiles></rdf:Description></rdf:RDF>"#;
    let db = ProfileDatabase::from_lcp(xml).unwrap();
    assert!(db.profiles[0].maker.is_empty());
    assert_eq!(db.profiles[0].camera.as_ref().unwrap().maker, "Acme & Co");
    assert_eq!(db.profiles[0].samples[0].distortion.k1, 0.05);
}
