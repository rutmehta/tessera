use engine_api::recipe::mask::{MaskComponent, MaskKind};
use serde::{Deserialize, Serialize};

#[test]
fn lr4_default_components_keep_legacy_json_bytes() {
    let component = MaskComponent::new(MaskKind::Linear {
        start: [0., 0.],
        end: [1., 0.],
    });
    assert_eq!(
        serde_json::to_string(&component).unwrap(),
        r#"{"kind":"linear","start":[0.0,0.0],"end":[1.0,0.0],"combine":"add","invert":false}"#
    );
}
#[test]
fn lr4_extensions_roundtrip_and_older_readers_ignore_them() {
    // Mirror the old wire layout, using only pre-existing mask variants.
    #[derive(Serialize, Deserialize)]
    struct LegacyComponent {
        #[serde(flatten)]
        kind: MaskKind,
        combine: engine_api::recipe::mask::MaskCombine,
        invert: bool,
    }
    let mut c = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
    c.enabled = false;
    c.group = Some(vec![MaskComponent::new(MaskKind::Linear {
        start: [0., 0.],
        end: [1., 0.],
    })]);
    let bytes = serde_json::to_vec(&c).unwrap();
    assert_eq!(serde_json::from_slice::<MaskComponent>(&bytes).unwrap(), c);
    let legacy: LegacyComponent = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(legacy.kind, MaskKind::Brush { strokes: vec![] });
    let legacy_bytes = serde_json::to_vec(&legacy).unwrap();
    let old = serde_json::from_slice::<MaskComponent>(&legacy_bytes).unwrap();
    assert!(old.enabled);
    assert!(old.group.is_none());
}

#[test]
fn lr4c_eight_levels_allowed_ninth_rejected() {
    let mut c = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
    for _ in 1..8 {
        let mut parent = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
        parent.group = Some(vec![c]);
        c = parent;
    }
    let mut g = engine_api::recipe::LocalAdjustment {
        components: vec![c],
        ..Default::default()
    };
    assert!(g.validate_mask_tree().is_ok());
    let mut parent = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
    parent.group = Some(g.components);
    g.components = vec![parent];
    assert!(g.validate_mask_tree().is_err());
}

#[test]
fn lr4e_luminance_domain_is_additive_and_roundtrips() {
    let legacy = r#"{"kind":"luminance_range","range":[0.4,0.5],"smoothness":0.0,"combine":"add","invert":false}"#;
    let c: MaskComponent = serde_json::from_str(legacy).unwrap();
    assert_eq!(serde_json::to_string(&c).unwrap(), legacy);
    let display = legacy.replace("\"range\"", "\"luminance_domain\":\"display\",\"range\"");
    let c: MaskComponent = serde_json::from_str(&display).unwrap();
    let value = serde_json::to_value(&c).unwrap();
    assert_eq!(value["luminance_domain"], "display");
    assert_eq!(serde_json::from_value::<MaskComponent>(value).unwrap(), c);
}

#[test]
fn lr4e_recipe_validate_rejects_ninth_level_even_when_disabled() {
    for depth in [8, 9] {
        let mut c = serde_json::json!({"kind":"brush","strokes":[]});
        for _ in 1..depth { c = serde_json::json!({"kind":"brush","strokes":[],"group":[c],"enabled":false}); }
        let mut r = engine_api::recipe::Recipe::default();
        r.settings.locals.adjustments = serde_json::from_value(serde_json::json!([{"components":[c]}])).unwrap();
        r.history.base = r.settings.clone();
        assert_eq!(r.validate().is_ok(), depth == 8);
    }
}
