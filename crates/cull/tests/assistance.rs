use cull::{
    CullSession, Decision,
    learning::{Learner, ReviewContext, ReviewMode, SuggestedDecision},
};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query, Score};

fn fixture() -> (tempfile::TempDir, Index, Vec<cull::ImageId>) {
    let dir = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).unwrap();
    for n in 0..3 {
        std::fs::write(dir.path().join(format!("{n}.jpg")), format!("image {n}")).unwrap();
    }
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    for (id, sharpness) in ids.iter().zip([0.0, 1.0, 0.5]) {
        index
            .set_score(
                *id,
                &Score {
                    signal: "sharpness".into(),
                    value: sharpness,
                    model: "synthetic".into(),
                },
            )
            .unwrap();
    }
    (dir, index, ids)
}

#[test]
fn catalog_features_cover_quality_faces_embedding_and_burst_rank() {
    use cull::learning::Measurements;
    let (_dir, index, ids) = fixture();
    for name in [
        "exposure_r",
        "exposure_g",
        "exposure_b",
        "shadow_clipping_r",
        "shadow_clipping_g",
        "shadow_clipping_b",
        "highlight_clipping_r",
        "highlight_clipping_g",
        "highlight_clipping_b",
        "motion_blur",
        "noise",
    ] {
        index
            .set_score(
                ids[2],
                &Score {
                    signal: name.into(),
                    value: 0.25,
                    model: "test".into(),
                },
            )
            .unwrap();
    }
    let face = |id, w, focus, eyes| index::FaceRecord {
        id,
        bbox: [10., 10., w, 20.],
        landmarks5: [[12., 12.]; 5],
        confidence: 1.,
        embedding: None,
        sharpness: focus,
        eyes_open: eyes,
    };
    index
        .replace_faces(
            ids[2],
            &[face(0, 20., 0.8, None), face(1, 40., 0.1, Some(0.1))],
        )
        .unwrap();
    let mut context = ReviewContext::default();
    context.dimensions.insert(ids[2], (100, 100));
    let group = cull::Group {
        images: ids.clone(),
    };
    let m = Measurements::from_index(&index, ids[2], &group, &context).unwrap();
    assert_eq!(m.sharpness, Some(0.5));
    assert_eq!(m.motion_blur, Some(0.25));
    assert_eq!(m.exposure, Some(0.25));
    assert_eq!(m.shadow_clipping, Some(0.25));
    assert_eq!(m.highlight_clipping, Some(0.25));
    assert_eq!(m.noise, Some(0.25));
    assert_eq!(m.face_count, 2);
    assert_eq!(m.min_face_focus, Some(0.1));
    assert!(m.any_eyes_closed);
    assert_eq!(m.largest_face_fraction, Some(0.08));
    assert_eq!(m.burst_sharpness_rank, Some(0.5));
    assert_eq!(
        Measurements::from_index(&index, ids[1], &group, &context)
            .unwrap()
            .burst_sharpness_rank,
        Some(1.)
    );
    let learner = Learner::with_embeddings("x", &[vec![1., 1.], vec![-1., -1.]]).unwrap();
    context.embeddings.insert(ids[2], vec![1., 1.]);
    context.embedding_model = "x".into();
    let session = CullSession::open(&index, Query::default()).unwrap();
    let plan = session
        .review(&learner, &context, ReviewMode::Assisted)
        .unwrap();
    assert!(
        plan.entries()
            .iter()
            .find(|e| e.image == ids[2])
            .unwrap()
            .prediction
            .explanation
            .iter()
            .any(|e| e.feature == "eyes_closed")
    );
}

#[test]
fn invalid_and_stale_confirmation_is_all_or_nothing() {
    let (_dir, index, ids) = fixture();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let mut learner = Learner::default();
    let context = ReviewContext::default();
    for (reject_below, keep_above) in [
        (f64::NAN, 0.8),
        (0.2, f64::INFINITY),
        (0.5, 0.8),
        (0.2, 0.5),
        (0.8, 0.2),
    ] {
        assert!(
            session
                .review(
                    &learner,
                    &context,
                    ReviewMode::Automated {
                        reject_below,
                        keep_above
                    }
                )
                .is_err()
        );
    }
    let plan = session
        .review(
            &learner,
            &context,
            ReviewMode::Automated {
                reject_below: 0.2,
                keep_above: 0.8,
            },
        )
        .unwrap();
    let mut incompatible =
        Learner::with_embeddings("different", &[vec![1., 0.], vec![0., 1.]]).unwrap();
    assert!(
        session
            .confirm_suggestions(&plan, &[ids[0]], &mut incompatible)
            .is_err()
    );
    assert_eq!(
        session.selection(ids[0]).unwrap().decision,
        Decision::Undecided
    );
    for batch in [
        vec![ids[0], ids[0]],
        vec![ids[0], ids[2]],
        vec![ids[0], cull::ImageId(999)],
    ] {
        assert!(
            session
                .confirm_suggestions(&plan, &batch, &mut learner)
                .is_err()
        );
        assert!(!session.can_undo());
        assert_eq!(learner.label_count(), 0);
        assert_eq!(
            session.selection(ids[0]).unwrap().decision,
            Decision::Undecided
        );
    }
    session.decide_images(&[ids[1]], Decision::Reject).unwrap();
    assert!(
        session
            .confirm_suggestions(&plan, &[ids[0], ids[1]], &mut learner)
            .is_err()
    );
    assert_eq!(
        session.selection(ids[0]).unwrap().decision,
        Decision::Undecided
    );
    let refreshed = session
        .review(
            &learner,
            &context,
            ReviewMode::Automated {
                reject_below: 0.2,
                keep_above: 0.8,
            },
        )
        .unwrap();
    assert!(
        refreshed
            .entries()
            .iter()
            .find(|e| e.image == ids[1])
            .unwrap()
            .suggested
            .is_none()
    );
    assert!(!refreshed.likely_rejects().contains(&ids[1]));
}

#[test]
fn queue_reordering_preserves_undo_redo_cursor_images() {
    let (_dir, index, ids) = fixture();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    session.set_current(ids[0]).unwrap();
    session.decide(Decision::Reject).unwrap();
    let advanced = session.current();
    let plan = session
        .review(
            &Learner::default(),
            &ReviewContext::default(),
            ReviewMode::Assisted,
        )
        .unwrap();
    session.reorder_review(&plan).unwrap();
    assert_eq!(session.current(), advanced);
    session.undo().unwrap();
    assert_eq!(session.current(), Some(ids[0]));
    session.redo().unwrap();
    assert_eq!(session.current(), advanced);
}

#[test]
fn best_of_burst_blends_taste_with_technical_and_ties_by_sharpness() {
    let (_dir, index, ids) = fixture();
    let session = CullSession::open(&index, Query::default()).unwrap();
    let learner = Learner::default();
    let plan = session
        .review(&learner, &ReviewContext::default(), ReviewMode::Assisted)
        .unwrap();
    let group = cull::Group {
        images: vec![ids[0], ids[2], ids[1]],
    };
    assert_eq!(plan.best_in_group(&group).unwrap(), ids[1]);
    // Equal sharpness + face-focus contributions give equal probabilities.
    // Equal stored technical quality, then sharpness breaks the tie.
    for (id, sharpness, focus) in [(ids[0], 0.25, 0.625), (ids[1], 0.75, 0.375)] {
        index
            .set_score(
                id,
                &Score {
                    signal: "sharpness".into(),
                    value: sharpness,
                    model: "test".into(),
                },
            )
            .unwrap();
        index
            .replace_faces(
                id,
                &[index::FaceRecord {
                    id: 0,
                    bbox: [0., 0., 10., 10.],
                    landmarks5: [[1., 1.]; 5],
                    confidence: 1.,
                    embedding: None,
                    sharpness: focus,
                    eyes_open: None,
                }],
            )
            .unwrap();
        // Swap exact binary fractions so both technical products are equal.
        index
            .set_score(
                id,
                &Score {
                    signal: "quality".into(),
                    value: if focus == 0.625 { 0.6875 } else { 0.8125 },
                    model: "test".into(),
                },
            )
            .unwrap();
    }
    let plan = session
        .review(&learner, &ReviewContext::default(), ReviewMode::Assisted)
        .unwrap();
    let group = cull::Group {
        images: vec![ids[0], ids[1]],
    };
    assert_eq!(plan.best_in_group(&group).unwrap(), ids[1]);
    assert!(plan.best_in_group(&cull::Group { images: vec![] }).is_err());
    assert!(
        plan.best_in_group(&cull::Group {
            images: vec![cull::ImageId(999)]
        })
        .is_err()
    );
    assert!(
        ids.iter()
            .all(|id| session.selection(*id).unwrap().decision == Decision::Undecided)
    );
}

#[test]
fn manual_decisions_train_only_after_successful_write() {
    let (_dir, index, ids) = fixture();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let mut learner = Learner::default();
    let context = ReviewContext::default();
    session.set_current(ids[0]).unwrap();
    session
        .decide_with_learning(Decision::Reject, &mut learner, &context)
        .unwrap();
    assert_eq!(learner.label_count(), 1);
    session.set_current(ids[0]).unwrap();
    session
        .decide_with_learning(Decision::Reject, &mut learner, &context)
        .unwrap();
    assert_eq!(learner.label_count(), 1, "no duplicate label for no-op");
    session.set_current(ids[0]).unwrap();
    session
        .decide_with_learning(Decision::Undecided, &mut learner, &context)
        .unwrap();
    assert_eq!(learner.label_count(), 1);
    session.set_current(ids[1]).unwrap();
    let path = index.image_info(ids[1]).unwrap().path;
    std::fs::remove_file(path).unwrap();
    assert!(
        session
            .decide_with_learning(Decision::Keep, &mut learner, &context)
            .is_err()
    );
    assert_eq!(learner.label_count(), 1);
}

#[test]
fn predictions_reorder_and_suggestions_only_commit_on_confirmation() {
    let (_dir, index, ids) = fixture();
    let mut session = CullSession::open(&index, Query::default()).unwrap();
    let mut learner = Learner::default();
    let context = ReviewContext::default();
    let assisted = session
        .review(&learner, &context, ReviewMode::Assisted)
        .unwrap();
    assert!(assisted.entries().iter().all(|e| e.suggested.is_none()));
    let automated = session
        .review(
            &learner,
            &context,
            ReviewMode::Automated {
                reject_below: 0.2,
                keep_above: 0.8,
            },
        )
        .unwrap();
    assert_eq!(
        automated
            .entries()
            .iter()
            .map(|e| e.image)
            .collect::<Vec<_>>(),
        vec![ids[1], ids[2], ids[0]]
    );
    assert_eq!(
        automated.entries()[0].suggested,
        Some(SuggestedDecision::Keep)
    );
    assert_eq!(automated.likely_rejects(), &[ids[0]]);
    session.reorder_review(&automated).unwrap();
    assert_eq!(session.images(), &[ids[1], ids[2], ids[0]]);
    for id in &ids {
        assert_eq!(
            session.selection(*id).unwrap().decision,
            Decision::Undecided
        );
        let paths = sidecar::Sidecar::paths(&index.image_info(*id).unwrap().path);
        assert!(!paths.recipe.exists() && !paths.xmp.exists());
    }
    assert!(!session.can_undo());
    assert_eq!(learner.label_count(), 0);
    session
        .confirm_suggestions(&automated, &[ids[1], ids[0]], &mut learner)
        .unwrap();
    assert_eq!(session.selection(ids[1]).unwrap().decision, Decision::Keep);
    assert_eq!(
        session.selection(ids[0]).unwrap().decision,
        Decision::Reject
    );
    assert_eq!(
        session.selection(ids[2]).unwrap().decision,
        Decision::Undecided
    );
    assert_eq!(learner.label_count(), 2);
    assert!(
        session
            .confirm_suggestions(&automated, &[ids[1]], &mut learner)
            .is_err()
    );
    session.undo().unwrap();
    assert!(
        ids.iter()
            .all(|id| session.selection(*id).unwrap().decision == Decision::Undecided)
    );
}
