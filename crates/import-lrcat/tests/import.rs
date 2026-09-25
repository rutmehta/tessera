mod make_fixture;
use engine_api::recipe::{Decision, Grade, ProcessVersion};
use import_lrcat::{SavedSearch, import, inspect};

#[test]
fn synthetic_catalog_plan() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lrcat");
    let _db = make_fixture::make_fixture(&path);
    let before = std::fs::read(&path).unwrap();
    let plan = import(&path).unwrap();
    assert_eq!(plan.schema_version, "1300000");
    assert_eq!(plan.images.len(), 3);
    let a = &plan.images[0];
    assert_eq!(a.path.to_str(), Some("/photos/a/one.CR3"));
    assert_eq!(a.selection.decision, Decision::Keep);
    assert_eq!(a.selection.grade, Some(Grade::Three));
    assert_eq!(a.selection.mark.as_ref().unwrap().0, "Client");
    assert_eq!(a.recipe.process_version, ProcessVersion::adobe(6));
    assert_eq!(a.recipe.settings.tone.exposure, 0.5);
    assert_eq!(a.recipe.settings.tone.contrast, 10.0);
    assert_eq!(a.recipe.settings.tone.curves.rgb.0.len(), 3);
    assert_eq!(a.recipe.settings.locals.adjustments.len(), 1);
    assert_eq!(a.recipe.settings.locals.adjustments[0].name, "Sky");
    assert_eq!(a.keywords, vec![2]);
    assert_eq!(a.collections, vec![2]);
    assert_eq!(a.gps.unwrap(), [40.7, -74.0]);
    assert_eq!(a.faces.len(), 1);
    assert_eq!(a.history.len(), 1);
    assert_eq!(a.snapshots.len(), 1);
    assert_eq!(plan.images[1].selection.decision, Decision::Reject);
    assert_eq!(plan.images[1].selection.grade, None);
    assert_eq!(plan.images[1].path.to_str(), Some("/photos/b/two.DNG"));
    let copy = &plan.images[2];
    assert_eq!(copy.master_image, Some(30));
    assert_eq!(copy.display_name, "one.CR3 (Black & White)");
    assert_eq!(copy.path, a.path);
    assert_ne!(copy.recipe.image_id, a.recipe.image_id);
    assert_eq!(copy.selection.grade, Some(Grade::One));
    for image in &plan.images {
        image.recipe.validate().unwrap();
    }
    assert_eq!(plan.library.keywords[0].children[0].name, "NYC");
    assert_eq!(
        plan.library.keywords[0].children[0].synonyms,
        vec!["New York"]
    );
    assert_eq!(plan.library.album_groups.len(), 1);
    assert_eq!(
        plan.library.albums["Selects"].images,
        vec![
            plan.images
                .iter()
                .find(|i| i.catalog_id == 30)
                .unwrap()
                .recipe
                .image_id
                .unwrap(),
            plan.images
                .iter()
                .find(|i| i.catalog_id == 32)
                .unwrap()
                .recipe
                .image_id
                .unwrap(),
        ]
    );
    assert_eq!(
        plan.library.smart_albums[0].search,
        SavedSearch::All(vec![SavedSearch::Rule {
            criteria: "rating".into(),
            operation: ">=".into(),
            value: 3.into()
        }])
    );
    assert_eq!(plan.library.people, vec!["Alice"]);
    assert_eq!(plan.stacks.len(), 1);
    assert!(plan.report.iter().any(|s| s.contains("FutureKnob")));
    let out = dir.path().join("library.json");
    plan.library.write(&out).unwrap();
    let back: import_lrcat::Library = serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap();
    assert_eq!(back, plan.library);
    assert_eq!(inspect(&path).unwrap().images, 3);
    assert_eq!(std::fs::read(path).unwrap(), before);
}

#[test]
fn extra_columns_and_empty_folders_are_supported() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lrcat");
    let db = make_fixture::make_fixture(&path);
    db.execute_batch("ALTER TABLE Adobe_images ADD COLUMN future BLOB; INSERT INTO AgLibraryFolder VALUES(12,1,'empty/');").unwrap();
    assert_eq!(import(&path).unwrap().images.len(), 3);
    assert_eq!(inspect(&path).unwrap().folders, 3);
}

#[test]
fn missing_table_is_a_named_engine_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lrcat");
    let db = make_fixture::make_fixture(&path);
    db.execute_batch("DROP TABLE AgLibraryFile").unwrap();
    let error = import(path).unwrap_err();
    assert!(matches!(error, engine_api::EngineError::Decode { .. }));
    assert!(error.to_string().contains("missing table AgLibraryFile"));
}

#[test]
fn committed_wal_and_lock_are_copied_without_touching_source() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("spaces : ? # ü.lrcat");
    let db = make_fixture::make_fixture(&path);
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE Adobe_images SET rating=4 WHERE id_local=30;").unwrap();
    let wal = path.with_file_name(format!(
        "{}-wal",
        path.file_name().unwrap().to_str().unwrap()
    ));
    let lock = path.with_file_name(format!(
        "{}-lock",
        path.file_name().unwrap().to_str().unwrap()
    ));
    std::fs::write(&lock, b"Lightroom").unwrap();
    let before = std::fs::read(&path).unwrap();
    let wal_before = std::fs::read(&wal).unwrap();
    let plan = import(&path).unwrap();
    assert_eq!(plan.images[0].selection.grade, Some(Grade::Two));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(std::fs::read(&wal).unwrap(), wal_before);
    assert_eq!(std::fs::read(&lock).unwrap(), b"Lightroom");
}

#[test]
fn all_selection_mappings_and_reject_precedence() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.lrcat");
    let db = make_fixture::make_fixture(&path);
    for (pick, rating, decision, grade) in [
        (0, 0, Decision::Undecided, None),
        (1, 0, Decision::Keep, None),
        (0, 1, Decision::Keep, None),
        (0, 2, Decision::Keep, Some(Grade::One)),
        (0, 3, Decision::Keep, Some(Grade::Two)),
        (0, 4, Decision::Keep, Some(Grade::Two)),
        (0, 5, Decision::Keep, Some(Grade::Three)),
        (-1, 5, Decision::Reject, None),
        (1, -1, Decision::Reject, None),
    ] {
        db.execute(
            "UPDATE Adobe_images SET pick=?1,rating=?2 WHERE id_local=30",
            [pick, rating],
        )
        .unwrap();
        let s = &import(&path).unwrap().images[0].selection;
        assert_eq!((s.decision, s.grade), (decision, grade));
    }
}
