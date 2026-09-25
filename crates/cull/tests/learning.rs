use cull::Decision;
use cull::learning::{Learner, Measurements};

#[test]
fn closed_eyes_and_missed_face_focus_dominate_cold_start() {
    let model = Learner::default();
    let mut signals = Measurements {
        sharpness: Some(1.),
        min_face_focus: Some(1.),
        any_eyes_closed: true,
        ..Default::default()
    };
    let prediction = model.predict(&signals.features().unwrap());
    assert!(prediction.p_keep < 0.2);
    assert_eq!(prediction.explanation[0].feature, "eyes_closed");
    signals.any_eyes_closed = false;
    signals.min_face_focus = Some(0.);
    assert!(model.predict(&signals.features().unwrap()).p_keep < 0.2);
    assert_eq!(
        model
            .predict(&Measurements::default().features().unwrap())
            .p_keep,
        0.5
    );
    signals.sharpness = Some(f64::NAN);
    assert!(signals.features().is_err());
}

#[test]
fn pca_recovers_largest_variance_not_largest_sample() {
    let mut samples = Vec::new();
    for _ in 0..10 {
        samples.extend([vec![1., 0.], vec![-1., 0.]]);
    }
    samples.extend([vec![0., 1.], vec![0., -1.]]);
    let model = Learner::with_embeddings("x", &samples).unwrap();
    let features = model
        .features(&Measurements::default(), Some(("x", &[1., 0.])))
        .unwrap();
    assert!((features.values()[11] - 1.).abs() < 1e-6);
    assert!(features.values()[12].abs() < 1e-6);
}

#[test]
fn pca_and_learning_roundtrip_are_library_local() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    // Correlated axes: PC1 must recover the diagonal, not select input columns.
    let samples: Vec<_> = (-10..=10).map(|i| vec![i as f32, i as f32, 0.]).collect();
    let mut learner = Learner::with_embeddings("synthetic-v1", &samples).unwrap();
    let a = learner
        .features(
            &Measurements::default(),
            Some(("synthetic-v1", &[2., 2., 0.])),
        )
        .unwrap();
    assert_eq!(a.values().len(), 43);
    assert!((a.values()[11].abs() - 1.).abs() < 1e-8);
    assert!(a.values()[12..].iter().all(|x| x.abs() < 1e-8));
    learner.observe(&a, Decision::Keep);
    learner.save(dir.path(), "library/a").unwrap();
    let loaded = Learner::open(dir.path(), "library/a").unwrap();
    let b = loaded
        .features(
            &Measurements::default(),
            Some(("synthetic-v1", &[2., 2., 0.])),
        )
        .unwrap();
    assert_eq!(a.values(), b.values());
    assert_eq!(learner.predict(&a), loaded.predict(&b));
    assert_eq!(loaded.label_count(), 1);
    assert_eq!(
        Learner::open(dir.path(), "library/b")
            .unwrap()
            .label_count(),
        0
    );
    assert!(
        loaded
            .features(
                &Measurements::default(),
                Some(("wrong-model", &[2., 2., 0.]))
            )
            .is_err()
    );
    assert!(
        loaded
            .features(&Measurements::default(), Some(("synthetic-v1", &[2.])))
            .is_err()
    );
    assert!(Learner::with_embeddings("x", &[vec![f32::NAN]]).is_err());
    assert!(Learner::with_embeddings("x", &[vec![1.], vec![1., 2.]]).is_err());
    assert!(Learner::with_embeddings("x", &[]).is_err());
}

#[test]
fn thirty_decisions_learn_blur_and_explain_it() {
    let mut learner = Learner::default();
    let feature = |blur| {
        Measurements {
            motion_blur: Some(blur),
            ..Default::default()
        }
        .features()
        .unwrap()
    };
    // The label boundary is deliberately different from the cold-start prior.
    let held_out: Vec<_> = (0..100)
        .map(|n| ((n as f64 + 0.5) / 100., n < 65))
        .collect();
    let accuracy = |model: &Learner| {
        held_out
            .iter()
            .filter(|(blur, keep)| (model.predict(&feature(*blur)).p_keep >= 0.5) == *keep)
            .count()
    };
    let before = accuracy(&learner);
    for n in 0..30 {
        let blur = ((n * 17 % 30) as f64 + 0.5) / 30.;
        learner.observe(
            &feature(blur),
            if blur < 0.65 {
                Decision::Keep
            } else {
                Decision::Reject
            },
        );
    }
    let after = accuracy(&learner);
    assert!(
        after > 90,
        "held-out accuracy {after}/100, before {before}/100"
    );
    assert!(after > before);
    let prediction = learner.predict(&feature(0.95));
    assert_eq!(prediction.explanation[0].feature, "motion_blur");
    assert!(prediction.explanation[0].contribution < 0.);
    assert_eq!(learner.label_count(), 30);
    eprintln!("held-out synthetic accuracy: {after}/100 after 30 labels (cold start {before}/100)");
}

#[test]
fn full_rank_pca_is_finite_and_corrupt_models_fail_closed() {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    let mut state = 42u64;
    let rows: Vec<Vec<f32>> = (0..80)
        .map(|_| {
            (0..40)
                .map(|_| {
                    state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
                    ((state >> 32) as f64 / u32::MAX as f64 - 0.5) as f32
                })
                .collect()
        })
        .collect();
    let model = Learner::with_embeddings("test", &rows).unwrap();
    model.save(dir.path(), "library").unwrap();
    let loaded = Learner::open(dir.path(), "library").unwrap();
    let features = loaded
        .features(&Measurements::default(), Some(("test", &rows[0])))
        .unwrap();
    assert!(
        features.values()[11..]
            .iter()
            .all(|x| x.is_finite() && x.abs() <= 1.)
    );
    assert_eq!(
        features.values()[11..]
            .iter()
            .filter(|x| x.abs() > 1e-8)
            .count(),
        32
    );
    let path = Learner::storage_path(dir.path(), "library").unwrap();
    let valid: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for field in ["version", "library", "weights", "bias", "pca"] {
        let mut damaged = valid.clone();
        match field {
            "version" => damaged["version"] = serde_json::json!(99),
            "library" => damaged["library"] = serde_json::json!("other"),
            "weights" => damaged["learner"]["weights"] = serde_json::json!([0.]),
            "bias" => damaged["learner"]["bias"] = serde_json::json!(1e99),
            "pca" => damaged["learner"]["pca"]["axes"][0] = serde_json::json!([1.]),
            _ => unreachable!(),
        }
        std::fs::write(&path, serde_json::to_vec(&damaged).unwrap()).unwrap();
        assert!(Learner::open(dir.path(), "library").is_err(), "{field}");
    }
    assert!(Learner::storage_path(dir.path(), "").is_err());
    assert!(Learner::storage_path(dir.path(), &"x".repeat(101)).is_err());
    let escaped = Learner::storage_path(dir.path(), "../../library").unwrap();
    assert_eq!(escaped.parent().unwrap(), dir.path().join("cull-learning"));
}
