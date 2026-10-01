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
