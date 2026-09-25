use engine_api::{
    id::ImageId,
    id::JobId,
    jobs::{CancellationToken, Job, JobContext, Priority, Scheduler},
    recipe::{
        history::{Author, EditMeta},
        Recipe,
    },
};
use style_profile::{
    apply_batch, group_amount, BatchImage, BatchJob, Features, Profile, Questionnaire, SidecarStore,
};
#[test]
fn overlapping_bursts_people_and_group_amount_are_reversible() {
    let p = Profile::new(
        "lib",
        Questionnaire {
            brightness: 1.,
            skin_tone_priority: 1.,
            warmth: 0.5,
            ..Questionnaire::default()
        },
    )
    .unwrap();
    let inputs = (0..3)
        .map(|i| BatchImage {
            image: ImageId(i),
            features: Features {
                face_mean_luminance: Some(0.02 + 0.07 * i as f64),
                face_count: 1,
                ..Features::default()
            },
            burst: if i < 2 { Some("burst".into()) } else { None },
            people: if i > 0 { vec!["person".into()] } else { vec![] },
        })
        .collect::<Vec<_>>();
    let mut recipes = vec![Recipe::default(); 3];
    recipes[0]
        .edit(EditMeta::user("existing", 0), |s| {
            s.geometry.constrain_crop = true
        })
        .unwrap();
    let before = recipes[0].settings.clone();
    let queue = apply_batch(&p, &inputs, &mut recipes, 1., 1).unwrap();
    assert_eq!(
        recipes[0].settings.white_balance,
        recipes[1].settings.white_balance
    );
    assert_eq!(recipes[0].settings.tone, recipes[1].settings.tone);
    assert_eq!(
        recipes[1].settings.tone.exposure,
        recipes[2].settings.tone.exposure
    );
    assert_eq!(recipes[0].settings.geometry, before.geometry);
    assert!(queue.windows(2).all(|w| w[0].confidence <= w[1].confidence));
    let r = &mut recipes[0];
    let e = r.history.entries.last().unwrap();
    assert!(matches!(e.meta.author, Author::Agent { .. }));
    assert!(
        e.meta.rationale.as_ref().unwrap().contains("scene")
            || e.meta.rationale.as_ref().unwrap().contains("questionnaire")
    );
    let group = e.meta.group.unwrap();
    assert_eq!(r.history.groups.last().unwrap().name, "Agent base edit");
    let meta = group_amount(r, group).unwrap();
    assert_eq!(meta.amount, 1.);
    assert_eq!(meta.settings_at(0.).unwrap(), before);
    assert_eq!(meta.settings_at(1.).unwrap(), r.settings);
    let decoded = Recipe::from_json(&r.to_json().unwrap()).unwrap();
    assert_eq!(group_amount(&decoded, group).unwrap().amount, 1.);
    r.undo().unwrap();
    assert_eq!(r.settings, before);
    r.redo().unwrap();
    r.validate().unwrap();
}
#[test]
fn score_job_writes_real_sidecars_and_cancelled_job_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("photo.jpg");
    std::fs::write(&path, []).unwrap();
    let store = SidecarStore::new([(ImageId(1), path.clone())], "test-machine").unwrap();
    let p = Profile::new(
        "lib",
        Questionnaire {
            brightness: 1.,
            ..Questionnaire::default()
        },
    )
    .unwrap();
    let inputs = vec![BatchImage {
        image: ImageId(1),
        features: Features::default(),
        burst: None,
        people: vec![],
    }];
    let (job, rx) = BatchJob::new(p.clone(), inputs.clone(), store, 1., 4);
    assert_eq!(job.priority(), Priority::Score);
    let pool = jobs::ThreadPoolScheduler::new(1);
    let _handle = pool.submit(Box::new(job), None);
    assert_eq!(
        rx.recv_timeout(std::time::Duration::from_secs(5))
            .unwrap()
            .len(),
        1
    );
    let file = sidecar::Sidecar::paths(&path).recipe;
    let doc = sidecar::Sidecar::read_recipe(&file).unwrap();
    assert_eq!(doc.recipe.settings.tone.exposure, 1.);
    assert_eq!(doc.vector_clock.get("test-machine"), Some(&1));
    let bytes = std::fs::read(&file).unwrap();
    let store = SidecarStore::new([(ImageId(1), path)], "test-machine").unwrap();
    let (job, _) = BatchJob::new(p, inputs, store, 1., 5);
    let token = CancellationToken::new();
    token.cancel();
    assert!(Box::new(job)
        .run(&JobContext::new(JobId(2), token, None))
        .is_err());
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}
