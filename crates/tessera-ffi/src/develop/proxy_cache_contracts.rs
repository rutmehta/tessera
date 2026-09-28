//! Task 2 opt-in Engine contracts; observation/control stubs only, integration UNIMPLEMENTED.
//! Real RAW copy/prefix preparation; controlled selection is not actual Metal performance evidence.
use super::*;
use std::{fs, path::PathBuf, time::Instant};
use serde_json::json;

#[derive(Clone, Copy)]
enum SelectionControl { MeasuredCpu, MeasuredMetal, CalibrationFailure, DeviceUnavailable, DeviceLost, ExplicitCpu, ExplicitMetal, UnversionedExternal }
#[derive(Clone, Default, Debug, PartialEq)]
struct Probe {
    validations: u64, lookups: u64, hits: u64, measurements: u64, entries: usize,
    capability_checks: u64, capability_hdr: bool, capability_headroom: f32,
    key_hdr: bool, key_headroom: f32,
    full_container: [u8; 32], incarnation: [u8; 32], generation: u64, recipe_digest: [u8; 32],
}
impl Engine {
    /// Stub. Future hook is Engine-local and controls measurement/device/override
    /// policy only AFTER real source/recipe validation, never fakes asset validity.
    fn cache_control_for_test(&self, _control: SelectionControl) {}
    fn cache_probe_for_test(&self) -> Probe { Probe::default() }
}
struct Fixture { _dir: tempfile::TempDir, engine: Arc<Engine>, id: String, original: PathBuf, local: PathBuf, fixture: PathBuf, hash: blake3::Hash }
impl Fixture {
    fn new(hdr: bool) -> Self {
        let fixture = PathBuf::from(std::env::var_os("TESSERA_SMART_PREVIEW_RAW").expect("explicit read-only Sony fixture required"));
        let bytes = fs::read(&fixture).unwrap(); let hash = blake3::hash(&bytes);
        let dir = tempfile::tempdir().unwrap(); let photos = dir.path().join("photos"); fs::create_dir(&photos).unwrap();
        let original = photos.join("copy.ARW"); fs::write(&original, bytes).unwrap();
        let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        engine.index_folder(photos.to_string_lossy().into()).unwrap();
        let images = engine.list_images(crate::ImageQuery::default()).unwrap(); assert_eq!(images.len(), 1); let id = images[0].id.clone();
        let mut recipe: engine_api::recipe::Recipe = serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
        recipe.process_version = engine_api::recipe::ProcessVersion::NATIVE_CURRENT;
        recipe.settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Off;
        recipe.settings.tone.exposure = 0.25;
        if hdr { recipe.settings.output.hdr = true; recipe.settings.output.hdr_headroom_stops = 2.; }
        recipe.history.record(&recipe.history.base.clone(), &recipe.settings, engine_api::recipe::EditMeta::user("cache contracts", 1)).unwrap();
        engine.set_recipe_json(id.clone(), String::from_utf8(recipe.to_json().unwrap()).unwrap()).unwrap();
        engine.build_smart_preview(id.clone()).unwrap();
        let local = dir.path().join("support/smart-previews").join(&id);
        Self { _dir: dir, engine, id, original, local, fixture, hash }
    }
    fn open(&self) -> Arc<DevelopSession> { self.engine.clone().open_smart_preview_develop_session(self.id.clone()).unwrap() }
    fn cycle(&self) {
        let session = self.open(); let shared = Arc::downgrade(&session.shared); let renderer = Arc::downgrade(&session.shared.renderer);
        session.close().unwrap(); drop(session);
        let start = Instant::now();
        while shared.upgrade().is_some() || renderer.upgrade().is_some() {
            assert!(start.elapsed() < Duration::from_secs(5)); std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn prime(&self, control: SelectionControl) {
        self.engine.cache_control_for_test(control); self.cycle();
        let p = self.engine.cache_probe_for_test(); assert_eq!(p.measurements, 1); assert_eq!(p.entries, 1);
    }
    fn assert_fixture(&self) { assert_eq!(blake3::hash(&fs::read(&self.fixture).unwrap()), self.hash); }
}
impl Drop for Fixture { fn drop(&mut self) { self.assert_fixture(); } }

#[test]
#[ignore = "explicit copied RAW; Task2 integration RED, no throughput claim"]
fn unchanged_public_open_reuses_cpu_and_metal_decisions_in_sdr_and_hdr() {
    for hdr in [false, true] { for control in [SelectionControl::MeasuredCpu, SelectionControl::MeasuredMetal] {
        let f = Fixture::new(hdr); let recipe = f.engine.get_recipe(f.id.clone()).unwrap(); f.prime(control);
        let before = f.engine.cache_probe_for_test(); f.cycle(); let after = f.engine.cache_probe_for_test();
        assert_eq!(after.validations, before.validations + 1); assert_eq!(after.lookups, before.lookups + 1);
        assert_eq!(after.hits, before.hits + 1); assert_eq!(after.measurements, before.measurements); assert_eq!(after.entries, 1);
        assert!(after.capability_checks > before.capability_checks, "measured CPU hit must still recheck normalized GPU capability");
        assert!(!after.capability_hdr); assert_eq!(after.capability_headroom, 0.);
        assert_eq!(after.key_hdr, hdr); if hdr { assert_eq!(after.key_headroom, 2.); }
        assert_eq!(f.engine.get_recipe(f.id.clone()).unwrap(), recipe);
        assert_eq!(blake3::hash(&fs::read(&f.original).unwrap()), f.hash);
    }}
}

fn corrupt(f: &Fixture, which: &str) {
    match which {
        "container" => fs::write(f.local.join("pixels.tsp"), b"corrupt").unwrap(),
        "journal" => fs::write(f.local.join("journal.json"), b"corrupt").unwrap(),
        "original" => fs::write(&f.original, b"external changed source").unwrap(),
        "sidecar" => fs::write(sidecar::Sidecar::paths(&f.original).recipe, b"external changed sidecar").unwrap(),
        _ => unreachable!(),
    }
}
#[test]
#[ignore = "existing validation controls; no new RED claim"]
fn cold_public_open_rejects_invalid_sources_without_cache() {
    for which in ["container", "journal", "original", "sidecar"] {
        let f = Fixture::new(false); corrupt(&f, which);
        assert!(f.engine.clone().open_smart_preview_develop_session(f.id.clone()).is_err(), "{which}");
    }
}
#[test]
#[ignore = "explicit copied RAW; new hot-cache validation ordering"]
fn cached_decision_cannot_bypass_real_source_or_sidecar_validation() {
    for which in ["container", "journal", "original", "sidecar"] {
        let f = Fixture::new(false); f.prime(SelectionControl::MeasuredCpu); let before = f.engine.cache_probe_for_test();
        corrupt(&f, which);
        assert!(f.engine.clone().open_smart_preview_develop_session(f.id.clone()).is_err(), "{which}");
        let after = f.engine.cache_probe_for_test(); assert_eq!(after.lookups, before.lookups, "lookup ran before {which} validation");
        assert_eq!(after.hits, before.hits); assert_eq!(after.measurements, before.measurements);
    }
}

#[test]
#[ignore = "explicit copied RAW; identity transport and rebuild/edit misses"]
fn validated_full_identity_is_transported_and_rebuild_or_edit_misses() {
    let f = Fixture::new(true); f.prime(SelectionControl::MeasuredCpu); let initial = f.engine.cache_probe_for_test();
    let decoded = pipeline_cpu::CameraLinearProxy::decode_persistent(&fs::read(f.local.join("pixels.tsp")).unwrap()).unwrap();
    let (_, snapshot) = f.engine.local_smart_preview(parse_id(&f.id).unwrap()).unwrap().unwrap();
    let journal: serde_json::Value = serde_json::from_slice(&fs::read(f.local.join("journal.json")).unwrap()).unwrap();
    assert_eq!(initial.full_container, decoded.container_digest);
    assert_eq!(initial.incarnation, serde_json::from_value::<[u8; 32]>(journal["incarnation"].clone()).unwrap());
    assert_eq!(initial.generation, snapshot.generation); assert_eq!(initial.recipe_digest, snapshot.recipe_digest);
    f.engine.discard_smart_preview(f.id.clone()).unwrap(); f.engine.build_smart_preview(f.id.clone()).unwrap(); f.cycle();
    let rebuilt = f.engine.cache_probe_for_test(); assert_ne!(rebuilt.incarnation, initial.incarnation); assert_eq!(rebuilt.measurements, initial.measurements + 1);
    let session = f.open(); session.set_settings(json!({"tone":{"exposure":0.75}}).to_string(), false).unwrap(); session.flush().unwrap(); session.close().unwrap(); drop(session);
    let before = f.engine.cache_probe_for_test(); f.cycle(); let edited = f.engine.cache_probe_for_test();
    assert_eq!(edited.measurements, before.measurements + 1); assert_ne!(edited.generation, rebuilt.generation); assert_ne!(edited.recipe_digest, rebuilt.recipe_digest);
    assert!(f.engine.clone().open_develop_session(f.id.clone()).is_err(), "dirty local recipe must still block Original");
}

#[test]
#[ignore = "explicit copied RAW; per-Engine policy controls are not real performance"]
fn overrides_external_geometry_and_device_failures_do_not_reuse_or_poison() {
    for control in [SelectionControl::ExplicitCpu, SelectionControl::ExplicitMetal, SelectionControl::UnversionedExternal,
        SelectionControl::DeviceUnavailable, SelectionControl::DeviceLost] {
        let f = Fixture::new(false); f.prime(SelectionControl::MeasuredCpu); let before = f.engine.cache_probe_for_test();
        f.engine.cache_control_for_test(control); f.cycle(); let after = f.engine.cache_probe_for_test();
        assert_eq!(after.hits, before.hits);
        if matches!(control, SelectionControl::DeviceLost | SelectionControl::DeviceUnavailable) { assert_eq!(after.entries, 0); }
        f.engine.cache_control_for_test(SelectionControl::MeasuredCpu); f.cycle();
        assert!(f.engine.cache_probe_for_test().entries > 0);
    }
    // Calibration failure is a miss outcome, not a reason to bypass an already
    // valid hit. Exercise it with an empty cache, then require later success.
    let failed = Fixture::new(false);
    failed.engine.cache_control_for_test(SelectionControl::CalibrationFailure);
    failed.cycle(); assert_eq!(failed.engine.cache_probe_for_test().entries, 0);
    failed.engine.cache_control_for_test(SelectionControl::MeasuredCpu); failed.cycle();
    assert_eq!(failed.engine.cache_probe_for_test().entries, 1);
    let f = Fixture::new(false); f.prime(SelectionControl::MeasuredCpu);
    let session = f.open(); session.set_settings(json!({"geometry":{"crop":{"rect":{"right":0.9},"angle":2.0}}}).to_string(), false).unwrap(); session.flush().unwrap(); session.close().unwrap(); drop(session);
    let before = f.engine.cache_probe_for_test(); f.cycle(); let after = f.engine.cache_probe_for_test();
    assert_eq!(after.hits, before.hits); assert_eq!(after.entries, before.entries);
}

#[test]
#[ignore = "explicit copied RAW; admission and failed-open lease controls"]
fn cache_does_not_override_active_editor_or_retain_failed_open_lease() {
    let f = Fixture::new(false); f.prime(SelectionControl::MeasuredCpu); let session = f.open();
    let before = f.engine.cache_probe_for_test();
    assert!(f.engine.clone().open_smart_preview_develop_session(f.id.clone()).is_err());
    assert_eq!(f.engine.cache_probe_for_test(), before); session.close().unwrap(); drop(session);
    let valid = fs::read(f.local.join("pixels.tsp")).unwrap(); corrupt(&f, "container");
    assert!(f.engine.clone().open_smart_preview_develop_session(f.id.clone()).is_err());
    fs::write(f.local.join("pixels.tsp"), valid).unwrap(); f.cycle();
}
