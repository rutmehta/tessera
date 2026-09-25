use tessera_ffi::*;

/// Three JPEGs: a near-duplicate pair (same gradient, the second with extra
/// noise so it is larger and therefore the default best) and one opposite image.
fn photos() -> (tempfile::TempDir, std::sync::Arc<Engine>, String) {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let gradient = |flip: bool, noise: bool| {
        image::RgbImage::from_fn(96, 64, |x, y| {
            let v = if flip { 255 - x * 2 } else { x * 2 } as u8;
            let n = if noise {
                ((x * 7 + y * 13) % 5) as u8
            } else {
                0
            };
            image::Rgb([v.saturating_add(n), v, (y * 3) as u8])
        })
    };
    gradient(false, false).save(photos.join("a.jpg")).unwrap();
    gradient(false, true).save(photos.join("b.jpg")).unwrap();
    gradient(true, false).save(photos.join("c.jpg")).unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap()
        .path;
    (dir, engine, folder)
}

fn names(session: &CullSession, ids: &[String]) -> Vec<String> {
    let images = session.images().unwrap();
    ids.iter()
        .map(|id| {
            let image = images.iter().find(|i| &i.id == id).unwrap();
            std::path::Path::new(&image.path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        })
        .collect()
}

#[test]
fn groups_navigation_and_keep_best_are_one_undo_step() {
    let (_dir, engine, folder) = photos();
    let session = engine.open_cull_session(folder).unwrap();
    let images = session.images().unwrap();
    assert_eq!(images.len(), 3);
    let groups = session.groups().unwrap();
    assert_eq!(groups.len(), 2, "a/b are near-duplicates, c stands alone");
    // Queue order is the index's capture-time / id order, so locate by name.
    let pair = groups.iter().position(|g| g.images.len() == 2).unwrap();
    let mut pair_names = names(&session, &groups[pair].images);
    pair_names.sort();
    assert_eq!(pair_names, ["a.jpg", "b.jpg"]);
    assert_eq!(names(&session, &[groups[pair].best.clone()]), ["b.jpg"]);
    for image in &images {
        assert!(groups[image.group as usize].images.contains(&image.id));
    }
    assert_eq!(
        groups[0].images[0], images[0].id,
        "groups follow queue order"
    );
    assert!(!session.auto_advance().unwrap(), "host owns the cursor");

    let (first, second) = (&groups[pair].images[0], &groups[pair].images[1]);
    session.set_current(first.clone()).unwrap();
    assert_eq!(session.prev_in_group().unwrap().as_ref(), Some(first));
    assert_eq!(session.next_in_group().unwrap().as_ref(), Some(second));
    assert_eq!(session.next_in_group().unwrap().as_ref(), Some(second));
    session.set_current(images[0].id.clone()).unwrap();
    assert_eq!(
        session.next_group().unwrap(),
        Some(groups[1].images[0].clone())
    );
    assert_eq!(
        session.next_group().unwrap(),
        Some(groups[1].images[0].clone())
    );
    assert_eq!(session.current_group().unwrap(), Some(1));
    assert_eq!(
        session.prev_group().unwrap(),
        Some(groups[0].images[0].clone())
    );
    assert_eq!(
        session.prev_group().unwrap(),
        Some(groups[0].images[0].clone())
    );

    let result = session.keep_best_reject_rest(pair as u32).unwrap();
    assert_eq!(result.best, groups[pair].best);
    assert_eq!(result.rejected, 1);
    assert_eq!(result.update.changed.len(), 2);
    for change in &result.update.changed {
        let expected = if change.image_id == result.best {
            Decision::Keep
        } else {
            Decision::Reject
        };
        assert_eq!(change.selection.decision, expected);
    }
    // Persisted and visible to the engine's own catalog connection.
    let listed = engine.list_images(ImageQuery::default()).unwrap();
    let keep = listed.iter().find(|r| r.id == result.best).unwrap();
    assert_eq!(keep.selection.decision, Decision::Keep);

    let undone = session.undo().unwrap().expect("one step");
    assert!(
        undone
            .changed
            .iter()
            .all(|c| c.selection.decision == Decision::Undecided)
    );
    assert_eq!(undone.changed.len(), 2);
    assert!(!session.can_undo().unwrap());
    assert!(session.undo().unwrap().is_none());
    let redone = session.redo().unwrap().unwrap();
    assert_eq!(redone.changed.len(), 2);
    assert!(session.keep_best_reject_rest(9).is_err());

    // "Choose this" in compare: one undo step with a decision per image.
    let lone = groups[1 - pair].images[0].clone();
    let chosen = session
        .decide_each(vec![
            ImageDecision {
                image_id: lone.clone(),
                decision: Decision::Keep,
            },
            ImageDecision {
                image_id: result.best.clone(),
                decision: Decision::Reject,
            },
        ])
        .unwrap();
    assert_eq!(chosen.changed[0].selection.decision, Decision::Keep);
    assert_eq!(chosen.changed[1].selection.decision, Decision::Reject);
    assert_eq!(session.undo().unwrap().unwrap().changed.len(), 2);
    assert_eq!(
        session.selection(result.best.clone()).unwrap().decision,
        Decision::Keep
    );
    assert!(
        session
            .decide_each(vec![
                ImageDecision {
                    image_id: lone.clone(),
                    decision: Decision::Keep,
                },
                ImageDecision {
                    image_id: lone,
                    decision: Decision::Reject,
                },
            ])
            .is_err()
    );
}

#[test]
fn decisions_basket_status_sweep_and_safe_album_delete() {
    let (_dir, engine, folder) = photos();
    let session = engine.open_cull_session(folder.clone()).unwrap();
    let images = session.images().unwrap();
    let ids: Vec<_> = images.iter().map(|i| i.id.clone()).collect();

    let update = session.decide(Decision::Keep).unwrap();
    assert_eq!(update.changed[0].selection.decision, Decision::Keep);
    assert_eq!(update.current, Some(ids[0].clone()), "no implicit advance");
    session.set_current(ids[2].clone()).unwrap();
    assert_eq!(session.position().unwrap(), Some(2));
    assert_eq!(
        session.grade(2).unwrap().changed[0].selection,
        Selection {
            decision: Decision::Keep,
            grade: Some(2),
            mark: None
        }
    );
    session.mark("Review".into()).unwrap();
    assert_eq!(
        session.selection(ids[2].clone()).unwrap().mark.as_deref(),
        Some("Review")
    );
    assert!(session.grade(7).is_err());
    assert!(session.set_current("0".repeat(32)).is_err());

    assert!(session.toggle_basket().is_err(), "no target yet");
    session.set_basket_target("Selects".into()).unwrap();
    assert_eq!(session.basket_target().unwrap().as_deref(), Some("Selects"));
    let added = session.set_basket(ids.clone(), true).unwrap();
    assert!(added.albums_changed);
    assert!(added.changed.iter().all(|c| c.in_basket));
    assert!(session.images().unwrap().iter().all(|i| i.in_basket));
    let toggled = session.toggle_basket().unwrap();
    assert!(!toggled.changed[0].in_basket);
    let statuses = session.derived_statuses(ids.clone()).unwrap();
    assert_eq!(statuses[0].phase, StatusPhase::Unedited);
    assert_eq!(statuses[0].in_album, ["Selects"]);
    assert!(statuses[2].in_album.is_empty());

    // Safe delete inside an album: membership only, files stay on disk.
    let removed = session
        .remove_from_album("Selects".into(), vec![ids[0].clone()])
        .unwrap();
    assert!(!removed.changed[0].in_basket);
    for image in &images {
        assert!(std::path::Path::new(&image.path).exists());
    }
    let albums = session.albums().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].images, [ids[1].clone()]);
    let restored = session.undo().unwrap().unwrap();
    assert!(restored.changed[0].in_basket);
    assert!(
        session
            .library_path()
            .unwrap()
            .unwrap()
            .ends_with("library.json")
    );

    // Synthetic scores; the sweep is review-only until the host applies it.
    engine
        .set_score(ids[0].clone(), "focus".into(), 0.2, "seed".into())
        .unwrap();
    engine
        .set_score(ids[1].clone(), "closed_eyes".into(), 0.95, "seed".into())
        .unwrap();
    engine
        .set_score(ids[2].clone(), "focus".into(), 0.9, "seed".into())
        .unwrap();
    assert!(
        engine
            .set_score(ids[2].clone(), "focus".into(), f64::NAN, "seed".into())
            .is_err()
    );
    let can_undo = session.can_undo().unwrap();
    let found = session
        .defect_sweep(vec![
            DefectThreshold {
                signal: "focus".into(),
                value: 0.4,
                direction: ThresholdDirection::Below,
            },
            DefectThreshold {
                signal: "closed_eyes".into(),
                value: 0.8,
                direction: ThresholdDirection::Above,
            },
        ])
        .unwrap();
    assert_eq!(
        found.iter().map(|c| c.image_id.clone()).collect::<Vec<_>>(),
        [ids[0].clone(), ids[1].clone()]
    );
    assert_eq!(found[0].reasons[0].value, 0.2);
    assert_eq!(found[1].reasons[0].direction, ThresholdDirection::Above);
    assert_eq!(session.can_undo().unwrap(), can_undo);
    assert_eq!(
        session.selection(ids[1].clone()).unwrap().decision,
        Decision::Undecided,
        "sweep never decides"
    );
    let applied = session
        .decide_images(vec![ids[0].clone(), ids[1].clone()], Decision::Reject)
        .unwrap();
    assert_eq!(applied.changed.len(), 2);
    session.undo().unwrap().unwrap();
    assert_eq!(
        session.selection(ids[1].clone()).unwrap().decision,
        Decision::Undecided
    );
    assert_eq!(
        session.selection(ids[0].clone()).unwrap().decision,
        Decision::Keep
    );

    // A reopened session reconciles from sidecars and keeps the library.
    drop(session);
    let reopened = engine.open_cull_session(folder).unwrap();
    assert_eq!(
        reopened.images().unwrap()[2].selection.mark.as_deref(),
        Some("Review")
    );
    assert_eq!(reopened.albums().unwrap()[0].images.len(), 2);
    assert!(!reopened.can_undo().unwrap(), "history is per session");
}

#[test]
fn query_sessions_need_an_explicit_library() {
    let (dir, engine, _folder) = photos();
    let session = engine
        .open_cull_session_for_query(ImageQuery {
            limit: 2,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(session.images().unwrap().len(), 2);
    session.set_basket_target("Picks".into()).unwrap();
    assert!(session.toggle_basket().is_err());
    let library = dir.path().join("library.json");
    session
        .set_library(library.to_string_lossy().into_owned())
        .unwrap();
    assert!(session.toggle_basket().unwrap().changed[0].in_basket);
    assert!(library.exists());
    assert!(
        engine
            .open_cull_session("/definitely/missing".into())
            .is_err()
    );
}
