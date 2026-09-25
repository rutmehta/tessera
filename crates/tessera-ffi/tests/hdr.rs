//! EDR presentation (M2-22): RGBA16F rings, headroom selection, SDR fallback.
//! Session tests use a real RAW fixture and skip without one.
#![cfg(target_os = "macos")]

use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};
use tessera_ffi::surface::{
    Surface, SurfaceKind,
    testing::{create_rgba8, create_rgba16f},
    write_display, write_rgba16f,
};
use tessera_ffi::*;

fn f16s(bytes: &[u8]) -> Vec<f32> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| half::f16::from_le_bytes(*b).to_f32())
        .collect()
}

fn linear_tile(coord: TileCoord, w: u32, h: u32, f: impl Fn(u32, u32, usize) -> f32) -> Tile {
    let layout = TileLayout {
        extent: Extent::new(w, h),
        halo: 0,
        channels: 3,
    };
    let n = layout.plane_len();
    let mut data = vec![0.0f32; 3 * n];
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                data[c * n + (y * w + x) as usize] = f(x, y, c);
            }
        }
    }
    Tile::from_samples(coord, layout, data).unwrap()
}

#[test]
fn float_surface_writer_keeps_values_above_one() {
    let (w, h) = (300, 270);
    let id = create_rgba16f(w, h);
    let surface = Surface::lookup_presentation(id, w, h).expect("RGhA lookup");
    assert_eq!(surface.kind(), SurfaceKind::Rgba16Float);
    assert!(surface.is_float());
    // RGBA8-only lookups (1:1 detail crop) refuse the EDR contract.
    assert!(Surface::lookup(id, w, h).is_err());
    assert!(Surface::lookup_presentation(id, w + 1, h).is_err());
    let rgba8 = create_rgba8(w, h);
    assert_eq!(
        Surface::lookup_presentation(rgba8, w, h).unwrap().kind(),
        SurfaceKind::Rgba8
    );
    // Tile (1, 1) at level 0 starts at (256, 256): values up to 8× SDR white.
    let tile = linear_tile(TileCoord::new(0, 1, 1), 256, 256, |x, y, c| {
        [0.25 + x as f32 / 32.0, 0.5 + y as f32 / 64.0, 7.5][c]
    });
    surface.write_tile(&tile).unwrap();
    // An SDR (U8) tile never reinterprets into a float surface.
    let u8_tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        },
        vec![1u8, 2, 3],
    )
    .unwrap();
    assert!(surface.write_tile(&u8_tile).is_err());
    surface
        .with_pixels(|px, stride| {
            assert!(stride >= w as usize * 8);
            for (x, y) in [(256usize, 256usize), (299, 256), (256, 269), (299, 269)] {
                let o = y * stride + x * 8;
                let v = f16s(&px[o..o + 8]);
                let (tx, ty) = ((x - 256) as f32, (y - 256) as f32);
                let expect = [0.25 + tx / 32.0, 0.5 + ty / 64.0, 7.5, 1.0];
                for c in 0..4 {
                    assert!(
                        (v[c] - expect[c]).abs() <= expect[c] * 1e-3,
                        "{x},{y} {v:?}"
                    );
                }
            }
            // Values above SDR white survive (1.59, 0.7, 7.5).
            let o = 269 * stride + 299 * 8;
            let v = f16s(&px[o..o + 6]);
            assert!(v[0] > 1.0 && v[2] > 7.0, "{v:?}");
        })
        .unwrap();
    // Pure writer: clipping to a band, and the wrong contract is an error.
    let mut band = vec![0u8; 2 * 300 * 8];
    write_rgba16f(&mut band, 300 * 8, 260, 300, &tile).unwrap();
    let v = f16s(&band[256 * 8..256 * 8 + 8]);
    assert_eq!(v, vec![0.25, 0.5 + 4.0 / 64.0, 7.5, 1.0]);
    let mut buf = vec![0u8; 64];
    assert!(write_display(&mut buf, 32, 0, 4, true, &u8_tile).is_err());
    assert!(write_display(&mut buf, 16, 0, 4, false, &tile).is_err());
}

#[test]
fn presentation_selects_sdr_fallback_and_caps_headroom() {
    use image_core::{Headroom, RenderOutput};
    let mut s = engine_api::recipe::DevelopSettings::default();
    let linear = |h: f32| RenderOutput::DisplayLinear(Headroom::new(h));
    // RGBA8 rings (SDR displays) always keep the SDR Output stage.
    for (hdr, stops, display) in [(false, 0.0, 1.0), (true, 3.0, 16.0), (true, 2.0, 1.0)] {
        s.output.hdr = hdr;
        s.output.hdr_headroom_stops = stops;
        assert_eq!(presentation(&s, false, display), RenderOutput::Display);
    }
    // Float ring, HDR off: SDR tone curve, unencoded.
    s.output.hdr = false;
    s.output.hdr_headroom_stops = 3.0;
    assert_eq!(presentation(&s, true, 16.0), linear(1.0));
    s.output.hdr = true;
    // Recipe headroom wins below the display's...
    assert_eq!(presentation(&s, true, 16.0), linear(8.0));
    // ...and the display's current headroom caps it.
    assert_eq!(presentation(&s, true, 2.5), linear(2.5));
    assert_eq!(presentation(&s, true, 1.0), linear(1.0));
    assert_eq!(presentation(&s, true, f32::NAN), linear(1.0));
    // 0 stops: HDR on, SDR tone-mapped. Invalid stops sanitize to 0.
    s.output.hdr_headroom_stops = 0.0;
    assert_eq!(presentation(&s, true, 16.0), linear(1.0));
    s.output.hdr_headroom_stops = f32::INFINITY;
    assert_eq!(presentation(&s, true, 16.0), linear(1.0));
    s.output.hdr_headroom_stops = 40.0;
    assert_eq!(hdr_headroom_stops(&s), 16.0);
    // The recipe fields are drawn (not "ignored") yet never alter SDR.
    s.output.hdr_headroom_stops = 2.0;
    assert!(ignored_settings(&s).iter().all(|p| !p.contains("hdr")));
}

// ───────────────────────────── session tests ─────────────────────────────

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

struct Open {
    _dir: tempfile::TempDir,
    _engine: Arc<Engine>,
    session: Arc<DevelopSession>,
    frames: mpsc::Receiver<FrameInfo>,
    failed: Arc<Mutex<Vec<String>>>,
    last_generation: u64,
}

impl Open {
    fn new(ext: &str, backend: Option<&str>) -> Option<Self> {
        let src = fixture(ext)?;
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        std::fs::copy(&src, photos.join(src.file_name().unwrap())).unwrap();
        if let Some(backend) = backend {
            // SAFETY (env): only the ignored bench sets it, alone in its process.
            unsafe { std::env::set_var("TESSERA_RENDER_BACKEND", backend) };
        }
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        let session = engine.clone().open_develop_session(id).unwrap();
        let (tx, frames) = mpsc::channel();
        let failed = Arc::new(Mutex::new(Vec::new()));
        session.set_listener(Some(Arc::new(Listener {
            frames: Mutex::new(tx),
            failed: failed.clone(),
        })));
        Some(Self {
            _dir: dir,
            _engine: engine,
            session,
            frames,
            failed,
            last_generation: 0,
        })
    }

    fn next_final(&mut self) -> FrameInfo {
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let frame = self.frames.recv_timeout(left).unwrap_or_else(|_| {
                panic!("no frame; failures: {:?}", self.failed.lock().unwrap())
            });
            if frame.generation > self.last_generation && frame.is_final {
                eprintln!(
                    "frame gen {} L{} {:.1} ms surface {}",
                    frame.generation, frame.level, frame.render_ms, frame.surface_id
                );
                self.last_generation = frame.generation;
                return frame;
            }
        }
    }

    /// Attaches a ring of `count` surfaces of one contract.
    fn attach(&mut self, view: (u32, u32), float: bool, count: usize) -> SurfacePlan {
        let plan = self.session.plan_surface(view.0, view.1);
        for _ in 0..count {
            let id = if float {
                create_rgba16f(plan.width, plan.height)
            } else {
                create_rgba8(plan.width, plan.height)
            };
            self.session
                .attach_surface(id, plan.width, plan.height)
                .unwrap();
        }
        plan
    }
}

/// Valid region of a frame's surface, row by row.
fn frame_bytes(f: &FrameInfo, plan: &SurfacePlan, float: bool) -> Vec<u8> {
    let surface = Surface::lookup_presentation(f.surface_id, plan.width, plan.height).unwrap();
    assert_eq!(surface.is_float(), float, "frame in the ring's contract");
    let bpe = if float { 8 } else { 4 };
    surface
        .with_pixels(|px, stride| {
            let mut out = Vec::new();
            for y in 0..f.height as usize {
                out.extend_from_slice(&px[y * stride..y * stride + f.width as usize * bpe]);
            }
            out
        })
        .unwrap()
}

/// RGB maximum and share of samples above SDR white of a float frame.
fn edr_stats(bytes: &[u8]) -> (f32, f64) {
    let v = f16s(bytes);
    let rgb: Vec<f32> = v
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| p[..3].to_vec())
        .collect();
    let max = rgb.iter().copied().fold(0.0, f32::max);
    let above = rgb.iter().filter(|&&x| x > 1.0).count() as f64 / rgb.len() as f64;
    assert!(v.as_chunks::<4>().0.iter().all(|p| p[3] == 1.0));
    (max, above)
}

#[test]
fn edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical() {
    let Some(mut open) = Open::new("nef", None) else {
        return;
    };
    let info = open.session.info();
    let view = (info.width.div_ceil(8), info.height.div_ceil(8));
    // SDR reference: RGBA8 ring, HDR off.
    let plan = open.attach(view, false, 2);
    let sdr = open.next_final();
    let reference = frame_bytes(&sdr, &plan, false);
    assert_eq!(open.session.presentation_headroom().unwrap(), 0.0);
    // A bright edit so the frame has highlights to place in the headroom.
    open.session
        .set_settings(r#"{"tone":{"exposure":1.5}}"#.into(), false)
        .unwrap();
    let bright_sdr = frame_bytes(&open.next_final(), &plan, false);
    // HDR on with an RGBA8 ring (SDR display): nothing to redraw, the SDR
    // path is untouched.
    open.session
        .set_settings(
            r#"{"output":{"hdr":true,"hdr_headroom_stops":2.0}}"#.into(),
            false,
        )
        .unwrap();
    assert!(
        open.frames
            .recv_timeout(Duration::from_millis(300))
            .is_err()
    );
    assert_eq!(open.session.presentation_headroom().unwrap(), 0.0);
    open.session.refresh().unwrap();
    assert_eq!(frame_bytes(&open.next_final(), &plan, false), bright_sdr);
    // EDR display (16× potential, 8× now) and a float ring: 2 stops = 4×.
    open.session.set_display_headroom(8.0).unwrap();
    let plan = open.attach(view, true, 3);
    let f = open.next_final();
    assert_eq!(open.session.presentation_headroom().unwrap(), 4.0);
    let (max, above) = edr_stats(&frame_bytes(&f, &plan, true));
    eprintln!(
        "EDR 4×: max {max:.3}, {:.1}% of samples above SDR white",
        above * 100.0
    );
    assert!(max > 1.5 && max <= 4.0, "{max}");
    assert!(above > 0.001, "{above}");
    // The display's headroom shrinks (brightness up): re-tone-mapped to 2×.
    open.session.set_display_headroom(2.0).unwrap();
    let f = open.next_final();
    assert_eq!(open.session.presentation_headroom().unwrap(), 2.0);
    let (max2, _) = edr_stats(&frame_bytes(&f, &plan, true));
    assert!(max2 > 1.0 && max2 <= 2.0 && max2 < max, "{max2}");
    // Headroom slider at 0 stops: SDR tone-mapped, nothing above white.
    open.session
        .set_settings(r#"{"output":{"hdr_headroom_stops":0.0}}"#.into(), true)
        .unwrap();
    let f = open.next_final();
    let (max0, above0) = edr_stats(&frame_bytes(&f, &plan, true));
    assert!(max0 <= 1.0 && above0 == 0.0, "{max0}");
    // Back to an SDR screen: the RGBA8 ring is byte-identical to before HDR.
    open.session.set_display_headroom(1.0).unwrap();
    open.session
        .set_settings(
            r#"{"output":{"hdr":false,"hdr_headroom_stops":0.0}}"#.into(),
            false,
        )
        .unwrap();
    let plan = open.attach(view, false, 2);
    let f = open.next_final();
    assert_eq!(frame_bytes(&f, &plan, false), bright_sdr);
    open.session
        .set_settings(r#"{"tone":{"exposure":0.0}}"#.into(), false)
        .unwrap();
    assert_eq!(frame_bytes(&open.next_final(), &plan, false), reference);
    assert!(open.failed.lock().unwrap().is_empty(), "{:?}", open.failed);
}

/// Slider latency on the float path vs the RGBA8 path (settings change →
/// frame, screen level L2), per backend. M2-17b budget: < 12 ms.
/// `cargo test -p tessera-ffi --release --test hdr -- --ignored --nocapture bench`
#[test]
#[ignore]
fn bench_edr_slider_latency() {
    let ext = std::env::var("TESSERA_BENCH_EXT").unwrap_or_else(|_| "nef".into());
    for backend in ["gpu", "cpu"] {
        for float in [false, true] {
            let Some(mut open) = Open::new(&ext, Some(backend)) else {
                return;
            };
            let info = open.session.info();
            open.session.set_display_headroom(16.0).unwrap();
            open.session
                .set_settings(
                    r#"{"output":{"hdr":true,"hdr_headroom_stops":3.0}}"#.into(),
                    false,
                )
                .unwrap();
            let plan = open.attach((info.width.div_ceil(4), info.height.div_ceil(4)), float, 3);
            let _cold = open.next_final();
            let mut samples = Vec::new();
            let mut levels = Vec::new();
            for i in 0..60 {
                let ev = -1.0 + f64::from(i) * 0.05;
                open.session
                    .set_settings(format!(r#"{{"tone":{{"exposure":{ev}}}}}"#), true)
                    .unwrap();
                let f = open.next_final();
                // First frames warm the caches.
                if i >= 5 {
                    samples.push(f.render_ms);
                    levels.push(f.level);
                }
            }
            samples.sort_by(f64::total_cmp);
            let p = |q: f64| samples[((samples.len() - 1) as f64 * q) as usize];
            println!(
                "{} {} L{} {}×{} ({:.1} MP), drag levels {:?}..{:?}: median {:.2} ms, p90 {:.2} ms, max {:.2} ms (headroom {})",
                info.backend,
                if float { "RGBA16F EDR" } else { "RGBA8 SDR " },
                plan.level,
                plan.width,
                plan.height,
                f64::from(plan.width * plan.height) / 1e6,
                levels.iter().min().unwrap(),
                levels.iter().max().unwrap(),
                p(0.5),
                p(0.9),
                p(1.0),
                open.session.presentation_headroom().unwrap(),
            );
            assert!(open.failed.lock().unwrap().is_empty(), "{:?}", open.failed);
        }
    }
}
