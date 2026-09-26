//! M3-11 bridge surface: real culling signals, the face strip and per-person
//! filter, assisted / automated culling with the learner, the style profile,
//! agent runs with the deterministic scripted (Fake) planner, redo, accept and
//! revert. No network and no API key.
use std::{path::Path, sync::Arc};
use tessera_ffi::*;

/// Four JPEGs: two sharp checkerboards (a burst), one blurred flat frame and
/// one blown-out white frame.
fn shoot() -> (
    tempfile::TempDir,
    Arc<Engine>,
    String,
    Vec<(String, String)>,
) {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let checker = |shift: u32| {
        image::RgbImage::from_fn(160, 120, move |x, y| {
            let v = if ((x + shift) / 4 + y / 4).is_multiple_of(2) {
                40
            } else {
                210
            };
            image::Rgb([v, v, v / 2 + 20])
        })
    };
    checker(0).save(photos.join("a_sharp.jpg")).unwrap();
    checker(1).save(photos.join("b_sharp.jpg")).unwrap();
    image::RgbImage::from_fn(160, 120, |x, _| {
        let v = 90 + (x / 40) as u8;
        image::Rgb([v, v, v])
    })
    .save(photos.join("c_blurred.jpg"))
    .unwrap();
    image::RgbImage::from_pixel(160, 120, image::Rgb([255, 255, 255]))
        .save(photos.join("d_white.jpg"))
        .unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap()
        .path;
    let names = engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .map(|i| {
            let name = Path::new(&i.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            (name, i.id)
        })
        .collect();
    (dir, engine, folder, names)
}

fn id(names: &[(String, String)], name: &str) -> String {
    names.iter().find(|(n, _)| n == name).unwrap().1.clone()
}

fn quality() -> AnalysisOptions {
    AnalysisOptions {
        quality: true,
        faces: false,
        force: false,
    }
}

#[test]
fn analysis_writes_real_scores_the_defect_sweep_reads() {
    let (_dir, engine, folder, names) = shoot();
    for (_, image) in &names {
        let r = engine.analyze_image(image.clone(), quality()).unwrap();
        assert!(!r.skipped);
        assert!(r.sharpness.is_some() && r.faces.is_none());
    }
    // Already analysed: skipped unless forced.
    assert!(
        engine
            .analyze_image(names[0].1.clone(), quality())
            .unwrap()
            .skipped
    );
    let forced = AnalysisOptions {
        force: true,
        ..quality()
    };
    assert!(
        !engine
            .analyze_image(names[0].1.clone(), forced)
            .unwrap()
            .skipped
    );

    let session = engine.open_cull_session(folder).unwrap();
    let blurred = id(&names, "c_blurred.jpg");
    let white = id(&names, "d_white.jpg");
    let found = session
        .defect_sweep(vec![
            DefectThreshold {
                signal: "sharpness".into(),
                value: 0.3,
                direction: ThresholdDirection::Below,
            },
            DefectThreshold {
                signal: "highlight_clipping".into(),
                value: 0.05,
                direction: ThresholdDirection::Above,
            },
        ])
        .unwrap();
    let by_id = |image: &str| found.iter().find(|c| c.image_id == image);
    assert!(
        by_id(&blurred)
            .unwrap()
            .reasons
            .iter()
            .any(|r| r.signal == "sharpness")
    );
    let white_reasons = &by_id(&white).unwrap().reasons;
    assert!(
        white_reasons
            .iter()
            .any(|r| r.signal == "highlight_clipping" && r.model == "tessera-analysis-v1")
    );
    assert!(
        by_id(&id(&names, "a_sharp.jpg")).is_none(),
        "sharp, unclipped frames pass"
    );
}

fn embedding(axis: usize, jitter: f32) -> Vec<f32> {
    let mut v = vec![0.01; 128];
    v[axis] = 1.;
    v[(axis + 1) % 128] = jitter;
    v
}

#[test]
fn people_summary_has_counts_and_the_indexed_medoid_not_the_cover() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let faces = [(-0.2, 0.9), (0.0, 0.4), (0.2, 0.6)]
        .into_iter()
        .map(|(jitter, focus)| FaceInput {
            x: 10.,
            y: 20.,
            width: 40.,
            height: 50.,
            focus,
            eyes_open: None,
            embedding: Some(embedding(0, jitter)),
        })
        .collect();
    engine.set_faces(a.clone(), faces, 160, 120).unwrap();
    let session = engine.open_cull_session(folder).unwrap();
    let p = session.people(false).unwrap().remove(0);
    assert!(!p.named);
    assert_eq!(p.face_count, 3);
    assert_eq!(p.face_count, p.faces);
    assert_eq!(p.confirmed_count, 0);
    assert_eq!(p.cover_ordinal, 0);
    assert_eq!(p.medoid_face.unwrap().ordinal, 1);
    assert_eq!(
        session.person_members(p.id.clone()).unwrap(),
        (0..3)
            .map(|ordinal| PersonFace {
                image_id: a.clone(),
                ordinal
            })
            .collect::<Vec<_>>()
    );
    assert!(session.person_members("missing".into()).unwrap().is_empty());
    session
        .confirm_person_face(
            PersonFace {
                image_id: a,
                ordinal: 1,
            },
            true,
        )
        .unwrap();
    session
        .name_person(p.id, Some("Ada".into()), PeopleNameOptions::default())
        .unwrap();
    let p = session.people(false).unwrap().remove(0);
    assert!(p.named);
    assert_eq!(p.confirmed_count, 1);
}

#[test]
fn people_job_reports_actual_training_sample_not_assignment_count() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let face = FaceInput {
        x: 10.,
        y: 20.,
        width: 40.,
        height: 50.,
        focus: 0.8,
        eyes_open: None,
        embedding: Some(embedding(0, 0.05)),
    };
    engine
        .set_faces(a.clone(), vec![face.clone(); 1025], 160, 120)
        .unwrap();
    let session = engine.open_cull_session(folder).unwrap();
    let report = session.refresh_people(true).unwrap();
    assert!(report.approximate);
    assert_eq!(report.sample_size, 1024);
    assert_eq!(report.assigned, 1025);
    let report = session.refresh_people(false).unwrap();
    assert!(!report.approximate);
    assert_eq!(report.sample_size, 0, "incremental no-op does not train");
    engine.set_faces(a, vec![face; 3], 160, 120).unwrap();
    let report = session.refresh_people(true).unwrap();
    assert!(!report.approximate);
    assert_eq!(report.sample_size, 3);
}

#[test]
fn indexed_medoid_resolution_does_not_choose_a_nearby_descriptor() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"fixture").unwrap();
    let mut index = index::Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(
            dir.path(),
            &index::NoopSidecarReader,
            &index::NoopMetadataProvider,
        )
        .unwrap();
    let image = index.search(&index::Query::default()).unwrap()[0];
    let mut medoid = vec![0.; 128];
    medoid[0] = 1.;
    let mut near = medoid.clone();
    near[1] = 0.0000005;
    let faces: Vec<_> = [near, medoid.clone()]
        .into_iter()
        .enumerate()
        .map(|(n, embedding)| index::FaceRecord {
            id: n as u32,
            bbox: [0., 0., 50., 50.],
            landmarks5: [[0.; 2]; 5],
            confidence: 1.,
            embedding: Some(embedding),
            sharpness: 0.8,
            eyes_open: None,
        })
        .collect();
    index.replace_faces(image, &faces).unwrap();
    index
        .apply_people_plan(
            &[index::Person {
                id: "person".into(),
                name: None,
                medoid: Some(medoid.clone()),
            }],
            &[
                (
                    index::FaceKey {
                        image_id: image,
                        ordinal: 0,
                    },
                    "person".into(),
                ),
                (
                    index::FaceKey {
                        image_id: image,
                        ordinal: 1,
                    },
                    "person".into(),
                ),
            ],
        )
        .unwrap();
    assert_eq!(
        ml_faces::people::medoid_face(&index, "person", &medoid)
            .unwrap()
            .unwrap()
            .ordinal,
        1
    );
}

#[test]
fn people_edits_undo_and_redo_restore_identity_members_confirmations_and_names() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let face = FaceInput {
        x: 10.,
        y: 20.,
        width: 40.,
        height: 50.,
        focus: 0.8,
        eyes_open: None,
        embedding: Some(embedding(0, 0.05)),
    };
    engine
        .set_faces(a.clone(), vec![face; 3], 160, 120)
        .unwrap();
    let session = engine.open_cull_session(folder.clone()).unwrap();
    let original = session.people(false).unwrap();
    let pid = original[0].id.clone();
    let key = PersonFace {
        image_id: a.clone(),
        ordinal: 0,
    };
    assert_eq!(session.undo_people_edit().unwrap(), None);
    assert_eq!(session.redo_people_edit().unwrap(), None);
    session
        .name_person(
            pid.clone(),
            Some("Ada".into()),
            PeopleNameOptions::default(),
        )
        .unwrap();
    session.confirm_person_face(key.clone(), true).unwrap();
    session
        .split_person(pid.clone(), "split".into(), vec![key.clone()])
        .unwrap();
    session
        .assign_person_face(key.clone(), pid.clone())
        .unwrap();
    session.undo_people_edit().unwrap();
    assert_eq!(
        session.person_members("split".into()).unwrap(),
        vec![key.clone()]
    );
    session.redo_people_edit().unwrap();
    assert!(session.person_members("split".into()).unwrap().is_empty());
    session.undo_people_edit().unwrap();
    session
        .name_person(
            "split".into(),
            Some("Bob".into()),
            PeopleNameOptions::default(),
        )
        .unwrap();
    assert_eq!(
        session.redo_people_edit().unwrap(),
        None,
        "a new edit clears redo"
    );
    session.confirm_person_face(key.clone(), true).unwrap();
    session.merge_people(pid.clone(), "split".into()).unwrap();
    assert_eq!(session.person_members(pid.clone()).unwrap().len(), 3);
    assert_eq!(
        session.undo_people_edit().unwrap().as_deref(),
        Some("Merge people")
    );
    let restored = &session.person_assignments(a.clone()).unwrap()[0];
    assert_eq!(restored.person_id, "split");
    assert_eq!(restored.name.as_deref(), Some("Bob"));
    assert!(restored.confirmed);
    assert_eq!(
        session.redo_people_edit().unwrap().as_deref(),
        Some("Merge people")
    );
    session.undo_people_edit().unwrap();
    for expected in [
        "Confirm face",
        "Name person",
        "Split person",
        "Confirm face",
        "Name person",
    ] {
        assert_eq!(
            session.undo_people_edit().unwrap().as_deref(),
            Some(expected)
        );
    }
    assert_eq!(session.undo_people_edit().unwrap(), None);
    assert_eq!(session.people(false).unwrap(), original);
    let other = engine.open_cull_session(folder).unwrap();
    assert_eq!(
        other.undo_people_edit().unwrap(),
        None,
        "history is per session"
    );
    assert!(session.merge_people(pid.clone(), "missing".into()).is_err());
    assert_eq!(
        session.redo_people_edit().unwrap().as_deref(),
        Some("Name person"),
        "failure preserves redo"
    );
    assert!(session.people(false).unwrap()[0].named);
}

#[test]
fn face_strip_people_and_per_person_eyes_filter() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let b = id(&names, "b_sharp.jpg");
    let face = |x: f32, focus: f64, eyes: Option<f64>, who: usize| FaceInput {
        x,
        y: 20.,
        width: 40.,
        height: 50.,
        focus,
        eyes_open: eyes,
        embedding: Some(embedding(who, 0.05)),
    };
    engine
        .set_faces(
            a.clone(),
            vec![face(10., 0.9, Some(0.9), 0), face(100., 0.2, Some(0.8), 5)],
            160,
            120,
        )
        .unwrap();
    engine
        .set_faces(b.clone(), vec![face(12., 0.8, Some(0.1), 0)], 160, 120)
        .unwrap();
    let session = engine.open_cull_session(folder).unwrap();

    let strip = session.face_strip(a.clone()).unwrap();
    assert_eq!(strip.len(), 2);
    assert_eq!(strip[0].ordinal, 0);
    assert!((strip[0].x - 10. / 160.).abs() < 1e-9 && (strip[0].height - 50. / 120.).abs() < 1e-9);
    assert_eq!(strip[1].focus, 0.2);
    assert!(
        session
            .face_strip(id(&names, "c_blurred.jpg"))
            .unwrap()
            .is_empty()
    );

    let people = session.people(false).unwrap();
    assert_eq!(people.len(), 2);
    assert_eq!(
        people[0].images.len(),
        2,
        "the same person across the burst"
    );
    assert_eq!(people[0].cover_image, a, "the sharpest face is the cover");
    let bride = people[0].id.clone();
    assert_eq!(strip[0].person_id.as_deref(), Some(bride.as_str()));
    assert_eq!(
        session.face_strip(b.clone()).unwrap()[0]
            .person_id
            .as_deref(),
        Some(bride.as_str())
    );

    let mut all = session.frames_with_person(bride.clone(), None).unwrap();
    all.sort();
    let mut expected = vec![a.clone(), b.clone()];
    expected.sort();
    assert_eq!(all, expected);
    assert_eq!(session.frames_with_person(bride, Some(0.3)).unwrap(), [b]);
}

#[test]
fn people_name_undo_restores_library_and_opted_in_sidecars_exactly() {
    let (dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    engine
        .set_faces(
            a.clone(),
            vec![FaceInput {
                x: 10.,
                y: 20.,
                width: 40.,
                height: 50.,
                focus: 0.8,
                eyes_open: None,
                embedding: Some(embedding(0, 0.05)),
            }],
            160,
            120,
        )
        .unwrap();
    let session = engine.open_cull_session(folder.clone()).unwrap();
    let pid = session.people(false).unwrap()[0].id.clone();
    let image_path = dir.path().join("photos/a_sharp.jpg");
    let xmp = sidecar::Sidecar::paths(&image_path).xmp;
    // Naming default must not even probe this unreadable-as-a-file destination.
    std::fs::create_dir(&xmp).unwrap();
    session
        .name_person(
            pid.clone(),
            Some("Ada".into()),
            PeopleNameOptions::default(),
        )
        .unwrap();
    session.undo_people_edit().unwrap();
    assert!(!session.people(false).unwrap()[0].named);
    std::fs::remove_dir(&xmp).unwrap();
    let options = PeopleNameOptions {
        write_sidecars: true,
        person_keywords: true,
    };
    session
        .name_person(pid.clone(), Some("Ada".into()), options)
        .unwrap();
    let ada_bytes = std::fs::read(&xmp).unwrap();
    session
        .name_person(pid.clone(), Some("Bob".into()), options)
        .unwrap();
    let bob_bytes = std::fs::read(&xmp).unwrap();
    assert_ne!(ada_bytes, bob_bytes);
    session.undo_people_edit().unwrap();
    assert_eq!(
        std::fs::read(&xmp).unwrap(),
        ada_bytes,
        "undo removes added keywords too"
    );
    assert_eq!(
        session.person_assignments(a.clone()).unwrap()[0]
            .name
            .as_deref(),
        Some("Ada")
    );
    session.undo_people_edit().unwrap();
    assert!(!xmp.exists(), "new sidecar removed by inverse");
    session.redo_people_edit().unwrap();
    session.redo_people_edit().unwrap();
    assert_eq!(std::fs::read(&xmp).unwrap(), bob_bytes);
    let reopened = engine.open_cull_session(folder).unwrap();
    assert_eq!(
        reopened.person_assignments(a.clone()).unwrap()[0]
            .name
            .as_deref(),
        Some("Bob")
    );
    // A failed replay must leave its stack entry and the name unchanged.
    std::fs::write(&xmp, b"external edit").unwrap();
    assert!(session.undo_people_edit().is_err());
    assert_eq!(
        session.person_assignments(a).unwrap()[0].name.as_deref(),
        Some("Bob")
    );
    std::fs::write(&xmp, bob_bytes).unwrap();
    session.undo_people_edit().unwrap();
    assert_eq!(std::fs::read(&xmp).unwrap(), ada_bytes);
}

#[test]
fn people_undo_handles_unassigned_faces_conflicts_and_bounded_history() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let b = id(&names, "b_sharp.jpg");
    let face = FaceInput {
        x: 10.,
        y: 20.,
        width: 40.,
        height: 50.,
        focus: 0.8,
        eyes_open: None,
        embedding: None,
    };
    engine
        .set_faces(a.clone(), vec![face.clone()], 160, 120)
        .unwrap();
    let session = engine.open_cull_session(folder.clone()).unwrap();
    let p = session.people(false).unwrap().remove(0);
    assert_eq!(p.medoid_face, None);
    engine
        .set_faces(b.clone(), vec![face.clone()], 160, 120)
        .unwrap();
    let key = PersonFace {
        image_id: b.clone(),
        ordinal: 0,
    };
    session
        .assign_person_face(key.clone(), p.id.clone())
        .unwrap();
    session.undo_people_edit().unwrap();
    assert!(session.person_assignments(b.clone()).unwrap().is_empty());
    session.redo_people_edit().unwrap();
    assert_eq!(session.person_members(p.id.clone()).unwrap().len(), 2);
    // A no-op and a failed edit must not consume the assignment undo entry.
    session
        .assign_person_face(key.clone(), p.id.clone())
        .unwrap();
    assert!(
        session
            .confirm_person_face(
                PersonFace {
                    ordinal: 99,
                    ..key.clone()
                },
                true
            )
            .is_err()
    );
    assert_eq!(
        session.undo_people_edit().unwrap().as_deref(),
        Some("Assign face")
    );
    session.redo_people_edit().unwrap();
    let other = engine.open_cull_session(folder).unwrap();
    other.confirm_person_face(key.clone(), true).unwrap();
    assert!(
        session.undo_people_edit().is_err(),
        "never erase another session's edit"
    );
    other.undo_people_edit().unwrap();
    session.undo_people_edit().unwrap();
    session.redo_people_edit().unwrap();
    for i in 0..40 {
        session
            .confirm_person_face(key.clone(), i % 2 == 0)
            .unwrap();
    }
    for _ in 0..32 {
        assert!(session.undo_people_edit().unwrap().is_some());
    }
    assert_eq!(session.undo_people_edit().unwrap(), None);
    session.redo_people_edit().unwrap();
    engine
        .set_faces(b.clone(), vec![FaceInput { x: 15., ..face }], 160, 120)
        .unwrap();
    assert!(
        session.undo_people_edit().is_err(),
        "re-detection invalidates old face references"
    );
    assert!(session.person_assignments(b).unwrap().is_empty());
}

#[test]
fn automated_suggestions_confirm_as_one_undo_step_and_teach_the_learner() {
    let (_dir, engine, folder, names) = shoot();
    for (_, image) in &names {
        engine.analyze_image(image.clone(), quality()).unwrap();
    }
    let session = engine.open_cull_session(folder.clone()).unwrap();
    let off = session.review().unwrap();
    assert!(
        off.iter().all(|p| p.suggested.is_none()),
        "no pre-filled decisions unless automated"
    );

    let status = session
        .set_assist_mode(AssistMode::Automated {
            reject_below: 0.3,
            keep_above: 0.7,
        })
        .unwrap();
    assert_eq!(status.labels, 0);
    assert!(status.library_id.starts_with("folder-"));
    assert!(
        session
            .set_assist_mode(AssistMode::Automated {
                reject_below: 0.6,
                keep_above: 0.7
            })
            .is_err()
    );
    // Cold start: the default weights (sharpness, blur, clipping, faces).
    session
        .set_assist_mode(AssistMode::Automated {
            reject_below: 0.3,
            keep_above: 0.55,
        })
        .unwrap();
    let review = session.review().unwrap();
    assert_eq!(review.len(), 4);
    let sharp = id(&names, "a_sharp.jpg");
    let blurred = id(&names, "c_blurred.jpg");
    let get = |image: &str| review.iter().find(|p| p.image_id == image).unwrap();
    assert_eq!(get(&sharp).suggested, Some(Decision::Keep));
    assert_eq!(get(&blurred).suggested, Some(Decision::Reject));
    assert!(get(&blurred).likely_reject);
    assert!(
        get(&blurred)
            .explanation
            .iter()
            .any(|c| c.feature == "sharpness" && c.contribution < 0.)
    );
    assert!(review.first().unwrap().p_keep >= review.last().unwrap().p_keep);

    // Queue reordering follows the review: likely rejects last.
    let order = session.reorder_queue().unwrap();
    assert_eq!(order.last(), Some(&review.last().unwrap().image_id));

    // Dismiss (per-image reject) one suggestion; it can no longer be confirmed.
    let white = id(&names, "d_white.jpg");
    session.dismiss_suggestions(vec![white.clone()]).unwrap();
    let review = session.review().unwrap();
    assert!(
        review
            .iter()
            .find(|p| p.image_id == white)
            .unwrap()
            .suggested
            .is_none()
    );
    assert!(session.confirm_suggestions(vec![white]).is_err());

    let suggested: Vec<String> = review
        .iter()
        .filter(|p| p.suggested.is_some())
        .map(|p| p.image_id.clone())
        .collect();
    let update = session.confirm_suggestions(suggested.clone()).unwrap();
    assert_eq!(update.changed.len(), suggested.len());
    assert_eq!(
        session.selection(sharp.clone()).unwrap().decision,
        Decision::Keep
    );
    assert_eq!(
        session.selection(blurred.clone()).unwrap().decision,
        Decision::Reject
    );
    assert_eq!(
        session.assist_status().unwrap().labels,
        suggested.len() as u64
    );
    assert!(
        session.confirm_suggestions(suggested.clone()).is_err(),
        "the plan is stale after a write"
    );

    // One undo step restores every confirmed frame.
    let undone = session.undo().unwrap().unwrap();
    assert_eq!(undone.changed.len(), suggested.len());
    assert_eq!(
        session.selection(sharp.clone()).unwrap().decision,
        Decision::Undecided
    );

    // Manual decisions teach the (persisted) learner too.
    session.set_current(blurred).unwrap();
    session.decide(Decision::Reject).unwrap();
    let labels = session.assist_status().unwrap().labels;
    assert_eq!(labels, suggested.len() as u64 + 1);
    let reopened = engine.open_cull_session(folder).unwrap();
    assert_eq!(
        reopened.assist_status().unwrap().labels,
        labels,
        "library-local learner persists"
    );
}

#[test]
fn style_profile_questionnaire_persists_and_trains_on_user_edits() {
    let (_dir, engine, folder, names) = shoot();
    let status = engine.style_profile_status(folder.clone()).unwrap();
    assert!(!status.stored);
    assert_eq!(status.samples, 0);
    let answers = StyleQuestionnaire {
        brightness: 0.5,
        contrast: -0.25,
        warmth: 0.5,
        saturation: 0.,
        skin_tone_priority: 1.,
    };
    let saved = engine
        .set_style_questionnaire(folder.clone(), answers.clone())
        .unwrap();
    assert!(saved.stored);
    assert_eq!(
        engine
            .style_profile_status(folder.clone())
            .unwrap()
            .questionnaire,
        answers
    );
    assert!(
        engine
            .set_style_questionnaire(
                folder.clone(),
                StyleQuestionnaire {
                    brightness: 3.,
                    ..answers.clone()
                }
            )
            .is_err()
    );

    // One user edit in the folder becomes a training sample.
    let a = id(&names, "a_sharp.jpg");
    let mut recipe: serde_json::Value =
        serde_json::from_str(&engine.get_recipe(a.clone()).unwrap()).unwrap();
    recipe["settings"]["tone"]["exposure"] = serde_json::json!(0.5);
    recipe["history"]["entries"] = serde_json::json!([{
        "id": 1, "label": "Exposure +0.50", "author": {"kind": "user"}, "timestamp_ms": 1,
        "changes": [{"op": "set", "path": "/tone/exposure", "value": 0.5}]
    }]);
    recipe["history"]["head"] = serde_json::json!(1);
    engine.set_recipe_json(a, recipe.to_string()).unwrap();
    let trained = engine
        .train_style_profile(folder.clone(), CancelFlag::new(), None)
        .unwrap();
    assert_eq!(trained.samples, 1);
    assert_eq!(
        trained.questionnaire, answers,
        "training keeps the questionnaire"
    );
}

fn guardrails() -> AgentGuardrails {
    AgentGuardrails {
        allow_masks: false,
        allow_crop: false,
        allow_skin_retouch: false,
        visual_critic: false,
        max_iterations: 2,
        time_budget_seconds: 120,
    }
}

struct Progress(std::sync::Mutex<Vec<AgentRunProgress>>);
impl AgentRunListener for Progress {
    fn on_progress(&self, progress: AgentRunProgress) {
        self.0.lock().unwrap().push(progress);
    }
}

#[test]
fn scripted_agent_run_review_redo_accept_and_revert() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let c = id(&names, "c_blurred.jpg");
    let request = |images: Vec<String>, instruction: Option<&str>| AgentRunRequest {
        images: images
            .into_iter()
            .map(|image_id| AgentImageInput {
                image_id,
                burst: None,
                people: vec![],
            })
            .collect(),
        library_folder: folder.clone(),
        provider: AgentProvider::Scripted,
        guardrails: guardrails(),
        instruction: instruction.map(str::to_owned),
    };
    let listener = Arc::new(Progress(Default::default()));
    let report = engine
        .run_agent(
            request(vec![a.clone(), c.clone()], None),
            CancelFlag::new(),
            Some(listener.clone()),
        )
        .unwrap();
    assert!(!report.cancelled);
    assert_eq!(report.provider, "scripted planner");
    assert_eq!(report.items.len(), 2);
    assert!(
        report.items[0].confidence <= report.items[1].confidence,
        "least confident first"
    );
    let item = report.items.iter().find(|i| i.image_id == a).unwrap();
    assert!(item.error.is_none(), "{:?}", item.error);
    assert_eq!(item.steps.len(), 3);
    assert!(item.steps[0].rationale.contains("mid-grey"));
    assert_eq!(item.steps[1].title, "Clarity, Contrast");
    assert_eq!(item.review_status, "needs review");
    let group = item.group_id.unwrap();
    let progress = listener.0.lock().unwrap();
    assert_eq!(progress.first().unwrap().done, 0);
    assert_eq!(progress.last().unwrap().done, 2);
    drop(progress);

    let recipe: serde_json::Value =
        serde_json::from_str(&engine.get_recipe(a.clone()).unwrap()).unwrap();
    assert_eq!(recipe["settings"]["tone"]["contrast"], 10.0);
    assert_eq!(recipe["history"]["groups"][0]["name"], "Agent base edit");
    let provenance = engine.agent_provenance(a.clone()).unwrap().unwrap();
    assert_eq!(provenance.provenance, "AI-assisted, non-generative edits");
    assert_eq!(provenance.runs, 1);
    assert!(
        engine
            .agent_provenance(id(&names, "d_white.jpg"))
            .unwrap()
            .is_none()
    );
    // The index follows the agent's recipe (derived status: edited).
    let session = engine.open_cull_session(folder.clone()).unwrap();
    assert_eq!(
        session.derived_statuses(vec![a.clone()]).unwrap()[0].phase,
        StatusPhase::Edited
    );

    // NL redo: scoped to white balance, a new named group.
    let temperature = recipe["settings"]["white_balance"]["temperature"]
        .as_f64()
        .unwrap();
    let redo = engine
        .run_agent(
            request(vec![a.clone()], Some("warmer, keep the sky")),
            CancelFlag::new(),
            None,
        )
        .unwrap();
    let redone = &redo.items[0];
    assert!(redone.error.is_none(), "{:?}", redone.error);
    assert_ne!(redone.group_id, Some(group));
    assert_eq!(redone.steps.len(), 1);
    let recipe: serde_json::Value =
        serde_json::from_str(&engine.get_recipe(a.clone()).unwrap()).unwrap();
    assert_eq!(
        recipe["settings"]["white_balance"]["temperature"]
            .as_f64()
            .unwrap(),
        temperature + 400.
    );
    assert_eq!(
        recipe["settings"]["tone"]["contrast"], 10.0,
        "redo leaves other controls"
    );
    assert!(
        recipe["history"]["groups"][1]["name"]
            .as_str()
            .unwrap()
            .starts_with("Agent redo: warmer")
    );
    let unscoped = engine
        .run_agent(
            request(vec![a.clone()], Some("make it pop")),
            CancelFlag::new(),
            None,
        )
        .unwrap();
    assert!(
        unscoped.items[0]
            .error
            .as_deref()
            .unwrap()
            .contains("scoped")
    );

    // Revert the base edit group: its controls return, the redo stays.
    engine.revert_agent_edit(a.clone(), group).unwrap();
    let recipe: serde_json::Value =
        serde_json::from_str(&engine.get_recipe(a.clone()).unwrap()).unwrap();
    assert_eq!(recipe["settings"]["tone"]["contrast"], 0.0);
    assert_eq!(recipe["settings"]["color"]["vibrance"], 0.0);
    assert_eq!(
        recipe["settings"]["white_balance"]["temperature"]
            .as_f64()
            .unwrap(),
        temperature + 400.
    );
    assert!(engine.revert_agent_edit(a.clone(), 99).is_err());

    // Accept marks the review item and feeds the style profile.
    let accepted = engine.accept_agent_edit(c.clone(), folder.clone()).unwrap();
    assert!(accepted.feedback_recorded, "{:?}", accepted.note);
    assert_eq!(accepted.samples, 1);
    assert_eq!(
        engine
            .agent_provenance(c)
            .unwrap()
            .unwrap()
            .item
            .review_status,
        "accepted"
    );
    assert!(
        engine
            .accept_agent_edit(id(&names, "d_white.jpg"), folder.clone())
            .is_err()
    );

    // Cancellation before the first image edits nothing.
    let cancel = CancelFlag::new();
    cancel.cancel();
    let b = id(&names, "b_sharp.jpg");
    let report = engine
        .run_agent(request(vec![b.clone()], None), cancel, None)
        .unwrap();
    assert!(report.cancelled);
    assert!(report.items[0].group_id.is_none());
    assert!(engine.agent_provenance(b).unwrap().is_none());
}

#[test]
fn scripted_batch_run_holds_a_burst_to_one_look() {
    let (_dir, engine, folder, names) = shoot();
    let a = id(&names, "a_sharp.jpg");
    let b = id(&names, "b_sharp.jpg");
    let report = engine
        .run_agent(
            AgentRunRequest {
                images: [a.clone(), b.clone()]
                    .into_iter()
                    .map(|image_id| AgentImageInput {
                        image_id,
                        burst: Some("G1".into()),
                        people: vec![],
                    })
                    .collect(),
                library_folder: folder,
                provider: AgentProvider::Scripted,
                guardrails: guardrails(),
                instruction: None,
            },
            CancelFlag::new(),
            None,
        )
        .unwrap();
    assert!(
        report.items.iter().all(|i| i.error.is_none()),
        "{:?}",
        report.items
    );
    let exposure = |image: &str| -> f64 {
        let r: serde_json::Value =
            serde_json::from_str(&engine.get_recipe(image.into()).unwrap()).unwrap();
        r["settings"]["tone"]["exposure"].as_f64().unwrap()
    };
    assert_eq!(exposure(&a), exposure(&b), "one exposure for the burst");
    assert!(report.items[0].steps[0].rationale.contains("consensus"));
}
