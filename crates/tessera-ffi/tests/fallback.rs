use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};
use tessera_ffi::{Engine, EngineEvent, EngineEventListener, ImageQuery};
struct Events(Mutex<mpsc::Sender<(String, u32, std::thread::ThreadId)>>);
impl EngineEventListener for Events {
    fn on_event(&self, event: EngineEvent) {
        if let EngineEvent::PreviewReady { image_id, max_px } = event {
            self.0
                .lock()
                .unwrap()
                .send((image_id, max_px, std::thread::current().id()))
                .unwrap();
        }
    }
}
#[test]
fn missing_jpeg_returns_pending_then_callback_and_cached_bytes() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    std::fs::copy(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/raw/sample.dng"),
        photos.join("sample.dng"),
    )
    .unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let (tx, rx) = mpsc::channel();
    engine.set_event_listener(Some(Arc::new(Events(Mutex::new(tx)))));
    let start = Instant::now();
    let response = engine.clone().embedded_preview(id.clone(), 384).unwrap();
    assert!(response.pending && response.bytes.is_none());
    assert!(start.elapsed() < Duration::from_millis(100));
    assert!(
        engine
            .clone()
            .embedded_preview(id.clone(), 384)
            .unwrap()
            .pending
    );
    let (callback_id, size, thread) = rx.recv_timeout(Duration::from_secs(3)).unwrap();
    assert_eq!(callback_id, id);
    assert_eq!(size, 384);
    assert_ne!(thread, std::thread::current().id());
    let response = engine.embedded_preview(id, 384).unwrap();
    assert!(!response.pending);
    let im = image::load_from_memory(&response.bytes.unwrap()).unwrap();
    assert!(im.width().max(im.height()) <= 384);
    assert!(rx.try_recv().is_err());
}
