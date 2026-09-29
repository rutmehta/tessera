//! Remove tool, Content-Aware Fill, distraction review and neural filters
//! for the app (WP B5-09): masks built engine-side from the selection or a
//! stroke, missing-weight errors, stale ids, cancellation and one history
//! node per apply. CPU PatchMatch only; no weights are installed here.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// Opaque texture with a dark 1 px horizontal line (a "wire") at y = 20.
fn png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        if y == 20 && (6..w - 6).contains(&x) {
            return image::Rgba([10, 10, 10, 255]);
        }
        let v = 150 + ((x * 7 + y * 13) % 17) as u8;
        image::Rgba([v, v.saturating_sub(20), 120 + (x % 9) as u8 * 3, 255])
    });
    let path = dir.join(name);
    img.save(&path).unwrap();
    path
}

fn open(engine: &Arc<Engine>, path: &Path) -> Arc<DocumentSession> {
    engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap()
}

fn pixels(s: &DocumentSession) -> Vec<f32> {
    s.wait_idle();
    s.read_level(0).unwrap().2
}

fn history(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn head_label(s: &DocumentSession) -> String {
    s.history_items()
        .unwrap()
        .into_iter()
        .find(|i| i.is_current)
        .unwrap()
        .label
}

#[test]
fn patchmatch_remove_from_selection_is_deterministic_and_one_undo_step() {
    let (dir, engine) = engine();
    let path = png(dir.path(), "a.png", 48, 40);
    let run = || {
        let s = open(&engine, &path);
        let layer = s.layers().unwrap()[0].id;
        s.set_selection_rect(18, 16, 10, 9, 0.0).unwrap();
        let before = pixels(&s);
        let n = history(&s);
        let r = s
            .remove_with_selection(layer, RemoveBackend::PatchMatch, r#"{"dilation":0}"#.into())
            .unwrap();
        assert_eq!(r.backend, "PatchMatch");
        assert_eq!(r.note, None);
        assert_eq!(history(&s), n + 1);
        assert_eq!(head_label(&s), "Remove");
        let after = pixels(&s);
        assert_ne!(before, after, "the selection was filled");
        // Outside the selection nothing changed.
        for y in 0..40usize {
            for x in 0..48usize {
                if !(18..28).contains(&x) || !(16..25).contains(&y) {
                    let i = (y * 48 + x) * 4;
                    assert_eq!(before[i..i + 4], after[i..i + 4], "({x},{y})");
                }
            }
        }
        s.undo().unwrap();
        assert_eq!(pixels(&s), before, "one undo step restores the layer");
        s.redo().unwrap();
        let redone = pixels(&s);
        s.close();
        redone
    };
    assert_eq!(run(), run());
}

#[test]
fn stroke_mask_equals_equivalent_selection_and_removes_the_same() {
    let (dir, engine) = engine();
    let path = png(dir.path(), "b.png", 40, 40);
    // A single dab of diameter 9 at (20.5, 20.5) and the same circle as a
    // hard elliptical marquee.
    let a = open(&engine, &path);
    let layer = a.layers().unwrap()[0].id;
    a.begin_remove_stroke(layer, 9.0, RemoveBackend::PatchMatch)
        .unwrap();
    let dirty = a
        .remove_stroke_points(vec![ToolPoint { x: 20.5, y: 20.5 }])
        .unwrap()
        .unwrap();
    assert!(dirty.width > 0 && dirty.height > 0);
    let stroke = a.remove_stroke_mask().unwrap();
    let b = open(&engine, &png(dir.path(), "b2.png", 40, 40));
    b.select_marquee(
        MarqueeShape::Ellipse,
        16.0,
        16.0,
        9.0,
        9.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    let st = b.document_state().unwrap();
    let sel = st.selection.as_ref().unwrap();
    let mut buf = Vec::new();
    let mut selection = vec![0.0f32; 1600];
    sel.read_tile(0, 0, &mut buf).unwrap();
    let lay = sel.layout(0, 0);
    for y in 0..40 {
        for x in 0..40 {
            selection[y * 40 + x] = buf[y * lay.stride() + x];
        }
    }
    assert_eq!(stroke, selection, "stroke mask equals the selection");
    assert!(stroke.iter().filter(|&&v| v == 1.0).count() > 40);

    let na = history(&a);
    let ra = a.end_remove_stroke(r#"{"dilation":0}"#.into()).unwrap();
    assert_eq!(ra.backend, "PatchMatch");
    assert_eq!(history(&a), na + 1);
    let lb = b.layers().unwrap()[0].id;
    b.remove_with_selection(lb, RemoveBackend::PatchMatch, r#"{"dilation":0}"#.into())
        .unwrap();
    assert_eq!(pixels(&a), pixels(&b), "same mask, same removal");
    assert!(a.remove_stroke_mask().is_none(), "the stroke is closed");
}

#[test]
fn auto_without_lama_uses_patchmatch_and_says_so() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "c.png", 32, 32));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(10, 10, 6, 6, 0.0).unwrap();
    let r = s
        .remove_with_selection(layer, RemoveBackend::Auto, "{}".into())
        .unwrap();
    assert_eq!(r.backend, "PatchMatch");
    assert!(r.note.unwrap().contains("remove/lama"));
    let models = s.retouch_models().unwrap();
    let lama = models.iter().find(|m| m.model_id == "remove/lama").unwrap();
    assert!(!lama.installed);
    assert!(lama.source_url.starts_with("https://"));
    assert!(lama.cache_path.ends_with(".onnx"));
}

#[test]
fn missing_lama_and_neural_weights_give_the_documented_error() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "d.png", 32, 32));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(10, 10, 6, 6, 0.0).unwrap();
    let before = pixels(&s);
    let n = history(&s);
    let e = s
        .remove_with_selection(layer, RemoveBackend::Lama, "{}".into())
        .unwrap_err()
        .to_string();
    assert!(e.contains("remove/lama"), "{e}");
    assert!(e.contains("never downloads"), "{e}");
    assert!(e.contains("huggingface.co"), "{e}");
    for (kind, id) in [
        (NeuralFilterKind::Colorize, "filters/ddcolor"),
        (
            NeuralFilterKind::JpegArtifactRemoval,
            "enhance/drunet-color",
        ),
    ] {
        for dest in [
            NeuralDestination::CurrentLayer,
            NeuralDestination::NewLayer,
            NeuralDestination::SmartFilter,
        ] {
            let e = s
                .neural_filter(layer, kind, "{}".into(), dest)
                .unwrap_err()
                .to_string();
            assert!(e.contains(id), "{e}");
            assert!(e.contains("not installed"), "{e}");
        }
    }
    // No face boxes, no detector weights, no selection.
    s.clear_selection().unwrap();
    let n2 = history(&s);
    let e = s
        .neural_filter(
            layer,
            NeuralFilterKind::SkinSmoothing,
            "{}".into(),
            NeuralDestination::CurrentLayer,
        )
        .unwrap_err()
        .to_string();
    assert!(e.contains("opencv/yunet"), "{e}");
    assert_eq!(history(&s), n2);
    assert_eq!(n2, n + 1, "only the Deselect node was added");
    assert_eq!(pixels(&s), before);
    // The catalogue matches M5-29's registration.
    let cat = neural_filters();
    assert_eq!(cat.len(), 4);
    assert!(!cat[0].requires_weights && cat[1..].iter().all(|f| f.requires_weights));
    assert_eq!(cat[2].params[0].key, "strength");
}

#[test]
fn stale_document_and_layer_ids_error_cleanly() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "e.png", 24, 24));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(4, 4, 6, 6, 0.0).unwrap();
    let e = s
        .remove_with_selection(9999, RemoveBackend::PatchMatch, "{}".into())
        .unwrap_err()
        .to_string();
    assert!(e.contains("9999") || e.contains("not found"), "{e}");
    assert!(
        s.begin_remove_stroke(9999, 10.0, RemoveBackend::PatchMatch)
            .is_err()
    );
    assert!(
        s.remove_stroke_points(vec![ToolPoint { x: 1.0, y: 1.0 }])
            .is_err()
    );
    assert!(s.end_remove_stroke("{}".into()).is_err());
    assert!(
        s.remove_distraction_suggestions(layer, vec![0], RemoveBackend::PatchMatch, "{}".into())
            .is_err()
    );
    assert!(
        s.neural_filter(
            9999,
            NeuralFilterKind::SkinSmoothing,
            r#"{"faces":[[1,1,4,4]]}"#.into(),
            NeuralDestination::NewLayer
        )
        .is_err()
    );
    assert!(s.content_aware_fill_selection(9999, "{}".into()).is_err());
    s.close();
    let e = s
        .content_aware_fill_selection(layer, "{}".into())
        .unwrap_err()
        .to_string();
    assert!(e.contains("closed"), "{e}");
}

#[test]
fn cancellation_leaves_history_unchanged() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "f.png", 640, 480));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(100, 80, 360, 300, 0.0).unwrap();
    let before = pixels(&s);
    let n = history(&s);
    let run = |f: Box<dyn FnOnce() -> Result<RetouchResult, BridgeError> + Send>| {
        let done = Arc::new(AtomicBool::new(false));
        let d = done.clone();
        let t = std::thread::spawn(move || {
            let r = f();
            d.store(true, Ordering::SeqCst);
            r
        });
        while !done.load(Ordering::SeqCst) {
            s.cancel_retouch();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        t.join().unwrap()
    };
    let s1 = s.clone();
    let e = run(Box::new(move || {
        s1.remove_with_selection(layer, RemoveBackend::PatchMatch, "{}".into())
    }))
    .unwrap_err()
    .to_string();
    assert!(e.contains("cancelled"), "{e}");
    let s2 = s.clone();
    let e = run(Box::new(move || {
        s2.neural_filter(
            layer,
            NeuralFilterKind::SkinSmoothing,
            r#"{"faces":[[100,80,360,300]],"blur":16}"#.into(),
            NeuralDestination::NewLayer,
        )
    }))
    .unwrap_err()
    .to_string();
    assert!(e.contains("cancelled"), "{e}");
    assert_eq!(history(&s), n);
    assert_eq!(s.layers().unwrap().len(), 1);
    assert_eq!(pixels(&s), before);
    // A later apply is not affected by the earlier cancel.
    s.content_aware_fill_selection(layer, "{}".into()).unwrap();
    assert_eq!(history(&s), n + 1);
}

#[test]
fn content_aware_fill_and_neural_destinations_are_one_node_each() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "g.png", 48, 40));
    let layer = s.layers().unwrap()[0].id;
    let original = pixels(&s);

    // Edit ▸ Content-Aware Fill.
    assert!(
        s.content_aware_fill_selection(layer, "{}".into()).is_err(),
        "needs a selection"
    );
    s.set_selection_rect(20, 10, 8, 8, 0.0).unwrap();
    let n = history(&s);
    let r = s.content_aware_fill_selection(layer, "{}".into()).unwrap();
    assert_eq!(r.backend, "Content-Aware Fill");
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Content-Aware Fill");
    s.undo().unwrap();

    // Skin Smoothing: faces from the selection's bounds, to a new layer.
    let n = history(&s);
    let r = s
        .neural_filter(
            layer,
            NeuralFilterKind::SkinSmoothing,
            r#"{"blur":8,"smoothness":1}"#.into(),
            NeuralDestination::NewLayer,
        )
        .unwrap();
    assert!(r.note.unwrap().contains("selection"));
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Skin Smoothing");
    let layers = s.layers().unwrap();
    assert_eq!(layers.len(), 2);
    assert!(layers.iter().any(|l| l.name.contains("Skin Smoothing")));
    s.undo().unwrap();
    assert_eq!(s.layers().unwrap().len(), 1);
    s.clear_selection().unwrap();

    // To the current layer, explicit faces.
    let n = history(&s);
    s.neural_filter(
        layer,
        NeuralFilterKind::SkinSmoothing,
        r#"{"faces":[[10,10,20,20]],"blur":8,"smoothness":1}"#.into(),
        NeuralDestination::CurrentLayer,
    )
    .unwrap();
    assert_eq!(history(&s), n + 1);
    assert_ne!(pixels(&s), original);
    s.undo().unwrap();

    // As a smart filter: the pixel layer becomes a smart object in the same node.
    let n = history(&s);
    s.neural_filter(
        layer,
        NeuralFilterKind::SkinSmoothing,
        r#"{"faces":[[10,10,20,20]],"blur":8,"smoothness":1}"#.into(),
        NeuralDestination::SmartFilter,
    )
    .unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Skin Smoothing");
    assert_eq!(s.layers().unwrap()[0].kind, DocLayerKind::SmartObject);
    let sf = s.smart_filters(layer).unwrap();
    assert_eq!(sf.len(), 1);
    assert_eq!(sf[0].filter_id, "neural/skin_smoothing");
    // Re-edit from the smart filter row.
    s.set_smart_filter(
        layer,
        0,
        SmartFilterEdit::Params {
            filter_json: r#"{"id":"neural/skin_smoothing","params":{"faces":[[10,10,20,20]],"blur":2,"smoothness":0.5}}"#.into(),
        },
    )
    .unwrap();
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.layers().unwrap()[0].kind, DocLayerKind::Pixel);
    assert_eq!(pixels(&s), original);
}

#[test]
fn distraction_suggestions_are_reviewed_then_only_accepted_ones_removed() {
    let (dir, engine) = engine();
    let s = open(&engine, &png(dir.path(), "h.png", 48, 40));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let scan = s
        .detect_distractions(layer, r#"{"faces":[[2,30,4,4]]}"#.into())
        .unwrap();
    assert_eq!(history(&s), n, "scanning adds no history");
    assert!(scan.limitation.contains("Not semantic"));
    assert_eq!(scan.faces, "supplied");
    let wire = scan
        .suggestions
        .iter()
        .find(|g| g.kind == DistractionKind::Wire)
        .expect("the line is a wire suggestion");
    assert!(
        wire.bounds.width > 20 && wire.bounds.y <= 20 && wire.bounds.y + wire.bounds.height > 20
    );
    let face = scan
        .suggestions
        .iter()
        .find(|g| g.kind == DistractionKind::FaceBox)
        .expect("the face box is a suggestion");
    // Only the wire is accepted: the face box area stays as it was.
    s.remove_distraction_suggestions(
        layer,
        vec![wire.id],
        RemoveBackend::PatchMatch,
        r#"{"dilation":1}"#.into(),
    )
    .unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Remove Distractions");
    let after = pixels(&s);
    let i = (20 * 48 + 24) * 4;
    assert!(after[i] > before[i] + 0.2, "the wire pixel was filled");
    let f = &face.bounds;
    for y in f.y..f.y + f.height {
        for x in f.x..f.x + f.width {
            let j = ((y * 48 + x) * 4) as usize;
            if !(18..=22).contains(&y) {
                assert_eq!(before[j..j + 4], after[j..j + 4]);
            }
        }
    }
    // The suggestions are consumed; a stale apply errors.
    assert!(
        s.remove_distraction_suggestions(
            layer,
            vec![wire.id],
            RemoveBackend::PatchMatch,
            "{}".into()
        )
        .is_err()
    );
}

/// B5-09b: the app downloads retouch weights with `ModelDownloads` into
/// `<support>/models/cache` (the manifest at `<support>/models/models.toml`).
/// Retouching must report and read exactly that file: same pinned version,
/// same path, and a file placed there is the one verified (a corrupt one is
/// an integrity error, never "not installed" nor a silent fallback).
#[test]
fn retouch_models_are_looked_up_where_model_downloads_put_them() {
    let (dir, engine) = engine();
    let support = dir.path().join("support");
    let s = open(&engine, &png(dir.path(), "m.png", 32, 32));
    let layer = s.layers().unwrap()[0].id;
    let models = s.retouch_models().unwrap();
    // The downloader's registry: the app's manifest and cache.
    let reg = ml_runtime::ModelRegistry::open(
        support.join("models").join("models.toml"),
        support.join("models").join("cache"),
    )
    .unwrap();
    for id in ["remove/lama", "filters/ddcolor", "enhance/drunet-color"] {
        let m = models.iter().find(|m| m.model_id == id).unwrap();
        let spec = reg.models().iter().find(|s| s.id == id).unwrap();
        assert_eq!(
            m.version, spec.version,
            "{id}: the version ModelDownloads is asked for"
        );
        assert!(!m.installed, "{id}");
        let expected = support
            .join("models")
            .join("cache")
            .join(format!("{}.onnx", spec.sha256));
        assert_eq!(Path::new(&m.cache_path), expected, "{id}");
        // With downloads off the downloader reports it missing from that cache.
        let r = engine_api::id::ModelRef {
            id: id.into(),
            version: m.version.clone(),
        };
        let e = reg.download(&r, false, |_, _| {}).unwrap_err().to_string();
        assert!(e.contains("not cached"), "{e}");
    }
    // A file where the downloader would put LaMa is the one retouching reads.
    let lama = models.iter().find(|m| m.model_id == "remove/lama").unwrap();
    std::fs::write(&lama.cache_path, b"not a model").unwrap();
    s.set_selection_rect(10, 10, 6, 6, 0.0).unwrap();
    let n = history(&s);
    let e = s
        .remove_with_selection(layer, RemoveBackend::Lama, "{}".into())
        .unwrap_err()
        .to_string();
    assert!(e.contains("SHA-256 mismatch"), "{e}");
    assert!(e.contains(&lama.cache_path), "{e}");
    assert_eq!(history(&s), n);
    let r = engine_api::id::ModelRef {
        id: "remove/lama".into(),
        version: lama.version.clone(),
    };
    let e = reg.download(&r, false, |_, _| {}).unwrap_err().to_string();
    assert!(e.contains("SHA-256 mismatch"), "{e}");
    s.close();
}
