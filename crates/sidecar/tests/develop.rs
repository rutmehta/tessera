use engine_api::recipe::settings::{VignetteStyle, WhiteBalanceMode};
use engine_api::recipe::{CrsKey, MaskKind, ProcessVersion, Recipe};
use sidecar::{MarkPreset, Metadata, XmpPacket};
#[test]
fn adobe_pv6_fixture_maps_to_valid_recipe() {
    let packet = XmpPacket::parse(include_str!("fixtures/lightroom-pv6.xmp")).unwrap();
    let imported = packet.to_recipe().unwrap();
    let r = imported.recipe;
    r.validate().unwrap();
    assert_eq!(r.process_version, ProcessVersion::adobe(6));
    assert_eq!(r.settings.tone.exposure, 1.25);
    assert_eq!(r.settings.white_balance.mode, WhiteBalanceMode::Custom);
    assert_eq!(r.settings.white_balance.temperature, 6200.0);
    assert_eq!(
        r.settings.effects.vignette.style,
        VignetteStyle::ColorPriority
    );
    assert_eq!(r.settings.tone.curves.rgb.0.len(), 3);
    assert!((r.settings.tone.curves.rgb.0[1].y - 140.0 / 255.0).abs() < 1e-6);
    let local = &r.settings.locals.adjustments[0];
    assert_eq!(local.name, "Foreground");
    assert_eq!(local.params.exposure, 0.5);
    assert_eq!(local.amount, 100.0);
    assert_eq!(
        local.components[0].kind,
        MaskKind::Linear {
            start: [0.8, 0.9],
            end: [0.2, 0.3]
        }
    );
    let emitted = packet.with_recipe(&r).unwrap();
    assert_eq!(emitted.to_recipe().unwrap().recipe.settings, r.settings);
}
#[test]
fn writes_every_mapped_key_and_reads_every_table_key() {
    let recipe = Recipe {
        process_version: ProcessVersion::adobe(6),
        ..Recipe::default()
    };
    let packet =
        XmpPacket::from_recipe(&recipe, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    let values = packet.crs_values().unwrap();
    for key in CrsKey::ALL {
        if key.recipe_path().is_some() {
            assert!(values.contains_key(key), "missing {key}");
        }
    }
    let back = packet.to_recipe().unwrap();
    back.recipe.validate().unwrap();
    assert_eq!(back.recipe.settings, recipe.settings);
    let fields: String = CrsKey::ALL
        .iter()
        .map(|k| format!("crs:{}=\"test\" ", k.xmp_name()))
        .collect();
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" {fields}/></rdf:RDF>"#
    );
    assert_eq!(
        XmpPacket::parse(xml).unwrap().crs_values().unwrap().len(),
        CrsKey::ALL.len()
    );
}
