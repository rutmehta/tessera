use lens::*;
#[test]
fn lcp_focal_coordinate_units() {
    let xml = r#"<rdf:RDF xmlns:rdf="r" xmlns:crs="c"><rdf:Description crs:Make="A" crs:LensPrettyName="B"><rdf:Description crs:FocalLength="50"><crs:PerspectiveModel><rdf:Description crs:FocalLengthX="1" crs:FocalLengthY="1" crs:RadialDistortParam1="0.1"/></crs:PerspectiveModel></rdf:Description></rdf:Description></rdf:RDF>"#;
    let db = ProfileDatabase::from_lcp(xml).unwrap();
    let s = &db.profiles[0].samples[0];
    assert!((s.distort([1., 0.])[0] - 1.025).abs() < 1e-12);
}
