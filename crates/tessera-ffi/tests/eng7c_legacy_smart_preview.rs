//! ENG-7c (REV2 N-B1): a Smart Preview generated before ENG-7b for a raw with
//! built-in DNG opcodes, in lens mode None, recorded no built-in correction.
//! It must open as Stale ("regenerate from original"), its offline edits must
//! still sync to the original (sync reads only the journal), and once synced
//! it can be rebuilt. No edit is lost on the way.
//!
//! The legacy container is produced from a real preview of a repo fixture by
//! rewriting its snapshot to the pre-ENG-7b shape for an opcode raw: opcode
//! lists present (OpcodeList1, or OpcodeList3), mode None, source Manual,
//! exactly what a pre-ENG-7b build wrote. Pixels are untouched.
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, mpsc},
    time::Duration,
};
use tessera_ffi::*;

/// The ARW fixture. Absent: a SKIPPED line naming the running test, or a
/// failure under `TESSERA_REQUIRE_RAW_FIXTURES`.
fn fixture() -> Option<PathBuf> {
    test_fixtures::raw::with_extension(&test_fixtures::current_test(), "arw")
}

/// One DNG FixVignetteRadial opcode payload.
fn opcode() -> Vec<u8> {
    let mut b = Vec::new();
    for x in [1_u32, 3, 0x01030000, 0, 56] {
        b.extend(x.to_be_bytes());
    }
    for x in [0.5_f64, 0., 0., 0., 0., 0.5, 0.5] {
        b.extend(x.to_be_bytes());
    }
    b
}

/// Rewrite the container snapshot to the pre-ENG-7b shape and re-sign it.
fn make_legacy(pixels: &Path, list: usize) {
    let bytes = fs::read(pixels).unwrap();
    let n = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut v: serde_json::Value = serde_json::from_slice(&bytes[96..96 + n]).unwrap();
    assert_eq!(v["lens"]["profile"]["kind"], "none");
    assert_eq!(v["correction"]["source"], "Manual");
    let mut lists = serde_json::json!([null, null, null]);
    lists[list] = serde_json::to_value(opcode()).unwrap();
    v["metadata"]["opcode_lists"] = lists;
    v["metadata"]["has_opcode_list"] = serde_json::json!(true);
    let json = serde_json::to_vec(&v).unwrap();
    let mut out = bytes[..96].to_vec();
    out[12..16].copy_from_slice(&(json.len() as u32).to_le_bytes());
    out.extend_from_slice(&json);
    out.extend_from_slice(&bytes[96 + n..]);
    let mut h = blake3::Hasher::new();
    h.update(&out[..64]);
    h.update(&out[96..]);
    out[64..96].copy_from_slice(h.finalize().as_bytes());
    fs::write(pixels, out).unwrap();
}

struct Frames(mpsc::Sender<Result<FrameInfo, String>>);
impl DevelopListener for Frames {
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

fn exposure(engine: &Engine, id: &str) -> f32 {
    let r: engine_api::recipe::Recipe =
        serde_json::from_str(&engine.get_recipe(id.into()).unwrap()).unwrap();
    r.settings.tone.exposure
}

fn run(list: usize, edited: bool) {
    let Some(src) = fixture() else { return };
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    fs::create_dir(&photos).unwrap();
    let raw = photos.join(src.file_name().unwrap());
    fs::copy(&src, &raw).unwrap();
    let support = dir.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    // A Lightroom LensProfileEnable=0 recipe: lens mode None.
    let mut recipe: engine_api::recipe::Recipe =
        serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
    recipe.process_version = engine_api::recipe::ProcessVersion {
        family: engine_api::recipe::ProcessFamily::Native,
        revision: 2,
    };
    recipe
        .edit(engine_api::recipe::EditMeta::user("lens off", 1), |s| {
            s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
            s.denoise.method = engine_api::recipe::settings::DenoiseMethod::Off;
        })
        .unwrap();
    engine
        .set_recipe_json(
            id.clone(),
            String::from_utf8(recipe.to_json().unwrap()).unwrap(),
        )
        .unwrap();
    let info = engine.build_smart_preview(id.clone()).unwrap();
    assert!(matches!(info.state, SmartPreviewState::Ready), "{info:?}");
    if edited {
        let session = engine
            .clone()
            .open_smart_preview_develop_session(id.clone())
            .unwrap();
        let (send, receive) = mpsc::channel();
        session.set_listener(Some(Arc::new(Frames(send))));
        session
            .set_settings(
                serde_json::json!({"tone":{"exposure":0.75}}).to_string(),
                false,
            )
            .unwrap();
        receive
            .recv_timeout(Duration::from_secs(120))
            .expect("proxy render timed out")
            .expect("proxy render failed");
        session.flush().unwrap();
        session.close().unwrap();
        assert!(engine.smart_preview_info(id.clone()).unwrap().dirty);
    }
    let pixels = support.join("smart-previews").join(&id).join("pixels.tsp");
    make_legacy(&pixels, list);

    // Stale, not Failed: the message tells the user what to do.
    let info = engine.smart_preview_info(id.clone()).unwrap();
    assert!(matches!(info.state, SmartPreviewState::Stale), "{info:?}");
    assert!(
        info.message.contains("regenerate from original"),
        "{info:?}"
    );
    assert_eq!(info.dirty, edited);
    if edited {
        assert_eq!(exposure(&engine, &id), 0.75);
        // Unsynced edits are never thrown away by a rebuild or a discard.
        let refused = engine.build_smart_preview(id.clone()).unwrap_err();
        assert!(refused.to_string().contains("synchronize"), "{refused}");
        assert!(engine.discard_smart_preview(id.clone()).is_err());
    }

    // Sync needs only the journal, not the stale pixels.
    let synced = engine.synchronize_smart_preview(id.clone()).unwrap();
    assert!(!synced.dirty, "{synced:?}");
    assert!(
        matches!(synced.state, SmartPreviewState::Stale),
        "{synced:?}"
    );
    let want = if edited { 0.75 } else { 0.0 };
    assert_eq!(exposure(&engine, &id), want);
    let published: sidecar::RecipeDocument =
        serde_json::from_slice(&fs::read(sidecar::Sidecar::paths(&raw).recipe).unwrap()).unwrap();
    assert_eq!(published.recipe.settings.tone.exposure, want);

    // A synced Stale preview is rebuilt in place from the original.
    let rebuilt = engine.build_smart_preview(id.clone()).unwrap();
    assert!(
        matches!(rebuilt.state, SmartPreviewState::Ready),
        "{rebuilt:?}"
    );
    assert_eq!(exposure(&engine, &id), want);
    pipeline_cpu::CameraLinearProxy::decode_persistent(&fs::read(&pixels).unwrap()).unwrap();
}

#[test]
fn legacy_opcode_list1_preview_without_edits_is_stale_and_rebuildable() {
    run(0, false);
}

#[test]
fn legacy_opcode_list1_preview_with_offline_edits_syncs_then_rebuilds() {
    run(0, true);
}

#[test]
fn legacy_opcode_list3_preview_without_edits_is_stale_and_rebuildable() {
    run(2, false);
}

#[test]
fn legacy_opcode_list3_preview_with_offline_edits_syncs_then_rebuilds() {
    run(2, true);
}
