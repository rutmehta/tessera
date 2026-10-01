use engine_api::recipe::settings::{Curve, CurvePoint, LegacyPv2010, ToneCurves};
use engine_api::recipe::{EditMeta, Recipe};

#[test]
fn lr2b_optional_tone_fields_roundtrip_with_conditional_schema_four() {
    let mut r = Recipe::default();
    let before = serde_json::to_value(&r).unwrap();
    assert!(before["settings"]["tone"].get("legacy_pv2010").is_none());
    assert!(before["settings"]["tone"].get("curves_extended").is_none());
    let version = r.process_version;
    r.edit(EditMeta::default(), |s| {
        s.tone.legacy_pv2010 = Some(LegacyPv2010 {
            exposure: Some(1.),
            brightness: Some(50.),
            contrast: Some(25.),
            fill_light: Some(10.),
            recovery: Some(15.),
            blacks: Some(5.),
        });
        s.tone.curves_extended = Some(ToneCurves {
            red: Curve(vec![
                CurvePoint { x: -1., y: -0.5 },
                CurvePoint { x: 2., y: 3. },
            ]),
            ..Default::default()
        });
    })
    .unwrap();
    let back = Recipe::from_json(&r.to_json().unwrap()).unwrap();
    assert_eq!(back.settings, r.settings);
    assert_eq!(back.process_version, version);
    back.validate().unwrap();
    let after = serde_json::to_value(&back).unwrap();
    assert_eq!(before["schema_version"], 3);
    assert_eq!(after["schema_version"], 4);
    assert_eq!(r.schema_version, 3);
}
