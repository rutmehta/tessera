use engine_api::id::ImageId;
use index::{
    Index, KeywordSuggestion, NoopMetadataProvider, NoopSidecarReader, OcrRegion, Query,
    Understanding,
};

fn sample() -> Understanding {
    Understanding {
        model_version: "captioner-v1+ocr-v2".into(),
        keywords: vec![KeywordSuggestion {
            keyword: "suggestion".into(),
            confidence: 0.8,
        }],
        caption: "A lighthouse".into(),
        alt_text: "White tower on a rocky coast".into(),
        ocr: vec![OcrRegion {
            text: "WELCOME".into(),
            bbox: [0.1, 0.2, 0.8, 0.9],
            confidence: 0.95,
        }],
    }
}

fn fixture() -> (tempfile::TempDir, Index, ImageId) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("photo.cr3"), b"raw").unwrap();
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let id = index.search(&Query::default()).unwrap()[0];
    (dir, index, id)
}

fn text(index: &Index, term: &str) -> Vec<ImageId> {
    index
        .search(&Query {
            text: Some(term.into()),
            ..Default::default()
        })
        .unwrap()
}

#[test]
fn caption_and_ocr_search_survive_rescan_and_replacement() {
    let (dir, mut index, id) = fixture();
    let value = sample();
    index.set_understanding(id, &value).unwrap();
    assert_eq!(text(&index, "lighthouse"), vec![id]);
    assert_eq!(text(&index, "WELCOME"), vec![id]);
    index.add_keyword("accepted", None).unwrap();
    index.tag(id, "accepted").unwrap();
    assert_eq!(text(&index, "accepted"), vec![id]);
    assert_eq!(text(&index, "WELCOME"), vec![id]);
    std::fs::write(dir.path().join("photo.cr3"), b"modified raw").unwrap();
    assert_eq!(
        index
            .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap(),
        1
    );
    assert_eq!(index.understanding(id).unwrap(), Some(value));
    assert_eq!(text(&index, "lighthouse"), vec![id]);
    assert_eq!(text(&index, "WELCOME"), vec![id]);
    let replacement = Understanding {
        model_version: "v2".into(),
        keywords: vec![],
        caption: "A forest".into(),
        alt_text: String::new(),
        ocr: vec![],
    };
    index.set_understanding(id, &replacement).unwrap();
    assert_eq!(index.understanding(id).unwrap(), Some(replacement));
    assert!(text(&index, "lighthouse").is_empty());
    assert!(text(&index, "WELCOME").is_empty());
    assert_eq!(text(&index, "forest"), vec![id]);
    assert_eq!(text(&index, "photo"), vec![id]);
}

#[test]
fn deleting_image_cascades_understanding_and_fts() {
    let (dir, index, id) = fixture();
    index.set_understanding(id, &sample()).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
    conn.execute_batch("PRAGMA foreign_keys=ON; DELETE FROM selection;")
        .unwrap();
    conn.execute("DELETE FROM image WHERE id=?", [id.to_string()])
        .unwrap();
    assert_eq!(index.understanding(id).unwrap(), None);
    let count: i64 = conn
        .query_row(
            "SELECT count(*) FROM fts WHERE fts MATCH 'WELCOME'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn pruning_missing_images_cleans_model_output_but_dry_run_preserves_it() {
    let (dir, mut index, id) = fixture();
    index.set_understanding(id, &sample()).unwrap();
    std::fs::remove_file(dir.path().join("photo.cr3")).unwrap();
    assert_eq!(index.prune_missing(true).unwrap().images, 1);
    assert_eq!(index.understanding(id).unwrap(), Some(sample()));
    assert_eq!(text(&index, "WELCOME"), vec![id]);
    assert_eq!(index.prune_missing(false).unwrap().images, 1);
    assert_eq!(index.understanding(id).unwrap(), None);
    assert!(text(&index, "WELCOME").is_empty());
    std::fs::write(dir.path().join("photo.cr3"), b"new raw").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    assert_eq!(index.understanding(id).unwrap(), None);
    assert!(text(&index, "WELCOME").is_empty());
}

#[test]
fn sql_failure_rolls_back_model_output_and_search_together() {
    let (dir, index, id) = fixture();
    index.set_understanding(id, &sample()).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("index.sqlite")).unwrap();
    conn.execute_batch("ALTER TABLE fts RENAME TO saved_fts;")
        .unwrap();
    let mut replacement = sample();
    replacement.caption = "replacement".into();
    replacement.model_version = "v2".into();
    assert!(index.set_understanding(id, &replacement).is_err());
    assert_eq!(index.understanding(id).unwrap(), Some(sample()));
    conn.execute_batch("ALTER TABLE saved_fts RENAME TO fts;")
        .unwrap();
    assert_eq!(text(&index, "lighthouse"), vec![id]);
    assert!(text(&index, "replacement").is_empty());
}

#[test]
fn v5_upgrade_preserves_accepted_metadata_and_sidecar_rescans() {
    struct Sidecar;
    impl index::SidecarReader for Sidecar {
        fn read(
            &self,
            path: &std::path::Path,
        ) -> engine_api::error::EngineResult<index::SidecarData> {
            Ok(index::SidecarData {
                caption: Some(std::fs::read_to_string(path.with_extension("xmp")).unwrap()),
                keywords: vec!["accepted".into()],
                ..Default::default()
            })
        }
    }
    let (dir, index, id) = fixture();
    index.add_keyword("original", None).unwrap();
    index.tag(id, "original").unwrap();
    drop(index);
    let db = dir.path().join("index.sqlite");
    let conn = rusqlite::Connection::open(&db).unwrap();
    conn.execute_batch("DROP TABLE accepted_keyword; DROP TABLE understanding; DROP TRIGGER image_fts_delete; DELETE FROM migration WHERE version=6;").unwrap();
    drop(conn);
    let mut index = Index::open(&db).unwrap();
    assert_eq!(text(&index, "original"), vec![id]);
    assert_eq!(index.understanding(id).unwrap(), None);
    index.set_understanding(id, &sample()).unwrap();
    for caption in ["harbour", "seaside"] {
        std::fs::write(dir.path().join("photo.xmp"), caption).unwrap();
        assert_eq!(
            index
                .scan(dir.path(), &Sidecar, &NoopMetadataProvider)
                .unwrap(),
            1
        );
        assert_eq!(text(&index, caption), vec![id]);
        assert_eq!(text(&index, "WELCOME"), vec![id]);
        assert_eq!(text(&index, "lighthouse"), vec![id]);
        assert_eq!(index.images_with_keyword("accepted").unwrap(), vec![id]);
        assert!(index.images_with_keyword("suggestion").unwrap().is_empty());
    }
    assert!(text(&index, "harbour").is_empty());
    assert_eq!(index.understanding(id).unwrap(), Some(sample()));
    drop(index);
    let index = Index::open(&db).unwrap();
    assert_eq!(text(&index, "WELCOME"), vec![id]);
    assert_eq!(text(&index, "seaside"), vec![id]);
}

#[test]
fn invalid_values_leave_previous_understanding_unchanged() {
    let (_dir, index, id) = fixture();
    let original = sample();
    index.set_understanding(id, &original).unwrap();
    let mut invalid = Vec::new();
    for model in ["", " \n\t"] {
        let mut value = original.clone();
        value.model_version = model.into();
        invalid.push(value);
    }
    for confidence in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.01, 1.01] {
        let mut value = original.clone();
        value.keywords[0].confidence = confidence;
        invalid.push(value);
        let mut value = original.clone();
        value.ocr[0].confidence = confidence;
        invalid.push(value);
    }
    for bbox in [
        [-0.1, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.1, 1.0],
        [0.8, 0.0, 0.2, 1.0],
        [0.0, 0.9, 1.0, 0.2],
        [f32::NAN, 0.0, 1.0, 1.0],
        [0.0, 0.0, 1.0, f32::INFINITY],
    ] {
        let mut value = original.clone();
        value.ocr[0].bbox = bbox;
        invalid.push(value);
    }
    for value in invalid {
        assert!(
            index.set_understanding(id, &value).is_err(),
            "accepted {value:?}"
        );
        assert_eq!(index.understanding(id).unwrap(), Some(original.clone()));
    }
    let mut boundaries = original;
    boundaries.keywords[0].confidence = 0.0;
    boundaries.ocr[0].confidence = 1.0;
    boundaries.ocr[0].bbox = [0.0, 0.0, 1.0, 1.0];
    index.set_understanding(id, &boundaries).unwrap();
    assert_eq!(index.understanding(id).unwrap(), Some(boundaries));
}

#[test]
fn understanding_persists_provenance_without_accepting_suggestions() {
    let (dir, index, id) = fixture();
    assert_eq!(index.understanding(id).unwrap(), None);
    let value = sample();
    let json = serde_json::to_string(&value).unwrap();
    assert_eq!(serde_json::from_str::<Understanding>(&json).unwrap(), value);
    index.set_understanding(id, &value).unwrap();
    assert!(index.images_with_keyword("suggestion").unwrap().is_empty());
    assert!(index.facets(&Query::default()).unwrap().keywords.is_empty());
    drop(index);
    let index = Index::open(dir.path().join("index.sqlite")).unwrap();
    assert_eq!(index.understanding(id).unwrap(), Some(value));
    assert_eq!(index.understanding(ImageId(42)).unwrap(), None);
    assert!(index.set_understanding(ImageId(42), &sample()).is_err());
}
