use engine_api::action::{Action, ActionCall, CommandDomain, CommandEffect};
use engine_api::tools::{LibraryToolCall, LibraryToolRequest};
use serde_json::{json, Value};

#[test]
fn library_calls_round_trip_and_register() {
    let fixtures: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/library-calls-1.3.json")).unwrap();
    assert_eq!(fixtures.len(), LibraryToolCall::NAMES.len());
    for (value, name) in fixtures.into_iter().zip(LibraryToolCall::NAMES) {
        let call: LibraryToolCall = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(call.name(), name);
        assert_eq!(serde_json::to_value(&call).unwrap(), value);
        assert!(!call.is_read_only());
        let action = Action::from_library_tool(&call).unwrap();
        assert_eq!(action.info().unwrap().domain, CommandDomain::Library);
        assert_eq!(action.info().unwrap().effect, CommandEffect::Effect);
        assert_eq!(action.decode().unwrap(), ActionCall::Library(call.clone()));
        let request = LibraryToolRequest {
            call,
            rationale: Some("user corrected identity".into()),
        };
        let encoded = serde_json::to_value(&request).unwrap();
        assert_eq!(
            serde_json::from_value::<LibraryToolRequest>(encoded).unwrap(),
            request
        );
        let mut minimal = value;
        minimal.as_object_mut().unwrap().remove("writes");
        let defaulted: LibraryToolCall = serde_json::from_value(minimal).unwrap();
        assert_eq!(
            serde_json::to_value(defaulted).unwrap()["writes"],
            json!({"write_sidecars":false,"person_keywords":false})
        );
    }
}

#[test]
fn unconfirm_and_clear_name_are_explicit() {
    for value in [
        json!({"tool":"confirm_person","person_id":42,"faces":[],"confirmed":false}),
        json!({"tool":"name_person","person_id":42,"name":null}),
    ] {
        let call: LibraryToolCall = serde_json::from_value(value).unwrap();
        assert_eq!(
            serde_json::from_value::<LibraryToolCall>(serde_json::to_value(&call).unwrap())
                .unwrap(),
            call
        );
    }
    // Avoid an omitted name accidentally clearing a catalog name.
    assert!(serde_json::from_value::<LibraryToolCall>(
        json!({"tool":"name_person","person_id":42})
    )
    .is_err());
}
