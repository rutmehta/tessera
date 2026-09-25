use engine_api::recipe::{EditMeta, ProcessVersion, Recipe};
use sidecar::{MarkPreset, Metadata, XmpPacket};

fn export(recipe: &Recipe) -> XmpPacket {
    XmpPacket::from_recipe(recipe, &Metadata::default(), &MarkPreset::lightroom()).unwrap()
}

#[test]
fn external_crs_edit_invalidates_native_revision() {
    let packet = export(&Recipe::default());
    let xml = packet.serialize();
    assert!(xml.contains("ts:ExportHash"));
    for edited in [
        xml.replace("<crs:Exposure2012>0</crs:Exposure2012>", "<crs:Exposure2012>1</crs:Exposure2012>"),
        xml.replace("<crs:Exposure2012>0.0</crs:Exposure2012>", "<crs:Exposure2012>1</crs:Exposure2012>"),
        xml.replace("</rdf:RDF>", "<rdf:Description xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:UnknownNewSetting=\"1\"/></rdf:RDF>"),
        xml.replace("<rdf:li>255, 255</rdf:li>", "<rdf:li>255, 240</rdf:li>"),
    ] {
        if edited == xml { continue; }
        assert_eq!(XmpPacket::parse(edited).unwrap().to_recipe().unwrap().recipe.process_version, ProcessVersion::adobe(6));
    }
}

#[test]
fn reexport_refreshes_hash_and_prefix_changes_do_not_invalidate_it() {
    let mut recipe = Recipe::default();
    let old = export(&recipe);
    recipe
        .edit(EditMeta::default(), |s| s.tone.exposure = 1.25)
        .unwrap();
    let packet = old.with_recipe(&recipe).unwrap();
    assert_eq!(
        packet.to_recipe().unwrap().recipe.process_version,
        recipe.process_version
    );
    let renamed = packet
        .serialize()
        .replace("crs:", "camera:")
        .replace("xmlns:crs=", "xmlns:camera=");
    assert_eq!(
        XmpPacket::parse(renamed)
            .unwrap()
            .to_recipe()
            .unwrap()
            .recipe
            .process_version,
        recipe.process_version
    );
}

#[test]
fn stale_revision_cannot_be_reactivated_by_reexport() {
    let xml = export(&Recipe::default()).serialize().replace(
        "<crs:Exposure2012>0.0</crs:Exposure2012>",
        "<crs:Exposure2012>1</crs:Exposure2012>",
    );
    let packet = XmpPacket::parse(xml).unwrap();
    let recipe = packet.to_recipe().unwrap().recipe;
    assert_eq!(recipe.process_version, ProcessVersion::adobe(6));
    let rewritten = packet.with_recipe(&recipe).unwrap();
    assert_eq!(
        rewritten.to_recipe().unwrap().recipe.process_version,
        ProcessVersion::adobe(6)
    );
    assert!(!rewritten.serialize().contains("NativeRevision"));
}

#[test]
fn unsigned_companion_is_not_trusted() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:ts="https://tessera.photo/ns/sidecar/1.0/" crs:ProcessVersion="15.4" ts:NativeRevision="1"/></rdf:RDF>"#;
    assert_eq!(
        XmpPacket::parse(xml)
            .unwrap()
            .to_recipe()
            .unwrap()
            .recipe
            .process_version,
        ProcessVersion::adobe(6)
    );
}
