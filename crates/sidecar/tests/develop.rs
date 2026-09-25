use engine_api::recipe::settings::{LensProfileSource, VignetteStyle, WhiteBalanceMode};
use engine_api::recipe::{
    CameraProfileRef, CrsKey, EditMeta, LensProfileRef, LensProfileSetup, MaskKind, ProcessVersion,
    Recipe,
};
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
        .map(|k| format!("{}=\"test\" ", k.qualified_name()))
        .collect();
    let xml = format!(
        r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:aux="http://ns.adobe.com/exif/1.0/aux/" {fields}/></rdf:RDF>"#
    );
    assert_eq!(
        XmpPacket::parse(xml).unwrap().crs_values().unwrap().len(),
        CrsKey::ALL.len()
    );
}

#[test]
fn native_recipe_exports_best_effort_pv6_with_companion() {
    let recipe = Recipe::default();
    assert_eq!(recipe.process_version, ProcessVersion::NATIVE_CURRENT);
    let packet =
        XmpPacket::from_recipe(&recipe, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    let xml = packet.serialize();
    assert!(xml.contains("<crs:ProcessVersion>15.4</crs:ProcessVersion>"));
    assert!(xml.contains("<ts:NativeRevision>2</ts:NativeRevision>"));
    let back = packet.to_recipe().unwrap().recipe;
    assert_eq!(back.process_version, ProcessVersion::NATIVE_CURRENT);
    assert_eq!(back.settings, recipe.settings);

    // Converting to Adobe PV6 drops the companion instead of leaving it stale.
    let mut adobe = back.clone();
    adobe.process_version = ProcessVersion::adobe(6);
    let out = packet.with_recipe(&adobe).unwrap();
    assert!(!out.serialize().contains("NativeRevision"));
    assert_eq!(
        out.to_recipe().unwrap().recipe.process_version,
        ProcessVersion::adobe(6)
    );
}

#[test]
fn lens_and_camera_profile_identity_round_trips() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" crs:LensProfileEnable="1" crs:LensProfileSetup="LensDefaults" crs:LensProfileName="Adobe (Canon EF 24-70mm f/2.8L II USM)" crs:LensProfileFilename="Canon EOS 5D Mark III (Canon EF 24-70mm f2.8L II USM) - RAW.lcp" crs:LensProfileDigest="0123456789ABCDEF0123456789ABCDEF" crs:CameraProfile="Adobe Standard" crs:CameraProfileDigest="FEDCBA9876543210FEDCBA9876543210"/></rdf:RDF>"#;
    let packet = XmpPacket::parse(xml).unwrap();
    let r = packet.to_recipe().unwrap().recipe;
    let lens = LensProfileRef {
        name: engine_api::id::LensProfileId::new("Adobe (Canon EF 24-70mm f/2.8L II USM)"),
        filename: "Canon EOS 5D Mark III (Canon EF 24-70mm f2.8L II USM) - RAW.lcp".into(),
        digest: "0123456789ABCDEF0123456789ABCDEF".into(),
        setup: LensProfileSetup::LensDefaults,
    };
    assert_eq!(
        r.settings.lens.profile,
        LensProfileSource::Database {
            profile: lens.clone()
        }
    );
    assert_eq!(
        r.settings.camera_profile.profile,
        CameraProfileRef {
            name: engine_api::id::ProfileId::new("Adobe Standard"),
            digest: "FEDCBA9876543210FEDCBA9876543210".into(),
        }
    );
    // A fresh packet (no source XMP to fall back on) carries every identity field.
    let fresh = XmpPacket::from_recipe(&r, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    let values = fresh.crs_values().unwrap();
    assert_eq!(values[&CrsKey::LensProfileSetup], "LensDefaults");
    assert_eq!(values[&CrsKey::LensProfileFilename], lens.filename);
    assert_eq!(values[&CrsKey::LensProfileDigest], lens.digest);
    assert_eq!(
        values[&CrsKey::CameraProfileDigest],
        "FEDCBA9876543210FEDCBA9876543210"
    );
    assert_eq!(fresh.to_recipe().unwrap().recipe.settings, r.settings);

    // Disabled profile wins over named fields.
    let off = XmpPacket::parse(xml.replace(
        r#"crs:LensProfileEnable="1""#,
        r#"crs:LensProfileEnable="0""#,
    ))
    .unwrap();
    assert_eq!(
        off.to_recipe().unwrap().recipe.settings.lens.profile,
        LensProfileSource::None
    );
}

#[test]
fn aux_enhance_properties_become_provenance_not_edits() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:aux="http://ns.adobe.com/exif/1.0/aux/" crs:ProcessVersion="15.4" aux:EnhanceDenoiseAlreadyApplied="True" aux:EnhanceDenoiseVersion="7" aux:EnhanceDenoiseLumaAmount="50"/></rdf:RDF>"#;
    let packet = XmpPacket::parse(xml).unwrap();
    let imported = packet.to_recipe().unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let r = imported.recipe;
    assert_eq!(r.settings, Default::default());
    let p = &r.provenance.properties;
    assert_eq!(p["aux:EnhanceDenoiseAlreadyApplied"], "True");
    assert_eq!(p["aux:EnhanceDenoiseVersion"], "7");
    assert_eq!(p["aux:EnhanceDenoiseLumaAmount"], "50");
    // Informational properties are never written from a recipe, but survive export.
    let mut edited = r.clone();
    edited
        .edit(EditMeta::user("Exposure", 1), |s| s.tone.exposure = 0.5)
        .unwrap();
    let out = packet.with_recipe(&edited).unwrap();
    assert!(
        out.serialize()
            .contains(r#"aux:EnhanceDenoiseAlreadyApplied="True""#)
    );
    let fresh =
        XmpPacket::from_recipe(&edited, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    assert!(!fresh.serialize().contains("Enhance"));
}
