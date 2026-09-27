//! WP B5-14: the document render path. Viewport-only composition (P13):
//! the rendered region is the viewport plus the documented halo, and pixels
//! equal the full-level render for pans, resizes and incremental edits.
//! Snapshot rendering (P14): edits never wait for a frame; stale frames are
//! dropped; save/render/undo races keep history and surfaces valid.
//! Resource policy (P17): frames register interactive pressure.
#![cfg(target_os = "macos")]

use compositor::{
    Adjustment, BlendMode, DocOp, DocState, Document, GroupMode, Layer, LayerId, geom::Rect,
    raster::Depth,
};
use engine_api::tile::{Extent, Tile, TileCoord};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tessera_ffi::surface::{Surface, testing::create_rgba8};
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

#[derive(Default)]
struct Recorder {
    frames: Mutex<Vec<DocFrameInfo>>,
    failures: Mutex<Vec<String>>,
}

impl DocumentListener for Recorder {
    fn on_frame(&self, frame: DocFrameInfo) {
        self.frames.lock().unwrap().push(frame);
    }
    fn on_layers_changed(&self, _: Vec<u64>) {}
    fn on_history_changed(&self, _: u64) {}
    fn on_render_failed(&self, message: String) {
        self.failures.lock().unwrap().push(message);
    }
}

impl Recorder {
    fn last(&self) -> DocFrameInfo {
        self.frames
            .lock()
            .unwrap()
            .last()
            .cloned()
            .expect("a frame")
    }
    fn ok(&self) {
        let f = self.failures.lock().unwrap();
        assert!(f.is_empty(), "render failures: {f:?}");
    }
}

fn content(seed: u32, coord: TileCoord, layout: engine_api::tile::TileLayout) -> Tile {
    let n = layout.plane_len();
    let mut v = vec![0u8; 4 * n];
    let mut x = seed.wrapping_mul(2654435761) ^ (coord.x << 16) ^ coord.y ^ 0x9e37;
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v[i] = x as u8;
        v[n + i] = (x >> 8) as u8;
        v[2 * n + i] = (x >> 16) as u8;
        v[3 * n + i] = 96 + ((x >> 24) as u8 >> 1);
    }
    Tile::from_samples(coord, layout, v).unwrap()
}

fn pixel_layer(d: &mut Document, seed: u32, e: Extent, background: bool) -> LayerId {
    let mut l = Layer::pixel(format!("layer {seed}"), e, Depth::U8);
    l.props.background = background;
    let r = l.raster_mut().unwrap();
    let (cols, rows) = r.grid();
    for ty in 0..rows {
        for tx in 0..cols {
            let mut t = content(seed, TileCoord::new(0, tx, ty), r.layout(tx, ty));
            if background {
                let n = t.layout().plane_len();
                t.samples_mut::<u8>().unwrap()[3 * n..].fill(255);
            }
            r.set_slot(tx, ty, Some(t), 1).unwrap();
        }
    }
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer: l,
    })
    .unwrap()
    .created[0]
}

/// 12 layers over `e`: a background, pixel layers in several modes, an
/// isolated group and a (pointwise) exposure adjustment.
fn small_document(e: Extent) -> (Document, Vec<LayerId>) {
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut ids = vec![pixel_layer(&mut d, 0, e, true)];
    for s in 1..10u32 {
        let id = pixel_layer(&mut d, s, e, false);
        let mut props = d.state().find(id).unwrap().props.clone();
        props.blend_mode = BlendMode::ALL[(s as usize * 5) % 27];
        props.opacity = 0.8;
        d.apply(DocOp::SetProps { id, props }).unwrap();
        ids.push(id);
    }
    let g = d
        .apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: Layer::group("group", GroupMode::Isolated),
        })
        .unwrap()
        .created[0];
    for id in &ids[6..9] {
        d.apply(DocOp::MoveLayer {
            id: *id,
            parent: Some(g),
            index: usize::MAX,
        })
        .unwrap();
    }
    (d, ids)
}

fn exposure_json(stops: f32) -> String {
    format!(r#"{{"kind":"exposure","exposure":{stops},"offset":0,"gamma":1}}"#)
}

fn attach(s: &DocumentSession, w: u32, h: u32) -> Vec<u32> {
    let ids: Vec<u32> = (0..3).map(|_| create_rgba8(w, h)).collect();
    for id in &ids {
        s.attach_surface(*id, w, h).unwrap();
    }
    ids
}

/// The frame's region as tightly packed RGBA8 rows.
fn pixels(f: &DocFrameInfo, sw: u32, sh: u32) -> Vec<u8> {
    let surface = Surface::lookup(f.surface_id, sw, sh).unwrap();
    let mut out = Vec::with_capacity((f.width * f.height * 4) as usize);
    surface
        .with_pixels(|px, stride| {
            for y in 0..f.height as usize {
                let o = y * stride;
                out.extend_from_slice(&px[o..o + f.width as usize * 4]);
            }
        })
        .unwrap();
    out
}

fn aligned(r: Rect, le: Extent) -> Rect {
    Rect::new(
        r.x0 / 16 * 16,
        r.y0 / 16 * 16,
        ((r.x1 + 15) / 16 * 16).min(i64::from(le.width)),
        ((r.y1 + 15) / 16 * 16).min(i64::from(le.height)),
    )
}

fn gpu(s: &DocumentSession) -> bool {
    let b = s.info().unwrap().backend;
    if b == "CPU" {
        eprintln!("skipping GPU assertions: backend {b}");
        return false;
    }
    true
}

/// P13 acceptance: at 100 % (level 0) on a 20 MP canvas with 100 layers, a
/// 3840×2160 viewport composites the viewport plus the halo, block-aligned,
/// not the level.
#[test]
fn viewport_frames_composite_only_the_viewport_plus_halo() {
    let (_d, engine) = engine();
    let e = Extent::new(5472, 3648);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut ids = vec![pixel_layer(&mut d, 0, e, true)];
    for s in 1..10u32 {
        ids.push(pixel_layer(&mut d, s, e, false));
    }
    // 90 copy-on-write duplicates in every mode: 100 layers, 20 MP.
    for j in 10..100usize {
        let id = d
            .apply(DocOp::DuplicateLayer { id: ids[1 + j % 9] })
            .unwrap()
            .created[0];
        let mut props = d.state().find(id).unwrap().props.clone();
        props.blend_mode = BlendMode::ALL[j % 27];
        d.apply(DocOp::SetProps { id, props }).unwrap();
        ids.push(id);
    }
    assert_eq!(d.state().root.len(), 100);
    let s = engine.adopt_document(d, "bench".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    if !gpu(&s) {
        return;
    }
    let (w, h) = (3840u32, 2160u32);
    attach(&s, w, h);
    s.set_viewport(0, 700, 500, w, h, 1.0).unwrap();
    s.wait_idle();
    rec.ok();
    let f = rec.last();
    assert_eq!((f.level, f.x, f.y, f.width, f.height), (0, 700, 500, w, h));
    let r = s.render_records().last().cloned().unwrap();
    assert_eq!(r.path, DocRenderPath::Viewport);
    let visible = Rect::new(700, 500, 700 + 3840, 500 + 2160);
    assert_eq!(r.visible, visible);
    let halo = 16;
    assert_eq!(
        r.requested,
        Rect::new(700 - halo, 500 - halo, 700 + 3840 + halo, 500 + 2160 + halo)
    );
    // The first frame of a fresh renderer dispatches exactly the
    // block-aligned requested region, and nothing else of the level.
    assert_eq!(r.dispatched, aligned(r.requested, e));
    assert_eq!(
        i64::from(r.blocks),
        r.dispatched.area() / 256,
        "every block of the region, once"
    );
    assert!(r.dispatched.area() * 2 < i64::from(e.width) * i64::from(e.height));
    assert!(!f.full_recomposite);
    // A pan by 64 px composites only the newly exposed blocks: the old
    // region's blocks are kept (copied), the new column strip is added.
    let before = aligned(r.requested, e);
    s.set_viewport(0, 764, 500, w, h, 1.0).unwrap();
    s.wait_idle();
    let r = s.render_records().last().cloned().unwrap();
    assert_eq!(r.path, DocRenderPath::Viewport);
    let after = aligned(r.requested, e);
    assert_eq!(
        r.dispatched,
        Rect::new(before.x1, after.y0, after.x1, after.y1)
    );
    // At the canvas edge the region is clipped to the level.
    s.set_viewport(0, 5472 - 1000, 3648 - 800, 1000, 800, 1.0)
        .unwrap();
    s.wait_idle();
    let r = s.render_records().last().cloned().unwrap();
    assert_eq!(
        r.requested,
        Rect::new(5472 - 1000 - halo, 3648 - 800 - halo, 5472, 3648)
    );
    eprintln!("{}", s.render_resources());
    s.close();
}

/// Two sessions of one document: viewport rendering vs the full-level path.
struct Pair {
    view: Arc<DocumentSession>,
    full: Arc<DocumentSession>,
    rv: Arc<Recorder>,
    rf: Arc<Recorder>,
    size: (u32, u32),
}

impl Pair {
    fn new(engine: &Arc<Engine>, doc: Document, size: (u32, u32)) -> Option<Self> {
        let view = engine.adopt_document(doc.clone(), "viewport".into());
        let full = engine.adopt_document(doc, "full".into());
        if !gpu(&view) {
            return None;
        }
        full.set_viewport_rendering(false);
        let (rv, rf) = (Arc::new(Recorder::default()), Arc::new(Recorder::default()));
        view.set_listener(Some(rv.clone()));
        full.set_listener(Some(rf.clone()));
        attach(&view, size.0, size.1);
        attach(&full, size.0, size.1);
        Some(Self {
            view,
            full,
            rv,
            rf,
            size,
        })
    }

    fn both(&self, f: impl Fn(&DocumentSession)) {
        f(&self.view);
        f(&self.full);
    }

    /// The latest frames of both sessions are pixel-identical.
    fn same(&self, what: &str, path: DocRenderPath) {
        self.view.wait_idle();
        self.full.wait_idle();
        self.rv.ok();
        self.rf.ok();
        let (a, b) = (self.rv.last(), self.rf.last());
        assert_eq!(
            (a.level, a.x, a.y, a.width, a.height, a.epoch),
            (b.level, b.x, b.y, b.width, b.height, b.epoch),
            "{what}: frames describe the same region"
        );
        let (pa, pb) = (
            pixels(&a, self.size.0, self.size.1),
            pixels(&b, self.size.0, self.size.1),
        );
        let diff = pa.iter().zip(&pb).filter(|(x, y)| x != y).count();
        assert_eq!(
            diff, 0,
            "{what}: {diff} bytes differ from the full-level render"
        );
        let rv = self.view.render_records().last().cloned().unwrap();
        let rf = self.full.render_records().last().cloned().unwrap();
        assert_eq!(rv.path, path, "{what}");
        assert_eq!(
            rf.path,
            if path == DocRenderPath::Cpu {
                DocRenderPath::Cpu
            } else {
                DocRenderPath::FullLevel
            },
            "{what}"
        );
    }
}

/// P13 acceptance: pixel parity with the full-level render for pan, resize
/// and incremental updates (opacity, visibility, adjustment, undo), plus
/// the full-halo (spatial adjustment) and CPU (layer style) fallbacks.
#[test]
fn viewport_frames_equal_full_level_frames() {
    let (_d, engine) = engine();
    let e = Extent::new(1536, 1024);
    let (doc, ids) = small_document(e);
    let Some(p) = Pair::new(&engine, doc, (640, 480)) else {
        return;
    };
    use DocRenderPath::*;
    p.both(|s| s.set_viewport(0, 300, 200, 640, 480, 1.0).unwrap());
    p.same("first frame", Viewport);
    for (i, (x, y)) in [(340, 200), (340, 260), (100, 30), (896, 544), (0, 0)]
        .into_iter()
        .enumerate()
    {
        p.both(|s| s.set_viewport(0, x, y, 640, 480, 1.0).unwrap());
        p.same(&format!("pan {i}"), Viewport);
    }
    // Resize (smaller viewport, then back), and level 1.
    p.both(|s| s.set_viewport(0, 50, 60, 512, 300, 1.0).unwrap());
    p.same("resize smaller", Viewport);
    p.both(|s| s.set_viewport(0, 50, 60, 640, 480, 1.0).unwrap());
    p.same("resize back", Viewport);
    p.both(|s| s.set_viewport(1, 100, 80, 600, 400, 0.5).unwrap());
    p.same("level 1", Viewport);
    p.both(|s| s.set_viewport(0, 400, 300, 640, 480, 1.0).unwrap());
    p.same("back to level 0", Viewport);
    // Incremental updates.
    let (a, b) = (ids[3].0, ids[7].0);
    for i in 0..4 {
        p.both(|s| {
            s.set_opacity(a, 0.2 + 0.2 * i as f32, true).unwrap();
        });
        p.same(&format!("opacity drag {i}"), Viewport);
    }
    p.both(|s| {
        s.commit("Opacity".into()).unwrap();
    });
    p.same("opacity commit", Viewport);
    p.both(|s| {
        s.set_visible(b, false).unwrap();
    });
    p.same("hide", Viewport);
    let adj = |s: &DocumentSession| {
        s.add_layer(
            NewLayer::Adjustment {
                json: exposure_json(0.5),
            },
            "exp".into(),
            None,
            None,
        )
        .unwrap()
        .created[0]
    };
    let (ja, jb) = (adj(&p.view), adj(&p.full));
    assert_eq!(ja, jb);
    p.same("adjustment added", Viewport);
    p.both(|s| {
        s.set_adjustment_json(ja, exposure_json(-0.7), true)
            .unwrap();
    });
    p.same("adjustment drag", Viewport);
    p.both(|s| {
        s.undo().unwrap();
    });
    p.same("undo", Viewport);
    // Pan after edits made off screen.
    p.both(|s| s.set_viewport(0, 896, 544, 640, 480, 1.0).unwrap());
    p.same("pan after edits", Viewport);
    // A spatial adjustment needs a full-level halo: both take the full level.
    let sh = serde_json::to_string(&Adjustment::ShadowsHighlights {
        settings: compositor::adjust::shadows::ShadowsHighlights {
            shadows_amount: 0.6,
            ..Default::default()
        },
    })
    .unwrap();
    p.both(|s| {
        s.add_layer(
            NewLayer::Adjustment { json: sh.clone() },
            "sh".into(),
            None,
            None,
        )
        .unwrap();
    });
    p.same("shadows/highlights (full halo)", FullLevel);
    p.both(|s| {
        s.undo().unwrap();
    });
    p.same("spatial removed", Viewport);
    // Layer styles: the CPU fallback (B5-07) on both.
    let styles = serde_json::json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 12.0, "size": 8.0}}
    ], "scale": 1.0});
    p.both(|s| {
        s.set_layer_styles_json(a, styles.to_string(), false)
            .unwrap();
    });
    p.same("layer style (CPU)", Cpu);
    p.both(|s| {
        s.undo().unwrap();
    });
    p.same("style removed", Viewport);
    p.view.close();
    p.full.close();
}

/// A styled document whose CPU frames are slow (the B5-07 fallback
/// recomputes the style planes per output tile: tens to hundreds of ms).
fn slow_styled(engine: &Arc<Engine>) -> (Arc<DocumentSession>, Arc<Recorder>, u64, u64) {
    let s = engine
        .clone()
        .new_document(512, 384, DocDepth::U8, None)
        .unwrap();
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let styled = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"solid","color":[1,0.5,0]}"#.into(),
            },
            "styled".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    let styles = serde_json::json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 30.0, "size": 40.0}},
        {"kind": "outer_glow", "settings": {"size": 30.0}},
    ], "scale": 1.0});
    s.set_layer_styles_json(styled, styles.to_string(), false)
        .unwrap();
    let plain = s
        .add_layer(NewLayer::Pixel, "plain".into(), None, None)
        .unwrap()
        .created[0];
    attach(&s, 512, 384);
    s.set_viewport(0, 0, 0, 512, 384, 1.0).unwrap();
    s.wait_idle();
    (s, rec, styled, plain)
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// P14: while a slow (CPU style) frame is in flight, synchronous edits do
/// not wait for it: they only snapshot-share the document.
#[test]
fn edits_do_not_wait_for_frames_in_flight() {
    let (_d, engine) = engine();
    let (s, rec, styled, plain) = slow_styled(&engine);
    let mut frame_ms = Vec::new();
    let mut call_ms = Vec::new();
    let mut in_flight = 0;
    for i in 0..12 {
        let t = Instant::now();
        s.set_opacity(styled, 0.5 + 0.04 * i as f32, true).unwrap();
        std::thread::sleep(Duration::from_millis(5));
        for k in 0..3 {
            let pending = jobs::interactive_pending();
            let c = Instant::now();
            match k {
                0 => {
                    s.set_opacity(plain, 0.1 * (i % 9) as f32, true).unwrap();
                }
                1 => {
                    s.rename_layer(plain, format!("plain {i}")).unwrap();
                }
                _ => {
                    s.set_visible(plain, i % 2 == 0).unwrap();
                }
            }
            call_ms.push(c.elapsed().as_secs_f64() * 1000.0);
            // P17: the frame in flight registers interactive pressure.
            if pending > 0 {
                in_flight += 1;
            }
        }
        s.wait_idle();
        frame_ms.push(t.elapsed().as_secs_f64() * 1000.0);
    }
    rec.ok();
    let (frame, call) = (median(frame_ms), median(call_ms.clone()));
    let max = call_ms.iter().cloned().fold(0.0, f64::max);
    eprintln!("styled frame median {frame:.1} ms; edit call median {call:.3} ms, max {max:.2} ms");
    assert!(
        call * 20.0 < frame,
        "edits wait for frames: call {call:.2} ms vs frame {frame:.1} ms"
    );
    assert!(
        in_flight > 0,
        "document frames register interactive pressure"
    );
    let r = s.render_records();
    assert!(r.iter().any(|r| r.path == DocRenderPath::Cpu));
    // The frame showing the final state was published last.
    assert_eq!(rec.last().epoch, s.info().unwrap().epoch);
    s.close();
}

/// P14: a frame rendered for a surface ring that was replaced meanwhile is
/// dropped, never published.
#[test]
fn frames_for_a_replaced_ring_are_dropped() {
    let (_d, engine) = engine();
    let (s, rec, styled, _) = slow_styled(&engine);
    let old = rec.frames.lock().unwrap().len();
    s.set_opacity(styled, 0.7, true).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    s.detach_surfaces();
    let ring = attach(&s, 384, 256);
    s.wait_idle();
    rec.ok();
    let frames = rec.frames.lock().unwrap()[old..].to_vec();
    assert!(!frames.is_empty());
    assert!(
        frames.iter().all(|f| ring.contains(&f.surface_id)),
        "a frame of the old ring was published: {frames:?}"
    );
    assert!(s.render_records().iter().any(|r| r.dropped));
    s.close();
}

/// P14: saves (file I/O outside the lock), frames and undo/redo racing on
/// three threads keep history, the saved files and the surfaces valid.
#[test]
fn save_render_undo_races_keep_history_and_surfaces_valid() {
    let (dir, engine) = engine();
    let e = Extent::new(1024, 768);
    let (doc, ids) = small_document(e);
    let s = engine.adopt_document(doc, "race".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    attach(&s, 512, 384);
    s.set_viewport(0, 100, 100, 512, 384, 1.0).unwrap();
    let path = dir.path().join("race.tessera-doc");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saver = {
        let (s, stop, dir) = (s.clone(), stop.clone(), dir.path().to_owned());
        std::thread::spawn(move || {
            let mut n = 0;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                let p = dir.join(format!("race-{}.tessera-doc", n % 3));
                s.save_as(p.to_string_lossy().into_owned()).unwrap();
                n += 1;
            }
            n
        })
    };
    let viewer = {
        let (s, stop) = (s.clone(), stop.clone());
        std::thread::spawn(move || {
            let mut i = 0u32;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                s.set_viewport(0, 100 + (i % 7) * 37, 80 + (i % 5) * 23, 512, 384, 1.0)
                    .unwrap();
                i += 1;
                std::thread::sleep(Duration::from_millis(2));
            }
        })
    };
    let layer = ids[4].0;
    for i in 0..60 {
        s.set_opacity(layer, 0.1 + 0.01 * i as f32, true).unwrap();
        if i % 3 == 2 {
            s.commit(format!("Opacity {i}")).unwrap();
        }
        if i % 10 == 9 {
            s.undo().unwrap();
            if i % 20 == 19 {
                s.redo().unwrap();
            }
        }
    }
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    let saves = saver.join().unwrap();
    viewer.join().unwrap();
    s.wait_idle();
    rec.ok();
    assert!(saves > 0);
    // History: every row is reachable and the head is the last one checked out.
    let items = s.history_items().unwrap();
    assert!(items.len() > 10, "{} history rows", items.len());
    let head = s.info().unwrap().history_head;
    assert!(items.iter().any(|h| h.id == head));
    for h in items.iter().rev().take(5) {
        s.checkout_history(h.id).unwrap();
    }
    s.checkout_history(head).unwrap();
    // A quiet save leaves the document clean, and every saved file reopens.
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    assert!(!s.info().unwrap().dirty);
    for n in 0..3 {
        let p = dir.path().join(format!("race-{n}.tessera-doc"));
        if p.exists() {
            let copy = dir.path().join(format!("copy-{n}.tessera-doc"));
            std::fs::copy(&p, &copy).unwrap();
            let o = engine
                .clone()
                .open_document(copy.to_string_lossy().into_owned())
                .unwrap();
            assert_eq!(o.layers().unwrap().len(), s.layers().unwrap().len());
            o.close();
        }
    }
    // The final frame shows the final state, byte-exact with a fresh full render.
    s.set_viewport(0, 100, 100, 512, 384, 1.0).unwrap();
    s.wait_idle();
    let f = rec.last();
    assert_eq!(f.epoch, s.info().unwrap().epoch);
    let (w, h, px) = s.read_level(0).unwrap();
    assert_eq!((w, h), (1024, 768));
    let got = pixels(&f, 512, 384);
    let mut worst = 0u8;
    for y in 0..f.height as usize {
        for x in 0..f.width as usize {
            let o = ((f.y as usize + y) * w as usize + f.x as usize + x) * 4;
            for c in 0..4 {
                let want = (px[o + c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                worst = worst.max(want.abs_diff(got[(y * f.width as usize + x) * 4 + c]));
            }
        }
    }
    assert!(worst <= 1, "surface differs from the state by {worst}");
    s.close();
}

/// The composite thumbnail (Channels panel, per edit on the main thread)
/// reuses its layers' mips after a property edit instead of reducing every
/// layer from level 0 again; during a drag it shows the committed state.
#[test]
fn composite_thumbnails_reuse_mips_across_edits() {
    let (_d, engine) = engine();
    let e = Extent::new(3072, 2048);
    let (doc, ids) = small_document(e);
    let s = engine.adopt_document(doc, "thumbs".into());
    let t = Instant::now();
    let first = s.composite_thumbnail(48).unwrap();
    let cold = t.elapsed();
    let layer = ids[5].0;
    s.set_opacity(layer, 0.4, false).unwrap();
    let t = Instant::now();
    let second = s.composite_thumbnail(48).unwrap();
    let warm = t.elapsed();
    assert_ne!(first, second, "a committed edit renders a new thumbnail");
    eprintln!("composite thumbnail: cold {cold:?}, after an opacity edit {warm:?}");
    assert!(warm * 4 < cold, "cold {cold:?} vs warm {warm:?}");
    // An interactive drag keeps the committed thumbnail (no render per tick).
    let n = s.thumbnail_renders();
    for i in 0..5 {
        s.set_opacity(layer, 0.1 * i as f32, true).unwrap();
        assert_eq!(s.composite_thumbnail(48).unwrap(), second);
    }
    assert_eq!(s.thumbnail_renders(), n);
    s.commit("Opacity".into()).unwrap();
    assert_ne!(s.composite_thumbnail(48).unwrap(), second);
    s.close();
}
