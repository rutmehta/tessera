use sidecar::{ExportMetadataPolicy as Policy, XmpPacket};

fn packet() -> XmpPacket {
    XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:e="http://ns.adobe.com/exif/1.0/" xmlns:c="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:i="http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/" xmlns:p="http://ns.adobe.com/photoshop/1.0/" xmlns:ie="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" xmlns:m="http://www.metadataworkinggroup.com/schemas/regions/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/" d:rights="Copyright Holder" e:GPSLatitude="GPS-secret" e:ExposureTime="1/125" c:Exposure2012="2" p:City="City-secret"><d:creator><r:Seq><r:li>Photographer</r:li></r:Seq></d:creator><i:CreatorContactInfo r:parseType="Resource" i:CiEmailWork="contact@example.test"/><ie:PersonInImage><r:Bag><r:li>Face Name</r:li></r:Bag></ie:PersonInImage><m:Regions r:parseType="Resource"><m:RegionList><r:Bag><r:li r:parseType="Resource" m:Name="Face Name"/></r:Bag></m:RegionList></m:Regions><d:subject><r:Bag><r:li>Face Name</r:li><r:li>Landscape</r:li></r:Bag></d:subject><lr:hierarchicalSubject><r:Bag><r:li>People|Face Name</r:li><r:li>Nature|Landscape</r:li></r:Bag></lr:hierarchicalSubject></r:Description></r:RDF></x:xmpmeta>"#).unwrap()
}

#[test]
fn restricted_policies_are_namespace_aware_and_preserve_contact() {
    let source = packet();
    for (policy, contact, camera) in [
        (Policy::All, true, true),
        (Policy::CopyrightOnly, false, false),
        (Policy::CopyrightAndContact, true, false),
        (Policy::AllExceptCamera, true, false),
    ] {
        let out = source.for_export(policy, false, false, true).unwrap();
        assert!(out.xml.contains("Copyright Holder"));
        assert_eq!(out.xml.contains("contact@example.test"), contact);
        assert_eq!(out.xml.contains("1/125"), camera);
        assert_eq!(out.xml.contains("Exposure2012=\"2\""), camera);
        assert_eq!(out.xml.contains("Photographer"), contact);
        assert_eq!(
            out.xml.contains("Landscape"),
            matches!(policy, Policy::All | Policy::AllExceptCamera)
        );
    }
    assert_eq!(source, packet(), "source must not change");
}

#[test]
fn removes_person_and_location_before_writing_keyword_hierarchy() {
    let out = packet().for_export(Policy::All, true, true, true).unwrap();
    for secret in [
        "Face Name",
        "GPS-secret",
        "City-secret",
        "RegionList",
        "PersonInImage",
    ] {
        assert!(!out.xml.contains(secret), "leaked {secret}: {}", out.xml);
    }
    let metadata = out.metadata().unwrap();
    assert_eq!(metadata.keywords, ["Landscape"]);
    assert_eq!(metadata.hierarchical_keywords, ["Nature|Landscape"]);
    assert!(out.xml.contains("1/125"));
    assert!(out.xml.contains("contact@example.test"));
}

#[test]
fn hierarchy_switch_removes_hierarchy_without_removing_flat_keywords() {
    let out = packet()
        .for_export(Policy::All, false, false, false)
        .unwrap();
    assert!(out.metadata().unwrap().hierarchical_keywords.is_empty());
    assert_eq!(out.metadata().unwrap().keywords, ["Face Name", "Landscape"]);
}

#[test]
fn nested_camera_and_structured_person_names_are_removed() {
    let xml = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:f="urn:foreign" xmlns:exif="http://ns.adobe.com/exif/1.0/" xmlns:ie="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" xmlns:dc="http://purl.org/dc/elements/1.1/"><f:record rdf:parseType="Resource" exif:ExposureTime="nested-camera"><exif:BodySerialNumber>serial-secret</exif:BodySerialNumber></f:record><ie:PersonInImageWDetails><rdf:Bag><rdf:li rdf:parseType="Resource" ie:PersonName="Alice Example"/></rdf:Bag></ie:PersonInImageWDetails><dc:subject><rdf:Bag><rdf:li>Alice Example</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#;
    let out = XmpPacket::parse(xml)
        .unwrap()
        .for_export(Policy::AllExceptCamera, true, false, true)
        .unwrap();
    for secret in ["nested-camera", "serial-secret", "Alice Example"] {
        assert!(!out.xml.contains(secret), "leaked {secret}");
    }
}

#[test]
fn hierarchy_person_names_are_collected_from_every_property() {
    for hierarchy in [
        r#"<rdf:Description><lr:hierarchicalSubject><rdf:Bag><rdf:li rdf:parseType="Resource"><rdf:value>People|Alice</rdf:value><f:source>Agency</f:source></rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description>"#,
        r#"<rdf:Description><lr:hierarchicalSubject><rdf:Bag><rdf:li>Nature|Landscape</rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description><rdf:Description><lr:hierarchicalSubject><rdf:Bag><rdf:li>People|Alice</rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description>"#,
        r#"<rdf:Description><lr:hierarchicalSubject><rdf:Bag><rdf:li>Nature|Landscape</rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description><rdf:Description lr:hierarchicalSubject="People|Alice"/>"#,
    ] {
        let xml = format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:f="urn:foreign" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/"><rdf:Description><dc:subject><rdf:Bag><rdf:li>Alice</rdf:li><rdf:li>Landscape</rdf:li></rdf:Bag></dc:subject></rdf:Description>{hierarchy}</rdf:RDF></x:xmpmeta>"#
        );
        let source = XmpPacket::parse(xml).unwrap();
        for keep_hierarchy in [true, false] {
            let out = source
                .for_export(Policy::All, true, false, keep_hierarchy)
                .unwrap();
            assert!(!out.xml.contains("Alice"), "leaked Alice: {}", out.xml);
            assert_eq!(out.metadata().unwrap().keywords, ["Landscape"]);
        }
    }
}

#[test]
fn qualified_keywords_are_preserved_when_not_removed() {
    let keyword = r#"<rdf:li rdf:parseType="Resource"><rdf:value>Landscape</rdf:value><f:source>Agency</f:source></rdf:li>"#;
    let xml = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:f="urn:foreign" xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:subject><rdf:Bag>{keyword}</rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    let out = XmpPacket::parse(xml)
        .unwrap()
        .for_export(Policy::All, true, false, true)
        .unwrap();
    assert!(out.xml.contains(keyword));
    assert_eq!(out.metadata().unwrap().hierarchical_keywords, ["Landscape"]);
}

#[test]
fn qualified_person_declarations_remove_flat_keywords() {
    let mut leaks = Vec::new();
    for (label, prefix, suffix) in [
        (
            "PersonInImage bag",
            "<ie:PersonInImage><rdf:Bag>",
            "</rdf:Bag></ie:PersonInImage>",
        ),
        ("PersonInImage scalar", "", ""),
        (
            "MWG Name",
            "<m:Regions rdf:parseType=\"Resource\">",
            "</m:Regions>",
        ),
        (
            "Microsoft PersonDisplayName",
            "<mp:Regions rdf:parseType=\"Resource\">",
            "</mp:Regions>",
        ),
        (
            "IPTC PersonName",
            "<ie:PersonInImageWDetails><rdf:Bag><rdf:li rdf:parseType=\"Resource\">",
            "</rdf:li></rdf:Bag></ie:PersonInImageWDetails>",
        ),
    ] {
        let tag = match label {
            "PersonInImage bag" => "rdf:li",
            "PersonInImage scalar" => "ie:PersonInImage",
            "MWG Name" => "m:Name",
            "Microsoft PersonDisplayName" => "mp:PersonDisplayName",
            _ => "ie:PersonName",
        };
        for attribute_value in [false, true] {
            let declaration = if attribute_value {
                format!(r#"<{tag} rdf:value="Alice" f:source="Agency"/>"#)
            } else {
                format!(
                    r#"<{tag} rdf:parseType="Resource"><rdf:value>Alice</rdf:value><f:source>Agency</f:source></{tag}>"#
                )
            };
            let xml = format!(
                r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:ie="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" xmlns:m="http://www.metadataworkinggroup.com/schemas/regions/" xmlns:mp="http://ns.microsoft.com/photo/1.2/t/Region#" xmlns:f="urn:foreign"><rdf:Description dc:rights="Copyright Holder"><f:record f:value="Untouched"/>{prefix}{declaration}{suffix}<dc:subject><rdf:Bag><rdf:li>Alice</rdf:li><rdf:li>Landscape</rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF>"#
            );
            let source = XmpPacket::parse(xml.clone()).unwrap();
            for keep_hierarchy in [false, true] {
                let out = source
                    .for_export(Policy::All, true, false, keep_hierarchy)
                    .unwrap();
                if out.xml.contains("Alice") {
                    leaks.push(format!(
                        "{label}, attribute={attribute_value}, hierarchy={keep_hierarchy}"
                    ));
                    continue;
                }
                assert!(!out.xml.contains(&declaration));
                assert!(out.xml.contains(r#"dc:rights="Copyright Holder""#));
                assert!(out.xml.contains(r#"<f:record f:value="Untouched"/>"#));
                let metadata = out.metadata().unwrap();
                assert_eq!(metadata.keywords, ["Landscape"]);
                assert_eq!(
                    metadata.hierarchical_keywords,
                    if keep_hierarchy {
                        vec!["Landscape"]
                    } else {
                        vec![]
                    }
                );
            }
            assert_eq!(source.xml, xml);
        }
    }
    assert!(leaks.is_empty(), "flat person keywords leaked: {leaks:?}");
}

#[test]
fn qualified_keyword_value_attributes_are_filtered_and_preserved() {
    let kept = r#"<rdf:li rdf:value="Landscape" f:source="Agency"/>"#;
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/" xmlns:f="urn:foreign"><rdf:Description><dc:subject><rdf:Bag><rdf:li>Alice</rdf:li><rdf:li rdf:value="Alice" f:source="Private"/>{kept}</rdf:Bag></dc:subject><lr:hierarchicalSubject><rdf:Bag><rdf:li rdf:value="People|Alice" f:source="Private"/><rdf:li>Nature|Landscape</rdf:li></rdf:Bag></lr:hierarchicalSubject></rdf:Description></rdf:RDF>"#
    );
    let source = XmpPacket::parse(xml.clone()).unwrap();
    for keep_hierarchy in [false, true] {
        let out = source
            .for_export(Policy::All, true, false, keep_hierarchy)
            .unwrap();
        assert!(!out.xml.contains("Alice"), "leaked Alice: {}", out.xml);
        assert!(out.xml.contains(kept));
        assert_eq!(
            out.metadata().unwrap().hierarchical_keywords,
            if keep_hierarchy {
                vec!["Nature|Landscape"]
            } else {
                vec![]
            }
        );
    }
    let without_hierarchy = xml.replace(r#"<lr:hierarchicalSubject><rdf:Bag><rdf:li rdf:value="People|Alice" f:source="Private"/><rdf:li>Nature|Landscape</rdf:li></rdf:Bag></lr:hierarchicalSubject>"#, "");
    let out = XmpPacket::parse(without_hierarchy)
        .unwrap()
        .for_export(Policy::All, false, false, true)
        .unwrap();
    assert!(out.xml.contains(kept));
    assert_eq!(
        out.metadata().unwrap().hierarchical_keywords,
        ["Alice", "Alice", "Landscape"]
    );
    assert_eq!(source.xml, xml);
}
