//! OF-1 public contract (`docs/design/offline/SPEC.md`) on disposable COPIES
//! of two real mosaic RAWs: coverage before and after a build, candidate
//! ordering, ensure idempotence, the pending-sync list around a journaled
//! save and a synchronize, the storage sum, and a clean stale rebuild.
//!
//! Lock discipline is asserted in the crate's own unit test
//! `offline::tests::coverage_and_candidates_never_wait_on_the_catalog`, which
//! holds the catalog lock while the sweep runs (the lock statistics are
//! crate-private); this binary documents it rather than repeating it.
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    time::SystemTime,
};
use tessera_ffi::*;

fn recipe(engine: &Engine, id: &str) -> engine_api::recipe::Recipe {
    serde_json::from_str(&engine.get_recipe(id.into()).unwrap()).unwrap()
}

struct RenderResult(std::sync::mpsc::Sender<std::result::Result<FrameInfo, String>>);
impl DevelopListener for RenderResult {
    fn frame_ready(&self, frame: FrameInfo) {
        if frame.is_final {
            let _ = self.0.send(Ok(frame));
        }
    }
    fn render_failed(&self, message: String) {
        let _ = self.0.send(Err(message));
    }
    fn saved(&self, _: String) {}
}
/// One edit through a Smart Preview develop session, flushed to the journal.
fn edit_preview(session: &std::sync::Arc<DevelopSession>, exposure: f32) {
    let (send, receive) = std::sync::mpsc::channel();
    session.set_listener(Some(std::sync::Arc::new(RenderResult(send))));
    session
        .set_settings(
            serde_json::json!({"tone":{"exposure":exposure}}).to_string(),
            false,
        )
        .unwrap();
    receive
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("proxy render timed out")
        .expect("proxy render failed");
    session.flush().unwrap();
    session.close().unwrap();
}

/// Bytes of every regular file under `dir`, the way a person would count them.
fn tree_bytes(dir: &Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let meta = fs::symlink_metadata(entry.path()).unwrap();
            if meta.is_dir() {
                tree_bytes(&entry.path())
            } else {
                meta.len()
            }
        })
        .sum()
}
fn pixels_identity(support: &Path, id: &str) -> (SystemTime, u64) {
    let meta = fs::metadata(support.join("smart-previews").join(id).join("pixels.tsp")).unwrap();
    (meta.modified().unwrap(), meta.len())
}
fn capture_seconds(row: &ImageSummary) -> i64 {
    row.capture_time
        .as_deref()
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| panic!("RAW capture time is Unix seconds: {row:?}"))
}
fn coverage(engine: &Engine, folder: &str) -> (u64, u64, u64, u64, u64) {
    let c = engine.offline_coverage(folder.into()).unwrap();
    assert_eq!(c.folder, folder);
    (c.raws, c.ready, c.pending_sync, c.needs_attention, c.bytes)
}

#[test]
fn offline_engine_contract() {
    let test = test_fixtures::current_test();
    let Some(fixtures) = test_fixtures::raw::files(&test, &["sony-arw.ARW", "nikon-nef.NEF"])
    else {
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    fs::create_dir(&photos).unwrap();
    // A sibling folder with a RAW the sweep must not count.
    let elsewhere = dir.path().join("photos-elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    for fixture in &fixtures {
        fs::copy(fixture, photos.join(fixture.file_name().unwrap())).unwrap();
    }
    fs::copy(&fixtures[0], elsewhere.join("other.ARW")).unwrap();
    image::RgbImage::new(32, 16)
        .save(photos.join("rendered.jpg"))
        .unwrap();
    let support = dir.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    let folder = engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap()
        .path;
    engine
        .index_folder(elsewhere.to_string_lossy().into())
        .unwrap();
    let rows: Vec<ImageSummary> = engine
        .list_images(ImageQuery {
            folder: Some(folder.clone()),
            ..Default::default()
        })
        .unwrap()
        .into_iter()
        .filter(|row| !row.path.ends_with(".jpg"))
        .collect();
    assert_eq!(rows.len(), 2, "{rows:?}");
    let (newer, older) = if capture_seconds(&rows[0]) >= capture_seconds(&rows[1]) {
        (rows[0].id.clone(), rows[1].id.clone())
    } else {
        (rows[1].id.clone(), rows[0].id.clone())
    };
    assert_ne!(
        capture_seconds(&rows[0]),
        capture_seconds(&rows[1]),
        "the two fixtures must differ in capture time for the ordering check"
    );

    // Before any build: two RAWs (the JPEG and the sibling folder are not
    // counted), nothing local, nothing stored.
    assert_eq!(coverage(&engine, &folder), (2, 0, 0, 0, 0));
    assert_eq!(engine.smart_preview_storage().unwrap(), 0);
    assert!(engine.pending_sync_smart_previews().unwrap().is_empty());
    assert!(engine.offline_coverage("photos".into()).is_err());

    // Candidates: newest capture first, then a keep decision jumps the queue.
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![newer.clone(), older.clone()]
    );
    engine
        .set_selection(
            older.clone(),
            Selection {
                decision: Decision::Keep,
                grade: None,
                mark: None,
            },
        )
        .unwrap();
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![older.clone(), newer.clone()]
    );
    assert_eq!(
        engine.offline_candidates(folder.clone(), 1).unwrap(),
        vec![older.clone()]
    );
    assert_eq!(coverage(&engine, &folder), (2, 0, 0, 0, 0));

    // A recipe the proxy renderer accepts (what the workflow test sets up).
    let mut initial = recipe(&engine, &older);
    initial.process_version = engine_api::recipe::ProcessVersion {
        family: engine_api::recipe::ProcessFamily::Native,
        revision: 2,
    };
    initial.settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Off;
    engine
        .set_recipe_json(
            older.clone(),
            String::from_utf8(initial.to_json().unwrap()).unwrap(),
        )
        .unwrap();

    // Ensure builds once; a second call returns the same preview untouched.
    let built = engine.ensure_smart_preview(older.clone()).unwrap();
    assert!(matches!(built.state, SmartPreviewState::Ready), "{built:?}");
    assert!(built.width > 0 && built.height > 0);
    let first = pixels_identity(&support, &older);
    let again = engine.ensure_smart_preview(older.clone()).unwrap();
    assert!(matches!(again.state, SmartPreviewState::Ready), "{again:?}");
    assert_eq!((again.width, again.height), (built.width, built.height));
    assert_eq!(pixels_identity(&support, &older), first, "second ensure must not rebuild");

    // Coverage after the build; bytes are the files on disk.
    let on_disk = tree_bytes(&support.join("smart-previews"));
    assert!(on_disk > 0);
    assert_eq!(coverage(&engine, &folder), (2, 1, 0, 0, on_disk));
    assert_eq!(engine.smart_preview_storage().unwrap(), on_disk);
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![newer.clone()]
    );
    assert!(engine.pending_sync_smart_previews().unwrap().is_empty());

    // A journaled save through a Smart Preview develop session is pending sync.
    let session = engine
        .clone()
        .open_smart_preview_develop_session(older.clone())
        .unwrap();
    edit_preview(&session, 0.75);
    drop(session);
    assert_eq!(recipe(&engine, &older).settings.tone.exposure, 0.75);
    assert_eq!(
        engine.pending_sync_smart_previews().unwrap(),
        vec![older.clone()]
    );
    let on_disk = tree_bytes(&support.join("smart-previews"));
    assert_eq!(coverage(&engine, &folder), (2, 0, 1, 0, on_disk));
    assert_eq!(engine.smart_preview_storage().unwrap(), on_disk);
    // Dirty is usable: ensure returns it and does not rebuild.
    let dirty = engine.ensure_smart_preview(older.clone()).unwrap();
    assert!(matches!(dirty.state, SmartPreviewState::Dirty), "{dirty:?}");
    assert!(dirty.dirty);
    assert_eq!(pixels_identity(&support, &older), first);
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![newer.clone()]
    );

    // Synchronize publishes the edit; nothing is pending afterwards.
    let synced = engine.synchronize_smart_preview(older.clone()).unwrap();
    assert!(matches!(synced.state, SmartPreviewState::Ready), "{synced:?}");
    assert!(engine.pending_sync_smart_previews().unwrap().is_empty());
    let on_disk = tree_bytes(&support.join("smart-previews"));
    assert_eq!(coverage(&engine, &folder), (2, 1, 0, 0, on_disk));
    assert_eq!(engine.smart_preview_storage().unwrap(), on_disk);
    assert_eq!(pixels_identity(&support, &older), first);

    // The original changes under a clean preview: it needs attention, it is a
    // candidate again, and ensure rebuilds it in place.
    let built_from: PathBuf = rows
        .iter()
        .find(|row| row.id == older)
        .map(|row| PathBuf::from(&row.path))
        .unwrap();
    assert_eq!(built_from.parent(), Some(Path::new(&folder)));
    fs::OpenOptions::new()
        .append(true)
        .open(&built_from)
        .unwrap()
        .write_all(b"\0")
        .unwrap();
    assert_eq!(coverage(&engine, &folder).3, 1, "size change is stale");
    assert_eq!(coverage(&engine, &folder).1, 0);
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![older.clone(), newer.clone()]
    );
    assert!(matches!(
        engine.smart_preview_info(older.clone()).unwrap().state,
        SmartPreviewState::Stale
    ));
    let rebuilt = engine.ensure_smart_preview(older.clone()).unwrap();
    assert!(matches!(rebuilt.state, SmartPreviewState::Ready), "{rebuilt:?}");
    assert_ne!(pixels_identity(&support, &older), first, "a stale preview is rebuilt");
    let on_disk = tree_bytes(&support.join("smart-previews"));
    assert_eq!(coverage(&engine, &folder), (2, 1, 0, 0, on_disk));
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![newer.clone()]
    );

    // Conflict: a foreign sidecar write after a local edit needs a person; it
    // is neither usable nor a candidate, and ensure reports the refusal.
    let session = engine
        .clone()
        .open_smart_preview_develop_session(older.clone())
        .unwrap();
    edit_preview(&session, 1.25);
    drop(session);
    let sidecars = sidecar::Sidecar::paths(&built_from);
    let mut foreign: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecars.recipe).unwrap()).unwrap();
    foreign["recipe"]["settings"]["tone"]["exposure"] = serde_json::json!(-0.5);
    fs::write(&sidecars.recipe, serde_json::to_vec(&foreign).unwrap()).unwrap();
    assert!(matches!(
        engine.smart_preview_info(older.clone()).unwrap().state,
        SmartPreviewState::Conflict
    ));
    assert_eq!(coverage(&engine, &folder).3, 1);
    assert_eq!(
        engine.pending_sync_smart_previews().unwrap(),
        vec![older.clone()]
    );
    assert_eq!(
        engine.offline_candidates(folder.clone(), 0).unwrap(),
        vec![newer.clone()]
    );
    let refused = engine.ensure_smart_preview(older.clone()).unwrap_err();
    assert!(
        refused.to_string().contains("already exists"),
        "the engine's message propagates: {refused}"
    );
    assert_eq!(recipe(&engine, &older).settings.tone.exposure, 1.25);
}
