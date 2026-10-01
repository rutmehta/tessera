use engine_api::recipe::settings::LensSettings;
use serde_json::json;

#[test]
fn each_legacy_ca_field_round_trips_independently() {
    for field in ["legacy_ca_red", "legacy_ca_blue"] {
        for amount in [-100., -25., 0., 35., 100.] {
            let mut value = serde_json::to_value(LensSettings::default()).unwrap();
            value[field] = json!(amount);
            let lens: LensSettings = serde_json::from_value(value).unwrap();
            let encoded = serde_json::to_value(&lens).unwrap();
            assert_eq!(encoded[field], json!(amount), "{field}");
            assert_eq!(
                serde_json::from_value::<LensSettings>(encoded).unwrap(),
                lens
            );
        }
    }
}

#[test]
fn absent_legacy_ca_fields_stay_absent() {
    let lens: LensSettings = serde_json::from_str("{}").unwrap();
    let value = serde_json::to_value(lens).unwrap();
    assert!(value.get("legacy_ca_red").is_none());
    assert!(value.get("legacy_ca_blue").is_none());
}
