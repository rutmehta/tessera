use sidecar::{MarkPreset, Metadata, XmpPacket};
#[test]
fn standalone_import_and_export_preserve_both_field_families() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="5.7" crs:ChromaticAberrationR="35" crs:PerspectiveUpright="1" crs:UprightTransform_1="1,0,0,0,1,0,0.2,0,1"/></rdf:RDF>"#;
    let imported = XmpPacket::parse(xml).unwrap().to_recipe().unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.recipe.settings.lens.legacy_ca_red, Some(35.));
    assert!(
        imported
            .recipe
            .settings
            .geometry
            .upright
            .homography
            .is_some()
    );
    let out = XmpPacket::from_recipe(
        &imported.recipe,
        &Metadata::default(),
        &MarkPreset::default(),
    )
    .unwrap()
    .to_recipe()
    .unwrap();
    assert_eq!(out.recipe.settings.lens, imported.recipe.settings.lens);
    assert_eq!(
        out.recipe.settings.geometry,
        imported.recipe.settings.geometry
    );
    assert_eq!(imported.recipe.history.entries.len(), 1);
}
