//! Masking over the bridge (M2-14): mask groups, brush streaming, range and
//! AI masks, the overlay plane, undo and persistence. Uses a real RAW fixture
//! and skips without one.
#![cfg(target_os = "macos")]

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tessera_ffi::surface::testing::{create_r8, create_rgba8};
use tessera_ffi::*;

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

struct Listener {
    frames: Mutex<mpsc::Sender<FrameInfo>>,
    failed: Arc<Mutex<Vec<String>>>,
}
impl DevelopListener for Listener {
    fn frame_ready(&self, frame: FrameInfo) {
        let _ = self.frames.lock().unwrap().send(frame);
    }
    fn render_failed(&self, message: String) {
        self.failed.lock().unwrap().push(message);
    }
    fn saved(&self, _recipe_hash: String) {}
}

struct Masks {
    overlays: Mutex<mpsc::Sender<MaskOverlayFrame>>,
    jobs: Mutex<mpsc::Sender<MaskJobUpdate>>,
}
impl MaskListener for Masks {
    fn overlay_ready(&self, frame: MaskOverlayFrame) {
        let _ = self.overlays.lock().unwrap().send(frame);
    }
    fn ai_progress(&self, update: MaskJobUpdate) {
        let _ = self.jobs.lock().unwrap().send(update);
    }
}

struct Open {
    _dir: tempfile::TempDir,
    engine: Arc<Engine>,
    image_id: String,
    session: Arc<DevelopSession>,
    frames: mpsc::Receiver<FrameInfo>,
    overlays: mpsc::Receiver<MaskOverlayFrame>,
    jobs: mpsc::Receiver<MaskJobUpdate>,
    failed: Arc<Mutex<Vec<String>>>,
    last: u64,
}

const WAIT: Duration = Duration::from_secs(120);

impl Open {
    fn new(ext: &str) -> Option<Self> {
        let src = fixture(ext)?;
        // A scratch copy: sidecars are written next to it.
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        std::fs::copy(&src, photos.join(src.file_name().unwrap())).unwrap();
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let image_id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        let mut open = Self::session(dir, engine, image_id);
        open.attach();
        Some(open)
    }

    fn session(dir: tempfile::TempDir, engine: Arc<Engine>, image_id: String) -> Self {
        let session = engine
            .clone()
            .open_develop_session(image_id.clone())
            .unwrap();
        let (tx, frames) = mpsc::channel();
        let failed = Arc::new(Mutex::new(Vec::new()));
        session.set_listener(Some(Arc::new(Listener {
            frames: Mutex::new(tx),
            failed: failed.clone(),
        })));
        let (otx, overlays) = mpsc::channel();
        let (jtx, jobs) = mpsc::channel();
        session.set_mask_listener(Some(Arc::new(Masks {
            overlays: Mutex::new(otx),
            jobs: Mutex::new(jtx),
        })));
        Self {
            _dir: dir,
            engine,
            image_id,
            session,
            frames,
            overlays,
            jobs,
            failed,
            last: 0,
        }
    }

    fn attach(&mut self) -> SurfacePlan {
        let plan = self.session.plan_surface(640, 480);
        for _ in 0..2 {
            let id = create_rgba8(plan.width, plan.height);
            self.session
                .attach_surface(id, plan.width, plan.height)
                .unwrap();
            let overlay = create_r8(plan.width, plan.height);
            self.session
                .attach_mask_overlay_surface(overlay, plan.width, plan.height)
                .unwrap();
        }
        self.next_final();
        plan
    }

    fn next_final(&mut self) -> FrameInfo {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = self.frames.recv_timeout(left).unwrap_or_else(|_| {
                panic!("no frame; failures: {:?}", self.failed.lock().unwrap())
            });
            if frame.generation > self.last && frame.is_final {
                self.last = frame.generation;
                return frame;
            }
        }
    }

    /// The latest overlay written for `group` (waiting for at least one).
    fn overlay(&self, group: u32) -> MaskOverlayFrame {
        let deadline = Instant::now() + WAIT;
        let mut last = None;
        loop {
            let wait = if last.is_some() {
                Duration::from_millis(300)
            } else {
                deadline.saturating_duration_since(Instant::now())
            };
            match self.overlays.recv_timeout(wait) {
                Ok(f) if f.group_id == group => last = Some(f),
                Ok(_) => {}
                Err(_) => return last.expect("no overlay"),
            }
        }
    }

    fn drain_overlays(&self) {
        while self.overlays.try_recv().is_ok() {}
    }

    fn mean_luminance(&self) -> f64 {
        let h = self.session.get_histogram().unwrap().luminance;
        let n: u64 = h.iter().map(|&c| u64::from(c)).sum();
        h.iter()
            .enumerate()
            .map(|(i, &c)| i as f64 * f64::from(c))
            .sum::<f64>()
            / n as f64
    }

    fn assert_no_failures(&self) {
        assert!(
            self.failed.lock().unwrap().is_empty(),
            "{:?}",
            self.failed.lock().unwrap()
        );
    }
}

#[test]
fn gradient_brush_range_overlay_undo_and_persistence() {
    let Some(mut o) = Open::new("arw") else {
        return;
    };
    let base = o.mean_luminance();

    // A linear gradient over the top half with local exposure +1.
    let linear = o
        .session
        .add_mask(
            r#"{"kind":"linear","start":[0.5,0.0],"end":[0.5,0.5]}"#.into(),
            false,
        )
        .unwrap();
    o.next_final();
    o.session
        .set_mask_param(linear, "exposure".into(), 1.0, false)
        .unwrap();
    o.next_final();
    o.assert_no_failures();
    assert!(o.mean_luminance() > base + 2.0, "local exposure brightens");
    assert!(o.session.commit("Linear Gradient".into()).unwrap());
    let groups = o.session.mask_groups().unwrap();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name, "Mask 1");
    assert_eq!(groups[0].components[0].kind, MaskComponentType::Linear);
    let exposure = groups[0]
        .params
        .iter()
        .find(|p| p.name == "exposure")
        .unwrap();
    assert_eq!(exposure.value, 1.0);

    // The overlay plane: fully on at the top, off from the middle down.
    o.drain_overlays();
    o.session.set_mask_overlay(Some(linear)).unwrap();
    let f = o.overlay(linear);
    assert!(
        (0.18..0.32).contains(&f.coverage),
        "gradient coverage {}",
        f.coverage
    );
    let thumb = o.session.mask_thumbnail(linear, 64).unwrap().unwrap();
    assert_eq!(thumb.width.max(thumb.height), 64);
    assert_eq!(thumb.alpha.len(), (thumb.width * thumb.height) as usize);

    // Brush streaming: one stroke in two per-frame batches is one undo step.
    let brush = BrushSettings {
        radius: 0.05,
        feather: 50.0,
        flow: 100.0,
        erase: false,
    };
    let painted = o.session.begin_brush_stroke(None, brush).unwrap();
    assert_ne!(painted, linear);
    let line = |from: f32, to: f32| -> Vec<BrushPoint> {
        (0..=10)
            .map(|i| BrushPoint {
                x: from + (to - from) * i as f32 / 10.0,
                y: 0.75,
                pressure: 1.0,
            })
            .collect()
    };
    o.session.add_brush_points(line(0.2, 0.5)).unwrap();
    o.session.add_brush_points(line(0.5, 0.8)).unwrap();
    o.session.end_brush_stroke().unwrap();
    o.next_final();
    o.session
        .set_mask_param(painted, "exposure".into(), -1.0, false)
        .unwrap();
    o.next_final();
    assert!(o.session.commit("Brush Stroke".into()).unwrap());
    let groups = o.session.mask_groups().unwrap();
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[1].components[0].title, "Brush (1 stroke)");
    o.drain_overlays();
    o.session.set_mask_overlay(Some(painted)).unwrap();
    let painted_cover = o.overlay(painted).coverage;
    assert!(painted_cover > 0.02, "brush coverage {painted_cover}");

    // Erasing through the same brush shrinks it.
    o.session
        .begin_brush_stroke(
            Some(painted),
            BrushSettings {
                erase: true,
                ..brush
            },
        )
        .unwrap();
    o.session.add_brush_points(line(0.2, 0.5)).unwrap();
    o.session.end_brush_stroke().unwrap();
    o.next_final();
    let erased = o.overlay(painted).coverage;
    assert!(
        erased < painted_cover * 0.8,
        "erase {erased} vs {painted_cover}"
    );
    assert!(o.session.commit("Erase".into()).unwrap());

    // Undo the erase and the brush group; redo brings them back.
    o.session.undo().unwrap();
    o.session.undo().unwrap();
    o.next_final();
    assert_eq!(o.session.mask_groups().unwrap().len(), 1);
    o.session.redo().unwrap();
    o.next_final();
    assert_eq!(o.session.mask_groups().unwrap().len(), 2);

    // Group edits: amount, invert, duplicate, delete, component modes.
    o.session
        .update_mask_group(
            linear,
            MaskGroupPatch {
                amount: Some(50.0),
                invert: Some(true),
                name: Some("Sky Grad".into()),
                ..Default::default()
            },
            false,
        )
        .unwrap();
    o.next_final();
    let copy = o.session.duplicate_mask(linear).unwrap();
    o.next_final();
    let groups = o.session.mask_groups().unwrap();
    assert_eq!(groups.len(), 3);
    assert_eq!(groups[1].id, copy, "the copy follows its original");
    assert_eq!(groups[1].name, "Sky Grad Copy");
    o.session
        .add_mask_component(
            copy,
            r#"{"kind":"radial","center":[0.5,0.5],"radii":[0.2,0.2],"feather":50}"#.into(),
            MaskCombineMode::Intersect,
            false,
        )
        .unwrap();
    o.next_final();
    o.session
        .set_mask_component_mode(copy, 1, MaskCombineMode::Subtract, true)
        .unwrap();
    o.next_final();
    o.session.delete_mask(copy).unwrap();
    o.next_final();

    // A luminance range picked from the image.
    let range = o
        .session
        .add_range_mask(None, RangeKind::Luminance, 0.5, 0.5, MaskCombineMode::Add)
        .unwrap();
    o.next_final();
    o.session
        .add_range_mask(
            Some(range),
            RangeKind::Color,
            0.3,
            0.3,
            MaskCombineMode::Intersect,
        )
        .unwrap();
    o.next_final();
    o.session
        .add_color_range_sample(range, 1, 0.6, 0.6)
        .unwrap();
    o.next_final();
    let g = o.session.mask_groups().unwrap();
    let r = g.iter().find(|g| g.id == range).unwrap();
    assert_eq!(r.components[0].kind, MaskComponentType::LuminanceRange);
    assert_eq!(r.components[1].title, "Color Range (2 samples)");
    assert_eq!(r.components[1].combine, MaskCombineMode::Intersect);
    assert!(o.session.commit("Range".into()).unwrap());
    o.assert_no_failures();
    assert!(o.session.ignored_settings().unwrap().is_empty());

    // The recipe persists; a new session renders the same masks.
    o.session.close().unwrap();
    let expected = o.session.mask_groups().unwrap();
    let (dir, engine, image_id) = (o._dir, o.engine, o.image_id);
    let mut again = Open::session(dir, engine, image_id);
    again.attach();
    let reopened = again.session.mask_groups().unwrap();
    assert_eq!(reopened, expected);
    again.assert_no_failures();
}

/// A stand-in for the segmentation models: a centred disc, or an error.
struct Disc {
    fail: bool,
}
impl MaskSegmenter for Disc {
    fn segment(
        &mut self,
        image: &image::RgbImage,
        request: &SegmentRequest,
    ) -> anyhow::Result<Vec<f32>> {
        anyhow::ensure!(!self.fail, "no weights");
        let (w, h) = image.dimensions();
        let r = w.min(h) as f32 * 0.3;
        let disc = (0..w * h).map(move |i| {
            let (x, y) = (
                (i % w) as f32 - w as f32 / 2.0,
                (i / w) as f32 - h as f32 / 2.0,
            );
            if x.hypot(y) < r { 1.0 } else { 0.0 }
        });
        Ok(match request {
            SegmentRequest::Background => disc.map(|v| 1.0 - v).collect(),
            SegmentRequest::Prompts { boxes, .. } => {
                assert!(!boxes.is_empty(), "object box reaches the model");
                disc.collect()
            }
            _ => disc.collect(),
        })
    }
}

impl Open {
    fn wait_job(&self) -> MaskJobUpdate {
        let deadline = Instant::now() + WAIT;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let u = self.jobs.recv_timeout(left).expect("no AI progress");
            if u.done {
                return u;
            }
        }
    }
}

#[test]
fn ai_masks_segment_in_a_job_and_render_through_the_mask_cache() {
    let Some(mut o) = Open::new("arw") else {
        return;
    };
    o.engine
        .install_mask_segmenter(Box::new(Disc { fail: false }));
    let base = o.mean_luminance();
    let subject = o
        .session
        .add_ai_mask(None, AiMaskRequest::Subject, MaskCombineMode::Add)
        .unwrap();
    let pending = &o.session.mask_groups().unwrap()[0].components[0];
    assert_eq!(pending.kind, MaskComponentType::Subject);
    assert!(matches!(
        pending.ai,
        AiMaskState::Pending { .. } | AiMaskState::Ready
    ));
    let key = pending.ai_key.clone().unwrap();
    assert!(
        pending.definition_json.contains("segment/u2net"),
        "model recorded"
    );
    let done = o.wait_job();
    assert_eq!(
        (done.key.as_str(), done.error.as_deref()),
        (key.as_str(), None)
    );
    assert_eq!(
        o.session.mask_groups().unwrap()[0].components[0].ai,
        AiMaskState::Ready
    );
    o.session
        .set_mask_param(subject, "exposure".into(), 1.5, false)
        .unwrap();
    o.next_final();
    o.assert_no_failures();
    assert!(o.mean_luminance() > base + 1.0);
    o.drain_overlays();
    o.session.set_mask_overlay(Some(subject)).unwrap();
    let disc = o.overlay(subject).coverage;
    // π·0.3² of the short side squared over the frame area.
    assert!((0.12..0.30).contains(&disc), "disc coverage {disc}");

    // Subtracting a background mask of the same image leaves the disc.
    o.session
        .add_ai_mask(
            Some(subject),
            AiMaskRequest::Background,
            MaskCombineMode::Subtract,
        )
        .unwrap();
    assert!(o.wait_job().error.is_none());
    o.next_final();
    let still = o.overlay(subject).coverage;
    assert!((still - disc).abs() < 0.02, "{still} vs {disc}");

    // An object box and a person from a face box become prompt components.
    let object = o
        .session
        .add_ai_mask(
            None,
            AiMaskRequest::Object {
                points: vec![MaskPoint { x: 0.5, y: 0.5 }],
                region: Some(MaskRect {
                    left: 0.3,
                    top: 0.3,
                    right: 0.7,
                    bottom: 0.7,
                }),
            },
            MaskCombineMode::Add,
        )
        .unwrap();
    assert!(o.wait_job().error.is_none());
    let person = o
        .session
        .add_ai_mask(
            None,
            AiMaskRequest::Person {
                face: MaskRect {
                    left: 0.45,
                    top: 0.2,
                    right: 0.55,
                    bottom: 0.3,
                },
            },
            MaskCombineMode::Add,
        )
        .unwrap();
    assert!(o.wait_job().error.is_none());
    let groups = o.session.mask_groups().unwrap();
    let title = |id: u32| {
        groups.iter().find(|g| g.id == id).unwrap().components[0]
            .title
            .clone()
    };
    assert_eq!(
        (title(object), title(person)),
        ("Object".into(), "Person".into())
    );
    let person_json =
        &groups.iter().find(|g| g.id == person).unwrap().components[0].definition_json;
    let v: serde_json::Value = serde_json::from_str(person_json).unwrap();
    let region = &v["region"];
    assert!(
        region["bottom"].as_f64().unwrap() > 0.8,
        "expanded down the body: {region}"
    );
    o.next_final();
    assert!(o.session.commit("AI masks".into()).unwrap());
    o.assert_no_failures();

    // A failing model marks the component failed; rendering continues.
    o.engine
        .install_mask_segmenter(Box::new(Disc { fail: true }));
    o.session
        .add_ai_mask(None, AiMaskRequest::Sky, MaskCombineMode::Add)
        .unwrap();
    let failed = o.wait_job();
    assert!(failed.error.unwrap().contains("no weights"));
    let sky = o.session.mask_groups().unwrap().pop().unwrap();
    assert!(matches!(sky.components[0].ai, AiMaskState::Failed { .. }));
    o.next_final();
    o.assert_no_failures();
    // Retry with a working model.
    o.engine
        .install_mask_segmenter(Box::new(Disc { fail: false }));
    o.session
        .retry_ai_mask(sky.components[0].ai_key.clone().unwrap())
        .unwrap();
    assert!(o.wait_job().error.is_none());
}

/// The real models on the tomato CR3, when the weights are installed
/// (`TESSERA_SEGMENT_MODELS`, see crates/ml-segment/README.md).
#[test]
fn subject_mask_on_the_canon_fixture_with_cached_models() {
    let cache = std::env::var_os("TESSERA_SEGMENT_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../tools/orchestrate/wp/M3-04/.cache/segment-registry")
        });
    if !cache.is_dir() {
        eprintln!(
            "SKIP offline: no segmentation weights at {}",
            cache.display()
        );
        return;
    }
    let Some(mut o) = Open::new("cr3") else {
        return;
    };
    // SAFETY: only this test reads the variable, before loading the models.
    unsafe { std::env::set_var("TESSERA_SEGMENT_MODELS", &cache) };
    let subject = o
        .session
        .add_ai_mask(None, AiMaskRequest::Subject, MaskCombineMode::Add)
        .unwrap();
    let done = o.wait_job();
    assert!(done.error.is_none(), "{:?}", done.error);
    o.next_final();
    o.session.set_mask_overlay(Some(subject)).unwrap();
    let c = o.overlay(subject).coverage;
    println!("subject coverage on the CR3: {c}");
    assert!((0.02..0.95).contains(&c));
}

/// Interactive latency of mask edits on the 36 MP NEF at a quarter-size
/// screen level: `cargo test -p tessera-ffi --release --test masks -- --ignored --nocapture bench_mask`.
#[test]
#[ignore]
fn bench_mask_latency() {
    let Some(mut o) = Open::new("nef") else {
        return;
    };
    let info = o.session.info();
    let plan = o
        .session
        .plan_surface(info.width.div_ceil(4), info.height.div_ceil(4));
    for _ in 0..2 {
        let id = create_rgba8(plan.width, plan.height);
        o.session
            .attach_surface(id, plan.width, plan.height)
            .unwrap();
    }
    o.next_final();
    let drag = |o: &mut Open, name: &str, step: &mut dyn FnMut(&DevelopSession, u32)| {
        let mut samples = Vec::new();
        let mut level = 0;
        for i in 1..25 {
            step(&o.session, i);
            // Wait for this change's frame (no settling: one frame per step).
            let f = loop {
                let f = o.frames.recv_timeout(WAIT).expect("frame");
                if f.is_final && f.generation > o.last {
                    o.last = f.generation;
                    break f;
                }
            };
            level = f.level;
            if i > 8 {
                samples.push(f.render_ms);
            }
        }
        o.session.commit(name.into()).unwrap();
        o.next_final();
        samples.sort_by(f64::total_cmp);
        let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
        println!(
            "  {name:<18} L{level}: median {:.1} ms, p90 {:.1} ms",
            p(0.5),
            p(0.9)
        );
    };
    println!(
        "{} masks, screen L{} {}×{}:",
        info.backend, plan.level, plan.width, plan.height
    );
    let g = o
        .session
        .add_mask(
            r#"{"kind":"linear","start":[0.5,0.0],"end":[0.5,0.5]}"#.into(),
            false,
        )
        .unwrap();
    o.next_final();
    drag(&mut o, "local exposure", &mut |s, i| {
        s.set_mask_param(g, "exposure".into(), i as f32 / 24.0, true)
            .unwrap()
    });
    drag(&mut o, "gradient handle", &mut |s, i| {
        let end = 0.3 + i as f32 / 60.0;
        s.set_mask_component(
            g,
            0,
            format!(r#"{{"kind":"linear","start":[0.5,0.0],"end":[0.5,{end}]}}"#),
            true,
        )
        .unwrap()
    });
    let brush = BrushSettings {
        radius: 0.03,
        feather: 50.0,
        flow: 100.0,
        erase: false,
    };
    let b = o.session.begin_brush_stroke(None, brush).unwrap();
    o.session
        .set_mask_param(b, "exposure".into(), -1.0, false)
        .unwrap();
    o.next_final();
    drag(&mut o, "brush batch", &mut |s, i| {
        let x = 0.2 + i as f32 * 0.02;
        s.add_brush_points(
            (0..4)
                .map(|k| BrushPoint {
                    x: x + k as f32 * 0.005,
                    y: 0.7,
                    pressure: 1.0,
                })
                .collect(),
        )
        .unwrap()
    });
    o.session.end_brush_stroke().unwrap();
    o.assert_no_failures();
}
