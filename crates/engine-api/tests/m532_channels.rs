use engine_api::document::{ChannelKind, StrokeTarget};

#[test]
fn explicit_alpha_display_and_channel_destination_are_additive() {
    let alpha = serde_json::json!({"kind":"alpha_display", "display_rgb":[0.25,0.5,0.75], "opacity":0.5, "selected":true});
    let value: ChannelKind = serde_json::from_value(alpha.clone()).expect("alpha display contract");
    assert_eq!(serde_json::to_value(value).unwrap(), alpha);
    assert_eq!(
        serde_json::from_str::<ChannelKind>(r#"{"kind":"alpha"}"#).unwrap(),
        ChannelKind::Alpha
    );
    let target = serde_json::json!({"channel":42});
    let value: StrokeTarget =
        serde_json::from_value(target.clone()).expect("channel stroke contract");
    assert_eq!(serde_json::to_value(value).unwrap(), target);
    for old in ["pixels", "mask"] {
        let value: StrokeTarget = serde_json::from_value(serde_json::json!(old)).unwrap();
        assert_eq!(serde_json::to_value(value).unwrap(), old);
    }
    assert_eq!(engine_api::CONTRACT_VERSION, "1.6.0");
}
