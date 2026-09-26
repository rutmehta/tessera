use engine_api::{
    action::{Action, CommandDomain, CommandEffect},
    tools::{DocumentToolCall, DocumentToolRequest},
};
use serde_json::json;

#[test]
fn filter_calls_are_document_actions_with_default_destructive_mode() {
    for name in [
        "document_remove_object",
        "remove_distractions",
        "content_aware_fill",
        "content_aware_move",
        "liquify",
        "camera_raw_filter",
        "neural_skin_smoothing",
        "neural_colorize",
        "neural_jpeg_artifact_removal",
    ] {
        let request: DocumentToolRequest = serde_json::from_value(json!({"tool":name,"document":1,"layer":2,"params":{},"expect_head":null,"rationale":"retouch","group":7})).unwrap();
        assert_eq!(request.call.name(), name);
        assert!(DocumentToolCall::NAMES.contains(&name));
        assert!(request.call.edits_document());
        assert!(!request.call.is_read_only());
        assert_eq!(request.call.document().unwrap().0, 1);
        assert_eq!(request.expect_head, Some(None));
        assert_eq!(serde_json::to_value(&request.call).unwrap()["smart"], false);
        let action = Action::from_document_tool(&request.call).unwrap();
        assert_eq!(action.info().unwrap().domain, CommandDomain::Document);
        assert_eq!(action.info().unwrap().effect, CommandEffect::Edit);
        assert!(action.decode().is_ok());
    }
}
