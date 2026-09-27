//! Identical temporary integration test for pre/post-M2-58 headless pixel capture.
//! Every process uses a fresh engine, copied RAW, native process v2 and RGBA8 ring.
#![cfg(target_os = "macos")]
use std::path::PathBuf;
use std::sync::{Arc, Weak, mpsc};
use std::time::Duration;
use tessera_ffi::surface::{Surface, testing::create_rgba8};
use tessera_ffi::{DevelopListener, DevelopSession, Engine, FrameInfo, Histogram, ImageQuery};

struct Capture {
    frame: FrameInfo,
    histogram: Histogram,
    pixels: Vec<u8>,
}
struct Listener {
    session: Weak<DevelopSession>,
    width: u32,
    height: u32,
    tx: mpsc::Sender<Result<Capture, String>>,
}
impl DevelopListener for Listener {
    fn frame_ready(&self, frame: FrameInfo) {
        if !frame.is_final || frame.is_overlay || frame.surface_id == 0 {
            return;
        }
        // Copy while publication still owns render_serial. This works on both
        // revisions and cannot observe producer reuse after callback return.
        let result = (|| {
            let surface = Surface::lookup(frame.surface_id, self.width, self.height)?;
            let pixels = surface.with_pixels(|bytes, stride| {
                let mut pixels = Vec::with_capacity((frame.width * frame.height * 4) as usize);
                for row in 0..frame.height as usize {
                    pixels.extend_from_slice(
                        &bytes[row * stride..row * stride + frame.width as usize * 4],
                    );
                }
                pixels
            })?;
            let histogram = self
                .session
                .upgrade()
                .ok_or("session closed")?
                .get_histogram()
                .map_err(|error| error.to_string())?;
            if histogram.generation != frame.generation {
                return Err("histogram/frame generation mismatch".into());
            }
            Ok(Capture {
                frame,
                histogram,
                pixels,
            })
        })();
        let _ = self.tx.send(result);
    }
    fn render_failed(&self, message: String) {
        let _ = self.tx.send(Err(message));
    }
    fn saved(&self, _: String) {}
}
fn next(rx: &mpsc::Receiver<Result<Capture, String>>, after: u64) -> Capture {
    loop {
        let capture = rx
            .recv_timeout(Duration::from_secs(120))
            .expect("final frame timeout")
            .expect("capture failed");
        if capture.frame.generation > after {
            return capture;
        }
    }
}

#[test]
fn capture_settled_upright_pixels() {
    let fixture =
        PathBuf::from(std::env::var_os("M2_58_PARITY_FIXTURE").expect("fixture required"));
    let output = PathBuf::from(std::env::var_os("M2_58_PARITY_OUTPUT").expect("output required"));
    let exposure: f64 = std::env::var("M2_58_PARITY_EXPOSURE")
        .expect("exposure required")
        .parse()
        .unwrap();
    assert!(exposure == 0.0 || exposure == 1.0);
    assert_eq!(std::env::var("TESSERA_RENDER_BACKEND").unwrap(), "gpu");
    std::fs::create_dir(&output).expect("new output directory required");
    let scratch = tempfile::tempdir().unwrap();
    let photos = scratch.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    std::fs::copy(&fixture, photos.join("sony-arw.ARW")).unwrap();
    let engine = Engine::open(
        scratch
            .path()
            .join("support")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let images = engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(images.len(), 1);
    let session = engine
        .clone()
        .open_develop_session(images[0].id.clone())
        .unwrap();
    let process = serde_json::json!({"family": "native", "revision": 2});
    session.set_process_version(process.to_string()).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&session.get_process_version().unwrap()).unwrap(),
        process
    );
    let backend = session.info().backend;
    assert!(
        backend.starts_with("Metal ("),
        "requested GPU backend unavailable: {backend}"
    );
    let plan = session.plan_surface(1280, 900);
    let (tx, rx) = mpsc::channel();
    session.set_listener(Some(Arc::new(Listener {
        session: Arc::downgrade(&session),
        width: plan.width,
        height: plan.height,
        tx,
    })));
    for _ in 0..3 {
        session
            .attach_surface(
                create_rgba8(plan.width, plan.height),
                plan.width,
                plan.height,
            )
            .unwrap();
    }
    let initial = next(&rx, 0);
    let patch = serde_json::json!({"geometry": {"upright": {"mode": "auto"}}, "tone": {"exposure": exposure}});
    session.set_settings(patch.to_string(), false).unwrap();
    let capture = next(&rx, initial.frame.generation);
    assert_eq!(
        capture.frame.level, plan.level,
        "settled frame must use planned level"
    );
    let metadata = serde_json::json!({
        "fixture_name": "sony-arw.ARW", "requested_backend": "gpu", "reported_backend": backend,
        "actual_residency": null,
        "residency_limit": "Baseline FrameInfo lacks residency; selected backend is not per-frame route proof.",
        "process": process, "patch": patch,
        "settings": serde_json::from_str::<serde_json::Value>(&session.get_settings_json().unwrap()).unwrap(),
        "format": "RGBA8", "viewport": [1280, 900], "surface": [plan.width, plan.height],
        "level": capture.frame.level, "width": capture.frame.width, "height": capture.frame.height,
        "display_width": capture.frame.display_width, "display_height": capture.frame.display_height,
        "histogram": {"red": capture.histogram.red, "green": capture.histogram.green,
            "blue": capture.histogram.blue, "luminance": capture.histogram.luminance,
            "level": capture.histogram.level},
        "generation_matched": true, "pixels_len": capture.pixels.len(),
    });
    std::fs::write(output.join("pixels.rgba"), &capture.pixels).unwrap();
    std::fs::write(
        output.join("metadata.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .unwrap();
    session.set_listener(None);
    session.close().unwrap();
}
