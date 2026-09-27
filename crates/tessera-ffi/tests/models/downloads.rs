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
#[test]
fn ai_download_uses_production_target_without_research_artifacts() {
    let dir = tempfile::tempdir().unwrap();
    let text = std::fs::read_to_string(manifest()).unwrap()
        .replace("https://huggingface.co/synthscript/drunet-color-onnx/resolve/a2b9fccfa27b197f44a3876c567f5e48970c44a7/drunet_color.onnx", "file:conv.onnx")
        .replace("2ae3ab5eb15daac2ee79be984d584b908ce7f0f60b27be87d005f728c2aa0087", "c64f58321fa5cfeca15daf11a4db55e9546e057d9d812acbf4f23e38f860d901");
    std::fs::write(dir.path().join("models.toml"), text).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ml-runtime/tests/data/conv.onnx"
        ),
        dir.path().join("conv.onnx"),
    )
    .unwrap();
    let downloads = ModelDownloads::open(
        dir.path().join("models.toml").display().to_string(),
        dir.path().join("cache").display().to_string(),
        true,
    )
    .unwrap();
    let (send, receive) = mpsc::channel();
    downloads
        .request(
            "enhance/cfa-unet-fp32".into(),
            "a138c59a65846c10967839e85817231ec6ea92b318a57814cb153e8ac8bb311b".into(),
            Arc::new(Listener(send)),
        )
        .unwrap();
    let events: Vec<_> = receive.iter().collect();
    assert!(
        matches!(events.last(), Some(ModelDownloadEvent::Ready { .. })),
        "{events:?}"
    );
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
