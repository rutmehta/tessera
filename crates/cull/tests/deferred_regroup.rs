use cull::{GroupingOptions, GroupingStrategy, OwnedCullSession};
use index::{ChangeBatch, ChangeFields, ChangeKind, ImageChange, ImageInfo, Index, NoopMetadataProvider, NoopSidecarReader};
use std::sync::{Arc, Mutex, atomic::{AtomicUsize, Ordering}, mpsc};
use std::time::{Duration, Instant};

struct EqualHashes;
impl GroupingStrategy for EqualHashes {
    fn related(&self, _: &ImageInfo, _: &ImageInfo, a: Option<u64>, b: Option<u64>, _: GroupingOptions) -> bool {
        a.is_some() && a == b
    }
}
fn fixture(count: usize) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..count {
        std::fs::write(dir.path().join(format!("{n}.jpg")), b"synthetic").unwrap();
    }
    let mut index = Index::open(":memory:").unwrap();
    index.scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider).unwrap();
    (dir, index)
}
fn finish(session: &mut OwnedCullSession) -> bool {
    let start = Instant::now();
    let mut changed = false;
    while session.previews_pending() {
        changed |= session.sync_catalog().unwrap().regrouped;
        assert!(start.elapsed() < Duration::from_secs(5));
        std::thread::yield_now();
    }
    changed
}
#[test]
fn lr13d_custom_final_discard_applies_hash_snapshot_and_group_delta() {
    let (dir, index) = fixture(3);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let wait_release = Mutex::new(wait_release);
    let mut session = OwnedCullSession::open_owned_with_previews(index, dir.path(), move |_| {
        if counter.fetch_add(1, Ordering::SeqCst) == 2 {
            entered.send(()).unwrap();
            wait_release.lock().unwrap().recv().unwrap();
        }
        Ok(Some(0))
    }).unwrap();
    session.set_grouping_strategy(Box::new(EqualHashes));
    session.regroup(GroupingOptions::default()).unwrap();
    let ids = session.images().to_vec();
    session.poll_previews().unwrap();
    wait_entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(!session.poll_previews().unwrap(), "custom policy waits for complete snapshot");
    assert_eq!(session.groups().len(), 3);
    session.remove_images(&[ids[2]]).unwrap();
    release.send(()).unwrap();
    assert!(finish(&mut session), "discarded last reply must still emit a group delta");
    assert_eq!(session.groups().len(), 1);
    assert_eq!(session.groups()[0].images, ids[..2]);
}
#[test]
fn lr13d_irrelevant_metadata_preserves_hashes_and_groups() {
    let (dir, index) = fixture(3);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut session = OwnedCullSession::open_owned_with_previews(index, dir.path(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Some(0))
    }).unwrap();
    finish(&mut session);
    let groups = session.groups().to_vec();
    let seq = session.change_sequence();
    let change = session.apply_changes(&ChangeBatch {
        from: seq, to: seq + 1, reset: false,
        changes: vec![ImageChange { seq: seq + 1, id: session.images()[0], kind: ChangeKind::Updated(ChangeFields::METADATA) }],
    }).unwrap();
    assert!(!change.regrouped, "unchanged grouping inputs must preserve component");
    assert!(!session.previews_pending(), "caption/camera-only metadata must not invalidate pixels");
    assert_eq!(session.groups(), groups);
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}
