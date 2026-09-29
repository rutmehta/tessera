//! WB diagnostic Stage D phase driver (D0; rev7 §6.4). Test + feature only.
//!
//! One process runs one variant/format through one [`EpochGuard`] epoch with
//! automatic backend selection only (nothing is forced: no backend override,
//! no controlled winner, no controlled samples, no clock or TTL change).
//! Each protocol step is one armed phase. After the step's matching final
//! frame the phase is disarmed, allowed to quiesce, drained, and persisted
//! BEFORE any assertion. The copied records are returned only after every
//! protocol contract held: recipe, route receipts, frame, fidelity, source
//! hashes and Weak release. Interpretation lives in `wb_diagnostic_contracts`.
//!
//! Protocol (docs/superpowers/plans/2026-09-28-wb-cache-priming-diagnostic.md):
//! open, close and release, reopen unchanged in the same Engine (within the
//! 30 s TTL), then on the reopened session: exposure 0.5/AsShot, the exact
//! failed first 0.5/Daylight edit, restore 0.5/AsShot, repeat 0.5/Daylight,
//! 0.5/Custom 6500 K tint +10, restore the original recipe; close, release.
//! No timing is recorded as evidence.
use super::*;
use crate::wb_diagnostic_contracts::{
    CUSTOM, Capture, DAYLIGHT, EXPOSURE, FrameSeen, OPEN, ORIGINAL, PhaseCapture, ProbeDelta,
    REOPEN, REPEAT, RESTORE, Txn, phase_json, phase_name,
};
use image_core::wb_diagnostic::{
    ARENA, CAPACITY, Counts, Error,
    harness::{EpochGuard, operator_matches},
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

/// One epoch per process: G12 runs each test in its own process.
static PROCESS_EPOCH: AtomicBool = AtomicBool::new(false);
const QUIESCE: Duration = Duration::from_secs(10);
const FRAME: Duration = Duration::from_secs(60);
const RELEASE: Duration = Duration::from_secs(5);
const VIEWPORT: (u32, u32) = (640, 426);
type Frames = mpsc::Receiver<(Instant, std::result::Result<FrameInfo, String>)>;

fn persist(out: &Path, name: &str, value: &serde_json::Value) {
    fs::write(
        out.join(name),
        serde_json::to_vec_pretty(value).expect("evidence JSON"),
    )
    .expect("persist evidence");
}
fn seen(f: &FrameInfo) -> FrameSeen {
    FrameSeen {
        generation: f.generation,
        level: f.level,
        is_final: f.is_final,
    }
}
fn frame_json(f: &FrameInfo) -> serde_json::Value {
    json!({"generation": f.generation, "level": f.level, "first_level": f.first_level,
        "is_final": f.is_final, "surface_id": f.surface_id, "dimensions": [f.width, f.height],
        "display_dimensions": [f.display_width, f.display_height]})
}
/// Copies every completed slot out of the scratch view (bounded).
fn drain_all(g: &EpochGuard) -> (Vec<Txn>, bool) {
    let mut out = Vec::new();
    for _ in 0..=2 * CAPACITY {
        if !ARENA.counts().is_ok_and(|c| c.completed > 0) {
            return (out, true);
        }
        let Ok(d) = g.epoch().drain() else {
            return (out, false);
        };
        out.push(d.inspect(|v| Txn {
            token: format!("{:?}", v.meta.token),
            request: v.meta.request,
            begin: v.meta.begin,
            route: v.meta.route,
            loss_at_finish: v.meta.loss_at_finish,
            operator_matches: operator_matches(v.meta),
            context: v.snapshot.context,
            records: v.snapshot.records[..v.snapshot.len.min(CAPACITY)].to_vec(),
            overflow: v.snapshot.overflow,
        }));
    }
    (out, ARENA.counts().is_ok_and(|c| c.completed == 0))
}
/// Waits (bounded) until no reservation or lease is outstanding.
fn quiesce() -> bool {
    let deadline = Instant::now() + QUIESCE;
    loop {
        if ARENA
            .counts()
            .is_ok_and(|c| c.reserved == 0 && c.active == 0)
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}
/// Waits for the matching final frame, recording every frame seen.
fn await_final(
    receive: &Frames,
    expected: u64,
    frames: &mut Vec<FrameSeen>,
    errors: &mut Vec<String>,
) -> std::result::Result<FrameInfo, String> {
    let deadline = Instant::now() + FRAME;
    loop {
        let wait = deadline
            .checked_duration_since(Instant::now())
            .ok_or("frame deadline")?;
        let (_, frame) = receive
            .recv_timeout(wait)
            .map_err(|e| format!("frame timeout: {e}"))?;
        match frame {
            Ok(frame) => {
                frames.push(seen(&frame));
                if frame.generation == expected && frame.is_final {
                    return Ok(frame);
                }
            }
            Err(e) => {
                errors.push(e.clone());
                return Err(format!("render error: {e}"));
            }
        }
    }
}

struct Phase<'g> {
    guard: &'g EpochGuard,
    ordinal: u64,
    before: Counts,
    frames: Vec<FrameSeen>,
    errors: Vec<String>,
}
fn arm(guard: &EpochGuard, ordinal: u64) -> Phase<'_> {
    let before = ARENA.counts().expect("arena counts");
    guard.epoch().arm(ordinal).expect("arm phase");
    Phase {
        guard,
        ordinal,
        before,
        frames: Vec::new(),
        errors: Vec::new(),
    }
}
/// Disarms, quiesces, collects late frames, drains, and persists the phase
/// before the caller asserts anything about it.
fn finish(
    p: Phase<'_>,
    receive: &Frames,
    final_generation: u64,
    metal: bool,
    out: &Path,
    protocol: serde_json::Value,
) -> PhaseCapture {
    p.guard.epoch().disarm().expect("disarm phase");
    let quiescent = quiesce();
    let (mut frames, mut errors) = (p.frames, p.errors);
    while let Ok((_, f)) = receive.try_recv() {
        match f {
            Ok(f) => frames.push(seen(&f)),
            Err(e) => errors.push(e),
        }
    }
    let counts_after = ARENA.counts().expect("arena counts");
    let (txns, drained_all) = drain_all(p.guard);
    let capture = PhaseCapture {
        ordinal: p.ordinal,
        counts_before: p.before,
        counts_after,
        quiescent,
        drained_all,
        txns,
        frames,
        final_generation,
        metal,
    };
    let mut value = phase_json(&capture);
    value["protocol"] = protocol;
    value["render_errors"] = json!(errors);
    value["validated"] = json!(false);
    persist(
        out,
        &format!("phase-{}-{}.json", p.ordinal, phase_name(p.ordinal)),
        &value,
    );
    capture
}
fn probe_delta(
    before: &DecisionCounts,
    after: &DecisionCounts,
    open_key: Option<[u8; 32]>,
) -> Option<ProbeDelta> {
    let d = |b: Option<u64>, a: Option<u64>| Some(a?.checked_sub(b?).unwrap_or(u64::MAX));
    Some(ProbeDelta {
        lookups: d(before.lookups, after.lookups)?,
        hits: d(before.hits, after.hits)?,
        measurements: d(before.measurements, after.measurements)?,
        publications: d(before.publications, after.publications)?,
        key_identical: open_key.is_some() && open_key == after.key,
    })
}
/// Frame, route-receipt and fidelity contracts of the qualification harness.
struct FrameCheck<'a> {
    label: &'a str,
    frame: std::result::Result<FrameInfo, String>,
    metal: bool,
    resident: bool,
    before: &'a pipeline_gpu::GpuStats,
    after: &'a pipeline_gpu::GpuStats,
    ring: &'a [Surface],
    float: bool,
    expect_hdr_values: bool,
    out: &'a Path,
}
fn check_frame(c: FrameCheck<'_>) {
    let label = c.label;
    let frame = c
        .frame
        .unwrap_or_else(|e| panic!("WB-D {label}: no matching final frame: {e}"));
    assert_eq!(
        c.resident, c.metal,
        "WB-D {label}: selected backend is not proof of this frame's route"
    );
    assert_eq!(
        c.after.submissions > c.before.submissions,
        c.metal,
        "{label}"
    );
    assert_eq!(
        c.after.pixel_readback_bytes, c.before.pixel_readback_bytes,
        "{label}"
    );
    if c.metal {
        assert!(c.after.last_resident_dispatches > 0, "{label}");
    }
    let bytes = pixels(
        c.ring
            .iter()
            .find(|s| s.id() == frame.surface_id)
            .expect("frame surface"),
        &frame,
        c.float,
    );
    if c.expect_hdr_values {
        assert!(
            bytes
                .as_chunks::<4>()
                .0
                .iter()
                .any(|v| f32::from_le_bytes(*v) > 1.),
            "WB-D {label}: EDR frame has no value above SDR white"
        );
    }
    fs::write(c.out.join(format!("{label}.rgb32f")), bytes).expect("pixel evidence");
}
fn output_kind(float: bool) -> RenderOutput {
    if float {
        RenderOutput::DisplayLinear(Headroom::new(4.))
    } else {
        RenderOutput::Display
    }
}
fn resident(frame: &std::result::Result<FrameInfo, String>, float: bool) -> bool {
    frame.as_ref().is_ok_and(|f| {
        RESIDENT
            .lock()
            .unwrap()
            .contains(&(f.generation, f.level, output_kind(float)))
    })
}
fn stats_json(
    before: &pipeline_gpu::GpuStats,
    after: &pipeline_gpu::GpuStats,
) -> serde_json::Value {
    json!({"submissions": after.submissions.saturating_sub(before.submissions),
        "pixel_readback_bytes": after.pixel_readback_bytes.saturating_sub(before.pixel_readback_bytes),
        "last_resident_dispatches": after.last_resident_dispatches})
}

/// An open session and what its release must drain.
struct Open {
    session: Arc<DevelopSession>,
    receive: Frames,
    ring: Vec<Surface>,
    plan: SurfacePlan,
    metal: bool,
    weak_shared: std::sync::Weak<Shared>,
    weak_renderer: std::sync::Weak<image_core::Renderer>,
    weak_gpu: Option<std::sync::Weak<pipeline_gpu::GpuStageOp>>,
}
fn release(o: Open, label: &str) -> serde_json::Value {
    let surface_ids: Vec<_> = o.ring.iter().map(Surface::id).collect();
    o.session.set_listener(None);
    o.session.close().unwrap();
    drop(o.session);
    drop(o.receive);
    drop(o.ring);
    GPU.lock().unwrap().take();
    let start = Instant::now();
    loop {
        let released = o.weak_shared.upgrade().is_none()
            && o.weak_renderer.upgrade().is_none()
            && o.weak_gpu.as_ref().is_none_or(|w| w.upgrade().is_none())
            && surface_ids.iter().all(|&sid| {
                Surface::lookup_presentation(sid, o.plan.width, o.plan.height).is_err()
            });
        if released {
            break;
        }
        assert!(
            start.elapsed() < RELEASE,
            "WB-D {label}: owned resources failed to drain"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    json!({"released": true, "deadline_ms": RELEASE.as_millis() as u64})
}

/// Runs the protocol; see the module docs. `candidate` must match the
/// composition this binary was built from.
pub(crate) fn run(
    candidate: bool,
    edr: bool,
    fixture: &Path,
) -> std::result::Result<Capture, Error> {
    assert_eq!(
        candidate,
        VARIANT == "candidate",
        "WB-D: {} tests must run on the {VARIANT} composition's binary",
        if candidate { "candidate" } else { "baseline" }
    );
    assert!(
        !PROCESS_EPOCH.swap(true, Ordering::SeqCst),
        "WB-D: one epoch per process; run each G12 test in its own process"
    );
    // Automatic selection only: never forced (review items 1 and 9).
    let forced = std::env::var("TESSERA_RENDER_BACKEND").unwrap_or_default();
    assert!(
        forced.is_empty(),
        "WB-D: TESSERA_RENDER_BACKEND must be unset (automatic selection only)"
    );
    assert!(std::env::var_os("TESSERA_SMART_PREVIEW_GPU").is_none());
    assert_eq!(
        std::env::var("TESSERA_QUALIFY_ROUTE").as_deref(),
        Ok("proxy-auto"),
        "WB-D: TESSERA_QUALIFY_ROUTE=proxy-auto enables route receipts and GPU stats"
    );
    let root = PathBuf::from(
        std::env::var_os("TESSERA_WB_DIAG_OUT").expect("TESSERA_WB_DIAG_OUT evidence root"),
    );
    let format = if edr { "edr" } else { "sdr" };
    let out = root.join(format!("{VARIANT}-{format}"));
    assert!(
        !out.exists()
            || fs::read_dir(&out)
                .expect("evidence directory")
                .next()
                .is_none(),
        "WB-D: evidence directory {} is not empty; a process is never rerun selectively",
        out.display()
    );
    fs::create_dir_all(&out).expect("evidence directory");
    let guard = EpochGuard::open().unwrap_or_else(|e| {
        panic!("WB-D: EpochGuard refused ({e:?}); requires --test-threads=1 and an Idle arena")
    });
    let float = edr;

    // Fixture, catalog and recipe: identical to the frozen qualification.
    let fixture_hash = digest(fixture);
    let temp = tempfile::tempdir().unwrap();
    let photos = temp.path().join("photos");
    fs::create_dir(&photos).unwrap();
    let original = photos.join(fixture.file_name().unwrap());
    fs::copy(fixture, &original).unwrap();
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
    prepare_engine(&engine, "actual");
    persist(
        &out,
        "process.json",
        &json!({"variant": VARIANT, "format": format, "epoch": guard.epoch().epoch(),
            "selector": "actual", "automatic": true, "viewport": [VIEWPORT.0, VIEWPORT.1],
            "fixture_blake3": fixture_hash, "journal_blake3": journal_hash,
            "proxy_blake3": proxy_hash, "smart_preview": [built.width, built.height],
            "settings": recipe.settings,
            "note": "diagnostic evidence only; no timing is recorded (rev7 §8)"}),
    );
    let hashes_unchanged = |label: &str| {
        assert_eq!(
            engine.get_recipe(id.clone()).unwrap(),
            captured_recipe,
            "WB-D {label}: recipe changed"
        );
        assert_eq!(
            digest(&journal_path),
            journal_hash,
            "WB-D {label}: journal changed"
        );
        assert_eq!(
            digest(&pixels_path),
            proxy_hash,
            "WB-D {label}: proxy changed"
        );
        assert_eq!(
            digest(fixture),
            fixture_hash,
            "WB-D {label}: fixture changed"
        );
        assert_eq!(
            digest(&original),
            fixture_hash,
            "WB-D {label}: copy changed"
        );
    };

    let mut phases = Vec::new();
    let mut open_key = None;
    let mut probe = None;
    let mut session = None;
    // Initial open, then one unchanged reopen in exactly this Engine.
    for ordinal in [OPEN, REOPEN] {
        let label = phase_name(ordinal);
        GPU.lock().unwrap().take();
        RESIDENT.lock().unwrap().clear();
        let decision_before = decision_counts(&engine);
        let mut phase = arm(&guard, ordinal);
        let s = engine
            .clone()
            .open_smart_preview_develop_session(id.clone())
            .unwrap();
        let info = s.info();
        let metal = info.backend.starts_with("Metal (");
        let weak_shared = Arc::downgrade(&s.shared);
        let weak_renderer = Arc::downgrade(&s.shared.renderer);
        let weak_gpu = GPU.lock().unwrap().as_ref().map(Arc::downgrade);
        if !metal {
            // Never retain an unselected GPU candidate (as the harness does).
            GPU.lock().unwrap().take();
        }
        s.set_display_headroom(if float { 4. } else { 1. }).unwrap();
        let plan = s.plan_surface(VIEWPORT.0, VIEWPORT.1);
        let ring: Vec<Surface> = (0..2)
            .map(|_| {
                if float {
                    crate::surface::testing::create_owned_rgba16f(plan.width, plan.height)
                } else {
                    Surface::create_rgba8(plan.width, plan.height).unwrap()
                }
            })
            .collect();
        let (send, receive) = mpsc::channel();
        s.set_listener(Some(Arc::new(ReopenListener(send))));
        RESIDENT.lock().unwrap().clear();
        let before = stats();
        for surface in &ring {
            s.attach_surface(surface.id(), plan.width, plan.height)
                .unwrap();
        }
        let expected = s.shared.state.lock().unwrap().generation;
        let frame = await_final(&receive, expected, &mut phase.frames, &mut phase.errors);
        let after = stats();
        let decision_after = decision_counts(&engine);
        let on_route = resident(&frame, float);
        let settings: serde_json::Value =
            serde_json::from_str(&s.get_settings_json().unwrap()).unwrap();
        let captured = finish(
            phase,
            &receive,
            expected,
            metal,
            &out,
            json!({"backend": info.backend, "metal": metal, "orientation": info.orientation,
                "plan_level": plan.level, "frame": frame.as_ref().map(frame_json).ok(),
                "resident": on_route, "stats": stats_json(&before, &after),
                "decision_before": decision_before, "decision_after": decision_after,
                "settings": settings}),
        );
        phases.push(captured);
        // Protocol contracts, only after the phase is on disk.
        assert_eq!(info.orientation, 1);
        assert_eq!(
            settings,
            serde_json::to_value(&recipe.settings).unwrap(),
            "WB-D {label}: session settings differ from the recipe"
        );
        check_frame(FrameCheck {
            label,
            frame,
            metal,
            resident: on_route,
            before: &before,
            after: &after,
            ring: &ring,
            float,
            expect_hdr_values: float,
            out: &out,
        });
        let o = Open {
            session: s,
            receive,
            ring,
            plan,
            metal,
            weak_shared,
            weak_renderer,
            weak_gpu,
        };
        if ordinal == OPEN {
            open_key = decision_after.key;
            let released = release(o, label);
            hashes_unchanged(label);
            persist(&out, "open-release.json", &released);
        } else {
            probe = probe_delta(&decision_before, &decision_after, open_key);
            session = Some((o, decision_after));
        }
    }

    // Edits on the reopened session (review items 4 and 5).
    let (o, decision_reopened) = session.expect("reopened session");
    let base = recipe.settings.clone();
    let mut exposure = base.clone();
    exposure.tone.exposure += 0.25;
    let mut daylight = exposure.clone();
    daylight.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Daylight;
    let mut custom = exposure.clone();
    custom.white_balance = engine_api::recipe::settings::WhiteBalanceSettings {
        mode: engine_api::recipe::settings::WhiteBalanceMode::Custom,
        temperature: 6500.,
        tint: 10.,
    };
    for (ordinal, settings) in [
        (EXPOSURE, &exposure),
        (DAYLIGHT, &daylight),
        (RESTORE, &exposure),
        (REPEAT, &daylight),
        (CUSTOM, &custom),
        (ORIGINAL, &base),
    ] {
        let label = phase_name(ordinal);
        let patch = serde_json::to_string(settings).unwrap();
        RESIDENT.lock().unwrap().clear();
        let before = stats();
        let mut phase = arm(&guard, ordinal);
        o.session.set_settings(patch, false).unwrap();
        let expected = o.session.shared.state.lock().unwrap().generation;
        let frame = await_final(&o.receive, expected, &mut phase.frames, &mut phase.errors);
        let after = stats();
        let on_route = resident(&frame, float);
        let live: serde_json::Value =
            serde_json::from_str(&o.session.get_settings_json().unwrap()).unwrap();
        let captured = finish(
            phase,
            &o.receive,
            expected,
            o.metal,
            &out,
            json!({"settings": settings, "live_settings": live,
                "frame": frame.as_ref().map(frame_json).ok(), "resident": on_route,
                "stats": stats_json(&before, &after)}),
        );
        phases.push(captured);
        check_frame(FrameCheck {
            label,
            frame,
            metal: o.metal,
            resident: on_route,
            before: &before,
            after: &after,
            ring: &o.ring,
            float,
            expect_hdr_values: false,
            out: &out,
        });
    }
    assert_eq!(
        decision_counts(&engine),
        decision_reopened,
        "WB-D: viewport edits must not recalibrate"
    );
    let released = release(o, "reopen");
    hashes_unchanged("reopen");
    persist(&out, "reopen-release.json", &released);
    let weak_engine = Arc::downgrade(&engine);
    drop(engine);
    let start = Instant::now();
    while weak_engine.upgrade().is_some() {
        assert!(start.elapsed() < RELEASE, "WB-D: Engine did not drain");
        std::thread::sleep(Duration::from_millis(5));
    }
    // Close the process's single epoch; the guard acknowledges it on drop.
    let final_counts = ARENA.counts().expect("arena counts");
    drop(guard);
    persist(
        &out,
        "protocol-validated.json",
        &json!({"validated": true, "phases": phases.iter().map(|p| phase_name(p.ordinal)).collect::<Vec<_>>(),
            "final_counts": {"loss": final_counts.loss, "disabled": final_counts.disabled,
                "cpu_iterations_unobserved": final_counts.cpu_iterations_unobserved,
                "reserved": final_counts.reserved, "active": final_counts.active},
            "arena_state_after_guard": format!("{:?}", ARENA.state())}),
    );
    Ok(Capture {
        candidate,
        edr,
        automatic: true,
        out,
        phases,
        probe,
    })
}
