use engine_api::action::{Action, ActionCall, CommandEffect};
use engine_api::document::{ChannelKind, ChannelRasterRef, ChannelSummary, DocumentSummary};
use engine_api::id::{ChannelId, HistoryEntryId, HistoryGroupId, SelectionId};
use engine_api::tools::{DocumentToolCall, DocumentToolRequest, DocumentToolResponse};
use serde_json::{json, Value};

fn fixture_round_trip<
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
>(
    value: &Value,
) {
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    let encoded = serde_json::to_value(&parsed).unwrap();
    assert_eq!(encoded, *value);
    assert_eq!(serde_json::from_value::<T>(encoded).unwrap(), parsed);
}

#[test]
fn channel_value_schema_fixtures() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/channels-1.3.json")).unwrap();
    for summary in fixture["summary"]["channels"].as_array().unwrap() {
        fixture_round_trip::<ChannelId>(&summary["id"]);
        fixture_round_trip::<ChannelKind>(&summary["kind"]);
        fixture_round_trip::<ChannelSummary>(summary);
    }
    fixture_round_trip::<ChannelRasterRef>(&fixture["calls"][0]["raster"]);
    assert_eq!(ChannelId::from(SelectionId(7)), ChannelId(7));
    assert_eq!(ChannelId(7).to_string(), "channel#7");
    assert!(serde_json::from_value::<ChannelId>(json!(-1)).is_err());
    assert!(serde_json::from_value::<ChannelRasterRef>(json!({})).is_err());
}

#[test]
fn ordered_channels_survive_summary_round_trip() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/channels-1.3.json")).unwrap();
    let summary: DocumentSummary = serde_json::from_value(fixture["summary"].clone()).unwrap();
    assert_eq!(serde_json::to_value(summary).unwrap(), fixture["summary"]);
    let old: DocumentToolResponse =
        serde_json::from_str(include_str!("fixtures/document-1.2.json")).unwrap();
    assert_eq!(
        serde_json::to_value(old).unwrap()["ok"]["channels"],
        json!([])
    );
}

#[test]
fn channel_calls_use_history_envelope_and_action_registry() {
    let fixture: Value = serde_json::from_str(include_str!("fixtures/channels-1.3.json")).unwrap();
    for value in fixture["calls"].as_array().unwrap() {
        let call: DocumentToolCall = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&call).unwrap(), *value);
        assert!(call.edits_document());
        assert!(!call.is_read_only());
        assert_eq!(call.document().unwrap().0, 1);
        let action = Action::from_document_tool(&call).unwrap();
        assert_eq!(action.info().unwrap().effect, CommandEffect::Edit);
        assert_eq!(action.decode().unwrap(), ActionCall::Document(call.clone()));
        for expect_head in [None, Some(None), Some(Some(HistoryEntryId(4)))] {
            let request = DocumentToolRequest {
                call: call.clone(),
                rationale: Some("channel edit".into()),
                group: Some(HistoryGroupId(3)),
                expect_head,
            };
            let encoded = serde_json::to_value(&request).unwrap();
            assert_eq!(encoded.get("expect_head").is_some(), expect_head.is_some());
            assert_eq!(
                serde_json::from_value::<DocumentToolRequest>(encoded).unwrap(),
                request
            );
        }
    }
}

#[test]
fn channel_edit_returns_allocated_identity_and_reads_old_outputs() {
    let value = json!({"ok":{"tool":"document_edited","document":1,"entry":5,"layer":null,"selection":null,"channel":7}});
    let response: DocumentToolResponse = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(serde_json::to_value(response).unwrap(), value);
    let old = json!({"ok":{"tool":"document_edited","document":1,"entry":5,"layer":null,"selection":null}});
    let response: DocumentToolResponse = serde_json::from_value(old).unwrap();
    assert_eq!(
        serde_json::to_value(response).unwrap()["ok"]["channel"],
        Value::Null
    );
}
