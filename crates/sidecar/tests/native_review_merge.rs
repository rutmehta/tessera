use sidecar::XmpPacket;

#[test]
fn overrides_match_expanded_names_preserving_qualified_values_and_empty_overrides() {
    let embedded = XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:q="urn:embedded"><r:RDF><r:Description d:title="Old title"><d:description>Old caption</d:description><d:rights><r:Alt><r:li xml:lang="fr">Droits</r:li><r:li xml:lang="x-default">Rights</r:li></r:Alt></d:rights><q:custom r:parseType="Resource"><q:value>preserved</q:value></q:custom></r:Description></r:RDF></x:xmpmeta>"#).unwrap();
    let sidecar = XmpPacket::parse(r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:d="urn:other" xmlns:dc="http://purl.org/dc/elements/1.1/"><rdf:RDF><rdf:Description dc:description="New caption"><dc:title/><d:custom rdf:parseType="Resource"><d:value>sidecar value</d:value></d:custom></rdf:Description></rdf:RDF></x:xmpmeta>"#).unwrap();
    let before = embedded.serialize();
    let merged = embedded.with_sidecar_overrides(&sidecar).unwrap();
    let metadata = merged.metadata().unwrap();
    assert_eq!(metadata.title, "");
    assert_eq!(metadata.description, "New caption");
    assert_eq!(metadata.copyright, "Rights");
    let xml = merged.serialize();
    for value in [
        "Droits",
        "preserved",
        "sidecar value",
        "urn:embedded",
        "urn:other",
    ] {
        assert!(xml.contains(value), "{value}");
    }
    assert!(!xml.contains("Old title"));
    assert!(!xml.contains("Old caption"));
    assert_eq!(embedded.serialize(), before);
}
