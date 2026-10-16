//! Phased scans (ST-4): the batching form indexes what the one-call form
//! does, each batch is visible to other connections as soon as it commits,
//! and drive state (CS-1) parses apart from its application.
use index::{
    ChangeKind, DriveState, Index, NoopMetadataProvider, NoopSidecarReader, Query, SCAN_BATCH,
};
use std::path::Path;

/// Every indexed image (the default query lists a page of 100).
fn all() -> Query {
    Query {
        limit: usize::MAX,
        ..Default::default()
    }
}

fn shoot(count: usize) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir_all(photos.join("nested")).unwrap();
    for n in 0..count {
        let folder = if n % 3 == 0 { "nested" } else { "" };
        std::fs::write(photos.join(folder).join(format!("p{n:03}.jpg")), b"jpeg").unwrap();
    }
    // Skipped: drive state, sidecars and a non-image.
    std::fs::create_dir_all(photos.join(".tessera").join("analysis")).unwrap();
    std::fs::write(photos.join(".tessera").join("x.jpg"), b"state").unwrap();
    std::fs::write(photos.join("p001.jpg.xmp"), b"<x:xmpmeta/>").unwrap();
    std::fs::write(photos.join("notes.txt"), b"text").unwrap();
    dir
}

/// Drives the phases the way a lock-holding host does; returns the number
/// of images each batch committed, in order.
fn phased_scan(index: &mut Index, root: &Path) -> Vec<usize> {
    let scope = index.begin_scan(root).unwrap();
    let mut walk = scope.walk(&NoopSidecarReader);
    let mut batches = Vec::new();
    loop {
        let entries = walk.next_batch(SCAN_BATCH).unwrap();
        if entries.is_empty() {
            break;
        }
        let pending = index.scan_check(&scope, entries).unwrap();
        let records: Vec<_> = pending
            .into_iter()
            .map(|p| p.read(&NoopSidecarReader, &NoopMetadataProvider).unwrap())
            .collect();
        batches.push(index.scan_apply(&scope, &records).unwrap());
    }
    let changed = batches.iter().sum();
    index.finish_scan(changed).unwrap();
    batches
}

#[test]
fn batches_commit_one_by_one_and_index_what_a_one_call_scan_does() {
    let dir = shoot(150);
    let photos = dir.path().join("photos");
    let db = dir.path().join("index.sqlite");
    let mut writer = Index::open(&db).unwrap();
    let reader = Index::open(&db).unwrap();

    // Each batch is a transaction of its own: the reader sees it committed
    // before the next one is read, with its changes in the feed.
    let scope = writer.begin_scan(&photos).unwrap();
    let mut walk = scope.walk(&NoopSidecarReader);
    let mut seen = 0;
    let mut head = reader.change_head().unwrap();
    let mut batches = 0;
    loop {
        let entries = walk.next_batch(SCAN_BATCH).unwrap();
        if entries.is_empty() {
            break;
        }
        assert!(entries.len() <= SCAN_BATCH);
        let pending = writer.scan_check(&scope, entries).unwrap();
        assert_eq!(reader.search(&all()).unwrap().len(), seen);
        let records: Vec<_> = pending
            .into_iter()
            .map(|p| p.read(&NoopSidecarReader, &NoopMetadataProvider).unwrap())
            .collect();
        let written = writer.scan_apply(&scope, &records).unwrap();
        seen += written;
        batches += 1;
        assert_eq!(reader.search(&all()).unwrap().len(), seen);
        let feed = reader.changes_since(head).unwrap();
        assert_eq!(feed.changes.len(), written);
        assert!(feed.changes.iter().all(|c| c.kind == ChangeKind::Added));
        head = feed.to;
    }
    writer.finish_scan(seen).unwrap();
    assert_eq!((seen, batches), (150, 3));

    // The skip rules held: nothing from `.tessera/`, no sidecars, no text.
    let paths: Vec<_> = reader
        .search(&all())
        .unwrap()
        .into_iter()
        .map(|id| reader.image_info(id).unwrap().path)
        .collect();
    assert!(paths.iter().all(|p| p.extension().unwrap() == "jpg"));
    assert!(
        !paths
            .iter()
            .any(|p| p.to_string_lossy().contains(".tessera"))
    );

    // Same rows as the one-call scan on a fresh index.
    let mut whole = Index::open(":memory:").unwrap();
    assert_eq!(
        whole
            .scan(&photos, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap(),
        150
    );
    let mut expected: Vec<_> = whole
        .search(&all())
        .unwrap()
        .into_iter()
        .map(|id| (id, whole.image_info(id).unwrap().path))
        .collect();
    let mut got: Vec<_> = reader
        .search(&all())
        .unwrap()
        .into_iter()
        .map(|id| (id, reader.image_info(id).unwrap().path))
        .collect();
    expected.sort();
    got.sort();
    assert_eq!(got, expected);

    // Incremental (M2-28): an unchanged rescan writes nothing; a touched
    // sidecar or a new file is one record, through either form.
    assert_eq!(phased_scan(&mut writer, &photos), vec![0, 0, 0]);
    assert!(reader.changes_since(head).unwrap().changes.is_empty());
    std::fs::write(photos.join("p001.jpg.xmp"), b"<x:xmpmeta changed/>").unwrap();
    std::fs::write(photos.join("p150.jpg"), b"jpeg").unwrap();
    assert_eq!(phased_scan(&mut writer, &photos).iter().sum::<usize>(), 2);
    assert_eq!(
        writer
            .scan(&photos, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap(),
        0
    );
    assert_eq!(reader.search(&all()).unwrap().len(), 151);
}

#[test]
fn a_file_scan_refreshes_only_its_original() {
    let dir = shoot(5);
    let photos = dir.path().join("photos");
    let mut index = Index::open(":memory:").unwrap();
    let scope = index.begin_file_scan(&photos.join("p001.jpg")).unwrap();
    let entries = scope
        .walk(&NoopSidecarReader)
        .next_batch(SCAN_BATCH)
        .unwrap();
    assert_eq!(entries.len(), 1);
    assert!(entries[0].path().ends_with("p001.jpg"));
    let pending = index.scan_check(&scope, entries).unwrap();
    let records: Vec<_> = pending
        .into_iter()
        .map(|p| p.read(&NoopSidecarReader, &NoopMetadataProvider).unwrap())
        .collect();
    assert_eq!(index.scan_apply(&scope, &records).unwrap(), 1);
    assert_eq!(index.search(&all()).unwrap().len(), 1);
    assert!(
        index
            .begin_file_scan(&photos.join("notes.txt").join("missing"))
            .is_err()
    );
}

#[test]
fn drive_state_reads_apart_from_its_application() {
    let dir = shoot(2);
    let photos = dir.path().join("photos");
    // The folder's `.tessera/` holds no records yet: nothing to apply.
    let state = DriveState::read(&photos).unwrap();
    assert!(state.is_empty());
    let mut index = Index::open(":memory:").unwrap();
    assert_eq!(
        index.apply_drive_state(&photos, None, &state).unwrap(),
        index::ImportCounts::default()
    );
    assert_eq!(
        index.roots().unwrap().len(),
        0,
        "an empty state registers no root"
    );
    // An unreadable record is left out rather than failing the read.
    std::fs::write(
        photos.join(".tessera").join("analysis").join("bad.json"),
        b"{not json",
    )
    .unwrap();
    assert!(DriveState::read(&photos).unwrap().is_empty());
    // A root without `.tessera/` reads empty.
    assert!(DriveState::read(&photos.join("nested")).unwrap().is_empty());
}
