//! Source-only qualification extension. Pixel IO is outside callback timing.
//! No physical display latency or cold-filesystem claim.
use super::*;
// WB diagnostic Stage D phase driver (rev7 §6.4); feature-only. A child
// module so it reuses this variant's selector and decision probe unchanged.
#[cfg(feature = "wb-diagnostic")]
#[path = "wb_phase_driver.rs"]
pub(crate) mod wb_phase_driver;
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize)]
struct DecisionCounts {
    measurements: Option<u64>,
    lookups: Option<u64>,
    hits: Option<u64>,
    publications: Option<u64>,
    entries: Option<usize>,
    key: Option<[u8; 32]>,
}
fn validate_decisions(before: &DecisionCounts, after: &DecisionCounts, cycle: usize, route: &str) {
    if VARIANT == "baseline" {
        assert_eq!(*before, *after); // Baseline has no instrumentation/cache; values are null.
        assert!(after.measurements.is_none());
    } else if route == "proxy-cpu" {
        assert_eq!(after.measurements, Some(0));
        assert_eq!(after.lookups, Some(0));
        assert_eq!(after.hits, Some(0));
        assert_eq!(after.publications, Some(0));
        assert_eq!(after.entries, Some(0));
        assert!(after.key.is_none());
    } else {
        assert_eq!(after.lookups.unwrap(), before.lookups.unwrap() + 1);
        assert_eq!(after.entries, Some(1));
        assert!(after.key.is_some());
        if cycle == 0 {
            assert_eq!(
                after.measurements.unwrap(),
                before.measurements.unwrap() + 1
            );
            assert_eq!(
                after.publications.unwrap(),
                before.publications.unwrap() + 1
            );
            assert_eq!(after.hits, before.hits);
        } else {
            assert_eq!(
                after.measurements, before.measurements,
                "eligible hit recalibrated"
            );
            assert_eq!(after.publications, before.publications);
            assert_eq!(after.hits.unwrap(), before.hits.unwrap() + 1);
            assert_eq!(after.key, before.key);
        }
    }
}
struct FrameOutput<'a> {
    ring: &'a [Surface],
    float: bool,
    directory: &'a Path,
}
fn edit_frame(
    label: &str,
    session: &DevelopSession,
    receive: &mpsc::Receiver<(Instant, std::result::Result<FrameInfo, String>)>,
    destination: FrameOutput<'_>,
    settings: &engine_api::recipe::DevelopSettings,
    expect_metal: bool,
    edits: &mut Vec<serde_json::Value>,
) {
    let FrameOutput {
        ring,
        float,
        directory: output,
    } = destination;
    let patch = serde_json::to_string(settings).unwrap();
    RESIDENT.lock().unwrap().clear();
    let before = stats();
    let start = Instant::now();
    session.set_settings(patch, false).unwrap();
    let expected = session.shared.state.lock().unwrap().generation;
    let deadline = Instant::now() + Duration::from_secs(60);
    let (delivered, frame) = loop {
        let wait = deadline
            .checked_duration_since(Instant::now())
            .expect("edit deadline");
        let (at, frame) = receive.recv_timeout(wait).expect("edit frame timeout");
        let frame = frame.expect("edit render error");
        if frame.generation == expected && frame.is_final {
            break (at, frame);
        }
    };
    let after = stats();
    let output_kind = if float {
        RenderOutput::DisplayLinear(Headroom::new(4.))
    } else {
        RenderOutput::Display
    };
    let resident = RESIDENT
        .lock()
        .unwrap()
        .contains(&(frame.generation, frame.level, output_kind));
    let file = format!("edit-{label}.rgb32f");
    let row = json!({"label":label,"settings":settings,"level":frame.level,
        "dimensions":[frame.width,frame.height],"display_dimensions":[frame.display_width,frame.display_height],
        "generation":frame.generation,"edit_to_final_callback_ms":delivered.duration_since(start).as_secs_f64()*1000.,
        "resident":resident,"submissions":after.submissions-before.submissions,
        "pixel_readback_bytes":after.pixel_readback_bytes-before.pixel_readback_bytes,"pixel_file":file});
    // Save the sample before any route/fidelity assertions or pixel reads.
    fs::write(
        output.join(format!("edit-{label}-measured.json")),
        serde_json::to_vec_pretty(&row).unwrap(),
    )
    .unwrap();
    edits.push(row);
    fs::write(
        output.join("edit-results.json"),
        serde_json::to_vec_pretty(edits).unwrap(),
    )
    .unwrap();
    assert_eq!(resident, expect_metal);
    assert_eq!(after.submissions > before.submissions, expect_metal);
    assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
    if expect_metal {
        assert!(after.last_resident_dispatches > 0);
    }
    let bytes = pixels(
        ring.iter().find(|s| s.id() == frame.surface_id).unwrap(),
        &frame,
        float,
    );
    fs::write(output.join(file), bytes).unwrap();
}

#[test]
#[ignore = "exclusive lane; explicit fixture/variant and preregistered runner"]
fn engine_proxy_cache_frame_qualification() {
    let selector = std::env::var("TESSERA_QUALIFY_SELECTOR").unwrap();
    let phase = std::env::var("TESSERA_QUALIFY_PHASE").unwrap();
    assert!(["functional", "performance"].contains(&phase.as_str()));
    if phase == "performance" {
        assert_eq!(selector, "actual");
    }
    let route = std::env::var("TESSERA_QUALIFY_ROUTE").unwrap();
    assert!(["proxy-auto", "proxy-cpu"].contains(&route.as_str()));
    assert_eq!(
        std::env::var("TESSERA_RENDER_BACKEND").unwrap_or_default(),
        if route == "proxy-cpu" { "cpu" } else { "" }
    );
    assert!(std::env::var_os("TESSERA_SMART_PREVIEW_GPU").is_none());
    let format = std::env::var("TESSERA_QUALIFY_FORMAT").unwrap();
    assert!(["sdr", "edr"].contains(&format.as_str()));
    let float = format == "edr";
    let output = std::path::PathBuf::from(std::env::var_os("TESSERA_QUALIFY_OUT").unwrap());
    fs::create_dir_all(&output).unwrap();
    let fixture = std::path::PathBuf::from(std::env::var_os("TESSERA_SMART_PREVIEW_RAW").unwrap());
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
        recipe.settings.output.hdr_headroom_stops = 2.;
    }
    recipe
        .history
        .record(
            &recipe.history.base.clone(),
            &recipe.settings,
            engine_api::recipe::EditMeta::user("reopen baseline", 1),
        )
        .unwrap();
    engine
        .set_recipe_json(
            id.clone(),
            String::from_utf8(recipe.to_json().unwrap()).unwrap(),
        )
        .unwrap();
    let captured_recipe = engine.get_recipe(id.clone()).unwrap();
    let built = engine.build_smart_preview(id.clone()).unwrap();
    assert!(built.width <= 2048 && built.height <= 2048);
    let preview_dir = temp.path().join("support").join("smart-previews").join(&id);
    let journal_path = preview_dir.join("journal.json");
    let pixels_path = preview_dir.join("pixels.tsp");
    let journal_hash = digest(&journal_path);
    let proxy_hash = digest(&pixels_path);
    prepare_engine(&engine, &selector);
    let mut rows = Vec::new();
    let mut edits = Vec::new();
    // One initial open then five unchanged reopens in exactly this Engine.
    // No edit/flush/save is introduced between cycles.
    for cycle in 0..6 {
        GPU.lock().unwrap().take();
        RESIDENT.lock().unwrap().clear();
        let decision_before = decision_counts(&engine);
        let start = Instant::now();
        let session = engine
            .clone()
            .open_smart_preview_develop_session(id.clone())
            .unwrap();
        let returned = Instant::now();
        let info = session.info();
        assert_eq!(info.orientation, 1);
        let metal = info.backend.starts_with("Metal (");
        if route == "proxy-cpu" {
            assert!(!metal);
        }
        let weak_shared = Arc::downgrade(&session.shared);
        let weak_renderer = Arc::downgrade(&session.shared.renderer);
        let weak_gpu = GPU.lock().unwrap().as_ref().map(Arc::downgrade);
        // A measured-CPU hit still constructs a GPU candidate for capability.
        // Retain its Weak for drainage, but never label it a GPU-rendered frame.
        if metal {
            assert!(weak_gpu.is_some());
        } else {
            // Do not retain an unselected GPU candidate just for telemetry.
            // The Weak still proves it drains; this drop remains inside the
            // combined open-through-callback interval on both variants.
            GPU.lock().unwrap().take();
        }
        session
            .set_display_headroom(if float { 4. } else { 1. })
            .unwrap();
        let plan = session.plan_surface(640, 426);
        let ring: Vec<Surface> = (0..2)
            .map(|_| {
                if float {
                    crate::surface::testing::create_owned_rgba16f(plan.width, plan.height)
                } else {
                    Surface::create_rgba8(plan.width, plan.height).unwrap()
                }
            })
            .collect();
        let surface_ids: Vec<_> = ring.iter().map(Surface::id).collect();
        let (send, receive) = mpsc::channel();
        session.set_listener(Some(Arc::new(ReopenListener(send))));
        RESIDENT.lock().unwrap().clear();
        let before = stats(); // Calibration excluded from actual-frame proof.
        for surface in &ring {
            session
                .attach_surface(surface.id(), plan.width, plan.height)
                .unwrap();
        }
        let expected = session.shared.state.lock().unwrap().generation;
        let deadline = Instant::now() + Duration::from_secs(60);
        let (delivered, frame) = loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("frame deadline");
            let (at, frame) = receive.recv_timeout(remaining).expect("frame timeout");
            let frame = frame.expect("render error");
            if frame.generation == expected && frame.is_final {
                break (at, frame);
            }
        };
        let after = stats();
        let decision_after = decision_counts(&engine);
        let output_kind = if float {
            RenderOutput::DisplayLinear(Headroom::new(4.))
        } else {
            RenderOutput::Display
        };
        let resident =
            RESIDENT
                .lock()
                .unwrap()
                .contains(&(frame.generation, frame.level, output_kind));
        let pixel_file = format!("cycle-{cycle}.rgb32f");
        let settings: serde_json::Value =
            serde_json::from_str(&session.get_settings_json().unwrap()).unwrap();
        let mut row = json!({"cycle":cycle,"initial_open":cycle==0,"route":route,"backend":info.backend,
            "format":format,"viewport":[640,426],"settings":settings,"plan_level":plan.level,
            "level":frame.level,"dimensions":[frame.width,frame.height],
            "display_dimensions":[frame.display_width,frame.display_height],"generation":frame.generation,
            "open_ms":returned.duration_since(start).as_secs_f64()*1000.,
            "post_open_delivery_ms":delivered.duration_since(returned).as_secs_f64()*1000.,
            "open_to_final_callback_ms":delivered.duration_since(start).as_secs_f64()*1000.,
            "resident":resident,"submissions":after.submissions-before.submissions,
            "pixel_readback_bytes":after.pixel_readback_bytes-before.pixel_readback_bytes,
            "released":false,"release_ms":null,"phase":"measured", "variant":VARIANT, "selector":selector,
            "decision_before":decision_before,"decision_after":decision_after,
            "release_deadline_ms":5000,"pixel_file":pixel_file});
        // Persist measured evidence before route/pixel/lifecycle assertions can fail.
        fs::write(
            output.join(format!("cycle-{cycle}-measured.json")),
            serde_json::to_vec_pretty(&row).unwrap(),
        )
        .unwrap();
        // Controlled selection is an independent oracle, not inferred from
        // the observed backend or its resulting frame receipt. Persist the
        // measured row first, so a wrong winner retains its timing evidence.
        match selector.as_str() {
            "controlled-cpu" => {
                assert!(!metal, "controlled CPU must materialize CPU on every cycle")
            }
            "controlled-metal" => assert!(
                metal,
                "controlled Metal must materialize Metal on every cycle"
            ),
            "actual" => {}
            _ => unreachable!("selector checked during preparation"),
        }
        validate_decisions(&decision_before, &decision_after, cycle, &route);
        assert_eq!(
            resident, metal,
            "selected backend is not proof of this frame's route"
        );
        assert_eq!(after.submissions > before.submissions, metal);
        assert_eq!(after.pixel_readback_bytes, before.pixel_readback_bytes);
        if metal {
            assert!(after.last_resident_dispatches > 0);
        }
        // All readback, file IO, settings comparison and lifecycle waits are AFTER delivery.
        let pixel_bytes = pixels(
            ring.iter().find(|s| s.id() == frame.surface_id).unwrap(),
            &frame,
            float,
        );
        if float {
            assert!(
                pixel_bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .any(|v| f32::from_le_bytes(*v) > 1.)
            );
        }
        fs::write(output.join(&pixel_file), pixel_bytes).unwrap();
        assert_eq!(settings, serde_json::to_value(&recipe.settings).unwrap());
        if cycle == 5 {
            let mut edited = recipe.settings.clone();
            edited.tone.exposure += 0.25;
            edit_frame(
                "exposure",
                &session,
                &receive,
                FrameOutput {
                    ring: &ring,
                    float,
                    directory: &output,
                },
                &edited,
                metal,
                &mut edits,
            );
            edited.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
            edit_frame(
                "white_balance",
                &session,
                &receive,
                FrameOutput {
                    ring: &ring,
                    float,
                    directory: &output,
                },
                &edited,
                metal,
                &mut edits,
            );
            if phase == "functional" {
                let mut value = serde_json::to_value(&edited).unwrap();
                value["geometry"]["crop"]["rect"]["right"] = json!(0.95);
                value["geometry"]["crop"]["angle"] = json!(2.0);
                let mapped = serde_json::from_value(value).unwrap();
                edit_frame(
                    "mapped_geometry",
                    &session,
                    &receive,
                    FrameOutput {
                        ring: &ring,
                        float,
                        directory: &output,
                    },
                    &mapped,
                    false,
                    &mut edits,
                );
            }
            // No commit/save between warm probes: restore the exact captured
            // live settings and await the restoring frame before close.
            edit_frame(
                "restored",
                &session,
                &receive,
                FrameOutput {
                    ring: &ring,
                    float,
                    directory: &output,
                },
                &recipe.settings,
                metal,
                &mut edits,
            );
            assert_eq!(
                decision_counts(&engine),
                decision_after,
                "viewport edits must not recalibrate"
            );
        }
        session.set_listener(None);
        session.close().unwrap();
        drop(session);
        drop(receive);
        drop(ring);
        GPU.lock().unwrap().take();
        let release_start = Instant::now();
        loop {
            let released = weak_shared.upgrade().is_none()
                && weak_renderer.upgrade().is_none()
                && weak_gpu.as_ref().is_none_or(|w| w.upgrade().is_none())
                && surface_ids.iter().all(|&sid| {
                    Surface::lookup_presentation(sid, plan.width, plan.height).is_err()
                });
            if released {
                break;
            }
            assert!(
                release_start.elapsed() < Duration::from_secs(5),
                "owned resources failed to drain"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let drain_ms = release_start.elapsed().as_secs_f64() * 1000.;
        assert_eq!(engine.get_recipe(id.clone()).unwrap(), captured_recipe);
        assert_eq!(
            digest(&journal_path),
            journal_hash,
            "unchanged open mutated journal"
        );
        assert_eq!(digest(&pixels_path), proxy_hash);
        assert_eq!(digest(&fixture), fixture_hash);
        assert_eq!(digest(&original), fixture_hash);
        row["released"] = json!(true);
        row["release_ms"] = json!(drain_ms);
        row["phase"] = json!("validated_and_released");
        rows.push(row);
        // Incremental evidence survives any later cycle failure.
        fs::write(output.join("reopen-results.json"), serde_json::to_vec_pretty(&json!({
            "metric":"public open to matching final IOSurface callback (not physical display)",
            "engine_count":1,"variant":VARIANT,"selector":selector,"phase":phase,"edits":edits,"fixture_blake3":fixture_hash,"journal_blake3":journal_hash,"proxy_blake3":proxy_hash,"rows":rows})).unwrap()).unwrap();
    }
    let weak_engine = Arc::downgrade(&engine);
    let start = Instant::now();
    drop(engine);
    while weak_engine.upgrade().is_some() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "Engine did not drain"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "Engine drop exceeded deadline"
    );
    fs::write(
        output.join("engine-release.json"),
        serde_json::to_vec_pretty(&json!({
            "released":true,"elapsed_ms":start.elapsed().as_secs_f64()*1000.,"deadline_ms":5000
        }))
        .unwrap(),
    )
    .unwrap();
}
const VARIANT: &str = "baseline";
fn prepare_engine(_: &Engine, selector: &str) {
    assert_eq!(selector, "actual");
}
fn decision_counts(_: &Engine) -> DecisionCounts {
    DecisionCounts {
        measurements: None,
        lookups: None,
        hits: None,
        publications: None,
        entries: None,
        key: None,
    }
}
