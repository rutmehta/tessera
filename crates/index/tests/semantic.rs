use engine_api::{
    error::EngineResult,
    id::ImageId,
    recipe::{Decision, Selection},
};
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query, SemanticSearch};

struct Vectors(Vec<(ImageId, f32)>);
impl SemanticSearch for Vectors {
    fn search_text(&mut self, query: &str, k: usize) -> EngineResult<Vec<(ImageId, f32)>> {
        assert_eq!(query, "sunset");
        Ok(self.0.iter().copied().take(k).collect())
    }
}

#[test]
fn hybrid_rrf_rewards_overlap_and_keeps_single_source_hits() {
    let (_dir, index, ids) = catalog();
    index.add_keyword("sunset", None).unwrap();
    index.tag(ids[1], "sunset").unwrap();
    index.tag(ids[2], "sunset").unwrap();
    let mut vectors = Vectors(vec![(ids[0], 100.0), (ids[1], -1.0)]);
    let query = Query {
        text: Some("sunset".into()),
        semantic: Some("sunset".into()),
        ..Default::default()
    };
    let found = index.search_with_semantic(&query, &mut vectors).unwrap();
    assert_eq!(
        found[0], ids[1],
        "overlap beats vector-only leader regardless of score scale"
    );
    assert_eq!(found.len(), 3, "RRF is a union, not an intersection");
}

#[test]
fn legacy_queries_do_not_call_provider_and_old_json_deserializes() {
    struct Unavailable;
    impl SemanticSearch for Unavailable {
        fn search_text(&mut self, _: &str, _: usize) -> EngineResult<Vec<(ImageId, f32)>> {
            Err(engine_api::error::EngineError::invalid(
                "provider", "offline",
            ))
        }
    }
    let (_dir, index, _) = catalog();
    let legacy: Query = serde_json::from_str(r#"{"text":null,"folder":null,"decision":null,"grade":null,"mark":null,"date_from":null,"date_to":null,"camera":null,"lens":null,"keyword":null,"limit":1,"offset":1}"#).unwrap();
    assert!(legacy.semantic.is_none());
    assert_eq!(
        index.search(&legacy).unwrap(),
        index
            .search_with_semantic(&legacy, &mut Unavailable)
            .unwrap()
    );
    assert_eq!(
        index.facets(&legacy).unwrap().decisions,
        index
            .facets_with_semantic(&legacy, &mut Unavailable)
            .unwrap()
            .decisions
    );
    let semantic = Query {
        semantic: Some("sunset".into()),
        ..legacy
    };
    assert!(
        index
            .search_with_semantic(&semantic, &mut Unavailable)
            .is_err()
    );
}

#[test]
fn vector_hits_are_sorted_deduplicated_and_catalog_scoped() {
    let (_dir, index, ids) = catalog();
    let mut vectors = Vectors(vec![
        (ImageId(0), 10.0),
        (ids[0], 0.1),
        (ids[1], f32::NAN),
        (ids[2], 0.9),
        (ids[0], 0.9),
        (ids[2], 0.2),
    ]);
    let query = Query {
        semantic: Some("sunset".into()),
        ..Default::default()
    };
    assert_eq!(
        index.search_with_semantic(&query, &mut vectors).unwrap(),
        vec![ids[0], ids[2]]
    );
    assert_eq!(
        index
            .facets_with_semantic(&query, &mut vectors)
            .unwrap()
            .decisions,
        vec![("undecided".into(), 2)]
    );
    assert!(
        index
            .search_with_semantic(&query, &mut Vectors(vec![]))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn hybrid_uses_bm25_not_capture_order_and_filters_lexical_hits() {
    let (_dir, index, ids) = catalog();
    index.add_keyword("sunset", None).unwrap();
    index.tag(ids[0], "sunset").unwrap();
    index.tag(ids[2], "sunset").unwrap();
    for word in ["extra", "words", "dilute", "relevance"] {
        index.add_keyword(word, None).unwrap();
        index.tag(ids[0], word).unwrap();
    }
    let query = Query {
        text: Some("sunset".into()),
        semantic: Some("sunset".into()),
        ..Default::default()
    };
    assert_eq!(
        index
            .search_with_semantic(&query, &mut Vectors(vec![]))
            .unwrap(),
        vec![ids[2], ids[0]]
    );
    index
        .set_selection(
            ids[0],
            &Selection {
                decision: Decision::Keep,
                grade: None,
                mark: None,
            },
        )
        .unwrap();
    let filtered = Query {
        decision: Some(Decision::Keep),
        ..query
    };
    assert_eq!(
        index
            .search_with_semantic(&filtered, &mut Vectors(vec![(ids[2], 1.0)]))
            .unwrap(),
        vec![ids[0]]
    );
}

fn catalog() -> (tempfile::TempDir, Index, Vec<ImageId>) {
    let dir = tempfile::tempdir().unwrap();
    for name in ["a.jpg", "b.jpg", "c.jpg"] {
        std::fs::write(dir.path().join(name), b"jpeg").unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let ids = index.search(&Query::default()).unwrap();
    (dir, index, ids)
}

#[test]
fn semantic_ranking_filters_before_pagination() {
    let (_dir, index, ids) = catalog();
    for &id in &ids[1..] {
        index
            .set_selection(
                id,
                &Selection {
                    decision: Decision::Keep,
                    grade: None,
                    mark: None,
                },
            )
            .unwrap();
    }
    let mut vectors = Vectors(vec![(ids[0], 1.0), (ids[2], 0.9), (ids[1], 0.8)]);
    let query = Query {
        semantic: Some("sunset".into()),
        decision: Some(Decision::Keep),
        limit: 1,
        offset: 1,
        ..Default::default()
    };
    assert_eq!(
        index.search_with_semantic(&query, &mut vectors).unwrap(),
        vec![ids[1]]
    );
    assert_eq!(
        index
            .facets_with_semantic(&query, &mut vectors)
            .unwrap()
            .decisions,
        vec![("keep".into(), 2)]
    );
    assert!(
        index.search(&query).is_err(),
        "must not silently ignore semantic text"
    );
    assert!(
        index.facets(&query).is_err(),
        "must not silently count unrelated images"
    );
}
