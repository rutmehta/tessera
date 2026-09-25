use engine_api::id::ImageId;
use index::{Index, Query};
use rusqlite::{Connection, params};
use serde_json::json;

// No image/model fixtures: exercise the real migrated schema with deterministic IDs.
fn catalog() -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("index.sqlite");
    let index = Index::open(&path).unwrap();
    let db = Connection::open(path).unwrap();
    db.execute_batch(
        "INSERT INTO root VALUES(1,'/photos');
        INSERT INTO folder VALUES(1,1,'/photos');
        INSERT INTO keyword VALUES(1,'People',NULL),(2,'Anna',1);
        INSERT INTO keyword_closure VALUES(1,1,0),(1,2,1),(2,2,0);",
    )
    .unwrap();
    for n in 1..=4 {
        let id = ImageId(n).to_string();
        let camera = if n <= 2 { "Canon" } else { "Sony" };
        db.execute(
            "INSERT INTO file(id,folder_id,path,name,size,mtime) VALUES(?,1,?,?,1,1)",
            params![n as i64, format!("/photos/{n}.jpg"), format!("{n}.jpg")],
        )
        .unwrap();
        db.execute(
            "INSERT INTO image(id,file_id,capture_time,camera,lens) VALUES(?,?,?,?,?)",
            params![id, n as i64, format!("2024-0{n}-01"), camera, "85mm"],
        )
        .unwrap();
        db.execute(
            "INSERT INTO fts(rowid,image_id,filename,caption) VALUES(?,?,?,'beach')",
            params![n as i64, id, format!("{n}.jpg")],
        )
        .unwrap();
        if n != 4 {
            db.execute(
                "INSERT INTO selection VALUES(?,?,?,?)",
                params![
                    id,
                    if n == 3 { "reject" } else { "keep" },
                    if n == 3 { None } else { Some(n as i64) },
                    if n == 1 { Some("hero") } else { None }
                ],
            )
            .unwrap();
        }
        if n <= 2 {
            db.execute("INSERT INTO image_keyword VALUES(?,2)", [&id])
                .unwrap();
            db.execute(
                "INSERT INTO score VALUES(?,'focus',?,'synthetic')",
                params![id, n as f64 / 3.0],
            )
            .unwrap();
        }
    }
    (dir, index)
}

fn query(predicate: serde_json::Value) -> Query {
    let mut value = serde_json::to_value(Query::default()).unwrap();
    value["predicate"] = predicate;
    serde_json::from_value(value).unwrap()
}

#[test]
fn not_matches_missing_selection_and_nullable_fields() {
    let (_dir, index) = catalog();
    let q = query(json!({"Not": {"Decision": "keep"}}));
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(3), ImageId(4)]);
    assert_eq!(
        index.facets(&q).unwrap().decisions,
        vec![("reject".into(), 1), ("undecided".into(), 1)]
    );
    let q = query(json!({"Not": {"Mark": "hero"}}));
    assert_eq!(
        index.search(&q).unwrap(),
        vec![ImageId(2), ImageId(3), ImageId(4)]
    );
    let q = query(json!({"Not": {"Not": {"Mark": "hero"}}}));
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(1)]);
}

#[test]
fn person_focus_and_metadata_leaves_intersect() {
    let (_dir, index) = catalog();
    let q = query(json!({"All": [
        {"Person": "Anna"}, {"Focus": ["Gt", 0.6]},
        {"Grade": ["Ge", 2.0]}, {"Keyword": "People"},
        {"Text": "beach"}, {"Lens": "85mm"},
        {"DateFrom": "2024-02-01"}, {"DateBefore": "2024-03-01"}
    ]}));
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(2)]);
    assert_eq!(index.facets(&q).unwrap().keywords, vec![("Anna".into(), 1)]);
}

#[test]
fn ids_scope_intersects_legacy_filters_before_paging_and_facets() {
    let (_dir, index) = catalog();
    let mut q =
        query(json!({"Ids": [ImageId(1), ImageId(2), ImageId(2), ImageId(3), ImageId(999)]}));
    q.camera = Some("Canon".into());
    q.folder = Some("/photos".into());
    q.limit = 1;
    q.offset = 1;
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(2)]);
    let facets = index.facets(&q).unwrap();
    assert_eq!(facets.cameras, vec![("Canon".into(), 2)]);
    assert_eq!(facets.lenses, vec![("85mm".into(), 2)]);
    assert_eq!(facets.keywords, vec![("Anna".into(), 2)]);
    assert_eq!(facets.decisions, vec![("keep".into(), 2)]);
    for value in [
        json!({"Ids": []}),
        json!({"Any": []}),
        json!({"Not": {"All": []}}),
    ] {
        let q = query(value);
        assert!(index.search(&q).unwrap().is_empty());
        assert!(index.facets(&q).unwrap().cameras.is_empty());
    }
    for value in [
        json!({"All": []}),
        json!({"Not": {"Any": []}}),
        json!({"Not": {"Ids": []}}),
    ] {
        assert_eq!(
            index.search(&query(value)).unwrap(),
            vec![ImageId(1), ImageId(2), ImageId(3), ImageId(4)]
        );
    }
}

#[test]
fn numeric_comparisons_and_missing_scores_are_two_valued() {
    use index::{Comparison::*, Predicate};
    let (_dir, index) = catalog();
    for (op, expected) in [
        (Eq, vec![2]),
        (Ne, vec![1]),
        (Lt, vec![1]),
        (Le, vec![1, 2]),
        (Gt, vec![]),
        (Ge, vec![2]),
    ] {
        for predicate in [Predicate::Grade(op, 2.0), Predicate::Focus(op, 2.0 / 3.0)] {
            let q = Query {
                predicate: Some(predicate),
                ..Default::default()
            };
            assert_eq!(
                index.search(&q).unwrap(),
                expected.iter().map(|n| ImageId(*n)).collect::<Vec<_>>()
            );
        }
    }
    let q = Query {
        predicate: Some(Predicate::Not(Box::new(Predicate::Focus(Ge, 0.0)))),
        ..Default::default()
    };
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(3), ImageId(4)]);
}

#[test]
fn user_strings_remain_values_and_dates_are_half_open() {
    use index::Predicate;
    let (_dir, index) = catalog();
    let hostile = "' OR 1=1 --";
    for predicate in [
        Predicate::Camera(hostile.into()),
        Predicate::Lens(hostile.into()),
        Predicate::Decision(hostile.into()),
        Predicate::Mark(hostile.into()),
        Predicate::Keyword(hostile.into()),
        Predicate::Person(hostile.into()),
    ] {
        let q = Query {
            predicate: Some(predicate),
            ..Default::default()
        };
        assert!(index.search(&q).unwrap().is_empty());
        assert!(index.facets(&q).unwrap().cameras.is_empty());
    }
    // FTS syntax stays FTS syntax, never SQL. A quoted phrase is valid FTS.
    let q = query(json!({"Text": "\"OR 1 1\""}));
    assert!(index.search(&q).unwrap().is_empty());
    let q = query(json!({"All": [{"DateFrom": "2024-02-01"}, {"DateBefore": "2024-03-01"}]}));
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(2)]);
    let mut q = query(json!({"Any": [{"Text": "beach"}, {"Person": "nobody"}]}));
    q.text = Some("absent".into());
    assert!(index.search(&q).unwrap().is_empty());
    assert!(index.facets(&q).unwrap().keywords.is_empty());
}

#[test]
fn typed_predicates_round_trip_and_legacy_queries_default_to_none() {
    use index::{Comparison, Predicate};
    let p = Predicate::All(vec![
        Predicate::Ids(vec![ImageId(u128::MAX)]),
        Predicate::Focus(Comparison::Ge, 0.6),
        Predicate::Not(Box::new(Predicate::Person("Anna".into()))),
    ]);
    let encoded = serde_json::to_string(&p).unwrap();
    assert_eq!(
        serde_json::from_str::<Predicate>(&encoded).unwrap(),
        p.clone()
    );
    let mut legacy = serde_json::to_value(Query::default()).unwrap();
    legacy.as_object_mut().unwrap().remove("predicate");
    assert!(
        serde_json::from_value::<Query>(legacy)
            .unwrap()
            .predicate
            .is_none()
    );
}

#[test]
fn semantic_candidates_are_filtered_by_predicate_without_api_changes() {
    struct Provider;
    impl index::SemanticSearch for Provider {
        fn search_text(
            &mut self,
            _: &str,
            _: usize,
        ) -> engine_api::error::EngineResult<Vec<(ImageId, f32)>> {
            Ok(vec![
                (ImageId(3), 0.9),
                (ImageId(2), 0.8),
                (ImageId(1), 0.7),
            ])
        }
    }
    let (_dir, index) = catalog();
    let q = Query {
        semantic: Some("portrait".into()),
        predicate: Some(index::Predicate::Camera("Canon".into())),
        limit: 1,
        offset: 1,
        ..Default::default()
    };
    assert_eq!(
        index.search_with_semantic(&q, &mut Provider).unwrap(),
        vec![ImageId(1)]
    );
    assert_eq!(
        index
            .facets_with_semantic(&q, &mut Provider)
            .unwrap()
            .cameras,
        vec![("Canon".into(), 2)]
    );
}

#[test]
fn nested_boolean_filters_use_the_same_set_for_facets() {
    let (_dir, index) = catalog();
    let q = query(json!({"All": [
        {"Any": [{"Camera": "Canon"}, {"Decision": "reject"}]},
        {"Not": {"Mark": "hero"}}
    ]}));
    assert_eq!(index.search(&q).unwrap(), vec![ImageId(2), ImageId(3)]);
    assert_eq!(
        index.facets(&q).unwrap().cameras,
        vec![("Canon".into(), 1), ("Sony".into(), 1)]
    );
}
