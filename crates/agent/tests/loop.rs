use agent::{Agent, Config, providers::FakePlanner};
use engine_api::{
    id::ImageId,
    tools::{ToneUpdate, ToolCall, ToolRequest},
};
use sidecar::Sidecar;
use style_profile::{Profile, Questionnaire};
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, Agent, ImageId) {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("photo.jpg");
    image::RgbImage::from_pixel(32, 32, image::Rgb([100, 110, 120]))
        .save(&p)
        .unwrap();
    let mut a = Agent::open(
        d.path().join("app"),
        Profile::new("test", Questionnaire::default()).unwrap(),
        Config::default(),
    )
    .unwrap();
    let id = a.perceive(&p).unwrap().image;
    (d, p, a, id)
}
fn tone(id: ImageId, exposure: f32) -> ToolRequest {
    ToolRequest {
        call: ToolCall::SetTone {
            image: id,
            update: ToneUpdate {
                exposure: Some(exposure),
                ..Default::default()
            },
        },
        rationale: Some("scripted exposure".into()),
        group: None,
        expect_recipe: None,
    }
}
#[test]
fn clipping_rejects_and_revises() {
    let (_d, p, mut a, id) = fixture();
    let mut planner = FakePlanner::new([vec![tone(id, 10.)], vec![tone(id, 0.)]]);
    let r = a.edit(&p, Some(&mut planner), None, false).unwrap();
    assert_eq!(r.critiques.len(), 2);
    assert!(r.critiques[0].metrics.highlight_clipping > 0.05);
    assert!(!r.critiques[0].accepted);
    assert!(r.accepted);
    assert!(
        planner.requests[1]["critic"]["reasons"][0]
            .as_str()
            .unwrap()
            .contains("clipping")
    );
    assert_eq!(
        Sidecar::read_recipe(Sidecar::paths(p).recipe)
            .unwrap()
            .recipe
            .history
            .entries
            .len(),
        2
    );
}
#[test]
fn whole_plan_guarded_before_any_write() {
    let (_d, p, mut a, id) = fixture();
    let mut forbidden = tone(id, 0.);
    forbidden.call = ToolCall::IndexFolder {
        path: "/tmp".into(),
        recursive: true,
    };
    let mut planner = FakePlanner::new([vec![tone(id, 1.), forbidden]]);
    assert!(
        a.edit(&p, Some(&mut planner), None, false)
            .unwrap_err()
            .to_string()
            .contains("guardrail")
    );
    assert!(!Sidecar::paths(p).recipe.exists());
}
#[test]
fn redo_only_white_balance_and_dry_run_no_recipe() {
    let (_d, p, mut a, id) = fixture();
    let mut planner = FakePlanner::new([vec![tone(id, 1.)]]);
    assert!(
        a.edit(&p, Some(&mut planner), Some("warmer, keep the sky"), false)
            .is_err()
    );
    assert!(!Sidecar::paths(&p).recipe.exists());
    let r = a
        .edit(&p, None, Some("warmer, keep the sky"), true)
        .unwrap();
    assert!(!r.accepted);
    assert!(!Sidecar::paths(&p).recipe.exists());
    let r = a
        .edit(&p, None, Some("warmer, keep the sky"), false)
        .unwrap();
    let ToolCall::SetTone { update, .. } = &r.plans[0][0].call else {
        panic!()
    };
    assert!(update.temperature.is_some());
    assert!(update.exposure.is_none());
    let s = Sidecar::read_recipe(Sidecar::paths(p).recipe)
        .unwrap()
        .recipe
        .settings;
    assert_eq!(s.tone, engine_api::recipe::DevelopSettings::default().tone);
}
#[test]
fn batch_locks_scene_tone_and_sorts_review() {
    let (d, p, mut a, id) = fixture();
    let other = d.path().join("other.jpg");
    std::fs::copy(&p, &other).unwrap();
    let id2 = a.perceive(&other).unwrap().image;
    let mut planner = FakePlanner::new([vec![tone(id, 1.)], vec![tone(id2, -1.)]]);
    let inputs = vec![
        agent::BatchInput {
            path: p.clone(),
            burst: Some("scene".into()),
            people: vec![],
        },
        agent::BatchInput {
            path: other.clone(),
            burst: Some("scene".into()),
            people: vec![],
        },
    ];
    let reports = a
        .edit_batch(&inputs, Some(&mut planner), None, false)
        .unwrap();
    assert_eq!(reports.len(), 2);
    assert!(
        reports
            .windows(2)
            .all(|p| p[0].confidence <= p[1].confidence)
    );
    let left = Sidecar::read_recipe(Sidecar::paths(p).recipe)
        .unwrap()
        .recipe
        .settings;
    let right = Sidecar::read_recipe(Sidecar::paths(other).recipe)
        .unwrap()
        .recipe
        .settings;
    assert_eq!(left.tone, right.tone);
    assert_eq!(left.white_balance, right.white_balance);
}
#[test]
fn clipping_counts_pixels_and_skin_band_is_optional() {
    let mut rgb = image::RgbImage::from_pixel(10, 10, image::Rgb([120, 120, 120]));
    for x in 0..6 {
        rgb.put_pixel(x, 0, image::Rgb([255, 120, 120]));
    }
    let m = agent::metrics::measure(&rgb, &[], None).unwrap();
    assert!((m.highlight_clipping - 0.06).abs() < 1e-6);
    assert!(!m.acceptable(0.18, 0.5));
    assert!(m.skin_delta_e.is_none());
}

#[test]
fn applied_steps_keep_provenance_when_next_planner_call_fails() {
    let (_d, p, mut a, id) = fixture();
    let mut planner = FakePlanner::new([vec![tone(id, 10.)]]);
    assert!(a.edit(&p, Some(&mut planner), None, false).is_err());
    let doc = Sidecar::read_recipe(Sidecar::paths(p).recipe).unwrap();
    assert_eq!(
        doc.recipe.unknown["tessera_agent_v1"]["provenance"],
        "AI-assisted, non-generative edits"
    );
}

#[test]
fn iteration_and_time_limits_never_accept_unexecuted_plans() {
    let (_d, p, mut a, id) = fixture();
    a.config.max_iterations = 2;
    let mut planner =
        FakePlanner::new([vec![tone(id, 10.)], vec![tone(id, 10.)], vec![tone(id, 0.)]]);
    let report = a.edit(&p, Some(&mut planner), None, false).unwrap();
    assert!(!report.accepted);
    assert_eq!(report.plans.len(), 2);
    assert_eq!(report.stop_reason, "iteration limit");
    a.config.time_budget = std::time::Duration::from_nanos(1);
    let report = a.edit(&p, Some(&mut planner), None, false).unwrap();
    assert!(!report.accepted);
    assert!(report.plans.is_empty());
    assert_eq!(report.stop_reason, "time budget");
}

#[test]
fn face_metric_measures_only_face_box() {
    let mut rgb = image::RgbImage::from_pixel(10, 10, image::Rgb([255, 0, 0]));
    for y in 0..5 {
        for x in 0..5 {
            rgb.put_pixel(x, y, image::Rgb([128, 128, 128]));
        }
    }
    let face = engine_api::tools::FaceScore {
        region: engine_api::recipe::settings::NormalizedRect {
            left: 0.,
            top: 0.,
            right: 0.5,
            bottom: 0.5,
        },
        ..Default::default()
    };
    let band = agent::metrics::SkinBand {
        low: [50., -1., -1.],
        high: [60., 1., 1.],
        max_delta_e: 3.,
    };
    let measured = agent::metrics::measure(&rgb, &[face], Some(&band)).unwrap();
    assert!(measured.skin_delta_e.unwrap()[0] < 0.01);
}
