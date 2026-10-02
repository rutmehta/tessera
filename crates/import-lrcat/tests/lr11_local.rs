//! LR-11 synthetic fixtures; no catalog rows or identifiers.
use import_lrcat::{diagnostics, lua_develop};

const GRADIENT: &str = "CorrectionMasks={{What='Mask/Gradient',MaskID='synthetic',FullX=0,FullY=0,ZeroX=1,ZeroY=0}}";
#[test]
fn local_adjustments_are_renderable_recipe_fields() {
    for (source, field) in [
        ("MainCurve={0,0,128,160,255,255}", "curves"),
        ("LocalPointColors={'0,0.8,0.5,0.1,0,0,0.5,0,0.25,0.75,1,0,0.25,0.75,1,0,0.25,0.75,1'}", "point_colors"),
        ("LocalToningHue=120,LocalToningSaturation=40", "color_overlay"),
        ("LocalDefringe=50", "defringe"),
    ] {
        let (r,w) = lua_develop::parse(&format!("s={{MaskGroupBasedCorrections={{{{{source},{GRADIENT}}}}}}}"), "15.4").unwrap();
        assert!(w.is_empty(), "{field}: {w:?}");
        let v = serde_json::to_value(&r).unwrap();
        let path = format!("/settings/locals/adjustments/0/params/{field}");
        assert!(v.pointer(&path).is_some_and(|v| !v.is_null()), "{field}");
        assert!(diagnostics::entries(&r).values().flatten().any(|d| d.status == "approximate" && d.field.as_deref() == Some(path.as_str())), "{field}");
        assert!(r.unknown["lrcat_develop_source"]["properties"].get("MaskGroupBasedCorrections").is_some());
        assert_eq!(r.history.entries.len(), 1);
        assert_eq!(engine_api::recipe::required_schema_version(&r), 4);
    }
}
#[test]
fn object_instance_is_approximate_and_retains_hint() {
    let (r,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{LocalExposure2012=1,CorrectionMasks={{What='Mask/Image',MaskSubType=0,ReferencePoint='0.5 0.5',InstanceIDs={{InstanceID=1}},InstanceBounds={{Left=0.2,Top=0.2,Right=0.8,Bottom=0.8}}}}}}}","15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    assert!(diagnostics::entries(&r).values().flatten().any(|d| d.status == "approximate" && d.reason.contains("per-instance")));
    assert!(serde_json::to_value(&r).unwrap().pointer("/settings/locals/adjustments/0/components/0/adobe_ai/instance_hint").is_some());
}
#[test]
fn radial_conflict_has_named_note() {
    let (_,w)=lua_develop::parse("s={MaskGroupBasedCorrections={{LocalExposure2012=1,CorrectionMasks={{What='Mask/CircularGradient',Left=0.2,Top=0.2,Right=0.8,Bottom=0.8,Flipped=true,MaskInverted=true}}}}}","15.4").unwrap();
    assert!(w.iter().any(|w| w.contains("radial mask inversion flags conflict")), "{w:?}");
}
