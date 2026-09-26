use engine_api::id::PersonId;
use engine_api::people::{
    ClusterOptions, FaceRef, FaceRegion, NameSuggestion, PeopleJobResult, PeopleWriteOptions,
    PersonSummary, QualityGate,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::Value;
use std::fmt::Debug;

fn round_trip<T: DeserializeOwned + Serialize + PartialEq + Debug>(value: &Value) {
    let parsed: T = serde_json::from_value(value.clone()).unwrap();
    let encoded = serde_json::to_value(&parsed).unwrap();
    assert_eq!(encoded, *value);
    assert_eq!(serde_json::from_value::<T>(encoded).unwrap(), parsed);
}

#[test]
fn people_schema_fixtures() {
    let f: Value = serde_json::from_str(include_str!("fixtures/people-1.3.json")).unwrap();
    round_trip::<PersonId>(&f["person_id"]);
    round_trip::<FaceRef>(&f["face"]);
    round_trip::<PersonSummary>(&f["person"]);
    round_trip::<FaceRegion>(&f["region"]);
    round_trip::<NameSuggestion>(&f["suggestion"]);
    round_trip::<ClusterOptions>(&f["options"]);
    round_trip::<QualityGate>(&f["options"]["quality_gate"]);
    round_trip::<PeopleJobResult>(&f["result"]);
    round_trip::<PeopleWriteOptions>(&f["writes"]);
}

#[test]
fn missing_people_options_are_safe() {
    let options: ClusterOptions = serde_json::from_str("{}").unwrap();
    assert_eq!(options, ClusterOptions::default());
    assert_eq!(options.cosine_threshold, 0.363);
    assert_eq!(options.refit_interval, 1000);
    assert_eq!(options.quality_gate.min_confidence, 0.9);
    assert_eq!(options.quality_gate.min_size, 32.0);
    assert_eq!(options.quality_gate.min_sharpness, 0.1);
    let writes: PeopleWriteOptions = serde_json::from_str("{}").unwrap();
    assert!(!writes.write_sidecars && !writes.person_keywords);
    let person: PersonSummary = serde_json::from_str(r#"{"id":42}"#).unwrap();
    assert_eq!(person.name, None);
    assert_eq!(person.confirmed_count, 0);
}
