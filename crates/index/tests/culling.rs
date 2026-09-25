use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query, Score};

#[test]
fn culling_records_scores_and_exports_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    let info = index.image_info(id).unwrap();
    assert_eq!(info.size, 4);
    assert_eq!(info.path.file_name().unwrap(), "a.jpg");
    assert!(index.scores(id).unwrap().is_empty());
    index
        .set_score(
            id,
            &Score {
                signal: "focus".into(),
                value: 0.2,
                model: "synthetic-v1".into(),
            },
        )
        .unwrap();
    assert!(
        index
            .set_score(
                id,
                &Score {
                    signal: "focus".into(),
                    value: f64::NAN,
                    model: "bad".into()
                }
            )
            .is_err()
    );
    assert_eq!(index.export_status(id).unwrap(), (false, false));
    index.record_export(id, "output.jpg", false).unwrap();
    assert_eq!(index.export_status(id).unwrap(), (true, false));
    index.record_export(id, "gallery", true).unwrap();
    drop(index);
    let index = Index::open(db).unwrap();
    assert_eq!(index.export_status(id).unwrap(), (true, true));
    assert_eq!(index.scores(id).unwrap()[0].value, 0.2);
}

#[test]
fn upgrade_v3_preserves_images_and_selection() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
    let db = dir.path().join("index.sqlite");
    let mut index = Index::open(&db).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    let selection = engine_api::recipe::Selection::keep(Some(engine_api::recipe::Grade::Three));
    index.set_selection(id, &selection).unwrap();
    drop(index);
    let conn = rusqlite::Connection::open(&db).unwrap();
    // Recreate the previous schema, then exercise the actual v3 -> v4 migration.
    conn.execute_batch(
        "DROP TABLE face; DROP TABLE export_log; DROP TABLE score; DELETE FROM migration WHERE version>=4;",
    )
    .unwrap();
    drop(conn);
    let index = Index::open(&db).unwrap();
    assert_eq!(index.selection(id).unwrap(), Some(selection));
    assert!(index.scores(id).unwrap().is_empty());
    assert_eq!(index.export_status(id).unwrap(), (false, false));
    drop(index);
    Index::open(&db).unwrap();
    let conn = rusqlite::Connection::open(&db).unwrap();
    assert_eq!(
        conn.query_row("SELECT count(*) FROM migration WHERE version=4", [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap(),
        1
    );
}
