use engine_api::recipe::{Decision, Grade, Mark, Selection};
use sidecar::{MarkPreset, XmpPacket};

#[test]
fn exhaustive_normalized_selections_and_custom_presets() {
    let presets = [
        MarkPreset::lightroom(),
        MarkPreset {
            labels: [
                ("retouch".into(), "Red".into()),
                ("client".into(), "Red".into()),
                ("empty".into(), "".into()),
            ]
            .into(),
        },
    ];
    for preset in presets {
        for decision in [Decision::Reject, Decision::Undecided, Decision::Keep] {
            for grade in [None, Some(Grade::One), Some(Grade::Two), Some(Grade::Three)] {
                for name in [
                    None,
                    Some("Red"),
                    Some("Yellow"),
                    Some("Green"),
                    Some("Blue"),
                    Some("Purple"),
                    Some("red"),
                    Some("retouch"),
                    Some("client"),
                    Some("empty"),
                    Some(""),
                    Some("é & <photo> \"quoted\""),
                ] {
                    let selection = Selection {
                        decision,
                        grade,
                        mark: name.map(Mark::new),
                    }
                    .normalized();
                    let packet = XmpPacket::from_selection(&selection, &preset);
                    assert_eq!(packet.selection().unwrap(), selection);
                }
            }
        }
    }
}

#[test]
fn opaque_and_untranslatable_values_are_not_overwritten_by_noop_export() {
    let xml = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:ProcessVersion="15.4" crs:EnhanceDenoiseAlreadyApplied="True" crs:EnhanceDenoiseVersion="7" crs:LensProfileDigest="ABCDEF"><crs:PointColors><rdf:Seq><rdf:li>opaque future data</rdf:li></rdf:Seq></crs:PointColors></rdf:Description></rdf:RDF>"#;
    let packet = XmpPacket::parse(xml).unwrap();
    let imported = packet.to_recipe().unwrap();
    assert!(!imported.warnings.is_empty());
    let out = packet.with_recipe(&imported.recipe).unwrap();
    assert!(
        out.serialize()
            .contains("crs:EnhanceDenoiseAlreadyApplied=\"True\"")
    );
    assert!(out.serialize().contains("crs:EnhanceDenoiseVersion=\"7\""));
    assert!(out.serialize().contains("crs:LensProfileDigest=\"ABCDEF\""));
    assert!(
        out.serialize()
            .contains("<rdf:li>opaque future data</rdf:li>")
    );
}
