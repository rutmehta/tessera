//! Opt-in Engine/IOSurface qualification. Run one route per fresh test process.
//! No GUI or physical input-to-screen latency claim. Pixel reads are AFTER timing.
use super::*;
use serde_json::json;
use std::{fs, io::Read, path::Path, sync::mpsc, time::Instant};

static GPU: Mutex<Option<Arc<pipeline_gpu::GpuStageOp>>> = Mutex::new(None);
static RESIDENT: Mutex<Vec<(u64, u8, RenderOutput)>> = Mutex::new(Vec::new());
pub(crate) fn capture_gpu(ops: Arc<pipeline_gpu::GpuStageOp>) {
    if std::env::var_os("TESSERA_QUALIFY_ROUTE").is_some() {
        *GPU.lock().unwrap() = Some(ops);
    }
}
pub(crate) fn resident_receipt(generation: u64, level: u8, output: RenderOutput) {
    if std::env::var_os("TESSERA_QUALIFY_ROUTE").is_some() {
        RESIDENT.lock().unwrap().push((generation, level, output));
    }
}
struct Listener(mpsc::Sender<std::result::Result<FrameInfo, String>>);
impl DevelopListener for Listener {
    fn frame_ready(&self, frame: FrameInfo) {
        let _ = self.0.send(Ok(frame));
    }
    fn render_failed(&self, message: String) {
        let _ = self.0.send(Err(message));
    }
    fn saved(&self, _: String) {}
}
fn digest(path: &Path) -> String {
    let mut file = fs::File::open(path).unwrap();
    let mut h = blake3::Hasher::new();
    let mut bytes = [0; 65536];
    loop {
        let n = file.read(&mut bytes).unwrap();
        if n == 0 {
            break;
        }
        h.update(&bytes[..n]);
    }
    h.finalize().to_hex().to_string()
}
fn stats() -> pipeline_gpu::GpuStats {
    GPU.lock()
        .unwrap()
        .as_ref()
        .map_or_else(Default::default, |gpu| gpu.stats())
}
fn pixels(surface: &Surface, frame: &FrameInfo, float: bool) -> Vec<u8> {
    surface
        .with_pixels(|bytes, stride| {
            let mut out = Vec::with_capacity((frame.width * frame.height * 12) as usize);
            for y in 0..frame.height as usize {
                for x in 0..frame.width as usize {
                    for c in 0..3 {
                        let value = if float {
                            let at = y * stride + x * 8 + c * 2;
                            half::f16::from_le_bytes([bytes[at], bytes[at + 1]]).to_f32()
                        } else {
                            bytes[y * stride + x * 4 + c] as f32 / 255.0
                        };
                        assert!(value.is_finite());
                        if float {
                            assert!((0.0..=4.0).contains(&value));
                        }
                        out.extend_from_slice(&value.to_le_bytes());
                    }
                }
            }
            out
        })
        .unwrap()
}

#[test]
#[ignore = "exclusive Metal lane, real read-only RAW, explicit qualification route/output"]
fn engine_iosurface_matched_viewport_qualification() {
    let route = std::env::var("TESSERA_QUALIFY_ROUTE")
        .expect("original-gpu/proxy-cpu/proxy-gpu/original-auto/proxy-auto");
    assert!(
        [
            "original-gpu",
            "proxy-cpu",
            "proxy-gpu",
            "original-auto",
            "proxy-auto"
        ]
        .contains(&route.as_str())
    );
    let proxy = route.starts_with("proxy");
    let force_gpu = route.ends_with("-gpu");
    let preference = std::env::var("TESSERA_RENDER_BACKEND").unwrap_or_default();
    assert_eq!(
        preference,
        if force_gpu {
            "gpu"
        } else if route == "proxy-cpu" {
            "cpu"
        } else {
            ""
        }
    );
    assert!(
        std::env::var_os("TESSERA_SMART_PREVIEW_GPU").is_none(),
        "qualification must exercise the shipped gate-absent route"
    );
    let float = std::env::var("TESSERA_QUALIFY_FORMAT").as_deref() == Ok("edr");
    let output = std::path::PathBuf::from(
        std::env::var_os("TESSERA_QUALIFY_OUT").expect("output directory"),
    );
    fs::create_dir_all(&output).unwrap();
    let fixture = std::path::PathBuf::from(
        std::env::var_os("TESSERA_SMART_PREVIEW_RAW").expect("read-only RAW"),
    );
    let fixture_hash = digest(&fixture);
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    fs::create_dir(&photos).unwrap();
    let original = photos.join(fixture.file_name().unwrap());
    fs::copy(&fixture, &original).unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into())
        .unwrap();
    let images = engine.list_images(crate::ImageQuery::default()).unwrap();
    assert_eq!(images.len(), 1);
    let id = images[0].id.clone();
    let mut recipe: engine_api::recipe::Recipe =
        serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
    recipe.process_version = engine_api::recipe::ProcessVersion {
        family: engine_api::recipe::ProcessFamily::Native,
        revision: 2,
    };
    recipe.settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Off;
    recipe.settings.tone.exposure = 0.25;
    if float {
        recipe.settings.output.hdr = true;
        recipe.settings.output.hdr_headroom_stops = 2.0;
    }
    recipe
        .history
        .record(
            &recipe.history.base.clone(),
            &recipe.settings,
            engine_api::recipe::EditMeta::user("qualification baseline", 1),
        )
        .unwrap();
    engine
        .set_recipe_json(
            id.clone(),
            String::from_utf8(recipe.to_json().unwrap()).unwrap(),
        )
        .unwrap();
    let baseline_recipe = engine.get_recipe(id.clone()).unwrap();
    let built = engine.build_smart_preview(id.clone()).unwrap();
    assert!(built.width <= 2048 && built.height <= 2048);
    assert_eq!(engine.get_recipe(id.clone()).unwrap(), baseline_recipe);
    let engine = if float {
        let support = temp.path().join("support");
        drop(engine);
        let reopened = Engine::open(support.to_string_lossy().into()).unwrap();
        assert_eq!(reopened.get_recipe(id.clone()).unwrap(), baseline_recipe);
        reopened
    } else {
        engine
    };
    let mut rows = Vec::new();
    let mut saw_hdr_highlight = false;
    *GPU.lock().unwrap() = None;
    RESIDENT.lock().unwrap().clear();
    let open_start = Instant::now();
    let session = if proxy {
        engine
            .clone()
            .open_smart_preview_develop_session(id.clone())
    } else {
        engine.clone().open_develop_session(id.clone())
    }
    .unwrap();
    let open_ms = open_start.elapsed().as_secs_f64() * 1000.0;
    let info = session.info();
    assert_eq!(
        info.orientation, 1,
        "qualification normalization requires orientation1"
    );
    let backend = info.backend;
    if force_gpu {
        assert!(
            backend.contains("Metal"),
            "GPU override silently fell back: {backend}"
        );
    }
    if route == "proxy-cpu" {
        assert!(backend.contains("CPU"));
    }
    session
        .set_display_headroom(if float { 4.0 } else { 1.0 })
        .unwrap();
    // One real session changes viewport/ring sizes. First attach is fresh-session;
    // later viewports retain settings/caches, matching host viewport transitions.
    for (viewport_index, (vw, vh)) in [(640, 426), (1280, 852), (320, 213)]
        .into_iter()
        .enumerate()
    {
        session.detach_surfaces();
        let plan = session.plan_surface(vw, vh);
        if proxy {
            assert_eq!(plan.level, [1, 0, 2][viewport_index]);
        }
        let ring: Vec<Surface> = (0..2)
            .map(|_| {
                let sid = if float {
                    crate::surface::testing::create_rgba16f(plan.width, plan.height)
                } else {
                    crate::surface::testing::create_rgba8(plan.width, plan.height)
                };
                Surface::lookup_presentation(sid, plan.width, plan.height).unwrap()
            })
            .collect();
        let (send, receive) = mpsc::channel();
        session.set_listener(Some(Arc::new(Listener(send))));
        let mut last_generation = 0;
        let edits = vec![
            ("first_surface", None, false),
            (
                "warm_exposure_1",
                Some(json!({"tone":{"exposure":0.75}})),
                false,
            ),
            (
                "warm_exposure_2",
                Some(json!({"tone":{"exposure":1.0}})),
                false,
            ),
            (
                "warm_wb_1",
                Some(json!({"white_balance":{"temperature":4200.0}})),
                false,
            ),
            (
                "warm_wb_2",
                Some(json!({"white_balance":{"temperature":5500.0}})),
                false,
            ),
            (
                "interactive_1",
                Some(json!({"tone":{"exposure":1.1}})),
                true,
            ),
            (
                "interactive_2",
                Some(json!({"tone":{"exposure":1.2}})),
                true,
            ),
            ("settled", Some(json!({"tone":{"exposure":1.25}})), false),
        ];
        for (label, patch, interactive) in edits {
            while receive.try_recv().is_ok() {}
            let before = stats();
            let start = Instant::now();
            if let Some(patch) = patch {
                session
                    .set_settings(patch.to_string(), interactive)
                    .unwrap();
            } else {
                for surface in &ring {
                    session
                        .attach_surface(surface.id(), plan.width, plan.height)
                        .unwrap();
                }
            }
            let expected = session.shared.state.lock().unwrap().generation;
            let mut frames = Vec::new();
            let frame = loop {
                let frame = receive
                    .recv_timeout(Duration::from_secs(60))
                    .expect("frame timeout")
                    .expect("render error");
                if frame.generation < expected || frame.generation <= last_generation {
                    continue;
                }
                let done = frame.is_final;
                frames.push(frame.clone());
                if done {
                    break frame;
                }
            };
            let delivery_ms = start.elapsed().as_secs_f64() * 1000.0;
            last_generation = frame.generation;
            let after = stats();
            let expected_output = if float {
                RenderOutput::DisplayLinear(Headroom::new(4.))
            } else {
                RenderOutput::Display
            };
            let resident = RESIDENT
                .lock()
                .unwrap()
                .iter()
                .find(|(generation, level, _)| {
                    *generation == frame.generation && *level == frame.level
                })
                .map(|(_, _, output)| {
                    assert_eq!(*output, expected_output);
                    true
                })
                .unwrap_or(false);
            // Capture diagnostic counters before CPU pixel inspection. Calibration
            // ran before `before`, and cannot masquerade as this frame's execution.
            if force_gpu {
                assert!(
                    resident,
                    "selected Metal but frame fell back: {label} L{}",
                    frame.level
                );
                assert!(after.submissions > before.submissions);
                assert!(after.last_resident_dispatches > 0);
                assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
            }
            let name = format!("{vw}x{vh}-{label}.rgb32f");
            let surface = ring.iter().find(|s| s.id() == frame.surface_id).unwrap();
            assert!(
                session
                    .get_histogram()
                    .unwrap()
                    .luminance
                    .iter()
                    .map(|v| u64::from(*v))
                    .sum::<u64>()
                    > 0
            );
            let pixel_bytes = pixels(surface, &frame, float);
            saw_hdr_highlight |= pixel_bytes
                .as_chunks::<4>()
                .0
                .iter()
                .any(|v| f32::from_le_bytes(*v) > 1.);
            fs::write(output.join(&name), pixel_bytes).unwrap();
            rows.push(json!({"settings":serde_json::from_str::<serde_json::Value>(&session.get_settings_json().unwrap()).unwrap(),"route":route,"backend":backend,"format":if float{"edr"}else{"sdr"},"viewport":[vw,vh],"viewport_index":viewport_index,"open_ms":open_ms,"label":label,"interactive":interactive,"delivery_ms":delivery_ms,"frame_render_ms":frame.render_ms,"level":frame.level,"plan_level":plan.level,"dimensions":[frame.width,frame.height],"display_dimensions":[frame.display_width,frame.display_height],"generation":frame.generation,"resident_surface":resident,"submissions":after.submissions-before.submissions,"pixel_readback_bytes":after.pixel_readback_bytes-before.pixel_readback_bytes,"dispatches":after.last_resident_dispatches,"uploads":after.uploads-before.uploads,"pixel_file":name,"delivered_levels":frames.iter().map(|f|f.level).collect::<Vec<_>>()}));
        }
    }
    if float {
        assert!(
            saw_hdr_highlight,
            "actual HDR presentation must retain values above SDR white"
        );
    }
    session.flush().unwrap();
    session.close().unwrap();
    if float {
        let reopened = if proxy {
            engine
                .clone()
                .open_smart_preview_develop_session(id.clone())
        } else {
            engine.clone().open_develop_session(id.clone())
        }
        .unwrap();
        let live: DevelopSettings =
            serde_json::from_str(&reopened.get_settings_json().unwrap()).unwrap();
        assert!(live.output.hdr);
        assert_eq!(live.output.hdr_headroom_stops, 2.);
        reopened.close().unwrap();
    }
    drop(session);
    let lifecycle = if route == "proxy-auto" || route == "proxy-cpu" {
        proxy_lifecycle(&engine, &id, route == "proxy-auto")
    } else {
        Vec::new()
    };
    assert_eq!(digest(&fixture), fixture_hash);
    assert_eq!(digest(&original), fixture_hash);
    fs::write(output.join("results.json"),serde_json::to_vec_pretty(&json!({"fixture_blake3":fixture_hash,"proxy_dimensions":[built.width,built.height],"rows":rows,"lifecycle":lifecycle})).unwrap()).unwrap();
}

/// Bounded session/operator lifetime check, not a global-memory or RSS claim.
/// Owned SDR rings avoid the process-lifetime testing surface helper.
fn proxy_lifecycle(engine: &Arc<Engine>, id: &str, expect_gpu: bool) -> Vec<serde_json::Value> {
    let mut evidence = Vec::new();
    GPU.lock().unwrap().take();
    for cycle in 0..3 {
        RESIDENT.lock().unwrap().clear();
        let session = engine
            .clone()
            .open_smart_preview_develop_session(id.into())
            .unwrap();
        assert_eq!(session.info().backend.starts_with("Metal ("), expect_gpu);
        let weak = GPU.lock().unwrap().as_ref().map(Arc::downgrade);
        let weak_shared = Arc::downgrade(&session.shared);
        let weak_renderer = Arc::downgrade(&session.shared.renderer);
        assert_eq!(weak.is_some(), expect_gpu);
        let plan = session.plan_surface(320, 213);
        let ring = [
            Surface::create_rgba8(plan.width, plan.height).unwrap(),
            Surface::create_rgba8(plan.width, plan.height).unwrap(),
        ];
        let surface_ids = ring.each_ref().map(Surface::id);
        let (send, receive) = mpsc::channel();
        session.set_listener(Some(Arc::new(Listener(send))));
        for phase in 0..3 {
            RESIDENT.lock().unwrap().clear();
            let before = stats();
            match phase {
                0 => {
                    for surface in &ring {
                        session
                            .attach_surface(surface.id(), plan.width, plan.height)
                            .unwrap();
                    }
                }
                1 => session
                    .set_settings(
                        json!({"geometry":{"crop":{"rect":{"right":0.93},"angle":2.0}}})
                            .to_string(),
                        false,
                    )
                    .unwrap(),
                _ => session
                    .set_settings(
                        json!({"geometry":DevelopSettings::default().geometry}).to_string(),
                        false,
                    )
                    .unwrap(),
            }
            let generation = session.shared.state.lock().unwrap().generation;
            let frame = loop {
                let frame = receive
                    .recv_timeout(Duration::from_secs(60))
                    .unwrap()
                    .unwrap();
                if frame.generation == generation && frame.is_final {
                    break frame;
                }
            };
            let resident = RESIDENT.lock().unwrap().contains(&(
                frame.generation,
                frame.level,
                RenderOutput::Display,
            ));
            let after = stats();
            let expected = expect_gpu && phase != 1;
            assert_eq!(resident, expected, "cycle {cycle} phase {phase}");
            assert_eq!(after.submissions > before.submissions, expected);
            assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
            evidence.push(json!({"cycle":cycle,"phase":phase,"resident":resident,"submissions":after.submissions-before.submissions,"level":frame.level}));
        }
        session.set_listener(None);
        session.close().unwrap();
        drop(session);
        drop(receive);
        drop(ring);
        // Instrumentation itself owns an Arc. Remove it before testing lifetime.
        GPU.lock().unwrap().take();
        // The frame callback precedes job return; allow bounded worker drainage.
        let release_start = Instant::now();
        loop {
            let released = weak.as_ref().is_none_or(|w| w.upgrade().is_none())
                && weak_shared.upgrade().is_none()
                && weak_renderer.upgrade().is_none()
                && surface_ids.iter().all(|&surface_id| {
                    Surface::lookup(surface_id, plan.width, plan.height).is_err()
                });
            if released {
                break;
            }
            assert!(
                release_start.elapsed() < Duration::from_secs(5),
                "closed proxy resources did not drain within five seconds"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        evidence.push(json!({"cycle":cycle,"released":true,"release_ms":release_start.elapsed().as_secs_f64()*1000.,"deadline_ms":5000}));
    }
    evidence
}
