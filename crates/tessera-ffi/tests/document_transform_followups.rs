//! WP B5-12b: transform follow-ups from the on-screen verification of B5-12.
//!
//! * With M5-35 on the base (stack evaluation outside the compositor's
//!   filter-cache lock), a multi-stage stack (warp, perspective, warp and a
//!   menu filter) rasterizes through the parallel tile render without
//!   hanging, also two copies at once. (The one-tile warm-up in
//!   `document/filters.rs::native_stack` stays, as de-duplication: M5-35 lets
//!   concurrent cold misses each evaluate the whole stack.)
//! * A transform preview is scratch-only: it must not announce the layer as
//!   changed (the host would reload its rows and show the uncommitted smart
//!   object and its filter row in Properties / Layers). Cancel and Apply do.
#![cfg(target_os = "macos")]

use compositor::{
    Document,
    document::{DocState, Layer, LayerId, LayerKind},
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn pixel_layer(name: &str, w: u32, h: u32) -> Layer {
    let e = Extent::new(w, h);
    let mut r = Raster::new(e, 4, Depth::U8, 0.0);
    r.edit_region(compositor::Rect::of_extent(e), 1, |x, y, p| {
        let checker = if (x / 7 + y / 5).is_multiple_of(2) {
            0.8
        } else {
            0.2
        };
        *p = [x as f32 / w as f32, checker, y as f32 / h as f32, 1.0];
    })
    .unwrap();
    Layer::new(name, LayerKind::Pixel(r))
}

fn adopt(engine: &Arc<Engine>, w: u32, h: u32, layers: Vec<Layer>) -> Arc<DocumentSession> {
    let mut s = DocState::new(Extent::new(w, h), Depth::U8);
    for (i, mut l) in layers.into_iter().enumerate() {
        l.id = LayerId(i as u64 + 1);
        s.root.push(Arc::new(l));
    }
    s.next_id = s.root.len() as u64 + 1;
    engine.adopt_document(Document::new(s), "transform-followups".into())
}

fn op(operation: Value, kernel: &str) -> String {
    json!({ "version": 1, "operation": operation, "kernel": kernel }).to_string()
}

fn warp_op(w: u32, h: u32, preset: &str, bend: f64) -> String {
    let mesh: Value =
        serde_json::from_str(&warp_preset(w as f64, h as f64, preset.into(), bend).unwrap())
            .unwrap();
    op(json!({ "Warp": mesh }), "Bicubic")
}

fn quad(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn commit(s: &DocumentSession, kind: AdvancedTransformKind, json: String, convert: bool) {
    let t = s.begin_advanced_transform(1, None, kind).unwrap();
    s.preview_advanced_transform(t.token, json, false).unwrap();
    s.commit_advanced_transform(t.token, convert).unwrap();
}

/// Runs `f` on its own thread; fails the test when it takes longer than
/// `limit` (a deadlocked rayon pool never returns).
fn within<T: Send + 'static>(
    limit: Duration,
    what: &str,
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("{what} did not finish within {limit:?} (deadlock?)"))
}

#[test]
fn b512b_multi_stage_stack_rasterizes_in_parallel_without_hanging() {
    let (dir, engine) = engine();
    // Several tiles per level so the parallel tile render has concurrent workers.
    let (w, h) = (1100, 700);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    commit(
        &s,
        AdvancedTransformKind::Warp,
        warp_op(w, h, "Arc", 0.3),
        true,
    );
    let (fw, fh) = (w as f64, h as f64);
    commit(
        &s,
        AdvancedTransformKind::Perspective,
        op(
            json!({ "Perspective": {
                "source_quads": [quad(0.0, 0.0, fw, fh)],
                "destination_quads": [json!([[30.0, 20.0], [fw - 10.0, 0.0], [fw, fh], [0.0, fh - 30.0]])],
            } }),
            "Bilinear",
        ),
        false,
    );
    commit(
        &s,
        AdvancedTransformKind::Warp,
        warp_op(w, h, "Wave", -0.2),
        false,
    );
    s.apply_filter(1, r#"{"id":"gaussian_blur","params":{"radius":2}}"#.into())
        .unwrap();
    let names: Vec<String> = s
        .smart_filters(1)
        .unwrap()
        .into_iter()
        .map(|f| f.name)
        .collect();
    assert_eq!(names, ["Warp", "Perspective Warp", "Warp", "Gaussian Blur"]);

    // Twice in parallel (two copies at once), then once more: every stack
    // evaluation starts cold inside the rayon tile render (no pre-warmed cache).
    let copies: Vec<_> = (0..3)
        .map(|i| dir.path().join(format!("copy-{i}.psd")))
        .collect();
    let (a, b) = (s.clone(), s.clone());
    let (pa, pb) = (copies[0].clone(), copies[1].clone());
    let (ra, rb) = within(
        Duration::from_secs(240),
        "two parallel rasterized copies",
        move || {
            let ta = std::thread::spawn(move || {
                a.save_psd_rasterizing_transforms(pa.to_string_lossy().into_owned())
            });
            let rb = b.save_psd_rasterizing_transforms(pb.to_string_lossy().into_owned());
            (ta.join().unwrap(), rb)
        },
    );
    ra.unwrap();
    rb.unwrap();
    let c = s.clone();
    let pc = copies[2].clone();
    within(Duration::from_secs(240), "rasterized copy", move || {
        c.save_psd_rasterizing_transforms(pc.to_string_lossy().into_owned())
    })
    .unwrap();
    for p in &copies[..3] {
        assert!(p.exists(), "{p:?}");
    }
    // The copy is the whole stack rasterized.
    let reopened = engine
        .clone()
        .open_document(copies[2].to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(
        reopened.layers().unwrap()[0].kind,
        DocLayerKind::Pixel,
        "the stack is rasterized"
    );
}

#[derive(Default)]
struct Rows {
    layers: Mutex<Vec<Vec<u64>>>,
}

impl DocumentListener for Rows {
    fn on_frame(&self, _frame: DocFrameInfo) {}
    fn on_layers_changed(&self, layer_ids: Vec<u64>) {
        self.layers.lock().unwrap().push(layer_ids);
    }
    fn on_history_changed(&self, _head: u64) {}
    fn on_render_failed(&self, _message: String) {}
}

#[test]
fn b512b_previews_never_announce_rows_until_cancel_or_apply() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    let rows = Arc::new(Rows::default());
    s.set_listener(Some(rows.clone()));
    s.wait_idle();
    rows.layers.lock().unwrap().clear();

    // Pixel layer: the preview wraps it in the scratch only.
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    for bend in [0.2, 0.4] {
        let p = s
            .preview_advanced_transform(t.token, warp_op(w, h, "Arc", bend), false)
            .unwrap();
        assert!(
            p.update.layers_changed.is_empty(),
            "a preview is not a row change: {:?}",
            p.update.layers_changed
        );
        assert!(p.update.dirty_rect.is_some(), "the preview still repaints");
    }
    s.wait_idle();
    assert!(
        rows.layers.lock().unwrap().iter().all(|l| l.is_empty()),
        "no row reload during a preview: {:?}",
        rows.layers.lock().unwrap()
    );
    // Cancel: the rows reload (to the untouched pixel layer).
    let u = s.cancel_advanced_transform(t.token).unwrap();
    assert_eq!(u.layers_changed, [1]);
    s.wait_idle();
    assert!(rows.layers.lock().unwrap().iter().any(|l| l.contains(&1)));
    assert_eq!(s.layer(1).unwrap().kind, DocLayerKind::Pixel);

    // Apply with consent: one row change (the smart object with its stage).
    rows.layers.lock().unwrap().clear();
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    let p = s
        .preview_advanced_transform(t.token, warp_op(w, h, "Flag", 0.3), false)
        .unwrap();
    assert!(p.update.layers_changed.is_empty());
    let e = s.commit_advanced_transform(t.token, false).unwrap_err();
    assert!(e.to_string().contains("confirm"), "{e}");
    let u = s.commit_advanced_transform(t.token, true).unwrap();
    assert_eq!(u.layers_changed, [1]);
    s.wait_idle();
    assert!(rows.layers.lock().unwrap().iter().any(|l| l.contains(&1)));
    assert_eq!(s.layer(1).unwrap().kind, DocLayerKind::SmartObject);

    // Re-edit of the stage: previews still announce nothing.
    rows.layers.lock().unwrap().clear();
    let t = s
        .begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
        .unwrap();
    let p = s
        .preview_advanced_transform(t.token, warp_op(w, h, "Bulge", 0.5), false)
        .unwrap();
    assert!(p.update.layers_changed.is_empty());
    s.wait_idle();
    assert!(rows.layers.lock().unwrap().iter().all(|l| l.is_empty()));
    s.cancel_advanced_transform(t.token).unwrap();
}
