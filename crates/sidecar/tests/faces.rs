use sidecar::{FaceRegion, XmpPacket};

#[test]
fn dimensions_are_written_as_standard_mwg_structure() {
    let packet = XmpPacket::parse(EMPTY)
        .unwrap()
        .with_face_regions(&[face()], false)
        .unwrap();
    let updated = packet.with_face_dimensions(6000, 4000).unwrap();
    assert!(updated.xml.contains("mwg-rs:AppliedToDimensions"));
    assert!(updated.xml.contains("stDim:w=\"6000\""));
    assert!(updated.xml.contains("stDim:h=\"4000\""));
    assert_eq!(updated.face_regions().unwrap(), vec![face()]);
    assert!(packet.with_face_dimensions(0, 4000).is_err());
    let replaced = updated.with_face_dimensions(3000, 2000).unwrap();
    assert!(!replaced.xml.contains("6000"));
    assert!(replaced.xml.contains("stDim:w=\"3000\""));
}

const EMPTY: &str = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:other="urn:other" other:keep="yes"><other:data a="1"> untouched </other:data></rdf:Description></rdf:RDF>"#;
#[test]
fn person_keywords_are_opt_in_additive_and_unique() {
    let source = EMPTY.replace("</rdf:Description>", r#"<dc:subject xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:Bag><rdf:li>travel</rdf:li></rdf:Bag></dc:subject></rdf:Description>"#);
    let packet = XmpPacket::parse(source).unwrap();
    assert_eq!(
        packet
            .with_face_regions(&[face()], false)
            .unwrap()
            .metadata()
            .unwrap()
            .keywords,
        vec!["travel"]
    );
    let updated = packet.with_face_regions(&[face(), face()], true).unwrap();
    assert_eq!(
        updated.metadata().unwrap().keywords,
        vec!["travel", &face().name]
    );
    assert_eq!(
        updated
            .with_face_regions(&[], true)
            .unwrap()
            .metadata()
            .unwrap()
            .keywords,
        updated.metadata().unwrap().keywords
    );
}

#[test]
fn alternate_prefixes_and_rdf_structs_preserve_non_face_regions() {
    let packet = XmpPacket::parse(include_str!("fixtures/mwg-faces.xmp")).unwrap();
    let expected = FaceRegion {
        name: "Ada & Bob".into(),
        ..face()
    };
    assert_eq!(packet.face_regions().unwrap(), vec![expected]);
    let updated = packet.with_face_regions(&[face()], false).unwrap();
    assert_eq!(updated.face_regions().unwrap(), vec![face()]);
    for raw in [
        r#"<m:AppliedToDimensions r:parseType="Resource" d:w="6000" d:h="4000" d:unit="pixel"/>"#,
        "<custom:keep> region metadata </custom:keep>",
        r#"<r:li r:parseType="Resource" m:Type="Pet" m:Name="Cat"><custom:payload/></r:li>"#,
    ] {
        assert!(updated.xml.contains(raw), "lost {raw}");
    }
    assert_eq!(
        updated
            .with_face_regions(&[], false)
            .unwrap()
            .face_regions()
            .unwrap(),
        vec![]
    );
}

#[test]
fn invalid_geometry_is_rejected_on_read_and_write() {
    let packet = XmpPacket::parse(EMPTY).unwrap();
    for invalid in [f64::NAN, f64::INFINITY, -0.1, 1.1] {
        assert!(
            packet
                .with_face_regions(
                    &[FaceRegion {
                        x: invalid,
                        ..face()
                    }],
                    false
                )
                .is_err()
        );
    }
    for invalid in [0.0, -0.1, 1.1, f64::NAN] {
        assert!(
            packet
                .with_face_regions(
                    &[FaceRegion {
                        w: invalid,
                        ..face()
                    }],
                    false
                )
                .is_err()
        );
    }
    let valid = packet.with_face_regions(&[face()], false).unwrap();
    for invalid in ["NaN", "inf", "-0.1", "1.1", "bad"] {
        let xml = valid
            .xml
            .replace("stArea:x=\"0.4\"", &format!("stArea:x=\"{invalid}\""));
        assert!(XmpPacket::parse(xml).unwrap().face_regions().is_err());
    }
    assert!(
        XmpPacket::parse(valid.xml.replace("normalized", "pixel"))
            .unwrap()
            .face_regions()
            .is_err()
    );
}

#[test]
fn empty_structures_accept_faces_without_losing_attributes() {
    for source in [
        r#"<r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"/>"#.to_owned(),
        EMPTY.replace("</rdf:Description>", r#"<m:Regions xmlns:m="http://www.metadataworkinggroup.com/schemas/regions/" rdf:parseType="Resource"/></rdf:Description>"#),
        EMPTY.replace("</rdf:Description>", r#"<m:Regions xmlns:m="http://www.metadataworkinggroup.com/schemas/regions/" rdf:parseType="Resource"><m:RegionList><rdf:Bag/></m:RegionList></m:Regions></rdf:Description>"#),
    ] {
        let packet = XmpPacket::parse(source).unwrap();
        let updated = packet.with_face_regions(&[face()], false).unwrap();
        assert_eq!(updated.face_regions().unwrap(), vec![face()]);
    }
}

fn face() -> FaceRegion {
    FaceRegion {
        name: "Zoë & <朋友>".into(),
        x: 0.4,
        y: 0.3,
        w: 0.2,
        h: 0.1,
    }
}

#[test]
fn normalized_faces_roundtrip_with_standard_namespaces_and_foreign_xml() {
    let packet = XmpPacket::parse(EMPTY).unwrap();
    assert!(packet.face_regions().unwrap().is_empty());
    let updated = packet.with_face_regions(&[face()], false).unwrap();
    let xml = &updated.xml;
    assert!(xml.contains("http://www.metadataworkinggroup.com/schemas/regions/"));
    assert!(xml.contains("http://ns.adobe.com/xmp/sType/Area#"));
    assert!(xml.contains("stArea:unit=\"normalized\""));
    assert!(xml.contains("<other:data a=\"1\"> untouched </other:data>"));
    assert_eq!(updated.face_regions().unwrap(), vec![face()]);
    let replaced = updated.with_face_regions(&[face()], false).unwrap();
    assert_eq!(replaced.face_regions().unwrap(), vec![face()]);
    assert!(
        replaced
            .with_face_regions(&[], false)
            .unwrap()
            .face_regions()
            .unwrap()
            .is_empty()
    );
}
