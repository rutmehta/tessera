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
