use sidecar::{ExportMetadataPolicy, XmpPacket};
#[test]
fn native_keyword_merge_preserves_qualified_xmp_and_person_filtering() {
    let packet=XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:q="urn:qualifier"><dc:subject><rdf:Bag><rdf:li rdf:parseType="Resource"><rdf:value>Alice</rdf:value><q:note>retain qualifier</q:note></rdf:li></rdf:Bag></dc:subject></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    let merged = packet.with_native_keywords(&["Nature".into()]).unwrap();
    assert!(merged.serialize().contains("retain qualifier"));
    assert!(merged.serialize().contains("Alice"));
    assert!(
        merged
            .metadata()
            .unwrap()
            .keywords
            .contains(&"Alice".into())
    );
    let identities=XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:p="http://iptc.org/std/Iptc4xmpExt/2008-02-29/" p:PersonInImage="Alice"/></rdf:RDF></x:xmpmeta>"#).unwrap();
    let filtered = merged
        .for_export_with_person_source(
            ExportMetadataPolicy::All,
            true,
            false,
            true,
            Some(&identities),
        )
        .unwrap();
    assert!(!filtered.serialize().contains("Alice"));
    assert!(filtered.serialize().contains("Nature"));
}

#[test]
fn overlay_retains_language_alternatives_qualifiers_and_namespace_shadowing() {
    let embedded=XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:rights><rdf:Alt><rdf:li xml:lang="fr">Droits</rdf:li><rdf:li xml:lang="x-default">Rights</rdf:li></rdf:Alt></dc:rights><dc:description>Old</dc:description></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    let overlay=XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:p="urn:outer" xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:RDF><r:Description xmlns:p="urn:inner"><d:description>New</d:description><p:custom r:parseType="Resource"><r:value>value</r:value><p:qualifier>qualified</p:qualifier></p:custom></r:Description></r:RDF></x:xmpmeta>"#).unwrap();
    let merged = embedded.with_sidecar_overrides(&overlay).unwrap();
    assert_eq!(merged.metadata().unwrap().description, "New");
    assert_eq!(merged.metadata().unwrap().copyright, "Rights");
    assert!(merged.serialize().contains("Droits"));
    assert!(merged.serialize().contains("qualified"));
    assert!(merged.serialize().contains("urn:inner"));
    assert!(!merged.serialize().contains(">Old<"));
}
