//! LR-11b review findings for the shared XMP codec. Synthetic packets only.
use engine_api::recipe::Recipe;
use sidecar::{MarkPreset, Metadata, XmpPacket};

fn packet(locals: serde_json::Value) -> String {
    let mut r = Recipe::default();
    let locals = serde_json::from_value(locals).unwrap();
    r.edit(Default::default(), |s| s.locals.adjustments = locals)
        .unwrap();
    XmpPacket::from_recipe(&r, &Metadata::default(), &MarkPreset::lightroom())
        .unwrap()
        .serialize()
        .to_owned()
}

/// B2: instance keys are rejected on every mask kind, including an object
/// selection that would otherwise render as the whole object.
#[test]
fn b2_instance_keys_reject_the_mask_group_on_every_kind() {
    for component in [
        serde_json::json!({"kind":"object","model":null,"points":[[0.2,0.4]]}),
        serde_json::json!({"kind":"subject","model":null}),
        serde_json::json!({"kind":"linear","start":[0,0],"end":[1,0]}),
    ] {
        let xml = packet(serde_json::json!([{"params":{"exposure":1.0},"components":[component]}]));
        let control = XmpPacket::parse(xml.clone()).unwrap().to_recipe().unwrap();
        assert!(control.warnings.is_empty(), "{:?}", control.warnings);
        assert_eq!(control.recipe.settings.locals.adjustments.len(), 1);
        for keys in [
            "<crs:InstanceIDs><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:InstanceID>2</crs:InstanceID></rdf:li></rdf:Seq></crs:InstanceIDs>",
            "<crs:InstanceBounds><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:Left>0.1</crs:Left><crs:Top>0.2</crs:Top><crs:Right>0.4</crs:Right><crs:Bottom>0.8</crs:Bottom></rdf:li></rdf:Seq></crs:InstanceBounds>",
        ] {
            let anchor = "<crs:MaskActive>";
            assert_eq!(xml.matches(anchor).count(), 1);
            let edited = xml.replace(anchor, &format!("{keys}{anchor}"));
            let imported = XmpPacket::parse(edited).unwrap().to_recipe().unwrap();
            assert!(
                imported
                    .warnings
                    .iter()
                    .any(|w| w.contains("MaskGroupBasedCorrections")
                        && w.contains("individual AI instance selection")),
                "{component}: {:?}",
                imported.warnings
            );
            assert!(
                imported.recipe.settings.locals.adjustments.is_empty(),
                "{component}"
            );
            assert!(
                !serde_json::to_string(&imported.recipe)
                    .unwrap()
                    .contains("instance_hint")
            );
        }
    }
}

fn adobe_packet(top: &str, extended: &str) -> String {
    format!(
        "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\" xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:ProcessVersion=\"15.4\"{top}><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:MainCurve><rdf:Seq><rdf:li>0,0</rdf:li><rdf:li>255,127.5</rdf:li></rdf:Seq></crs:MainCurve><crs:ExtendedMainCurve><rdf:Seq>{extended}</rdf:Seq></crs:ExtendedMainCurve><crs:CorrectionMasks><rdf:Seq><rdf:li rdf:parseType=\"Resource\"><crs:What>Mask/Gradient</crs:What><crs:FullX>0</crs:FullX><crs:FullY>0</crs:FullY><crs:ZeroX>1</crs:ZeroX><crs:ZeroY>0</crs:ZeroY></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF></x:xmpmeta>"
    )
}

/// B3: Adobe extended local curves populate `curves_extended` only for HDR
/// output and only when they are not the identity, like the global curve.
#[test]
fn b3_extended_local_curves_follow_the_global_hdr_rule() {
    let real = "<rdf:li>0,0</rdf:li><rdf:li>255,255</rdf:li><rdf:li>510,600</rdf:li>";
    let identity = "<rdf:li>0,0</rdf:li><rdf:li>255,255</rdf:li><rdf:li>510,510</rdf:li>";
    for (top, extended, expected) in [
        ("", real, false),
        (" crs:HDREditMode=\"0\"", real, false),
        (" crs:HDREditMode=\"1\"", real, true),
        (" crs:HDREditMode=\"1\"", identity, false),
    ] {
        let imported = XmpPacket::parse(adobe_packet(top, extended))
            .unwrap()
            .to_recipe()
            .unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let p = &imported.recipe.settings.locals.adjustments[0].params;
        assert_eq!(p.curves.as_ref().unwrap().rgb.0[1].y, 0.5, "{top}");
        assert_eq!(p.curves_extended.is_some(), expected, "{top}");
    }
    // A malformed extended curve is still an error on an SDR image.
    let imported = XmpPacket::parse(adobe_packet(
        "",
        "<rdf:li>0,0</rdf:li><rdf:li>0,255</rdf:li>",
    ))
    .unwrap()
    .to_recipe()
    .unwrap();
    assert!(
        imported.warnings.iter().any(|w| w.contains("local curve")),
        "{:?}",
        imported.warnings
    );
    assert!(imported.recipe.settings.locals.adjustments.is_empty());
}

/// B3: a native `ts:curves_extended` field is an explicit recipe value and
/// keeps round-tripping, as the global native field does.
#[test]
fn b3_native_extended_local_curve_still_round_trips() {
    let xml = packet(serde_json::json!([{
        "params":{"curves_extended":{"blue":[{"x":0,"y":0},{"x":2,"y":1.5}]}},
        "components":[{"kind":"linear","start":[0,0],"end":[1,0]}]
    }]));
    let imported = XmpPacket::parse(xml).unwrap().to_recipe().unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(
        imported.recipe.settings.locals.adjustments[0]
            .params
            .curves_extended
            .is_some()
    );
}

/// S9: the codec accepts Adobe's signed local defringe range and round-trips it.
#[test]
fn s9_local_defringe_accepts_the_signed_adobe_range() {
    for value in [-100.0, -50.0, 0.0, 100.0] {
        let xml = packet(serde_json::json!([{
            "params":{"defringe":value},
            "components":[{"kind":"linear","start":[0,0],"end":[1,0]}]
        }]));
        let imported = XmpPacket::parse(xml).unwrap().to_recipe().unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{value}: {:?}",
            imported.warnings
        );
        assert_eq!(
            imported.recipe.settings.locals.adjustments[0]
                .params
                .defringe,
            value as f32
        );
    }
    for value in ["-100.5", "100.5"] {
        let xml = packet(serde_json::json!([{
            "params":{"defringe":50.0},
            "components":[{"kind":"linear","start":[0,0],"end":[1,0]}]
        }]));
        let anchor = "<crs:LocalDefringe>50</crs:LocalDefringe>";
        assert_eq!(xml.matches(anchor).count(), 1, "{xml}");
        let edited = xml.replace(
            anchor,
            &format!("<crs:LocalDefringe>{value}</crs:LocalDefringe>"),
        );
        let imported = XmpPacket::parse(edited).unwrap().to_recipe().unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|w| w.contains("invalid local defringe")),
            "{value}: {:?}",
            imported.warnings
        );
        assert!(imported.recipe.settings.locals.adjustments.is_empty());
    }
}
