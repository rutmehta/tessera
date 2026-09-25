use agent::{Agent, Config, providers::FakePlanner};
use engine_api::{
    recipe::Author,
    tools::{ToneUpdate, ToolCall, ToolRequest},
};
use sidecar::Sidecar;
use style_profile::{Profile, Questionnaire};

#[test]
fn console_records_scripted_rationale_and_provenance() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    image::RgbImage::from_pixel(32, 32, image::Rgb([100, 110, 120]))
        .save(&path)
        .unwrap();
    let profile = Profile::new("test", Questionnaire::default()).unwrap();
    let mut agent = Agent::open(dir.path().join("app"), profile, Config::default()).unwrap();
    let packet = agent.perceive(&path).unwrap();
    let request = ToolRequest {
        call: ToolCall::SetTone {
            image: packet.image,
            update: ToneUpdate {
                exposure: Some(0.25),
                ..Default::default()
            },
        },
        rationale: Some("Lift midtones".into()),
        group: None,
        expect_recipe: None,
    };
    let mut planner = FakePlanner::new([vec![request]]);
    let report = agent.edit(&path, Some(&mut planner), None, false).unwrap();
    assert_eq!(report.plans.len(), 1);
    let doc = Sidecar::read_recipe(Sidecar::paths(&path).recipe).unwrap();
    assert_eq!(doc.recipe.history.entries.len(), 1);
    let entry = &doc.recipe.history.entries[0];
    assert!(matches!(entry.meta.author, Author::Agent { .. }));
    assert_eq!(entry.meta.rationale.as_deref(), Some("Lift midtones"));
    assert_eq!(doc.recipe.history.groups[0].name, "Agent base edit");
    assert_eq!(
        doc.recipe.unknown["tessera_agent_v1"]["provenance"],
        "AI-assisted, non-generative edits"
    );
}
