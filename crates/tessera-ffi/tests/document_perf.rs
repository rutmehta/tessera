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

// ─────────────────────────── B5-15 (P16, P19) ───────────────────────────
//
// `bench_p16_*` and `bench_p19_*` use only calls that exist before B5-15
// (the baseline build runs the same file without the `b515_*` benches).

/// Physical footprint of this process (MiB), `proc_pid_rusage` V2.
fn footprint_mib() -> f64 {
    unsafe extern "C" {
        fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut u64) -> i32;
    }
    let mut buf = [0u64; 32];
    // SAFETY: RUSAGE_INFO_V2 is 20 u64-sized fields (uuid = 2); the buffer is larger.
    let r = unsafe { proc_pid_rusage(std::process::id() as i32, 2, buf.as_mut_ptr()) };
    if r != 0 {
        return f64::NAN;
    }
    // uuid[16], user, system, pkg_idle, interrupt, pageins, wired, resident, phys_footprint.
    buf[9] as f64 / (1 << 20) as f64
}

/// An opaque 8-bit noise layer with smooth structure (a photo stand-in).
fn photo_layer(name: &str, e: Extent) -> Layer {
    let mut l = Layer::pixel(name.to_owned(), e, Depth::U8);
    let r = l.raster_mut().unwrap();
    let (cols, rows) = r.grid();
    for ty in 0..rows {
        for tx in 0..cols {
            let layout = r.layout(tx, ty);
            let n = layout.plane_len();
            let mut v = vec![255u8; 4 * n];
            let mut x = 0x2545_f491u32 ^ (tx << 16) ^ ty;
            for py in 0..layout.extent.height as usize {
                for px in 0..layout.extent.width as usize {
                    x ^= x << 13;
                    x ^= x >> 17;
                    x ^= x << 5;
                    let (gx, gy) = (tx as usize * 256 + px, ty as usize * 256 + py);
                    let i = py * layout.stride() + px;
                    v[i] = ((gx / 3) % 256) as u8 ^ (x as u8 >> 3);
                    v[n + i] = ((gy / 2) % 256) as u8 ^ ((x >> 8) as u8 >> 3);
                    v[2 * n + i] = (((gx + gy) / 5) % 256) as u8;
                }
            }
            r.set_slot(
                tx,
                ty,
                Some(Tile::from_samples(TileCoord::new(0, tx, ty), layout, v).unwrap()),
                1,
            )
            .unwrap();
        }
    }
    l
}

/// A 5472×3648 8-bit document: an opaque photo layer, a smart object of a
/// second photo layer with a Gaussian Blur smart filter at 70 %, and a text
/// layer with a drop shadow and an outer glow (a "styled" document: the
/// CPU compositor renders it).
fn styled_20mp(engine: &Arc<Engine>) -> Arc<DocumentSession> {
    let e = Extent::new(5472, 3648);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    for name in ["photo", "detail"] {
        d.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: photo_layer(name, e),
        })
        .unwrap();
    }
    let s = engine.adopt_document(d, "styled 20 MP".into());
    let detail = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(detail).unwrap();
    s.apply_filter(detail, r#"{"id":"gaussian_blur","params":{"radius":6}}"#.into())
        .unwrap();
    s.set_opacity(detail, 0.7, false).unwrap();
    let text = serde_json::json!({"runs": [{"text": "Tessera export", "family": "Noto Sans", "size": 220.0}]});
    let t = s
        .add_text_layer(
            "caption".into(),
            None,
            None,
            text.to_string(),
            TransformMatrix {
                a: 1.0,
                b: 0.0,
                c: 600.0,
                d: 0.0,
                e: 1.0,
                f: 2600.0,
            },
            false,
        )
        .unwrap()
        .created[0];
    let styles = serde_json::json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 30.0, "size": 40.0}},
        {"kind": "outer_glow", "settings": {"size": 30.0}},
    ], "scale": 1.0});
    s.set_layer_styles_json(t, styles.to_string(), false).unwrap();
    s
}

/// P16 before/after: Export Flat of the styled 20 MP document through the
/// synchronous call (what the app's main thread ran before B5-15: its
/// duration is the main-thread span), PNG sRGB.
#[test]
#[ignore]
fn bench_p16_export_flat_20mp_styled_sync() {
    let (dir, engine) = engine();
    let s = styled_20mp(&engine);
    let mut v = Vec::new();
    for i in 0..3 {
        let out = dir.path().join(format!("flat{i}.png"));
        let t = Instant::now();
        s.export_flat(
            out.to_string_lossy().into_owned(),
            ExportFormat::Png,
            90,
            ExportColor::Srgb,
        )
        .unwrap();
        v.push(t.elapsed().as_secs_f64() * 1000.0);
        eprintln!("export {i}: {:.0} ms, footprint {:.0} MiB", v[i], footprint_mib());
    }
    summary("p16 20 MP styled Export Flat (sync call)", &v);
    s.close();
}

/// P16 parity: writes PNG / TIFF / JPEG exports of the styled document in
/// three colour choices to `$B515_PARITY_DIR` (compare the files of two
/// builds byte for byte).
#[test]
#[ignore]
fn bench_p16_export_parity_files() {
    let Some(out) = std::env::var_os("B515_PARITY_DIR") else {
        eprintln!("set B515_PARITY_DIR");
        return;
    };
    let out = std::path::PathBuf::from(out);
    std::fs::create_dir_all(&out).unwrap();
    let (_dir, engine) = engine();
    let s = styled_20mp(&engine);
    for (format, ext) in [
        (ExportFormat::Png, "png"),
        (ExportFormat::Tiff, "tif"),
        (ExportFormat::Jpeg, "jpg"),
    ] {
        for (color, name) in [
            (ExportColor::Document, "document"),
            (ExportColor::Srgb, "srgb"),
            (ExportColor::DisplayP3, "p3"),
        ] {
            let p = out.join(format!("styled-{name}.{ext}"));
            s.export_flat(p.to_string_lossy().into_owned(), format, 85, color)
                .unwrap();
        }
    }
    s.close();
}

/// P19 before/after: a Gaussian Blur smart filter drag on a 20 MP smart
/// object (opaque photo layer converted for smart filters, radius 8), fit
/// view (level 2) and 100 % in a 4K viewport: preview call → completed
/// frame, sequential ticks at display rate; the footprint around it.
/// `TESSERA_DOC_CPU_SMART_FILTERS=1` forces the CPU bake in B5-15 builds.
#[test]
#[ignore]
fn bench_p19_smart_filter_drag_20mp() {
    let (_dir, engine) = engine();
    let e = Extent::new(5472, 3648);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer: photo_layer("photo", e),
    })
    .unwrap();
    let s = engine.adopt_document(d, "smart 20 MP".into());
    let id = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(id).unwrap();
    s.apply_filter(id, r#"{"id":"gaussian_blur","params":{"radius":8}}"#.into())
        .unwrap();
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let start_mib = footprint_mib();
    for (label, level, w, h) in [("fit L2", 2u8, 1368u32, 912u32), ("100% 4K L0", 0, 3840, 2160)] {
        let plan = s.plan_surface(w, h).unwrap();
        attach(&s, plan.width.max(w), plan.height.max(h));
        s.set_viewport(level, 512, 512, w, h, 1.0 / f64::from(1u32 << level))
            .unwrap();
        s.wait_filters_idle();
        s.wait_idle();
        s.wait_filters_idle();
        s.wait_idle();
        let warm_mib = footprint_mib();
        let mut calls = Vec::new();
        let mut v = Vec::new();
        let mut peak = warm_mib;
        for i in 0..32 {
            let r = 6.0 + (i % 9) as f32;
            let json = format!(r#"{{"id":"gaussian_blur","params":{{"radius":{r}}}}}"#);
            let t = Instant::now();
            s.preview_smart_filter(id, 0, json, None).unwrap();
            calls.push(t.elapsed().as_secs_f64() * 1000.0);
            s.wait_filters_idle();
            s.wait_idle();
            if i >= 2 {
                v.push(t.elapsed().as_secs_f64() * 1000.0);
            }
            peak = peak.max(footprint_mib());
            std::thread::sleep(Duration::from_millis(16));
        }
        summary(&format!("p19 {label} preview call"), &calls);
        summary(&format!("p19 {label} filter drag tick → completed frame"), &v);
        s.clear_preview().unwrap();
        s.wait_idle();
        eprintln!(
            "RESULT p19 {label} footprint: start {start_mib:.0} MiB, warm {warm_mib:.0}, peak during drag {peak:.0}, after {:.0} MiB",
            footprint_mib()
        );
        eprintln!("resources: {}", s.render_resources());
        s.detach_surfaces();
    }
    assert!(rec.failures.lock().unwrap().is_empty());
    s.close();
}

/// P16 after: the background export (`begin_export_flat` + `run` on a
/// worker) of the styled 20 MP document while a "main thread" keeps making
/// edits: begin (the main-thread part), run, edit call spans, cancel
/// latency. B5-15 builds only.
#[test]
#[ignore]
fn b515_bench_p16_background_export_20mp_styled() {
    let (dir, engine) = engine();
    let s = styled_20mp(&engine);
    let top = s.layers().unwrap()[1].id;
    let mut begins = Vec::new();
    let mut runs = Vec::new();
    let mut edits = Vec::new();
    for i in 0..3 {
        let out = dir.path().join(format!("bg{i}.png"));
        let t = Instant::now();
        let job = s
            .begin_export_flat(
                out.to_string_lossy().into_owned(),
                ExportFormat::Png,
                90,
                ExportColor::Srgb,
            )
            .unwrap();
        begins.push(t.elapsed().as_secs_f64() * 1000.0);
        let t = Instant::now();
        let runner = std::thread::spawn(move || job.run(None));
        while !runner.is_finished() {
            let c = Instant::now();
            s.set_opacity(top, 0.5 + (edits.len() % 5) as f32 * 0.1, true)
                .unwrap();
            edits.push(c.elapsed().as_secs_f64() * 1000.0);
            std::thread::sleep(Duration::from_millis(16));
        }
        runner.join().unwrap().unwrap();
        runs.push(t.elapsed().as_secs_f64() * 1000.0);
        s.commit("Opacity".into()).unwrap();
    }
    summary("p16 begin_export_flat (main thread)", &begins);
    summary("p16 background run (worker)", &runs);
    summary("p16 edit calls during the export", &edits);
    // Cancel latency at ~30 %.
    struct At(Arc<DocFlatExport>, Mutex<Option<Instant>>);
    impl DocExportListener for At {
        fn on_progress(&self, fraction: f32, _: String) {
            if fraction >= 0.3 && self.1.lock().unwrap().is_none() {
                *self.1.lock().unwrap() = Some(Instant::now());
                self.0.cancel();
            }
        }
    }
    let mut lat = Vec::new();
    for i in 0..3 {
        let out = dir.path().join(format!("cancel{i}.png"));
        let job = s
            .begin_export_flat(
                out.to_string_lossy().into_owned(),
                ExportFormat::Png,
                90,
                ExportColor::Srgb,
            )
            .unwrap();
        let at = Arc::new(At(job.clone(), Mutex::new(None)));
        assert!(job.run(Some(at.clone())).is_err());
        let when = at.1.lock().unwrap().expect("reached 30 %");
        lat.push(when.elapsed().as_secs_f64() * 1000.0);
        assert!(!out.exists());
    }
    summary("p16 cancel → run returns", &lat);
    s.close();
}
