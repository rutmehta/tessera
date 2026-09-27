use sidecar::{MarkPreset, XmpPacket};

#[test]
fn selection_update_preserves_language_alternatives_and_qualified_keywords() {
    let foreign = r#"<dc:title><rdf:Alt><rdf:li xml:lang="x-default">Lake</rdf:li><rdf:li xml:lang="fr">Lac</rdf:li></rdf:Alt></dc:title><dc:subject><rdf:Bag><rdf:li rdf:parseType="Resource"><rdf:value>Landscape</rdf:value><f:source>Agency</f:source></rdf:li></rdf:Bag></dc:subject>"#;
    let xml = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:f="urn:foreign">{foreign}</rdf:Description></rdf:RDF></x:xmpmeta>"#
    );
    let packet = XmpPacket::parse(xml).unwrap();
    let selection = engine_api::recipe::Selection::keep(Some(engine_api::recipe::Grade::Three));
    let out = packet
        .with_selection(&selection, &MarkPreset::lightroom())
        .unwrap();
    assert!(out.xml.contains(foreign));
    assert_eq!(out.selection().unwrap(), selection);
}
