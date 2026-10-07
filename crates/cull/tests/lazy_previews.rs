use cull::OwnedCullSession;
use index::{Index, NoopMetadataProvider, NoopSidecarReader};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(count: usize) -> (tempfile::TempDir, Index) {
    let dir = tempfile::tempdir().unwrap();
    for n in 0..count {
        std::fs::write(
            dir.path().join(format!("proxy-{n:05}.dng")),
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
    }
    let mut index = Index::open(dir.path().join("index.sqlite")).unwrap();
    index
        .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    (dir, index)
}

#[test]
fn lr13c_open_never_calls_pixel_provider() {
    let (dir, index) = fixture(32);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let session = OwnedCullSession::open_owned_with_previews(index, dir.path(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Some(0))
    })
    .unwrap();
    assert_eq!(session.images().len(), 32);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        0,
        "open must do zero pixel operations"
    );
    assert_eq!(session.groups().len(), 32);
}

#[test]
#[ignore = "release scale gate: 5,000 synthetic proxies; open < 5 seconds"]
fn lr13c_open_5000_proxies_under_five_seconds() {
    let (dir, index) = fixture(5_000);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let start = std::time::Instant::now();
    let session = OwnedCullSession::open_owned_with_previews(index, dir.path(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(Some(0))
    })
    .unwrap();
    let elapsed = start.elapsed();
    eprintln!(
        "LR-13c: 5000 proxies open={elapsed:?}, pixel_operations={}",
        calls.load(Ordering::SeqCst)
    );
    assert_eq!(session.images().len(), 5_000);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(elapsed < std::time::Duration::from_secs(5));
}
