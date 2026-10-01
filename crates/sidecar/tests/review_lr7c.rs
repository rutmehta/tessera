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

#[test]
fn inactive_ca_source_survives_sidecar_export() {
    for (pv, raw) in [("15.4", "035.000"), ("5.7", "0.000")] {
        let xml = format!(
            r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="{pv}" crs:ChromaticAberrationR="{raw}"/></rdf:RDF>"#
        );
        let r = XmpPacket::parse(xml).unwrap().to_recipe().unwrap().recipe;
        assert!(r.settings.lens.legacy_ca_red.is_none());
        let out = XmpPacket::from_imported_recipe(&r, &MarkPreset::default()).unwrap();
        assert!(
            out.xml
                .contains(&format!("crs:ChromaticAberrationR=\"{raw}\""))
        );
    }
}

#[test]
fn native_sidecar_companion_round_trips_optional_values() {
    let mut r = engine_api::recipe::Recipe::default();
    r.edit(engine_api::recipe::EditMeta::user("saved",0),|s| {
        s.lens.legacy_ca_blue=Some(-35.);
        s.geometry.upright=serde_json::from_value(serde_json::json!({"mode":"level","homography_mode":"level","homography":[[0.,-1.,1.],[1.,0.,0.],[0.,0.,1.]]})).unwrap();
    }).unwrap();
    let out = XmpPacket::from_recipe(&r, &Metadata::default(), &MarkPreset::default())
        .unwrap()
        .to_recipe()
        .unwrap();
    assert_eq!(out.recipe.settings.lens, r.settings.lens);
    assert_eq!(out.recipe.settings.geometry, r.settings.geometry);
}

#[test]
fn lr7d_default_acr_packet_has_no_catalog_bucket() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="15.4" crs:PerspectiveUpright="0" crs:UprightCenterMode="0" crs:UprightCenterNormX="0.5" crs:UprightCenterNormY="0.5" crs:UprightFocalMode="0" crs:UprightFocalLength35mm="35"/></rdf:RDF>"#;
    let recipe = XmpPacket::parse(xml).unwrap().to_recipe().unwrap().recipe;
    let mut expected = recipe.clone();
    expected.unknown.remove("lrcat_develop_source");
    assert_eq!(recipe.to_json().unwrap(), expected.to_json().unwrap());
    eprintln!(
        "ACR default fingerprint {}",
        blake3::hash(&expected.to_json().unwrap())
    );
}

#[test]
fn lr7d_geometry_rejects_existing_history_without_mutation() {
    let mut r = engine_api::recipe::Recipe::default();
    r.edit(engine_api::recipe::EditMeta::user("my edit", 1), |s| {
        s.tone.exposure = 1.
    })
    .unwrap();
    let before = r.clone();
    let mut warnings = vec![];
    let result = sidecar::apply_adobe_geometry(
        &mut r,
        &mut warnings,
        [("ChromaticAberrationR", "35")].into_iter(),
    );
    assert!(result.is_err());
    assert_eq!(r, before);
}

#[test]
fn lr7d_standalone_keeps_xmp_author_and_one_replayable_edit() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:Description crs:ProcessVersion="5.7" crs:Exposure2012="0.5" crs:ChromaticAberrationR="35"/></rdf:RDF>"#;
    let r = XmpPacket::parse(xml).unwrap().to_recipe().unwrap().recipe;
    assert_eq!(r.history.entries.len(), 1);
    assert_eq!(
        r.history.entries[0].meta.author,
        engine_api::recipe::Author::Import {
            source: "xmp".into()
        }
    );
    assert_eq!(r.history.state_at(r.history.head).unwrap(), r.settings);
    assert!(!r.unknown.contains_key("translation_diagnostics"));
}
