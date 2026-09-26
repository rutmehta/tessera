use engine_api::id::ImageId;
use index::{FaceKey, FaceRecord, Index, NoopMetadataProvider, NoopSidecarReader, Query};

fn catalog() -> (tempfile::TempDir, Index, Vec<ImageId>) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(dir.path().join(name), b"jpeg").unwrap();
    }
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    for id in &ids {
        index.replace_faces(*id, &[face(0), face(1)]).unwrap();
    }
    (dir, index, ids)
}
fn face(id: u32) -> FaceRecord {
    FaceRecord {
        id,
        bbox: [0., 0., 1., 1.],
        landmarks5: [[0., 0.]; 5],
        confidence: 0.9,
        embedding: None,
        sharpness: 0.5,
        eyes_open: None,
    }
}

#[test]
fn assignments_join_names_confirm_and_search_unique_images() {
    let (dir, index, ids) = catalog();
    index.create_person("a", None, None).unwrap();
    index.create_person("b", Some("Bob"), None).unwrap();
    let f = FaceKey {
        image_id: ids[0],
        ordinal: 0,
    };
    index.assign_face(f, "a").unwrap();
    index.assign_face(FaceKey { ordinal: 1, ..f }, "a").unwrap();
    index
        .assign_face(
            FaceKey {
                image_id: ids[1],
                ..f
            },
            "a",
        )
        .unwrap();
    index.confirm_face(f, true).unwrap();
    index.assign_face(f, "a").unwrap(); // Idempotence retains confirmation.
    index.name_person("a", Some("Ada")).unwrap();
    let rows = index.face_assignments(ids[0]).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].person_name.as_deref(), Some("Ada"));
    assert!(rows[0].confirmed);
    assert!(!rows[1].confirmed);
    assert_eq!(index.images_with_person("a", false, 100, 0).unwrap(), ids);
    assert_eq!(
        index.images_with_person("a", true, 100, 0).unwrap(),
        vec![ids[0]]
    );
    assert_eq!(
        index.images_with_person("a", false, 1, 1).unwrap(),
        vec![ids[1]]
    );
    assert!(index.assign_face(f, "missing").is_err());
    assert!(
        index
            .assign_face(FaceKey { ordinal: 99, ..f }, "a")
            .is_err()
    );
    assert!(
        index
            .confirm_face(FaceKey { ordinal: 99, ..f }, true)
            .is_err()
    );
    index.assign_face(f, "b").unwrap();
    assert!(!index.face_assignments(ids[0]).unwrap()[0].confirmed);
    drop(index);
    let index = Index::open(dir.path().join("index.sqlite")).unwrap();
    assert_eq!(index.face_assignments(ids[0]).unwrap()[0].person_id, "b");
    index.replace_faces(ids[0], &[]).unwrap();
    assert!(index.face_assignments(ids[0]).unwrap().is_empty());
    assert!(
        index
            .images_with_person("b", false, 100, 0)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn scoped_people_inverse_restores_source_id_and_medoids_without_erasing_unrelated_edits() {
    let (_dir, index, ids) = catalog();
    let a = FaceKey {
        image_id: ids[0],
        ordinal: 0,
    };
    let b = FaceKey {
        image_id: ids[1],
        ordinal: 0,
    };
    index.create_person("a", Some("Ada"), None).unwrap();
    index.create_person("b", Some("Bob"), None).unwrap();
    index.create_person("unrelated", None, None).unwrap();
    index.assign_face(a, "a").unwrap();
    index.assign_face(b, "b").unwrap();
    index.confirm_face(b, true).unwrap();
    index
        .apply_people_plan(
            &[
                index::Person {
                    id: "a".into(),
                    name: None,
                    medoid: Some(vec![0.1; 128]),
                },
                index::Person {
                    id: "b".into(),
                    name: None,
                    medoid: Some(vec![0.2; 128]),
                },
            ],
            &[],
        )
        .unwrap();
    let before = index
        .snapshot_people_edit(&["a".into(), "b".into()], &[])
        .unwrap();
    let people = index.people().unwrap();
    index.merge_people("a", "b").unwrap();
    let after = index.resnapshot_people_edit(&before).unwrap();
    index.name_person("unrelated", Some("Eve")).unwrap();
    index.restore_people_edit(&after, &before).unwrap();
    assert_eq!(&index.people().unwrap()[..2], &people[..2]);
    assert_eq!(index.people().unwrap()[2].name.as_deref(), Some("Eve"));
    assert_eq!(index.person_members("b").unwrap()[0].face, b);
    assert!(index.person_members("b").unwrap()[0].confirmed);
    index.restore_people_edit(&before, &after).unwrap();
    assert!(index.person_members("b").unwrap().is_empty());
    assert_eq!(index.person_members("a").unwrap().len(), 2);
    // Representative-only repairs do not make an inverse stale.
    index
        .apply_people_plan(
            &[index::Person {
                id: "a".into(),
                name: None,
                medoid: Some(vec![0.3; 128]),
            }],
            &[],
        )
        .unwrap();
    index.restore_people_edit(&after, &before).unwrap();
    assert_eq!(&index.people().unwrap()[..2], &people[..2]);
    // Even a re-detected ordinal reassigned to the same person is not the old face.
    let mut replacement = face(0);
    replacement.bbox[0] = 4.;
    index.replace_faces(ids[1], &[replacement]).unwrap();
    index.assign_face(b, "b").unwrap();
    index.confirm_face(b, true).unwrap();
    let changed = index.people().unwrap();
    assert!(index.restore_people_edit(&before, &after).is_err());
    assert_eq!(index.people().unwrap(), changed);
}

#[test]
fn merge_split_are_atomic_and_preserve_identity_rules() {
    let (dir, index, ids) = catalog();
    index
        .create_person("a", Some("Ada"), Some(&[0.1; 128]))
        .unwrap();
    index.create_person("b", Some("Bob"), None).unwrap();
    let f = FaceKey {
        image_id: ids[0],
        ordinal: 0,
    };
    let g = FaceKey {
        image_id: ids[1],
        ordinal: 0,
    };
    index.assign_face(f, "a").unwrap();
    index.assign_face(g, "b").unwrap();
    index.confirm_face(g, true).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_merge BEFORE DELETE ON person BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(index.merge_people("a", "b").is_err());
    assert_eq!(index.face_assignments(ids[1]).unwrap()[0].person_id, "b");
    assert_eq!(index.people().unwrap().len(), 2);
    conn.execute_batch("DROP TRIGGER fail_merge;").unwrap();
    assert!(index.merge_people("a", "a").is_err());
    assert!(index.merge_people("missing", "a").is_err());
    index.merge_people("a", "b").unwrap();
    assert_eq!(index.people().unwrap().len(), 1);
    let row = &index.face_assignments(ids[1]).unwrap()[0];
    assert_eq!(row.person_name.as_deref(), Some("Ada"));
    assert!(row.confirmed);
    assert!(
        index
            .split_person("a", "c", &[g, FaceKey { ordinal: 99, ..f }])
            .is_err()
    );
    assert!(index.split_person("a", "c", &[g, g]).is_err());
    assert!(index.split_person("a", "c", &[]).is_err());
    assert!(index.split_person("a", "a", &[g]).is_err());
    assert_eq!(index.people().unwrap().len(), 1);
    assert_eq!(index.face_assignments(ids[1]).unwrap()[0].person_id, "a");
    index.split_person("a", "c", &[g]).unwrap();
    let row = &index.face_assignments(ids[1]).unwrap()[0];
    assert_eq!(row.person_id, "c");
    assert_eq!(row.person_name, None);
    assert!(!row.confirmed);
    assert!(index.people().unwrap().iter().all(|p| p.medoid.is_none()));
    assert_eq!(
        index.images_with_person("a", false, 100, 0).unwrap(),
        vec![ids[0]]
    );
    assert_eq!(
        index.images_with_person("c", false, 100, 0).unwrap(),
        vec![ids[1]]
    );
}

#[test]
fn manual_reassignment_invalidates_both_medoids_atomically() {
    let (_dir, index, ids) = catalog();
    for id in ["a", "b", "unrelated"] {
        index.create_person(id, None, Some(&[0.1; 128])).unwrap();
    }
    let face = FaceKey {
        image_id: ids[0],
        ordinal: 0,
    };
    index.assign_face(face, "a").unwrap();
    // Recreate the representative as an automatic job would.
    index
        .apply_people_plan(
            &[index::Person {
                id: "a".into(),
                name: None,
                medoid: Some(vec![0.1; 128]),
            }],
            &[],
        )
        .unwrap();
    let before = index.people().unwrap();
    assert!(index.assign_face(face, "missing").is_err());
    assert_eq!(index.people().unwrap(), before);
    index.assign_face(face, "b").unwrap();
    for person in index.people().unwrap() {
        assert_eq!(person.medoid.is_some(), person.id == "unrelated");
    }
}

#[test]
fn deleted_faces_invalidate_medoids_for_redetection_and_missing_images() {
    for forget in [false, true] {
        let (_dir, mut index, ids) = catalog();
        let person = index::Person {
            id: "a".into(),
            name: None,
            medoid: Some(vec![0.1; 128]),
        };
        index
            .apply_people_plan(
                &[person],
                &[(
                    FaceKey {
                        image_id: ids[0],
                        ordinal: 0,
                    },
                    "a".into(),
                )],
            )
            .unwrap();
        assert!(index.people().unwrap()[0].medoid.is_some());
        if forget {
            std::fs::remove_file(index.image_info(ids[0]).unwrap().path).unwrap();
            index.forget_missing(&[ids[0]]).unwrap();
        } else {
            index.replace_faces(ids[0], &[face(0)]).unwrap();
        }
        assert!(index.people().unwrap()[0].medoid.is_none());
    }
}

#[test]
fn person_predicate_resolves_current_names_without_keyword_copies() {
    let (_dir, index, ids) = catalog();
    index.create_person("a", Some("Ada"), None).unwrap();
    index
        .assign_face(
            FaceKey {
                image_id: ids[0],
                ordinal: 0,
            },
            "a",
        )
        .unwrap();
    let query = |name: &str| Query {
        predicate: Some(index::Predicate::Person(name.into())),
        ..Default::default()
    };
    assert_eq!(index.search(&query("Ada")).unwrap(), vec![ids[0]]);
    assert_eq!(index.facets(&query("Ada")).unwrap().total, 1);
    index.name_person("a", Some("Grace")).unwrap();
    assert!(index.search(&query("Ada")).unwrap().is_empty());
    assert_eq!(index.search(&query("Grace")).unwrap(), vec![ids[0]]);
}

#[test]
#[ignore = "synthetic 100k-image person search benchmark"]
fn person_search_100k_under_50ms() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.sqlite");
    let index = Index::open(&db).unwrap();
    index.create_person("benchmark", Some("Ada"), None).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; BEGIN;
        INSERT INTO root(path) VALUES('/synthetic');
        INSERT INTO folder(root_id,path) VALUES(1,'/synthetic');
        WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<100000)
        INSERT INTO file(id,folder_id,path,name,size,mtime) SELECT x,1,'/synthetic/'||x,'image',1,1 FROM n;
        INSERT INTO image(id,file_id) SELECT printf('%032x',id),id FROM file;
        INSERT INTO face(image_id,id,bbox,landmarks5,confidence,model) SELECT id,0,'[0,0,1,1]','[]',1,'synthetic' FROM image;
        INSERT INTO face_person SELECT id,0,'benchmark',1 FROM image;
        COMMIT;").unwrap();
    let count: i64 = conn
        .query_row("SELECT count(*) FROM face_person", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 100000);
    for confirmed in [false, true] {
        let start = std::time::Instant::now();
        let hits = index
            .images_with_person("benchmark", confirmed, 100_000, 0)
            .unwrap();
        let elapsed = start.elapsed();
        assert_eq!(hits.len(), 100_000);
        println!("100k person search confirmed={confirmed}: {elapsed:?}");
        assert!(elapsed < std::time::Duration::from_millis(50));
    }
    let plan: String = conn.query_row("EXPLAIN QUERY PLAN SELECT DISTINCT image_id FROM face_person WHERE person_id='benchmark' ORDER BY image_id LIMIT 100",[],|r|r.get(3)).unwrap();
    assert!(
        plan.contains("COVERING INDEX face_person_search_idx"),
        "{plan}"
    );
}

#[test]
fn failed_face_replacement_restores_assignments_and_foreign_keys_hold() {
    let (dir, index, ids) = catalog();
    index.create_person("a", None, None).unwrap();
    let f = FaceKey {
        image_id: ids[0],
        ordinal: 0,
    };
    index.assign_face(f, "a").unwrap();
    index.confirm_face(f, true).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; CREATE TRIGGER fail_face BEFORE INSERT ON face BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(index.replace_faces(ids[0], &[face(2)]).is_err());
    let assignments = index.face_assignments(ids[0]).unwrap();
    assert_eq!(assignments.len(), 1);
    assert_eq!(assignments[0].face, f);
    assert!(assignments[0].confirmed);
    assert!(
        conn.execute("UPDATE face_person SET confirmed=2", [])
            .is_err()
    );
    assert!(
        conn.execute("UPDATE face_person SET ordinal=999", [])
            .is_err()
    );
    assert!(
        conn.execute("UPDATE face_person SET person_id='missing'", [])
            .is_err()
    );
    let violations: i64 = conn
        .query_row("SELECT count(*) FROM pragma_foreign_key_check", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(violations, 0);
    conn.execute_batch("DROP TRIGGER fail_face; CREATE TRIGGER fail_split BEFORE UPDATE ON face_person BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(index.split_person("a", "new", &[f]).is_err());
    assert_eq!(index.people().unwrap().len(), 1);
    assert_eq!(index.face_assignments(ids[0]).unwrap(), assignments);
    conn.execute_batch("DROP TRIGGER fail_split;").unwrap();
    std::fs::remove_file(index.image_info(ids[0]).unwrap().path).unwrap();
    let mut index = index;
    index.forget_missing(&[ids[0]]).unwrap();
    assert!(index.face_assignments(ids[0]).unwrap().is_empty());
    assert_eq!(index.people().unwrap().len(), 1);
}

#[test]
fn people_persist_and_names_are_mutable() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("index.sqlite");
    let index = Index::open(&db).unwrap();
    index
        .create_person("stable-person", None, Some(&[0.25; 128]))
        .unwrap();
    assert!(index.create_person("stable-person", None, None).is_err());
    assert!(index.create_person("", None, None).is_err());
    assert!(
        index
            .create_person("invalid", None, Some(&[f32::NAN; 128]))
            .is_err()
    );
    index.name_person("stable-person", Some("Ada")).unwrap();
    assert!(index.name_person("missing", Some("Nobody")).is_err());
    drop(index);
    let index = Index::open(&db).unwrap();
    let people = index.people().unwrap();
    assert_eq!(people.len(), 1);
    assert_eq!(people[0].id, "stable-person");
    assert_eq!(people[0].name.as_deref(), Some("Ada"));
    assert_eq!(people[0].medoid, Some(vec![0.25; 128]));
    index.name_person("stable-person", None).unwrap();
    assert_eq!(index.people().unwrap()[0].name, None);
}
