use engine_api::{
    action::{Action, ActionCall},
    tools::DocumentToolCall,
};
use serde_json::json;

#[test]
fn editable_commands_are_registered_and_replayable() {
    for value in [
        json!({"tool":"add_text","document":1,"model":{"runs":[{"text":"Hello"}]}}),
        json!({"tool":"edit_text","document":1,"layer":2,"model":{}}),
        json!({"tool":"edit_text_runs","document":1,"layer":2,"range":{"start":0,"end":1},"runs":[{"text":"Hi"}]}),
        json!({"tool":"remove_vector_mask","document":1,"layer":2}),
        json!({"tool":"convert_to_pixels","document":1,"layer":2}),
    ] {
        let call: DocumentToolCall = serde_json::from_value(value).unwrap();
        assert!(call.edits_document());
        assert!(DocumentToolCall::NAMES.contains(&call.name()));
        let action = Action::from_document_tool(&call).unwrap();
        assert_eq!(action.decode().unwrap(), ActionCall::Document(call));
    }
}

#[test]
fn legacy_layer_summary_defaults_editable_metadata() {
    let layer: engine_api::document::LayerSummary = serde_json::from_value(json!({
        "id":1,"name":"legacy","kind":"pixel","visible":true,
        "opacity":1,"fill_opacity":1,"blend_mode":"normal"
    }))
    .unwrap();
    assert!(layer.text.is_none());
    assert!(layer.shape.is_none());
    assert!(!layer.has_vector_mask);
}
