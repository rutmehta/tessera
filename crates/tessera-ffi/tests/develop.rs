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
        self.next_final_within(Duration::from_secs(120))
            .unwrap_or_else(|| {
                panic!(
                    "no frame; failures: {:?}",
                    self.events.failed.lock().unwrap()
                )
            })
    }

    /// Like [`Open::next_final`], but None when no newer render finishes.
    fn next_final_within(&mut self, timeout: Duration) -> Option<FrameInfo> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = self.frames.recv_timeout(left).ok()?;
            if frame.generation > self.last_generation && frame.is_final {
                self.last_generation = frame.generation;
                return Some(frame);
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
fn process_version_is_undoable_and_persisted() {
    let Some(h) = harness("nef") else {
        return;
    };
    let session = h
        .engine
        .clone()
        .open_develop_session(h.image_id.clone())
        .unwrap();
    let native = session.get_process_version().unwrap();
    let adobe = r#"{"family":"adobe","revision":6}"#.to_string();
    let n = session.history_state().unwrap().entries;
    assert!(session.set_process_version(adobe.clone()).unwrap());
    assert_eq!(session.history_state().unwrap().entries, n + 1);
    assert!(!session.set_process_version(adobe).unwrap());
    assert!(session.undo().unwrap());
    assert_eq!(session.get_process_version().unwrap(), native);
    assert!(session.redo().unwrap());
    let v: serde_json::Value =
        serde_json::from_str(&session.get_process_version().unwrap()).unwrap();
    assert_eq!(v["family"], "adobe");
    assert!(
        session
            .set_process_version(r#"{"family":"adobe","revision":2}"#.into())
            .is_err()
    );
    session.close().unwrap();
    let reopened = h
        .engine
        .clone()
        .open_develop_session(h.image_id.clone())
        .unwrap();
    let v: serde_json::Value =
        serde_json::from_str(&reopened.get_process_version().unwrap()).unwrap();
    assert_eq!(v["family"], "adobe");
    assert!(reopened.undo().unwrap());
    assert_eq!(reopened.get_process_version().unwrap(), native);
    reopened.close().unwrap();
}

#[test]
fn as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch() {
    use engine_api::recipe::settings::WhiteBalanceMode;
    use engine_api::{color::ColorMatrix3, recipe::DevelopSettings};
    for ext in ["arw", "cr3", "nef", "raf", "dng"] {
        let Some(h) = harness(ext) else { continue };
        let session = h
            .engine
            .clone()
            .open_develop_session(h.image_id.clone())
            .unwrap();
        let info = session.info();
        let raw = image_core::RawImage::open(engine_api::id::ImageId::default(), &h.raw).unwrap();
        let m = raw.metadata();
        let camera = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))
        .unwrap();
        let expected = pipeline_cpu::as_shot_temperature_tint(camera, m.as_shot_wb).unwrap();
        assert_eq!(
            (info.as_shot_temperature, info.as_shot_tint),
            expected,
            "{ext}: unrounded inverse"
        );
        let original = pipeline_cpu::white_balance_matrix(
            &DevelopSettings::default().white_balance,
            camera,
            m.as_shot_wb,
        )
        .unwrap();
        for (key, value) in [("temperature", expected.0), ("tint", expected.1)] {
            session.reset().unwrap();
            session
                .set_settings(
                    serde_json::json!({"white_balance": {key: value}}).to_string(),
                    false,
                )
                .unwrap();
            let settings: DevelopSettings =
                serde_json::from_str(&session.get_settings_json().unwrap()).unwrap();
            assert_eq!(settings.white_balance.mode, WhiteBalanceMode::Custom);
            let custom =
                pipeline_cpu::white_balance_matrix(&settings.white_balance, camera, m.as_shot_wb)
                    .unwrap();
            for (a, b) in original.0.iter().flatten().zip(custom.0.iter().flatten()) {
                assert!((a - b).abs() < 1e-4, "{ext}: first {key} touch jumps");
            }
        }
        session.close().unwrap();
    }
}

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

/// The M2 panels on a session: crop changes the displayed extent (and the
/// crop tool shows the whole frame), the masking preview is an overlay,
/// 1:1 detail crops render into their own surface and history steps can be
/// checked out and toggled.
#[test]
fn panels_crop_masking_detail_and_history() {
    let Some(h) = harness("arw") else { return };
    let mut open = Open::new(&h.engine, &h.image_id);
    let (plan, _) = open.attach((800, 600), 2);
    let first = open.next_final();
    assert_eq!(
        (first.display_width, first.display_height),
        (plan.width, plan.height)
    );
    assert!(!first.is_overlay);

    // Every panel in one patch renders without error.
    open.session
        .set_settings(
            r#"{"tone":{"curves":{"parametric":{"darks":-20},"rgb":[{"x":0,"y":0},{"x":0.5,"y":0.6},{"x":1,"y":1}]}},
                "color":{"hsl":{"saturation":{"blue":-40}},"grading":{"highlights":{"hue":50,"saturation":30}}},
                "detail":{"noise_reduction":{"luminance":20}},
                "effects":{"vignette":{"amount":-30},"grain":{"amount":20}}}"#
                .into(),
            false,
        )
        .unwrap();
    let panels = open.next_final();
    assert!(open.events.failed.lock().unwrap().is_empty());
    assert!(open.session.commit("Panels".into()).unwrap());
    assert!(open.session.ignored_settings().unwrap().is_empty());

    // A half-size crop: the frame reports the cropped picture.
    open.session
        .set_settings(
            r#"{"geometry":{"crop":{"rect":{"left":0.25,"top":0.25,"right":0.75,"bottom":0.75},"angle":2.0}}}"#
                .into(),
            false,
        )
        .unwrap();
    let cropped = open.next_final();
    assert_eq!(cropped.level, plan.level);
    assert_eq!(
        cropped.display_width,
        (plan.width as f32 * 0.5).round() as u32
    );
    assert_eq!(
        cropped.display_height,
        (plan.height as f32 * 0.5).round() as u32
    );
    assert_eq!(
        (cropped.width, cropped.height),
        (cropped.display_width, cropped.display_height)
    );
    assert!(open.session.commit("Crop".into()).unwrap());
    // The crop tool renders the whole frame; leaving it crops again.
    open.session.set_crop_editing(true).unwrap();
    let whole = open.next_final();
    assert_eq!(
        (whole.display_width, whole.display_height),
        (plan.width, plan.height)
    );
    open.session.set_crop_editing(false).unwrap();
    assert_eq!(open.next_final().display_width, cropped.display_width);

    // Masking preview: a grey overlay that follows the Masking slider.
    open.session.set_masking_preview(true).unwrap();
    let mask = open.next_final();
    assert!(mask.is_overlay);
    let white = |f: &FrameInfo| {
        let s = Surface::lookup(f.surface_id, plan.width, plan.height).unwrap();
        s.with_pixels(|px, stride| {
            let mut n = 0u64;
            for y in 0..f.height as usize {
                for x in 0..f.width as usize {
                    let p = &px[y * stride + 4 * x..][..4];
                    assert!(p[0] == p[1] && p[1] == p[2], "grey");
                    n += u64::from(p[0] > 127);
                }
            }
            n as f64 / f64::from(f.width * f.height)
        })
        .unwrap()
    };
    assert_eq!(white(&mask), 1.0, "Masking 0 sharpens everywhere");
    open.session
        .set_settings(r#"{"detail":{"sharpening":{"masking":80}}}"#.into(), true)
        .unwrap();
    let masked = open.next_final();
    assert!(masked.is_overlay);
    let w = white(&masked);
    assert!(w > 0.0 && w < 0.9, "edges only: {w}");
    open.session.set_masking_preview(false).unwrap();
    assert!(!open.next_final().is_overlay);
    assert!(open.session.commit("Masking 80".into()).unwrap());

    // 1:1 detail crop into a separate surface.
    let id = create_rgba8(160, 120);
    let d = open
        .session
        .render_detail_preview(id, 160, 120, 0.5, 0.5)
        .unwrap();
    let info = open.session.info();
    assert_eq!((d.width, d.height), (160, 120));
    assert!(d.x.abs_diff(info.width / 2 - 80) <= 1 && d.y.abs_diff(info.height / 2 - 60) <= 1);
    let lit = Surface::lookup(id, 160, 120)
        .unwrap()
        .with_pixels(|px, stride| (0..120).any(|y| px[y * stride..][..640].iter().any(|&v| v != 0)))
        .unwrap();
    assert!(lit, "detail crop written");
    let corner = open
        .session
        .render_detail_preview(id, 160, 120, 1.0, 1.0)
        .unwrap();
    assert_eq!((corner.x + 160, corner.y + 120), (info.width, info.height));

    // History list, checkout and step toggles.
    let items = open.session.history_items().unwrap();
    let labels: Vec<_> = items.iter().map(|i| i.label.as_str()).collect();
    assert_eq!(labels, ["Panels", "Crop", "Masking 80"]);
    assert!(items[2].is_head && items.iter().all(|i| i.applied && i.enabled));
    assert!(open.session.checkout_history(Some(items[0].id)).unwrap());
    open.next_final();
    let items = open.session.history_items().unwrap();
    assert!(items[0].is_head && !items[1].applied && !items[2].applied);
    assert!(open.session.checkout_history(Some(items[2].id)).unwrap());
    open.next_final();
    assert!(
        open.session
            .set_history_step_enabled(items[1].id, false)
            .unwrap()
    );
    let off = open.next_final();
    assert_eq!(off.display_width, plan.width, "crop step turned off");
    let items = open.session.history_items().unwrap();
    assert_eq!(items.len(), 4);
    assert!(!items[1].enabled && items[3].toggles == Some(items[1].id));
    assert!(
        open.session
            .set_history_step_enabled(items[1].id, true)
            .unwrap()
    );
    assert_eq!(open.next_final().display_width, cropped.display_width);
    assert!(
        open.session
            .set_history_step_enabled(items[3].id, false)
            .is_err()
    );
    assert!(open.session.checkout_history(None).unwrap());
    let base = open.next_final();
    assert_eq!(base.display_width, plan.width);
    drop(panels);
}

/// A drag faster than the frames it causes still shows progress: interactive
/// changes queue behind the in-flight frame instead of cancelling it.
#[test]
fn slow_interactive_frames_are_not_starved() {
    // Interactive-latency behaviour needs a GPU and an unloaded machine; CI runners
    // have neither and time out on the CPU path. It remains mandatory locally.
    if std::env::var_os("CI").is_some() {
        eprintln!("CI: skipping interactive starvation test");
        return;
    }
    let Some(h) = harness("arw") else { return };
    let mut open = Open::new(&h.engine, &h.image_id);
    open.attach((1600, 1200), 2);
    open.next_final();
    let start = Instant::now();
    for i in 0..40 {
        open.session
            .set_settings(
                format!(r#"{{"color":{{"hsl":{{"hue":{{"orange":{i}}}}}}}}}"#),
                true,
            )
            .unwrap();
        std::thread::sleep(Duration::from_millis(4));
    }
    let sent = start.elapsed();
    let mut during = 0;
    let mut drained = None;
    while let Ok(f) = open.frames.try_recv() {
        during += 1;
        open.last_generation = open.last_generation.max(f.generation);
        if f.is_final
            && drained
                .as_ref()
                .is_none_or(|d: &FrameInfo| f.generation >= d.generation)
        {
            drained = Some(f);
        }
    }
    assert!(open.session.commit("HSL".into()).unwrap());
    // Commit re-renders only when the drag has not already drawn the final
    // settings at the screen level. With fast frames every edit is rendered
    // at the screen level during the burst (the latest one was drained).
    let last = open
        .next_final_within(Duration::from_secs(5))
        .or(drained)
        .expect("the final settings are rendered");
    let v: serde_json::Value =
        serde_json::from_str(&open.session.get_settings_json().unwrap()).unwrap();
    assert_eq!(v["color"]["hsl"]["hue"]["orange"], 39.0);
    eprintln!(
        "{during} frames during a {sent:?} burst; final L{}",
        last.level
    );
    assert!(during >= 1, "frames arrive while the drag continues");
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
        let mut drag_level = plan.level;
        for i in 0..40 {
            let ev = -1.0 + f64::from(i) * 0.05;
            open.session
                .set_settings(format!(r#"{{"tone":{{"exposure":{ev}}}}}"#), true)
                .unwrap();
            let f = open.next_final();
            // The drag level adapts to the frame budget (L2 when frames fit).
            drag_level = f.level;
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
            "{} {}×{} L2 {}×{} ({:.1} MP): open {open_ms:.0} ms, first frame {:.0} ms, white balance median {:.0} ms; tone-only (drag at L{drag_level}) median {:.1} ms, p90 {:.1} ms, max {:.1} ms",
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

/// Develop-panel latency: interactive drags of every M2 panel control at the
/// screen level, on each available backend (settings change → frame).
/// `cargo test -p tessera-ffi --release --test develop -- --ignored --nocapture bench_panel`
#[test]
#[ignore]
fn bench_panel_latency() {
    let ext = std::env::var("TESSERA_BENCH_EXT").unwrap_or_else(|_| "nef".into());
    let Some(h) = harness(&ext) else { return };
    let backends = std::env::var("TESSERA_BENCH_BACKENDS").unwrap_or_else(|_| "cpu,gpu".into());
    for backend in backends.split(',') {
        // SAFETY (env): the bench is the only test in this process touching it.
        unsafe { std::env::set_var("TESSERA_RENDER_BACKEND", backend) };
        let engine = Engine::open(format!("{}-panel-{backend}", h.support)).unwrap();
        engine
            .index_folder(h.raw.parent().unwrap().to_string_lossy().into_owned())
            .unwrap();
        let mut open = Open::new(&engine, &h.image_id);
        let info = open.session.info();
        let e2 = (info.width.div_ceil(4), info.height.div_ceil(4));
        let (plan, _) = open.attach(e2, 3);
        open.next_final();
        let only = std::env::var("TESSERA_BENCH_PANELS").ok();
        let panels: Vec<(&str, &dyn Fn(f64) -> String)> = vec![
            ("tone exposure", &|v| {
                format!(r#"{{"tone":{{"exposure":{}}}}}"#, v - 0.5)
            }),
            ("parametric curve", &|v| {
                format!(
                    r#"{{"tone":{{"curves":{{"parametric":{{"lights":{}}}}}}}}}"#,
                    v * 60.0
                )
            }),
            ("point curve", &|v| {
                format!(
                    r#"{{"tone":{{"curves":{{"rgb":[{{"x":0,"y":0}},{{"x":0.5,"y":{}}},{{"x":1,"y":1}}]}}}}}}"#,
                    0.5 + v * 0.2
                )
            }),
            ("hsl", &|v| {
                format!(
                    r#"{{"color":{{"hsl":{{"hue":{{"orange":{}}}}}}}}}"#,
                    v * 60.0
                )
            }),
            ("grading", &|v| {
                format!(
                    r#"{{"color":{{"grading":{{"shadows":{{"hue":220,"saturation":{}}}}}}}}}"#,
                    v * 40.0
                )
            }),
            ("texture", &|v| {
                format!(r#"{{"tone":{{"texture":{}}}}}"#, v * 80.0 - 20.0)
            }),
            ("clarity", &|v| {
                format!(r#"{{"tone":{{"clarity":{}}}}}"#, v * 80.0 - 20.0)
            }),
            ("dehaze", &|v| {
                format!(r#"{{"tone":{{"dehaze":{}}}}}"#, v * 60.0)
            }),
            ("sharpening", &|v| {
                format!(
                    r#"{{"detail":{{"sharpening":{{"amount":{}}}}}}}"#,
                    40.0 + v * 60.0
                )
            }),
            ("luminance nr", &|v| {
                format!(
                    r#"{{"detail":{{"noise_reduction":{{"luminance":{}}}}}}}"#,
                    10.0 + v * 60.0
                )
            }),
            ("color nr", &|v| {
                format!(
                    r#"{{"detail":{{"noise_reduction":{{"color":{}}}}}}}"#,
                    10.0 + v * 60.0
                )
            }),
            ("grain", &|v| {
                format!(
                    r#"{{"effects":{{"grain":{{"amount":{}}}}}}}"#,
                    10.0 + v * 60.0
                )
            }),
            ("vignette", &|v| {
                format!(r#"{{"effects":{{"vignette":{{"amount":{}}}}}}}"#, -v * 60.0)
            }),
            ("straighten", &|v| {
                format!(
                    r#"{{"geometry":{{"crop":{{"rect":{{"left":0.1,"top":0.1,"right":0.9,"bottom":0.9}},"angle":{}}}}}}}"#,
                    v * 5.0
                )
            }),
        ];
        let mut lines = Vec::new();
        for (name, patch) in panels {
            if only
                .as_ref()
                .is_some_and(|o| !o.split(',').any(|n| n.trim() == name))
            {
                continue;
            }
            let mut samples = Vec::new();
            let mut level = 0;
            // From i = 1: a patch that changes nothing renders nothing.
            for i in 1..25 {
                open.session
                    .set_settings(patch(f64::from(i) / 24.0), true)
                    .unwrap();
                let f = open.next_final();
                level = f.level;
                // The drag level adapts over the first frames of a session.
                if i > 8 {
                    samples.push(f.render_ms);
                }
            }
            if open.session.commit(name.into()).unwrap() {
                // Mouse-up refines only when the drag rendered coarser.
                if level != plan.level {
                    open.next_final();
                }
            }
            samples.sort_by(f64::total_cmp);
            let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
            lines.push(format!(
                "  {name:<17} L{level}: median {:.1} ms, p90 {:.1} ms",
                p(0.5),
                p(0.9)
            ));
            open.session.reset().unwrap();
            open.next_final();
        }
        println!(
            "{} panels, screen L{} {}×{}:\n{}",
            info.backend,
            plan.level,
            plan.width,
            plan.height,
            lines.join("\n")
        );
        drop(open);
    }
}

/// A settings patch for slider value `v`.
type Patch = fn(i32) -> String;

/// 1:1 region refinement: `render_detail_preview` of a 1024² window (the
/// loupe) after each heavy edit, on each available backend.
/// `cargo test -p tessera-ffi --release --test develop -- --ignored --nocapture bench_detail_preview`
#[test]
#[ignore]
fn bench_detail_preview() {
    let ext = std::env::var("TESSERA_BENCH_EXT").unwrap_or_else(|_| "nef".into());
    let Some(h) = harness(&ext) else { return };
    let backends = std::env::var("TESSERA_BENCH_BACKENDS").unwrap_or_else(|_| "cpu,gpu".into());
    for backend in backends.split(',') {
        // SAFETY (env): the bench is the only test in this process touching it.
        unsafe { std::env::set_var("TESSERA_RENDER_BACKEND", backend) };
        let engine = Engine::open(format!("{}-loupe-{backend}", h.support)).unwrap();
        engine
            .index_folder(h.raw.parent().unwrap().to_string_lossy().into_owned())
            .unwrap();
        let open = Open::new(&engine, &h.image_id);
        let info = open.session.info();
        let id = create_rgba8(1024, 1024);
        let mut lines = Vec::new();
        let ops: [(&str, Patch); 5] = [
            ("tone exposure", |v| {
                format!(r#"{{"tone":{{"exposure":{}}}}}"#, f64::from(v) / 100.0)
            }),
            ("texture", |v| {
                format!(r#"{{"tone":{{"exposure":0.0,"texture":{v}}}}}"#)
            }),
            ("clarity", |v| {
                format!(r#"{{"tone":{{"texture":0,"clarity":{v}}}}}"#)
            }),
            ("dehaze", |v| {
                format!(r#"{{"tone":{{"clarity":0,"dehaze":{v}}}}}"#)
            }),
            ("luminance nr", |v| {
                format!(
                    r#"{{"tone":{{"dehaze":0}},"detail":{{"noise_reduction":{{"luminance":{v}}}}}}}"#
                )
            }),
        ];
        for (op, (name, patch)) in ops.into_iter().enumerate() {
            open.session.set_settings(patch(30), false).unwrap();
            // Cold: every refinement pans to a window not rendered before
            // (decode through Develop for the window, then the edit).
            let mut cold = Vec::new();
            for i in 0..6 {
                let t = Instant::now();
                open.session
                    .render_detail_preview(
                        id,
                        1024,
                        1024,
                        0.2 + 0.1 * i as f32,
                        0.15 + 0.14 * op as f32,
                    )
                    .unwrap();
                cold.push(t.elapsed().as_secs_f64() * 1e3);
            }
            // Warm: the same window again after an edit (the loupe while
            // dragging a slider).
            let mut samples = Vec::new();
            for i in 0..6 {
                open.session.set_settings(patch(31 + i), false).unwrap();
                let t = Instant::now();
                open.session
                    .render_detail_preview(id, 1024, 1024, 0.7, 0.15 + 0.14 * op as f32)
                    .unwrap();
                samples.push(t.elapsed().as_secs_f64() * 1e3);
            }
            cold.sort_by(f64::total_cmp);
            samples.sort_by(f64::total_cmp);
            let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
            lines.push(format!(
                "  {name:<14} 1:1 1024²: cold pan median {:.1} ms, p90 {:.1} ms; edit median {:.1} ms, p90 {:.1} ms",
                cold[cold.len() / 2],
                cold[(cold.len() - 1) * 9 / 10],
                p(0.5),
                p(0.9)
            ));
        }
        println!("{} loupe refinement:\n{}", info.backend, lines.join("\n"));
    }
}

struct ExportProgressLog(Mutex<Vec<(Instant, u32)>>);
impl ExportProgressListener for ExportProgressLog {
    fn on_progress(&self, progress: ExportProgress) {
        self.0.lock().unwrap().push((Instant::now(), progress.done));
    }
}

/// A full-size export batch of the five RAW fixtures runs while a develop
/// session drags a slider at L2. Export bands are Export-priority work that
/// yields to interactive renders: slider frames keep p90 < 16 ms at L2, and
/// the export still completes (neither side starves).
#[test]
fn export_batch_does_not_starve_slider_drag() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let names = [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ];
    if names.iter().any(|n| !root.join(n).is_file()) {
        eprintln!("skipping: five RAW fixtures required in {}", root.display());
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for n in names {
        std::fs::copy(root.join(n), photos.join(n)).unwrap();
    }
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let images = engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(images.len(), 5);
    let nef = images
        .iter()
        .find(|i| i.path.ends_with(".NEF"))
        .unwrap()
        .id
        .clone();
    let mut open = Open::new(&engine, &nef);
    let info = open.session.info();
    let (plan, _) = open.attach((info.width.div_ceil(4), info.height.div_ceil(4)), 3);
    assert_eq!(plan.level, 2);
    open.next_final();
    // Warm the drag path (level adaptation, pipelines) before the export.
    for i in 0..8 {
        open.session
            .set_settings(
                format!(
                    r#"{{"tone":{{"exposure":{}}}}}"#,
                    0.05 + f64::from(i) * 0.01
                ),
                true,
            )
            .unwrap();
        open.next_final();
    }

    // The same drag with no export running: the reference latency.
    let mut idle = Vec::new();
    for i in 0..120 {
        let sent = Instant::now();
        open.session
            .set_settings(
                format!(
                    r#"{{"tone":{{"exposure":{}}}}}"#,
                    1.0 - f64::from(i) * 0.015
                ),
                true,
            )
            .unwrap();
        idle.push(open.next_final().render_ms);
        std::thread::sleep(Duration::from_millis(16).saturating_sub(sent.elapsed()));
    }
    idle.sort_by(f64::total_cmp);

    let out = dir.path().join("out");
    let progress = Arc::new(ExportProgressLog(Mutex::new(Vec::new())));
    let export = {
        let engine = engine.clone();
        let ids = images.iter().map(|i| i.id.clone()).collect();
        let settings = serde_json::json!({
            "destination": out.to_string_lossy(),
            "quality": 90,
            "metadata": "none",
        })
        .to_string();
        let progress = progress.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let report = engine
                .export_batch(
                    ExportTarget::Images { image_ids: ids },
                    settings,
                    Some(progress),
                    None,
                )
                .unwrap();
            (report, started.elapsed())
        })
    };
    // Let the export get past its first decode into GPU rendering.
    std::thread::sleep(Duration::from_millis(1500));
    let mut samples = Vec::new();
    let mut levels = Vec::new();
    let drag = Instant::now();
    for i in 0..120 {
        let sent = Instant::now();
        open.session
            .set_settings(
                format!(
                    r#"{{"tone":{{"exposure":{}}}}}"#,
                    -1.0 + f64::from(i) * 0.015
                ),
                true,
            )
            .unwrap();
        let f = open.next_final();
        samples.push((f.render_ms, sent.elapsed().as_secs_f64() * 1e3));
        levels.push(f.level);
        // A 60 Hz pointer: the next event arrives one display frame later.
        std::thread::sleep(Duration::from_millis(16).saturating_sub(sent.elapsed()));
    }
    let dragged = drag.elapsed();
    assert!(open.session.commit("Exposure".into()).unwrap());
    let (report, seconds) = export.join().unwrap();
    let mut render: Vec<f64> = samples.iter().map(|s| s.0).collect();
    let mut latency: Vec<f64> = samples.iter().map(|s| s.1).collect();
    render.sort_by(f64::total_cmp);
    latency.sort_by(f64::total_cmp);
    let p = |v: &[f64], q: f64| v[((v.len() - 1) as f64 * q) as usize];
    let during = progress
        .0
        .lock()
        .unwrap()
        .iter()
        .filter(|(t, _)| *t >= drag && *t <= drag + dragged)
        .count();
    let at_l2 = levels.iter().filter(|&&l| l == 2).count();
    eprintln!(
        "slider without export: render p50 {:.1} ms p90 {:.1} ms max {:.1} ms",
        p(&idle, 0.5),
        p(&idle, 0.9),
        p(&idle, 1.0),
    );
    eprintln!(
        "slider during export: {} frames in {dragged:?} ({at_l2} at L2): render p50 {:.1} ms p90 {:.1} ms max {:.1} ms; set→frame p50 {:.1} ms p90 {:.1} ms max {:.1} ms; export {} images in {seconds:?}, {during} completed during the drag",
        samples.len(),
        p(&render, 0.5),
        p(&render, 0.9),
        p(&render, 1.0),
        p(&latency, 0.5),
        p(&latency, 0.9),
        p(&latency, 1.0),
        report.exported,
    );
    assert_eq!((report.exported, report.failed), (5, 0), "{report:?}");
    assert!(
        at_l2 * 10 >= levels.len() * 9,
        "drag stays at L2: {levels:?}"
    );
    assert!(
        p(&render, 0.9) < 16.0,
        "slider p90 {:.1} ms",
        p(&render, 0.9)
    );
}
