//! Opt-in public-API acceptance on a disposable COPY of a real mosaic RAW.
//! Run only in the serialized native lane:
//! TESSERA_SMART_PREVIEW_RAW=/read-only/source.ARW cargo test -p tessera-ffi \
//!   --test smart_preview_workflow --release -- --ignored --nocapture
use std::{fs, io::Read, path::Path};
use tessera_ffi::*;

fn digest(path: &Path) -> blake3::Hash {
    let mut file = fs::File::open(path).unwrap();
    let mut hasher = blake3::Hasher::new();
    let mut buffer = [0; 65536];
    loop {
        let count = file.read(&mut buffer).unwrap();
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    hasher.finalize()
}
fn recipe(engine: &Engine, id: &str) -> engine_api::recipe::Recipe {
    serde_json::from_str(&engine.get_recipe(id.into()).unwrap()).unwrap()
}
fn export(engine: &Engine, id: &str, destination: &Path, format: &str) -> ExportReport {
    engine.export_batch(ExportTarget::Images{image_ids:vec![id.into()]},serde_json::json!({"format":format,"destination":destination.to_string_lossy(),"naming":"{name}","resize":{"mode":"none"}}).to_string(),None,None).unwrap()
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
fn edit_preview(session: &std::sync::Arc<DevelopSession>, exposure: f32) {
    let (send, receive) = std::sync::mpsc::channel();
    session.set_listener(Some(std::sync::Arc::new(RenderResult(send))));
    session
        .set_settings(
            serde_json::json!({"tone":{"exposure":exposure}}).to_string(),
            false,
        )
        .unwrap();
    let frame = receive
        .recv_timeout(std::time::Duration::from_secs(60))
        .expect("proxy render timed out")
        .expect("proxy render failed");
    assert!(frame.width > 0 && frame.height > 0 && frame.render_ms.is_finite());
    assert!(
        session
            .get_histogram()
            .unwrap()
            .luminance
            .iter()
            .map(|value| u64::from(*value))
            .sum::<u64>()
            > 0
    );
    session.flush().unwrap();
    session.close().unwrap();
}

#[test]
#[ignore = "requires TESSERA_SMART_PREVIEW_RAW and the exclusive native test lane"]
fn public_engine_offline_restart_sync_original_export_and_conflict() {
    let fixture =
        std::env::var_os("TESSERA_SMART_PREVIEW_RAW").expect("set read-only real RAW fixture path");
    let fixture = Path::new(&fixture);
    let fixture_before = digest(fixture);
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    fs::create_dir(&photos).unwrap();
    let filename = fixture.file_name().unwrap();
    let original = photos.join(filename);
    fs::copy(fixture, &original).unwrap();
    let source_before = digest(&original);
    assert_eq!(source_before, fixture_before);
    let support = dir.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let images = engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(images.len(), 1);
    let id = images[0].id.clone();
    let mut initial = recipe(&engine, &id);
    initial.process_version = engine_api::recipe::ProcessVersion {
        family: engine_api::recipe::ProcessFamily::Native,
        revision: 2,
    };
    initial.settings.tone.exposure = 0.25;
    initial.settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Off;
    initial
        .history
        .record(
            &initial.history.base.clone(),
            &initial.settings,
            engine_api::recipe::EditMeta::user("initial offline baseline", 1),
        )
        .unwrap();
    initial.unknown.insert(
        "future_test_value".into(),
        serde_json::json!({"exact":["keep",17]}),
    );
    engine
        .set_recipe_json(
            id.clone(),
            String::from_utf8(initial.to_json().unwrap()).unwrap(),
        )
        .unwrap();
    let sidecars = sidecar::Sidecar::paths(&original);
    let mut envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecars.recipe).unwrap()).unwrap();
    envelope["future_envelope"] = serde_json::json!({"opaque":true});
    fs::write(&sidecars.recipe, serde_json::to_vec(&envelope).unwrap()).unwrap();
    let initial_recipe = fs::read(&sidecars.recipe).unwrap();
    let initial_xmp = fs::read(&sidecars.xmp).unwrap();
    let original_metadata = raw_decode::RawSource::open(&original).unwrap().metadata();
    let [_, _, mut expected_width, mut expected_height] = original_metadata.default_crop;
    if original_metadata.orientation >= 5 {
        std::mem::swap(&mut expected_width, &mut expected_height);
    }
    let baseline_render = export(&engine, &id, &dir.path().join("baseline-full-jpeg"), "jpeg");
    assert_eq!(
        (baseline_render.exported, baseline_render.failed),
        (1, 0),
        "{baseline_render:?}"
    );
    let baseline_path = Path::new(baseline_render.items[0].output_path.as_ref().unwrap());
    assert_eq!(
        image::image_dimensions(baseline_path).unwrap(),
        (expected_width, expected_height)
    );
    let baseline_pixels = image::open(baseline_path)
        .unwrap()
        .resize(64, 64, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let info = engine.build_smart_preview(id.clone()).unwrap();
    assert!(matches!(info.state, SmartPreviewState::Ready));
    assert!(info.width.max(info.height) <= 2560);
    assert_eq!(fs::read(&sidecars.recipe).unwrap(), initial_recipe);
    assert_eq!(fs::read(&sidecars.xmp).unwrap(), initial_xmp);
    assert_eq!(digest(&original), source_before);
    let clean_recipe = engine.get_recipe(id.clone()).unwrap();
    drop(engine);
    let clean_offline = dir.path().join("clean-offline-photo-copy");
    fs::rename(&photos, &clean_offline).unwrap();
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    let clean_info = engine.smart_preview_info(id.clone()).unwrap();
    assert!(matches!(
        clean_info.state,
        SmartPreviewState::OriginalOffline
    ));
    assert!(!clean_info.dirty);
    assert_eq!(engine.get_recipe(id.clone()).unwrap(), clean_recipe);
    assert_eq!(recipe(&engine, &id).settings.tone.exposure, 0.25);
    let clean_session = engine
        .clone()
        .open_smart_preview_develop_session(id.clone())
        .unwrap();
    assert_eq!(clean_session.info().image_id, id);
    clean_session.close().unwrap();
    drop(clean_session);
    assert!(!photos.exists());
    fs::rename(&clean_offline, &photos).unwrap();
    let cull = engine
        .open_cull_session(photos.to_string_lossy().into())
        .unwrap();
    let session = engine
        .clone()
        .open_smart_preview_develop_session(id.clone())
        .unwrap();
    assert_eq!(session.info().image_id, id);
    assert!(engine.clone().open_develop_session(id.clone()).is_err());
    assert!(
        engine
            .set_recipe_json(id.clone(), engine.get_recipe(id.clone()).unwrap())
            .is_err()
    );
    assert!(cull.grade_images(vec![id.clone()], 4).is_err());
    edit_preview(&session, 0.75);
    drop(session);
    drop(cull);
    assert_eq!(recipe(&engine, &id).settings.tone.exposure, 0.75);
    assert!(engine.clone().open_develop_session(id.clone()).is_err());
    assert!(engine.discard_smart_preview(id.clone()).is_err());
    assert_eq!(fs::read(&sidecars.recipe).unwrap(), initial_recipe);
    assert_eq!(digest(&original), source_before);
    drop(engine);
    // Only the disposable COPY directory moves; the source fixture is never renamed.
    let offline = dir.path().join("offline-photo-copy");
    fs::rename(&photos, &offline).unwrap();
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    let info = engine.smart_preview_info(id.clone()).unwrap();
    assert!(info.dirty);
    assert!(!info.original_available);
    let session = engine
        .clone()
        .open_smart_preview_develop_session(id.clone())
        .unwrap();
    edit_preview(&session, 1.5);
    drop(session);
    assert!(
        !photos.exists(),
        "offline save must not recreate the original folder"
    );
    assert_eq!(recipe(&engine, &id).settings.tone.exposure, 1.5);
    let blocked = export(
        &engine,
        &id,
        &dir.path().join("blocked-original"),
        "original",
    );
    assert_eq!((blocked.exported, blocked.failed), (0, 1));
    let blocked = export(&engine, &id, &dir.path().join("blocked-jpeg"), "jpeg");
    assert_eq!((blocked.exported, blocked.failed), (0, 1));
    drop(engine);
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    assert_eq!(recipe(&engine, &id).settings.tone.exposure, 1.5);
    fs::rename(&offline, &photos).unwrap();
    let synchronized = engine.synchronize_smart_preview(id.clone()).unwrap();
    assert!(!synchronized.dirty);
    assert!(matches!(synchronized.state, SmartPreviewState::Ready));
    let published: sidecar::RecipeDocument =
        serde_json::from_slice(&fs::read(&sidecars.recipe).unwrap()).unwrap();
    assert_eq!(published.recipe.settings.tone.exposure, 1.5);
    let envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecars.recipe).unwrap()).unwrap();
    assert_eq!(envelope["future_envelope"]["opaque"], true);
    let exported = export(
        &engine,
        &id,
        &dir.path().join("full-quality-original"),
        "original",
    );
    assert_eq!((exported.exported, exported.failed), (1, 0), "{exported:?}");
    assert_eq!(
        digest(Path::new(exported.items[0].output_path.as_ref().unwrap())),
        source_before
    );
    let rendered = export(&engine, &id, &dir.path().join("edited-full-jpeg"), "jpeg");
    assert_eq!((rendered.exported, rendered.failed), (1, 0), "{rendered:?}");
    let rendered_path = Path::new(rendered.items[0].output_path.as_ref().unwrap());
    assert_eq!(
        image::image_dimensions(rendered_path).unwrap(),
        (expected_width, expected_height)
    );
    let edited_pixels = image::open(rendered_path)
        .unwrap()
        .resize(64, 64, image::imageops::FilterType::Triangle)
        .to_rgb8();
    assert_ne!(
        edited_pixels.as_raw(),
        baseline_pixels.as_raw(),
        "synchronized edit must change exported pixels"
    );
    let brightness = |pixels: &image::RgbImage| {
        pixels
            .as_raw()
            .iter()
            .map(|value| u64::from(*value))
            .sum::<u64>()
    };
    assert!(
        brightness(&edited_pixels) > brightness(&baseline_pixels),
        "positive exposure must brighten rendered original export"
    );
    eprintln!(
        "full-quality JPEG export verified at {expected_width}x{expected_height}, synchronized exposure changed image pixels"
    );
    fs::rename(&photos, &offline).unwrap();
    let missing = export(
        &engine,
        &id,
        &dir.path().join("missing-original"),
        "original",
    );
    assert_eq!((missing.exported, missing.failed), (0, 1));
    assert!(
        missing.items[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("original unavailable")
    );
    fs::rename(&offline, &photos).unwrap();
    let original_session = engine.clone().open_develop_session(id.clone()).unwrap();
    original_session.close().unwrap();
    drop(original_session);
    // A second offline/local edit cannot overwrite external sidecar work on reconnect.
    let session = engine
        .clone()
        .open_smart_preview_develop_session(id.clone())
        .unwrap();
    edit_preview(&session, 2.0);
    drop(session);
    let mut foreign: serde_json::Value =
        serde_json::from_slice(&fs::read(&sidecars.recipe).unwrap()).unwrap();
    foreign["recipe"]["settings"]["tone"]["exposure"] = serde_json::json!(-0.5);
    let foreign = serde_json::to_vec(&foreign).unwrap();
    fs::write(&sidecars.recipe, &foreign).unwrap();
    assert!(
        engine
            .synchronize_smart_preview(id.clone())
            .unwrap_err()
            .to_string()
            .contains("conflict")
    );
    assert_eq!(fs::read(&sidecars.recipe).unwrap(), foreign);
    assert_eq!(recipe(&engine, &id).settings.tone.exposure, 2.0);
    assert!(engine.discard_smart_preview(id.clone()).is_err());
    assert!(matches!(
        engine.smart_preview_info(id).unwrap().state,
        SmartPreviewState::Conflict
    ));
    assert_eq!(digest(&original), source_before);
    assert_eq!(digest(fixture), fixture_before);
    eprintln!("public Smart Preview workflow source hash unchanged: {fixture_before}");
}
