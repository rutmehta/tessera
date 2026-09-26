use engine_api::id::ImageId;
use index::{FaceRecord, Index, NoopMetadataProvider, NoopSidecarReader, Query, Score};

fn catalog() -> (tempfile::TempDir, Index, ImageId) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    (dir, index, id)
}

fn face(id: u32) -> FaceRecord {
    FaceRecord {
        id,
        bbox: [10.5, 20.25, 80.0, 100.0],
        landmarks5: [
            [30.0, 40.0],
            [60.0, 40.0],
            [45.0, 60.0],
            [30.0, 80.0],
            [60.0, 80.0],
        ],
        confidence: 0.95,
        embedding: Some(vec![0.125; 128]),
        sharpness: 0.75,
        eyes_open: Some(0.8),
    }
}

#[test]
fn replacing_faces_rebuilds_only_face_scores_and_minima() {
    let (_dir, index, id) = catalog();
    let unrelated = Score {
        signal: "focus".into(),
        value: 0.9,
        model: "other-v1".into(),
    };
    index.set_score(id, &unrelated).unwrap();
    let a = face(0);
    let mut b = face(1);
    b.sharpness = 0.2;
    b.eyes_open = Some(0.3);
    index.replace_faces(id, &[a.clone(), b]).unwrap();
    let scores = index.scores(id).unwrap();
    for (signal, value) in [
        ("face_sharpness", 0.2),
        ("eyes_open", 0.3),
        ("face/1/sharpness", 0.2),
        ("face/1/eyes_open", 0.3),
    ] {
        assert!(scores.contains(&Score {
            signal: signal.into(),
            value,
            model: "yunet-sface-v1".into()
        }));
    }
    let mut a = a;
    a.eyes_open = None;
    index.replace_faces(id, &[a.clone()]).unwrap();
    assert_eq!(index.faces(id).unwrap(), vec![a]);
    let scores = index.scores(id).unwrap();
    assert_eq!(scores.len(), 3);
    assert!(
        !scores
            .iter()
            .any(|s| s.signal.contains("eyes_open") || s.signal.starts_with("face/1/"))
    );
    index.replace_faces(id, &[]).unwrap();
    assert!(index.faces(id).unwrap().is_empty());
    assert_eq!(index.scores(id).unwrap(), vec![unrelated]);
}

#[test]
fn invalid_face_replacement_preserves_previous_records_and_scores() {
    let (_dir, index, id) = catalog();
    let original = face(0);
    index
        .replace_faces(id, std::slice::from_ref(&original))
        .unwrap();
    let scores = index.scores(id).unwrap();
    let mut invalid = Vec::new();
    for value in [f32::NAN, f32::INFINITY, -1.0] {
        let mut f = face(1);
        f.bbox[0] = value;
        invalid.push(f);
        let mut f = face(1);
        f.landmarks5[0][0] = value;
        invalid.push(f);
    }
    for value in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        let mut f = face(1);
        f.bbox[2] = value;
        invalid.push(f);
        let mut f = face(1);
        f.bbox[3] = value;
        invalid.push(f);
    }
    let mut f = face(1);
    f.bbox = [f32::MAX, 1.0, f32::MAX, 1.0];
    invalid.push(f);
    for value in [-0.1, 1.1, f64::NAN, f64::INFINITY] {
        let mut f = face(1);
        f.confidence = value as f32;
        invalid.push(f);
        let mut f = face(1);
        f.sharpness = value;
        invalid.push(f);
        let mut f = face(1);
        f.eyes_open = Some(value);
        invalid.push(f);
    }
    for embedding in [
        vec![],
        vec![0.0; 127],
        vec![0.0; 129],
        vec![f32::NAN; 128],
        vec![f32::INFINITY; 128],
    ] {
        let mut f = face(1);
        f.embedding = Some(embedding);
        invalid.push(f);
    }
    for f in invalid {
        assert!(
            index.replace_faces(id, &[face(2), f.clone()]).is_err(),
            "accepted {f:?}"
        );
        assert_eq!(index.faces(id).unwrap(), vec![original.clone()]);
        assert_eq!(index.scores(id).unwrap(), scores);
    }
    assert!(index.replace_faces(id, &[face(1), face(1)]).is_err());
    assert_eq!(index.faces(id).unwrap(), vec![original]);
    assert_eq!(index.scores(id).unwrap(), scores);
}

#[test]
fn replacing_faces_requires_an_existing_image_even_for_empty_input() {
    let index = Index::open(":memory:").unwrap();
    assert!(index.replace_faces(ImageId(999), &[]).is_err());
    assert!(index.replace_faces(ImageId(999), &[face(0)]).is_err());
}

#[test]
fn database_write_failure_rolls_back_faces_and_scores() {
    let (dir, index, id) = catalog();
    index.replace_faces(id, &[face(0)]).unwrap();
    let before = index.scores(id).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
    conn.execute_batch("CREATE TRIGGER fail_face_score BEFORE INSERT ON score WHEN new.signal='face/2/sharpness' BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
    assert!(index.replace_faces(id, &[face(1), face(2)]).is_err());
    assert_eq!(index.faces(id).unwrap(), vec![face(0)]);
    assert_eq!(index.scores(id).unwrap(), before);
}

#[test]
fn upgrade_v4_preserves_scores_and_face_rows_cascade_on_image_delete() {
    let (dir, index, id) = catalog();
    let old = Score {
        signal: "focus".into(),
        value: 0.25,
        model: "original".into(),
    };
    index.set_score(id, &old).unwrap();
    let selection = index.selection(id).unwrap();
    drop(index);
    let db = dir.path().join("index.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE face; DROP TABLE accepted_keyword; DROP TABLE understanding; DROP TRIGGER image_fts_delete; DELETE FROM migration WHERE version>=5;")
        .unwrap();
    drop(conn);
    let index = Index::open(&db).unwrap();
    assert_eq!(index.scores(id).unwrap(), vec![old]);
    assert_eq!(index.selection(id).unwrap(), selection);
    assert!(index.faces(id).unwrap().is_empty());
    index.replace_faces(id, &[face(0)]).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("SELECT model FROM face", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "yunet-sface-v1"
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM migration WHERE version=5", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
    conn.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    // Legacy selection rows are not cascading; remove those before deleting the image.
    conn.execute("DELETE FROM selection WHERE image_id=?", [id.to_string()])
        .unwrap();
    conn.execute("DELETE FROM image WHERE id=?", [id.to_string()])
        .unwrap();
    assert!(index.faces(id).unwrap().is_empty());
    assert!(index.scores(id).unwrap().is_empty());
    assert_eq!(
        conn.query_row("SELECT count(*) FROM face", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn face_ordinals_are_image_local() {
    let (dir, mut index, id) = catalog();
    std::fs::write(dir.path().join("b.jpg"), b"jpeg").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let other = index
        .search(&Query::default())
        .unwrap()
        .into_iter()
        .find(|i| *i != id)
        .unwrap();
    index.replace_faces(id, &[face(0)]).unwrap();
    index.replace_faces(other, &[face(0)]).unwrap();
    index.replace_faces(id, &[]).unwrap();
    assert_eq!(index.faces(other).unwrap(), vec![face(0)]);
    assert_eq!(index.scores(other).unwrap().len(), 4);
}

#[test]
fn face_records_roundtrip_after_reopen() {
    let (dir, index, id) = catalog();
    let a = face(0);
    let mut b = face(2);
    b.embedding = None;
    b.eyes_open = None;
    index.replace_faces(id, &[b.clone(), a.clone()]).unwrap();
    drop(index);
    let index = Index::open(dir.path().join("index.sqlite")).unwrap();
    assert_eq!(index.faces(id).unwrap(), vec![a, b]);
    assert!(index.scores(id).unwrap().contains(&Score {
        signal: "face/0/sharpness".into(),
        value: 0.75,
        model: "yunet-sface-v1".into(),
    }));
}
