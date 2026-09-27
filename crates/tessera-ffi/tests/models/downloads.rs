// Included by models.rs to test the private module's exported UniFFI surface.
use super::*;
use std::sync::mpsc;
use std::time::Duration;

struct Listener(mpsc::Sender<ModelDownloadEvent>);
impl ModelDownloadListener for Listener {
    fn on_event(&self, event: ModelDownloadEvent) {
        self.0.send(event).unwrap();
    }
}
fn manifest() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml").into()
}
fn request(downloads: &ModelDownloads) -> Vec<ModelDownloadEvent> {
    let (send, receive) = mpsc::channel();
    downloads
        .request("test/conv".into(), "1".into(), Arc::new(Listener(send)))
        .unwrap();
    // The worker drops its sole listener after the terminal event. This also
    // detects duplicate terminals and hanging workers, without timing sleeps.
    let mut events = Vec::new();
    loop {
        match receive.recv_timeout(Duration::from_secs(10)) {
            Ok(event) => events.push(event),
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
            Err(e) => panic!("worker did not finish: {e}"),
        }
    }
    assert!(matches!(events.first(), Some(ModelDownloadEvent::Queued)));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(
                e,
                ModelDownloadEvent::Ready { .. } | ModelDownloadEvent::Failed { .. }
            ))
            .count(),
        1
    );
    events
}
#[test]
fn model_download_missing_disabled_has_one_failure() {
    let cache = tempfile::tempdir().unwrap();
    let downloads =
        ModelDownloads::open(manifest(), cache.path().display().to_string(), false).unwrap();
    let events = request(&downloads);
    assert_eq!(events.len(), 2);
    assert!(
        matches!(events.last(), Some(ModelDownloadEvent::Failed { reason }) if reason.contains("downloads are disabled"))
    );
    assert_eq!(std::fs::read_dir(cache.path()).unwrap().count(), 0);
}
#[test]
fn model_download_local_progress_cached_ready_and_corruption() {
    let cache = tempfile::tempdir().unwrap();
    let downloads =
        ModelDownloads::open(manifest(), cache.path().display().to_string(), true).unwrap();
    let events = request(&downloads);
    let path = match events.last().unwrap() {
        ModelDownloadEvent::Ready { path } => path,
        other => panic!("unexpected terminal: {other:?}"),
    };
    let bytes = std::fs::metadata(path).unwrap().len();
    let progress: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            ModelDownloadEvent::Downloading { bytes, total } => Some((*bytes, *total)),
            _ => None,
        })
        .collect();
    assert_eq!(progress.first(), Some(&(0, Some(bytes))));
    assert_eq!(progress.last(), Some(&(bytes, Some(bytes))));
    assert!(progress.windows(2).all(|p| p[0].0 <= p[1].0));
    let offline =
        ModelDownloads::open(manifest(), cache.path().display().to_string(), false).unwrap();
    let cached = request(&offline);
    assert_eq!(cached.len(), 2);
    assert!(matches!(
        cached.last(),
        Some(ModelDownloadEvent::Ready { .. })
    ));
    std::fs::write(path, b"corrupt").unwrap();
    assert!(
        matches!(request(&offline).last(), Some(ModelDownloadEvent::Failed { reason }) if reason.contains("SHA-256 mismatch"))
    );
}
#[test]
fn model_download_checksum_failure_never_installs() {
    let dir = tempfile::tempdir().unwrap();
    let manifest_path = dir.path().join("models.toml");
    std::fs::write(dir.path().join("bad.onnx"), b"bad").unwrap();
    std::fs::write(
        &manifest_path,
        format!(
            r#"
[[models]]
id = "test/conv"
version = "1"
task = "test"
dtype = "fp32"
sha256 = "{}"
download_url = "file:bad.onnx"
inputs = [{{ name = "in", shape = [1], dtype = "fp32" }}]
outputs = [{{ name = "out", shape = [1], dtype = "fp32" }}]
"#,
            "0".repeat(64)
        ),
    )
    .unwrap();
    let cache = dir.path().join("cache");
    let downloads = ModelDownloads::open(
        manifest_path.display().to_string(),
        cache.display().to_string(),
        true,
    )
    .unwrap();
    assert!(
        matches!(request(&downloads).last(), Some(ModelDownloadEvent::Failed { reason }) if reason.contains("SHA-256 mismatch"))
    );
    assert_eq!(std::fs::read_dir(cache).unwrap().count(), 0);
}
