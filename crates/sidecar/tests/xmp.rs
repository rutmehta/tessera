use engine_api::recipe::{Decision, Selection};
use sidecar::{MarkPreset, XmpPacket};

const FOREIGN: &str =
    r#"<f:payload f:Rating="99"><![CDATA[<opaque>&data]]><f:child /></f:payload>"#;
#[test]
fn metadata_edit_preserves_raw_foreign_xml_and_resolves_namespaces() {
    let xml = format!(
        r#"<?xpacket begin=""?><x:xmpmeta xmlns:x="adobe:ns:meta/"><r:RDF xmlns:r="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><r:Description r:about="" xmlns:m="http://ns.adobe.com/xap/1.0/" xmlns:d="http://purl.org/dc/elements/1.1/" xmlns:f="urn:foreign" m:Rating="3" m:Label="Green" f:Rating="99">{FOREIGN}<d:subject><r:Bag><r:li>A &amp; B</r:li><r:li>two</r:li></r:Bag></d:subject></r:Description></r:RDF></x:xmpmeta><?xpacket end="w"?>"#
    );
    let packet = XmpPacket::parse(xml.clone()).unwrap();
    assert_eq!(packet.serialize(), xml);
    assert_eq!(packet.selection().unwrap().decision, Decision::Keep);
    let mut metadata = packet.metadata().unwrap();
    assert_eq!(metadata.keywords, ["A & B", "two"]);
    metadata.title = "T & <title>".into();
    metadata.description = "caption".into();
    metadata.creators = vec!["Photographer".into(), "Second author".into()];
    metadata.copyright = "© author".into();
    metadata.hierarchical_keywords = vec!["Places|NYC".into()];
    let updated = packet
        .with_metadata(&Selection::default(), &metadata, &MarkPreset::lightroom())
        .unwrap();
    assert!(updated.serialize().contains(FOREIGN));
    assert!(updated.serialize().contains("f:Rating=\"99\""));
    assert_eq!(updated.selection().unwrap(), Selection::default());
    assert_eq!(updated.metadata().unwrap(), metadata);
    assert!(XmpPacket::parse("<x>").is_err());
    assert!(XmpPacket::parse("<a/><b/>").is_err());
}
