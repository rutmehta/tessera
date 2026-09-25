//! Develop session over the bridge: IOSurface writer, rendering, history and
//! persistence. Session tests use a real RAW fixture and skip without one.
#![cfg(target_os = "macos")]

use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tessera_ffi::surface::{Surface, testing::create_rgba8, write_rgba8};
use tessera_ffi::*;

#[test]
fn iosurface_writer_round_trips_a_known_pattern() {
    let (w, h) = (300, 270);
    let id = create_rgba8(w, h);
    let surface = Surface::lookup(id, w, h).expect("in-process lookup");
    assert!(Surface::lookup(id, w + 1, h).is_err(), "size is checked");
    // Tile (1, 1) at level 0 starts at (256, 256): only 44 × 14 pixels fit.
    let layout = TileLayout {
        extent: Extent::new(256, 256),
        halo: 0,
        channels: 3,
    };
    let n = layout.plane_len();
    let mut data = vec![0u8; 3 * n];
    for y in 0..256usize {
        for x in 0..256usize {
            let i = y * 256 + x;
            data[i] = x as u8;
            data[n + i] = y as u8;
            data[2 * n + i] = (x ^ y) as u8;
        }
    }
    let tile = Tile::from_samples(TileCoord::new(0, 1, 1), layout, data).unwrap();
    surface.write_tile(&tile).unwrap();
    let origin = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(2, 1),
            halo: 0,
            channels: 3,
        },
        vec![10u8, 20, 30, 40, 50, 60],
    )
    .unwrap();
    surface.write_tile(&origin).unwrap();
    surface
        .with_pixels(|px, stride| {
            assert!(stride >= w as usize * 4);
            assert_eq!(&px[0..8], &[10, 30, 50, 255, 20, 40, 60, 255]);
            for (x, y) in [
                (256usize, 256usize),
                (299, 256),
                (256, 269),
                (299, 269),
                (280, 260),
            ] {
                let o = y * stride + x * 4;
                let (tx, ty) = (x - 256, y - 256);
                assert_eq!(
                    &px[o..o + 4],
                    &[tx as u8, ty as u8, (tx ^ ty) as u8, 255],
                    "pixel {x},{y}"
                );
            }
        })
        .unwrap();
    // The pure writer clips tiles that start outside the surface.
    let mut buf = vec![0u8; 16];
    write_rgba8(&mut buf, 8, 0, 2, &tile).unwrap();
    assert!(buf.iter().all(|&b| b == 0));
    // A band of rows 260..262 receives only those rows of the tile.
    let mut band = vec![0u8; 2 * 300 * 4];
    write_rgba8(&mut band, 300 * 4, 260, 300, &tile).unwrap();
    assert_eq!(&band[256 * 4..256 * 4 + 4], &[0, 4, 4, 255]);
    assert_eq!(
        &band[300 * 4 + 257 * 4..300 * 4 + 257 * 4 + 4],
        &[1, 5, 4, 255]
    );
}

// ───────────────────────────── session helpers ─────────────────────────────

fn fixture(ext: &str) -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let found = std::fs::read_dir(&root)
        .ok()?
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)));
    if found.is_none() {
        eprintln!("skipping: no .{ext} fixture in {}", root.display());
    }
    found
}

#[derive(Default)]
struct Events {
    saved: Mutex<Vec<String>>,
    failed: Mutex<Vec<String>>,
}
struct Listener {
    frames: Mutex<mpsc::Sender<FrameInfo>>,
    events: Arc<Events>,
}
impl DevelopListener for Listener {
    fn frame_ready(&self, frame: FrameInfo) {
        let _ = self.frames.lock().unwrap().send(frame);
    }
    fn render_failed(&self, message: String) {
        self.events.failed.lock().unwrap().push(message);
    }
    fn saved(&self, recipe_hash: String) {
        self.events.saved.lock().unwrap().push(recipe_hash);
    }
}

struct Harness {
    _dir: tempfile::TempDir,
    support: String,
    raw: PathBuf,
    engine: Arc<Engine>,
    image_id: String,
}

fn harness(ext: &str) -> Option<Harness> {
    let src = fixture(ext)?;
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let raw = photos.join(src.file_name().unwrap());
    std::fs::copy(&src, &raw).unwrap();
    let support = dir.path().join("support").to_string_lossy().into_owned();
    let engine = Engine::open(support.clone()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let image_id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    Some(Harness {
        _dir: dir,
        support,
        raw,
        engine,
        image_id,
    })
}

struct Open {
    session: Arc<DevelopSession>,
    frames: mpsc::Receiver<FrameInfo>,
    events: Arc<Events>,
    last_generation: u64,
}

impl Open {
    fn new(engine: &Arc<Engine>, image_id: &str) -> Self {
        let session = engine
            .clone()
            .open_develop_session(image_id.into())
            .unwrap();
        let (tx, frames) = mpsc::channel();
        let events = Arc::new(Events::default());
        session.set_listener(Some(Arc::new(Listener {
            frames: Mutex::new(tx),
            events: events.clone(),
        })));
        Self {
            session,
            frames,
            events,
            last_generation: 0,
        }
    }

    /// The final frame of the next render after the last one observed.
    fn next_final(&mut self) -> FrameInfo {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = self.frames.recv_timeout(left).unwrap_or_else(|_| {
                panic!(
                    "no frame; failures: {:?}",
                    self.events.failed.lock().unwrap()
                )
            });
            if frame.generation > self.last_generation && frame.is_final {
                self.last_generation = frame.generation;
                return frame;
            }
        }
    }

    fn attach(&mut self, view: (u32, u32), count: usize) -> (SurfacePlan, Vec<u32>) {
        let plan = self.session.plan_surface(view.0, view.1);
        let ids: Vec<u32> = (0..count)
            .map(|_| create_rgba8(plan.width, plan.height))
            .collect();
        for &id in &ids {
            self.session
                .attach_surface(id, plan.width, plan.height)
                .unwrap();
        }
        (plan, ids)
    }
}

fn exposure(session: &DevelopSession) -> f64 {
    let v: serde_json::Value = serde_json::from_str(&session.get_settings_json().unwrap()).unwrap();
    v["tone"]["exposure"].as_f64().unwrap()
}

fn mean(h: &[u32]) -> f64 {
    let n: u64 = h.iter().map(|&c| u64::from(c)).sum();
    h.iter()
        .enumerate()
        .map(|(i, &c)| i as f64 * f64::from(c))
        .sum::<f64>()
        / n as f64
}

// ─────────────────────────────── session tests ───────────────────────────────

#[test]
fn session_renders_into_surfaces_and_persists_undoable_edits() {
    let Some(h) = harness("arw") else { return };
    let mut open = Open::new(&h.engine, &h.image_id);
    let info = open.session.info();
    assert_eq!(info.image_id, h.image_id);
    assert!(info.width > 1000 && info.height > 1000);
    assert!((2000.0..=25000.0).contains(&info.as_shot_temperature));

    let (plan, ids) = open.attach((800, 600), 2);
    assert!(plan.width >= 800 && plan.height >= 600);
    assert!(
        plan.width < 1600 || plan.height < 1200,
        "coarsest covering level"
    );
    let first = open.next_final();
    assert!(ids.contains(&first.surface_id));
    assert_eq!((first.width, first.height), (plan.width, plan.height));
    assert_eq!(first.level, plan.level);
    let before = open.session.get_histogram().unwrap();
    assert_eq!(before.generation, first.generation);
    assert_eq!(before.luminance.len(), 256);
    let pixels: u64 = before.luminance.iter().map(|&c| u64::from(c)).sum();
    assert_eq!(pixels, u64::from(plan.width) * u64::from(plan.height));
    // The surface holds the frame: its mean matches the histogram.
    let surface = Surface::lookup(first.surface_id, plan.width, plan.height).unwrap();
    let green_mean = surface
        .with_pixels(|px, stride| {
            let mut sum = 0u64;
            for y in 0..plan.height as usize {
                for x in 0..plan.width as usize {
                    sum += u64::from(px[y * stride + x * 4 + 1]);
                }
            }
            sum as f64 / pixels as f64
        })
        .unwrap();
    assert!((green_mean - mean(&before.green)).abs() < 1e-6);

    // Tone-only change: a new frame in the other surface, brighter.
    open.session
        .set_settings(r#"{"tone":{"exposure":1.0}}"#.into(), true)
        .unwrap();
    let bright = open.next_final();
    assert_eq!(bright.dirty_stage.as_deref(), Some("Tone"));
    assert_ne!(bright.surface_id, first.surface_id, "ring alternates");
    let after = open.session.get_histogram().unwrap();
    assert!(mean(&after.luminance) > mean(&before.luminance) + 10.0);
    assert!(open.session.commit("Exposure +1.00".into()).unwrap());

    // White balance switches to custom and invalidates from WhiteBalance.
    open.session
        .set_settings(r#"{"white_balance":{"temperature":3200}}"#.into(), false)
        .unwrap();
    let wb = open.next_final();
    assert_eq!(wb.dirty_stage.as_deref(), Some("WhiteBalance"));
    let s: serde_json::Value =
        serde_json::from_str(&open.session.get_settings_json().unwrap()).unwrap();
    assert_eq!(s["white_balance"]["mode"], "custom");
    assert!(open.session.commit("Temperature".into()).unwrap());

    // Undo / redo / snapshots / reset.
    assert!(open.session.undo().unwrap());
    open.next_final();
    let s: serde_json::Value =
        serde_json::from_str(&open.session.get_settings_json().unwrap()).unwrap();
    assert_eq!(s["white_balance"]["mode"], "as_shot");
    assert_eq!(exposure(&open.session), 1.0);
    assert!(open.session.redo().unwrap());
    open.next_final();
    assert!(open.session.undo().unwrap());
    open.next_final();
    open.session.snapshot("Bright".into()).unwrap();
    assert!(open.session.reset().unwrap());
    open.next_final();
    assert_eq!(exposure(&open.session), 0.0);
    let state = open.session.history_state().unwrap();
    assert!(state.can_undo);
    assert_eq!(state.head_label.as_deref(), Some("Reset"));
    assert_eq!(state.snapshots, vec!["Bright".to_owned()]);
    open.session.restore_snapshot("Bright".into()).unwrap();
    open.next_final();
    assert_eq!(exposure(&open.session), 1.0);
    open.session.flush().unwrap();
    assert!(!open.events.saved.lock().unwrap().is_empty());
    assert!(open.events.failed.lock().unwrap().is_empty());

    // Persisted to the recipe JSON and the XMP.
    let recipe: serde_json::Value =
        serde_json::from_str(&h.engine.get_recipe(h.image_id.clone()).unwrap()).unwrap();
    assert_eq!(recipe["settings"]["tone"]["exposure"], 1.0);
    assert!(recipe["history"]["entries"].as_array().unwrap().len() >= 3);
    let xmp = std::fs::read_to_string(sidecar::Sidecar::paths(&h.raw).xmp).unwrap();
    assert!(xmp.contains("Exposure2012"), "{xmp}");
    open.session.close().unwrap();
    drop(open);

    // Reopening (new engine, as after a relaunch) restores the edit.
    drop(h.engine);
    let engine = Engine::open(h.support.clone()).unwrap();
    let rows = engine.list_images(ImageQuery::default()).unwrap();
    let default_hash = rows[0].recipe_hash.clone();
    let mut reopened = Open::new(&engine, &h.image_id);
    assert_eq!(exposure(&reopened.session), 1.0);
    assert!(reopened.session.history_state().unwrap().can_undo);
    reopened.attach((400, 300), 1);
    reopened.next_final();
    assert!(
        mean(&reopened.session.get_histogram().unwrap().luminance) > mean(&before.luminance) + 10.0
    );
    assert!(!default_hash.is_empty());
}

#[test]
fn edited_previews_follow_the_recipe_hash() {
    let Some(h) = harness("arw") else { return };
    let (tx, rx) = mpsc::channel();
    struct Ready(Mutex<mpsc::Sender<(String, u32)>>);
    impl EngineEventListener for Ready {
        fn on_event(&self, event: EngineEvent) {
            if let EngineEvent::PreviewReady { image_id, max_px } = event {
                let _ = self.0.lock().unwrap().send((image_id, max_px));
            }
        }
    }
    h.engine
        .set_event_listener(Some(Arc::new(Ready(Mutex::new(tx)))));
    let fetch = |engine: &Arc<Engine>| -> Vec<u8> {
        for _ in 0..3 {
            let r = engine
                .clone()
                .embedded_preview(h.image_id.clone(), 256)
                .unwrap();
            if let Some(bytes) = r.bytes {
                return bytes;
            }
            rx.recv_timeout(Duration::from_secs(120)).unwrap();
        }
        panic!("preview never became ready");
    };
    let original = fetch(&h.engine);
    let mut open = Open::new(&h.engine, &h.image_id);
    open.attach((600, 400), 2);
    open.next_final();
    open.session
        .set_settings(r#"{"tone":{"exposure":-2.0}}"#.into(), false)
        .unwrap();
    open.next_final();
    open.session.commit("Exposure".into()).unwrap();
    open.session.flush().unwrap();
    let hash = open.events.saved.lock().unwrap().last().cloned().unwrap();
    let rows = h.engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(rows[0].recipe_hash, hash, "index follows the saved recipe");
    let edited = fetch(&h.engine);
    assert_ne!(original, edited);
    let luma = |jpeg: &[u8]| -> f64 {
        let img = image::load_from_memory(jpeg).unwrap().to_luma8();
        img.pixels().map(|p| f64::from(p.0[0])).sum::<f64>() / f64::from(img.width() * img.height())
    };
    assert!(
        luma(&edited) < luma(&original) - 10.0,
        "edited preview is darker"
    );
}

#[test]
fn jpeg_images_are_refused() {
    let dir = tempfile::tempdir().unwrap();
    image::RgbImage::new(8, 8)
        .save(dir.path().join("a.jpg"))
        .unwrap();
    let engine = Engine::open(dir.path().join("s").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(dir.path().to_string_lossy().into_owned())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let err = engine.clone().open_develop_session(id).err().unwrap();
    assert!(err.to_string().contains("RAW"));
}

/// Slider latency: tone-only edits at level 2 (settings change → frame in the
/// IOSurface), on each available backend.
/// `cargo test -p tessera-ffi --release --test develop -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_slider_latency() {
    let ext = std::env::var("TESSERA_BENCH_EXT").unwrap_or_else(|_| "nef".into());
    let Some(h) = harness(&ext) else { return };
    for backend in ["cpu", "gpu"] {
        // SAFETY (env): the bench is the only test in this process touching it.
        unsafe { std::env::set_var("TESSERA_RENDER_BACKEND", backend) };
        let engine = Engine::open(format!("{}-{backend}", h.support)).unwrap();
        engine
            .index_folder(h.raw.parent().unwrap().to_string_lossy().into_owned())
            .unwrap();
        let t = Instant::now();
        let mut open = Open::new(&engine, &h.image_id);
        let info = open.session.info();
        let open_ms = t.elapsed().as_secs_f64() * 1e3;
        let e2 = (info.width.div_ceil(4), info.height.div_ceil(4));
        let (plan, _) = open.attach(e2, 3);
        assert_eq!(plan.level, 2);
        let cold = open.next_final();
        let mut samples = Vec::new();
        for i in 0..40 {
            let ev = -1.0 + f64::from(i) * 0.05;
            open.session
                .set_settings(format!(r#"{{"tone":{{"exposure":{ev}}}}}"#), true)
                .unwrap();
            let f = open.next_final();
            assert_eq!(f.level, 2);
            samples.push(f.render_ms);
        }
        let mut wb = Vec::new();
        for i in 0..6 {
            open.session
                .set_settings(
                    format!(
                        r#"{{"white_balance":{{"temperature":{}}}}}"#,
                        4000 + i * 300
                    ),
                    false,
                )
                .unwrap();
            wb.push(open.next_final().render_ms);
        }
        wb.sort_by(f64::total_cmp);
        samples.sort_by(f64::total_cmp);
        let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
        println!(
            "{} {}×{} L2 {}×{} ({:.1} MP): open {open_ms:.0} ms, first frame {:.0} ms, white balance median {:.0} ms; tone-only median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
            info.backend,
            info.width,
            info.height,
            plan.width,
            plan.height,
            f64::from(plan.width * plan.height) / 1e6,
            cold.render_ms,
            wb[wb.len() / 2],
            p(0.5),
            p(0.9),
            p(1.0),
        );
        drop(open);
    }
}
