//! Change feed (M2-28): ordering, coalescing and cross-connection visibility.
use engine_api::recipe::{Decision, Selection};
use index::{
    ChangeFields, ChangeKind, Index, NoopMetadataProvider, NoopSidecarReader, Query, Score,
};

fn scan(index: &mut Index, dir: &std::path::Path) -> usize {
    index
        .scan(dir, &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap()
}

fn id_of(index: &Index, path: &std::path::Path) -> index::ImageChange {
    let id = index
        .image_at(&path.canonicalize().unwrap())
        .unwrap()
        .unwrap();
    index::ImageChange {
        seq: 0,
        id,
        kind: ChangeKind::Added,
    }
}

#[test]
fn feed_orders_and_coalesces_across_connections() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let db = dir.path().join("index.sqlite");
    for name in ["a.jpg", "b.jpg"] {
        std::fs::write(photos.join(name), name).unwrap();
    }
    let mut writer = Index::open(&db).unwrap();
    let reader = Index::open(&db).unwrap();
    assert_eq!(reader.change_head().unwrap(), 0);
    let empty = reader.changes_since(0).unwrap();
    assert!(empty.is_empty() && !empty.reset);

    scan(&mut writer, &photos);
    let a = id_of(&reader, &photos.join("a.jpg")).id;
    let b = id_of(&reader, &photos.join("b.jpg")).id;
    let first = reader.changes_since(0).unwrap();
    assert_eq!(first.from, 0);
    assert_eq!(first.to, reader.change_head().unwrap());
    // Each image once, as Added (its selection/metadata rows fold into it).
    assert_eq!(first.changes.len(), 2);
    assert!(first.changes.iter().all(|c| c.kind == ChangeKind::Added));
    assert!(first.changes.windows(2).all(|w| w[0].seq < w[1].seq));
    let mut ids: Vec<_> = first.changes.iter().map(|c| c.id).collect();
    ids.sort();
    let mut expected = vec![a, b];
    expected.sort();
    assert_eq!(ids, expected);

    // An unchanged rescan writes nothing, so the feed stays quiet.
    assert_eq!(scan(&mut writer, &photos), 0);
    assert!(reader.changes_since(first.to).unwrap().changes.is_empty());

    // Writes through another connection, in order: b's score, then a's selection twice.
    writer
        .set_score(
            b,
            &Score {
                signal: "sharpness".into(),
                value: 0.5,
                model: "test".into(),
            },
        )
        .unwrap();
    for decision in [Decision::Keep, Decision::Reject] {
        writer
            .set_selection(
                a,
                &Selection {
                    decision,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    let second = reader.changes_since(first.to).unwrap();
    assert_eq!(
        second
            .changes
            .iter()
            .map(|c| (c.id, c.kind))
            .collect::<Vec<_>>(),
        vec![
            (b, ChangeKind::Updated(ChangeFields::SCORES)),
            (a, ChangeKind::Updated(ChangeFields::SELECTION)),
        ],
        "one entry per image, ordered by its latest write"
    );
    // Setting the same selection again is not a change.
    writer
        .set_selection(
            a,
            &Selection {
                decision: Decision::Reject,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(reader.changes_since(second.to).unwrap().changes.is_empty());

    // A new file and a removed one arrive in one batch; pulling from the start
    // still reports the removed image as never having existed.
    std::fs::write(photos.join("c.jpg"), "c").unwrap();
    scan(&mut writer, &photos);
    std::fs::remove_file(photos.join("b.jpg")).unwrap();
    writer.prune_missing(false).unwrap();
    let third = reader.changes_since(second.to).unwrap();
    let c = id_of(&reader, &photos.join("c.jpg")).id;
    assert_eq!(
        third
            .changes
            .iter()
            .map(|x| (x.id, x.kind))
            .collect::<Vec<_>>(),
        vec![(c, ChangeKind::Added), (b, ChangeKind::Removed)]
    );
    let all = reader.changes_since(0).unwrap();
    assert!(all.changes.iter().all(|x| x.id != b));
    assert_eq!(
        all.changes.iter().find(|x| x.id == a).unwrap().kind,
        ChangeKind::Added
    );
    // Rewritten pixels are a file change.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(photos.join("a.jpg"), "new pixels").unwrap();
    scan(&mut writer, &photos);
    let fourth = reader.changes_since(third.to).unwrap();
    assert_eq!(fourth.changes.len(), 1);
    assert!(
        matches!(fourth.changes[0].kind, ChangeKind::Updated(f) if f.contains(ChangeFields::FILE))
    );

    // A cursor from another (or a rebuilt) catalog cannot be applied.
    assert!(reader.changes_since(fourth.to + 10).unwrap().reset);
    assert_eq!(
        reader.search(&Query::default()).unwrap().len(),
        2,
        "feed rows are not catalog rows"
    );
}

#[test]
fn order_keys_follow_search_order() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["x.jpg", "y.jpg", "z.jpg"] {
        std::fs::write(dir.path().join(name), name).unwrap();
    }
    let mut index = Index::open(dir.path().join("i.sqlite")).unwrap();
    scan(&mut index, dir.path());
    let ids = index
        .search(&Query {
            limit: 10,
            ..Default::default()
        })
        .unwrap();
    let keys = index.order_keys(&ids).unwrap();
    assert!(keys.windows(2).all(|w| w[0] <= w[1]));
}
