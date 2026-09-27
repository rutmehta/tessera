//! Document render path benchmarks (WP B5-14, perf audit P13/P14/P17).
//! Ignored; run on a quiet machine:
//! `cargo test -p tessera-ffi --release --test document_perf -- --ignored --nocapture --test-threads 1`
//!
//! Only the public session API is used, so the same file measures the code
//! before and after the change. Every figure is "to completed frame": the
//! listener's `on_frame`, which fires after the GPU finished the surface.
#![cfg(target_os = "macos")]

use compositor::{
    DocOp, DocState, Document, Layer, LayerId,
    edit::{PaintTarget, TileDelta},
    geom::Rect,
    raster::Depth,
};
use engine_api::tile::{Extent, Tile, TileCoord};
use std::path::Path;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};
use tessera_ffi::surface::testing::create_rgba8;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    static FONTS: std::sync::Once = std::sync::Once::new();
    FONTS.call_once(|| {
        load_text_fonts_for_tests(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../typography/tests/fonts"
        ))
    });
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

#[derive(Default)]
struct Recorder {
    frames: Mutex<Vec<(Instant, DocFrameInfo)>>,
    cv: Condvar,
    failures: Mutex<Vec<String>>,
}

impl DocumentListener for Recorder {
    fn on_frame(&self, frame: DocFrameInfo) {
        self.frames.lock().unwrap().push((Instant::now(), frame));
        self.cv.notify_all();
    }
    fn on_layers_changed(&self, _: Vec<u64>) {}
    fn on_history_changed(&self, _: u64) {}
    fn on_render_failed(&self, message: String) {
        eprintln!("render failed: {message}");
        self.failures.lock().unwrap().push(message);
    }
}

impl Recorder {
    /// Time the first frame showing `epoch` (or later) arrived.
    fn wait_epoch(&self, epoch: u64, timeout: Duration) -> Option<Instant> {
        let end = Instant::now() + timeout;
        let mut f = self.frames.lock().unwrap();
        loop {
            if let Some((t, _)) = f.iter().find(|(_, x)| x.epoch >= epoch) {
                return Some(*t);
            }
            let now = Instant::now();
            if now >= end {
                return None;
            }
            f = self.cv.wait_timeout(f, end - now).unwrap().0;
        }
    }
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    s[((s.len() - 1) as f64 * p).round() as usize]
}

fn summary(label: &str, v: &[f64]) -> (f64, f64) {
    let (p50, p95) = (pct(v, 0.5), pct(v, 0.95));
    eprintln!(
        "RESULT {label}: n {} p50 {p50:.2} ms p95 {p95:.2} ms max {:.2} ms",
        v.len(),
        pct(v, 1.0)
    );
    (p50, p95)
}

fn content(seed: u32, coord: TileCoord, layout: engine_api::tile::TileLayout) -> Tile {
    let n = layout.plane_len();
    let mut v = vec![0u8; 4 * n];
    let mut x = seed.wrapping_mul(2654435761) ^ (coord.x << 16) ^ coord.y;
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

/// COMPOSITOR.md §10/§12.4: 100 semi-transparent 8-bit layers at 20 MP (10
/// distinct + 90 copy-on-write duplicates with one repainted tile), all 27
/// modes, a pass-through and an isolated group (as tests/document.rs).
fn bench_document() -> (Document, Vec<LayerId>) {
    use compositor::{BlendMode, GroupMode};
    let e = Extent::new(5472, 3648);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut ids = Vec::new();
    for s in 0..10u32 {
        let mut l = Layer::pixel(format!("base {s}"), e, Depth::U8);
        l.props.background = s == 0;
        let r = l.raster_mut().unwrap();
        let (cols, rows) = r.grid();
        for ty in 0..rows {
            for tx in 0..cols {
                let mut t = content(s, TileCoord::new(0, tx, ty), r.layout(tx, ty));
                if s == 0 {
                    let n = t.layout().plane_len();
                    t.samples_mut::<u8>().unwrap()[3 * n..].fill(255);
                }
                r.set_slot(tx, ty, Some(t), 1).unwrap();
            }
        }
        ids.push(
            d.apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: l,
            })
            .unwrap()
            .created[0],
        );
    }
    let (cols, rows) = e.tile_grid(256);
    for j in 10..100u32 {
        let src = ids[1 + (j as usize % 9)];
        let id = d.apply(DocOp::DuplicateLayer { id: src }).unwrap().created[0];
        let raster = d.state().find(id).unwrap().raster().unwrap().clone();
        let (tx, ty) = (j % cols, (j / cols) % rows);
        d.apply(DocOp::PaintTiles {
            id,
            target: PaintTarget::Content,
            tiles: vec![TileDelta {
                tx,
                ty,
                tile: Some(content(j, TileCoord::new(0, tx, ty), raster.layout(tx, ty))),
            }],
            dirty: Rect::of_extent(e),
        })
        .unwrap();
        let mut props = d.state().find(id).unwrap().props.clone();
        props.blend_mode = BlendMode::ALL[j as usize % 27];
        d.apply(DocOp::SetProps { id, props }).unwrap();
        ids.push(id);
    }
    for (mode, range) in [
        (GroupMode::PassThrough, 20..30usize),
        (GroupMode::Isolated, 40..50),
    ] {
        let g = d
            .apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: Layer::group("group", mode),
            })
            .unwrap()
            .created[0];
        for id in &ids[range] {
            d.apply(DocOp::MoveLayer {
                id: *id,
                parent: Some(g),
                index: usize::MAX,
            })
            .unwrap();
        }
    }
    (d, ids)
}

fn attach(s: &DocumentSession, w: u32, h: u32) {
    for _ in 0..3 {
        s.attach_surface(create_rgba8(w, h), w, h).unwrap();
    }
}

/// Runs `edit` `n` times, each followed by a completed frame; returns the
/// call → completed-frame wall times (the first `warm` discarded).
fn frames(
    s: &DocumentSession,
    rec: &Recorder,
    n: usize,
    warm: usize,
    edit: &dyn Fn(usize),
) -> Vec<f64> {
    let mut out = Vec::new();
    for i in 0..n {
        let t = Instant::now();
        edit(i);
        s.wait_idle();
        if i >= warm {
            out.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    assert!(rec.failures.lock().unwrap().is_empty());
    out
}

/// P13: a 3840×2160 viewport at 100 % (level 0) of the 20 MP / 100-layer
/// bench document: cold first frame, then pan, resize, opacity and
/// visibility frames, each measured to the completed frame.
#[test]
#[ignore]
fn bench_p13_4k_viewport_l0_100_layers_20mp() {
    let (_d, engine) = engine();
    let (doc, ids) = bench_document();
    let s = engine.adopt_document(doc, "bench".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let (w, h) = (3840u32, 2160u32);
    attach(&s, w, h);
    // The ring's first (fit) frame is not part of the measurement.
    s.wait_idle();
    let t = Instant::now();
    s.set_viewport(0, 512, 512, w, h, 1.0).unwrap();
    s.wait_idle();
    eprintln!(
        "RESULT p13 cold first 4K L0 frame: {:.1} ms (backend {})",
        t.elapsed().as_secs_f64() * 1000.0,
        s.info().unwrap().backend
    );
    // Let the specialized kernel for this structure compile (background).
    std::thread::sleep(Duration::from_secs(4));
    let layer = ids[55].0;
    let other = ids[73].0;
    let pan = frames(&s, &rec, 44, 4, &|i| {
        let x = 512 + ((i % 8) as u32) * 97;
        let y = 512 + ((i % 5) as u32) * 61;
        s.set_viewport(0, x, y, w, h, 1.0).unwrap();
    });
    summary("p13 4K L0 pan", &pan);
    s.set_viewport(0, 512, 512, w, h, 1.0).unwrap();
    s.wait_idle();
    let resize = frames(&s, &rec, 44, 4, &|i| {
        let (vw, vh) = if i % 2 == 0 { (3584, 2048) } else { (w, h) };
        s.set_viewport(0, 512, 512, vw, vh, 1.0).unwrap();
    });
    summary("p13 4K L0 resize", &resize);
    s.set_viewport(0, 512, 512, w, h, 1.0).unwrap();
    s.wait_idle();
    let opacity = frames(&s, &rec, 44, 4, &|i| {
        s.set_opacity(layer, 0.3 + (i % 10) as f32 * 0.05, true)
            .unwrap();
    });
    summary("p13 4K L0 opacity (incremental)", &opacity);
    s.commit("Opacity".into()).unwrap();
    s.wait_idle();
    let vis = frames(&s, &rec, 44, 4, &|i| {
        s.set_visible(other, i % 2 == 1).unwrap();
    });
    summary("p13 4K L0 visibility", &vis);
    let render: Vec<f64> = rec
        .frames
        .lock()
        .unwrap()
        .iter()
        .skip(8)
        .map(|(_, f)| f.render_ms)
        .collect();
    summary("p13 4K L0 render_ms (all warm frames)", &render);
    s.close();
}

/// A styled document the resident program refuses (CPU composition, B5-07):
/// a 768×512 8-bit gradient layer with a drop shadow and an outer glow,
/// a plain layer and a text layer above.
fn styled_session(engine: &Arc<Engine>) -> (Arc<DocumentSession>, u64, u64, Option<u64>) {
    let s = engine
        .clone()
        .new_document(768, 512, DocDepth::U8, None)
        .unwrap();
    let a = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"linear_gradient","stops":[{"position":0,"color":[1,0.5,0],"opacity":1},{"position":1,"color":[0,0.3,1],"opacity":0.4}],"angle":30}"#.into(),
            },
            "styled".into(),
            None,
            None,
        )
        .or_else(|_| {
            s.add_layer(
                NewLayer::Fill {
                    json: r#"{"kind":"solid","color":[1,0.5,0]}"#.into(),
                },
                "styled".into(),
                None,
                None,
            )
        })
        .unwrap()
        .created[0];
    let styles = serde_json::json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 30.0, "size": 40.0}},
        {"kind": "outer_glow", "settings": {"size": 30.0}},
    ], "scale": 1.0});
    s.set_layer_styles_json(a, styles.to_string(), false)
        .unwrap();
    let b = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"solid","color":[0.2,0.8,0.3]}"#.into(),
            },
            "plain".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.set_opacity(b, 0.5, false).unwrap();
    let text =
        serde_json::json!({"runs": [{"text": "Tessera", "family": "Noto Sans", "size": 48.0}]});
    let t = s
        .add_text_layer(
            "caption".into(),
            None,
            None,
            text.to_string(),
            TransformMatrix {
                a: 1.0,
                b: 0.0,
                c: 100.0,
                d: 0.0,
                e: 1.0,
                f: 200.0,
            },
            false,
        )
        .ok()
        .map(|u| u.created[0]);
    (s, a, b, t)
}

/// P14: synchronous UI mutations while slow (CPU style fallback) frames are
/// in flight. The render thread is kept busy by re-requesting a styled frame.
#[test]
#[ignore]
fn bench_p14_mutations_during_slow_style_frames() {
    let (_d, engine) = engine();
    let (s, styled, plain, text) = styled_session(&engine);
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    attach(&s, 768, 512);
    s.set_viewport(0, 0, 0, 768, 512, 1.0).unwrap();
    s.wait_idle();
    // One slow frame's duration, for context.
    let t = Instant::now();
    s.set_opacity(styled, 0.9, true).unwrap();
    s.wait_idle();
    let slow = t.elapsed().as_secs_f64() * 1000.0;
    eprintln!("RESULT p14 one styled frame (CPU fallback): {slow:.1} ms");
    let mut ops: Vec<(&str, Vec<f64>)> = vec![
        ("opacity", vec![]),
        ("rename", vec![]),
        ("visibility", vec![]),
        ("text", vec![]),
    ];
    let mut busy_samples = 0;
    for i in 0..240 {
        // Keep a slow frame in flight: the styled layer changes every round.
        s.set_opacity(styled, 0.6 + (i % 4) as f32 * 0.1, true)
            .unwrap();
        std::thread::sleep(Duration::from_millis(3));
        let k = i % 4;
        let t = Instant::now();
        match k {
            0 => {
                s.set_opacity(plain, 0.3 + (i % 7) as f32 * 0.1, true)
                    .unwrap();
            }
            1 => {
                s.rename_layer(plain, format!("plain {i}")).unwrap();
            }
            2 => {
                s.set_visible(plain, i % 8 != 2).unwrap();
            }
            _ => {
                if let Some(t) = text {
                    let m = serde_json::json!({"runs": [{"text": format!("Tessera {i}"), "family": "Noto Sans", "size": 48.0}]});
                    s.set_text_layer(
                        t,
                        m.to_string(),
                        TransformMatrix {
                            a: 1.0,
                            b: 0.0,
                            c: 100.0,
                            d: 0.0,
                            e: 1.0,
                            f: 200.0,
                        },
                        true,
                        None,
                    )
                    .unwrap();
                } else {
                    s.set_fill_opacity(plain, 0.5, true).unwrap();
                }
            }
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        ops[k].1.push(ms);
        busy_samples += 1;
        std::thread::sleep(Duration::from_millis(12));
    }
    s.commit("done".into()).unwrap();
    s.wait_idle();
    let mut all = Vec::new();
    for (name, v) in &ops {
        summary(&format!("p14 {name} call during slow frames"), v);
        all.extend(v);
    }
    summary("p14 all mutation calls during slow frames", &all);
    eprintln!("({busy_samples} samples; text layer: {})", text.is_some());
    assert!(rec.failures.lock().unwrap().is_empty());
    s.close();
}

/// P17: document input → completed frame while photo exports run, against
/// the idle baseline, on the bench document at level 2 (fit, 1368×912).
#[test]
#[ignore]
fn bench_p17_document_frames_during_photo_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let raw = root.join("sample.dng");
    if !raw.exists() {
        eprintln!("skipping: no {}", raw.display());
        return;
    }
    let (dir, engine) = engine();
    let photos = dir.path().join("raw");
    std::fs::create_dir(&photos).unwrap();
    std::fs::copy(&raw, photos.join("sample.dng")).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let image = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    let web = engine.export_presets().unwrap().remove(0);
    let mut json: serde_json::Value = serde_json::from_str(&web.settings_json).unwrap();
    json["destination"] = dir.path().join("out").to_string_lossy().into();
    json["on_conflict"] = "unique".into();
    let settings = json.to_string();

    let (doc, ids) = bench_document();
    let s = engine.adopt_document(doc, "bench".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let plan = s.plan_surface(1368, 912).unwrap();
    attach(&s, plan.width, plan.height);
    s.wait_idle();
    std::thread::sleep(Duration::from_secs(3));
    let layer = ids[55].0;
    let measure = |n: usize| -> Vec<f64> {
        let mut v = Vec::new();
        for i in 0..n {
            let t = Instant::now();
            let u = s
                .set_opacity(layer, 0.3 + (i % 10) as f32 * 0.05, true)
                .unwrap();
            let at = rec
                .wait_epoch(u.epoch, Duration::from_secs(10))
                .expect("frame");
            v.push(at.duration_since(t).as_secs_f64() * 1000.0);
            // Display-rate input.
            std::thread::sleep(Duration::from_millis(16));
        }
        v
    };
    let _ = measure(10);
    let idle = measure(120);
    let (_, idle95) = summary("p17 input→frame idle", &idle);

    let stop = Arc::new(AtomicBool::new(false));
    let done = Arc::new(AtomicUsize::new(0));
    let exporter = {
        let (engine, stop, done, id, settings) = (
            engine.clone(),
            stop.clone(),
            done.clone(),
            image.id.clone(),
            settings.clone(),
        );
        std::thread::spawn(move || {
            let mut secs = Vec::new();
            while !stop.load(Ordering::Relaxed) {
                let t = Instant::now();
                let r = engine
                    .export_batch(
                        ExportTarget::Images {
                            image_ids: vec![id.clone()],
                        },
                        settings.clone(),
                        None,
                        None,
                    )
                    .unwrap();
                assert_eq!(r.exported, 1, "{:?}", r.items);
                secs.push(t.elapsed().as_secs_f64());
                done.fetch_add(1, Ordering::Relaxed);
            }
            secs
        })
    };
    // Wait until the first export is underway (decode done, bands running).
    std::thread::sleep(Duration::from_millis(1500));
    let busy = measure(240);
    stop.store(true, Ordering::Relaxed);
    let secs = exporter.join().unwrap();
    let (_, busy95) = summary("p17 input→frame during export", &busy);
    eprintln!(
        "RESULT p17 ratio p95 export/idle {:.2} (target ≤ 1.25); exports completed {} (each {:.2}–{:.2} s)",
        busy95 / idle95,
        done.load(Ordering::Relaxed),
        secs.iter().cloned().fold(f64::MAX, f64::min),
        secs.iter().cloned().fold(0.0, f64::max),
    );
    s.close();
}
