use sidecar::{ExportMetadataPolicy as Policy, XmpPacket};

fn packet(properties: &str) -> XmpPacket {
    XmpPacket::parse(format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:i="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" xmlns:l="http://ns.adobe.com/lightroom/1.0/" xmlns:f="urn:foreign">{properties}</r:Description></r:RDF></x:xmpmeta>"#
    ))
    .unwrap()
}

fn wrapped(value: &str, attribute: bool) -> String {
    if attribute {
        format!(r#"<r:Description r:value="{value}" f:source="Agency"/>"#)
    } else {
        format!(
            "<r:Description><r:value>{value}</r:value><f:source>Agency</f:source></r:Description>"
        )
    }
}

#[test]
fn wrapped_person_keyword_is_removed() {
    for attribute in [false, true] {
        let keyword = wrapped("Alice", attribute);
        let kept = wrapped("Landscape", attribute);
        let source = packet(&format!(
            "<i:PersonInImage><r:Bag><r:li>Alice</r:li></r:Bag></i:PersonInImage><d:subject><r:Bag><r:li>{keyword}</r:li><r:li>{kept}</r:li></r:Bag></d:subject>"
        ));
        for hierarchy in [false, true] {
            let out = source
                .for_export(Policy::All, true, false, hierarchy)
                .unwrap();
            assert!(!out.xml.contains("Alice"), "{}", out.xml);
            assert!(out.xml.contains(&kept));
            if hierarchy {
                assert_eq!(out.metadata().unwrap().hierarchical_keywords, ["Landscape"]);
            }
        }
        assert!(source.xml.contains("Alice"));
    }
}

#[test]
fn wrapped_person_declaration_removes_flat_keyword() {
    for attribute in [false, true] {
        let name = wrapped("Alice", attribute);
        for declaration in [
            format!("<i:PersonInImage><r:Bag><r:li>{name}</r:li></r:Bag></i:PersonInImage>"),
            format!("<i:PersonInImage>{name}</i:PersonInImage>"),
            format!(
                "<i:PersonInImageWDetails><r:Bag><r:li r:parseType=\"Resource\"><i:PersonName>{name}</i:PersonName></r:li></r:Bag></i:PersonInImageWDetails>"
            ),
        ] {
            let source = packet(&format!(
                "{declaration}<d:subject><r:Bag><r:li>Alice</r:li><r:li>Landscape</r:li></r:Bag></d:subject>"
            ));
            for hierarchy in [false, true] {
                let out = source
                    .for_export(Policy::All, true, false, hierarchy)
                    .unwrap();
                assert!(!out.xml.contains("Alice"), "{}", out.xml);
                assert_eq!(out.metadata().unwrap().keywords, ["Landscape"]);
            }
        }
    }
}

#[test]
fn wrapped_people_hierarchy_removes_flat_keyword() {
    for attribute in [false, true] {
        let path = wrapped("People|Alice", attribute);
        let source = packet(&format!(
            "<l:hierarchicalSubject><r:Bag><r:li>{path}</r:li></r:Bag></l:hierarchicalSubject><d:subject><r:Bag><r:li>Alice</r:li><r:li>Landscape</r:li></r:Bag></d:subject>"
        ));
        for hierarchy in [false, true] {
            let out = source
                .for_export(Policy::All, true, false, hierarchy)
                .unwrap();
            assert!(!out.xml.contains("Alice"), "{}", out.xml);
            assert_eq!(out.metadata().unwrap().keywords, ["Landscape"]);
        }
    }
}
