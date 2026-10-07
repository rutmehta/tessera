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
    for id in index
        .search(&index::Query {
            limit: i64::MAX as usize,
            ..Default::default()
        })
        .unwrap()
    {
        let path = index.image_info(id).unwrap().path;
        let mut recipe = engine_api::recipe::Recipe::new(id);
        recipe.unknown.insert(
            "lightroom_smart_preview".into(),
            serde_json::json!({ "original_path": dir.path().join("offline.raw") }),
        );
        sidecar::Sidecar::write_recipe(
            sidecar::Sidecar::paths(path).recipe,
            &sidecar::RecipeDocument {
                recipe,
                ..Default::default()
            },
        )
        .unwrap();
    }
    (dir, index)
}

#[test]
fn lr13c_open_never_calls_pixel_provider() {
    let (dir, index) = fixture(32);
    let reads = index.image_info_read_count();
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
    assert!(session.index().image_info_read_count() - reads <= 3 * 32);
}

#[test]
#[ignore = "release scale gate: 5,000 synthetic proxies; open < 5 seconds"]
fn lr13c_open_5000_proxies_under_five_seconds() {
    let (dir, index) = fixture(5_000);
    let reads = index.image_info_read_count();
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
    let operations = session.index().image_info_read_count() - reads;
    eprintln!("LR-13c: metadata lookups={operations}, bound=15000");
    assert!(operations <= 3 * 5_000);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert!(elapsed < std::time::Duration::from_secs(5));
}

fn finish(session: &mut OwnedCullSession) {
    let start = std::time::Instant::now();
    while session.previews_pending() {
        session.poll_previews().unwrap();
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        std::thread::yield_now();
    }
}

#[test]
fn lr13c_background_is_incremental_and_reopen_reuses_hashes() {
    let (dir, index) = fixture(40);
    let db = dir.path().join("index.sqlite");
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = |counter: Arc<AtomicUsize>| {
        move |_: &index::ImageInfo| {
            counter.fetch_add(1, Ordering::SeqCst);
            Ok(Some(0))
        }
    };
    let mut session =
        OwnedCullSession::open_owned_with_previews(index, dir.path(), provider(calls.clone()))
            .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(session.groups().len(), 40);
    finish(&mut session);
    assert_eq!(session.groups().len(), 1);
    assert_eq!(session.groups()[0].images.len(), 40);
    assert_eq!(calls.load(Ordering::SeqCst), 40);
    drop(session);
    let mut session = OwnedCullSession::open_owned_with_previews(
        Index::open(&db).unwrap(),
        dir.path(),
        provider(calls.clone()),
    )
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 40);
    finish(&mut session);
    assert_eq!(
        calls.load(Ordering::SeqCst),
        40,
        "persistent hits must not call the decoder"
    );
    assert_eq!(session.groups().len(), 1);
    drop(session);
    // In-place, same-size source replacement must invalidate even without a rescan.
    let path = dir.path().join("proxy-00000.dng");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[0] ^= 1;
    std::fs::write(&path, bytes).unwrap();
    let mut session = OwnedCullSession::open_owned_with_previews(
        Index::open(&db).unwrap(),
        dir.path(),
        provider(calls.clone()),
    )
    .unwrap();
    finish(&mut session);
    assert_eq!(calls.load(Ordering::SeqCst), 41);
}

#[test]
fn lr13c_drop_cancels_queued_hashes_without_waiting_for_running_image() {
    use std::sync::{Mutex, mpsc};
    let (dir, index) = fixture(40);
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let (entered, wait_entered) = mpsc::channel();
    let (release, wait_release) = mpsc::channel();
    let wait_release = Mutex::new(wait_release);
    // The worker owns the sole notifier sender once the session has dropped.
    let (retired, wait_retired) = mpsc::channel::<()>();
    let mut session = OwnedCullSession::open_owned_with_previews(index, dir.path(), move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        entered.send(()).unwrap();
        wait_release.lock().unwrap().recv().unwrap();
        Ok(Some(0))
    })
    .unwrap();
    session.set_preview_notifier(move || {
        let _ = retired.send(());
    });
    session.poll_previews().unwrap();
    wait_entered
        .recv_timeout(std::time::Duration::from_secs(5))
        .unwrap();
    let start = std::time::Instant::now();
    drop(session);
    assert!(start.elapsed() < std::time::Duration::from_secs(1));
    release.send(()).unwrap();
    assert_eq!(
        wait_retired.recv_timeout(std::time::Duration::from_secs(5)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
