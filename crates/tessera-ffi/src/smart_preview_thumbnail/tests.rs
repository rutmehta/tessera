use super::*;
use engine_api::{
    color::ColorMatrix3,
    id::JobId,
    jobs::CancellationToken,
    recipe::{ProcessVersion, Recipe},
};
use pipeline_cpu::LensContext;
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
use std::time::{Duration, Instant};

fn fixture(w: u32, h: u32) -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 6,
        width: w,
        height: h,
        cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, w, h],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let c = CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| 0.1 + (i % w) as f32 / (w as f32) * 0.15 + ((i / w) % 2) as f32 * 0.1)
            .collect(),
    )
    .unwrap();
    (c, m)
}

struct Fixture {
    dir: tempfile::TempDir,
    engine: Arc<Engine>,
    id: ImageId,
    original: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let support = dir.path().join("support");
        let original = dir.path().join("disconnected/photo.arw");
        let engine = Engine::open(support.to_string_lossy().into()).unwrap();
        let id = ImageId(501);
        let db = rusqlite::Connection::open(&engine.db).unwrap();
        db.execute(
            "INSERT INTO root(id,path) VALUES(1,?)",
            [original.parent().unwrap().to_str().unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO folder(id,root_id,path) VALUES(1,1,?)",
            [original.parent().unwrap().to_str().unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO file(id,folder_id,path,name,size,mtime) VALUES(1,1,?,'photo.arw',100,1)",
            [original.to_str().unwrap()],
        )
        .unwrap();
        db.execute(
            "INSERT INTO image(id,file_id) VALUES(?,1)",
            [id.to_string()],
        )
        .unwrap();
        let recipe = Recipe::new(id);
        let (cfa, metadata) = fixture(32, 24);
        let proxy = CameraLinearProxy::generate(
            &cfa,
            &metadata,
            &recipe.settings,
            ProcessVersion::NATIVE_CURRENT,
            [7; 32],
            &LensContext::default(),
        )
        .unwrap();
        let bytes = proxy.encode_persistent(100).unwrap();
        let recipe = serde_json::to_vec(&sidecar::RecipeDocument {
            recipe,
            ..Default::default()
        })
        .unwrap();
        SmartPreviewJournal::create(&support, id, [7; 32], 100, recipe, None, None).unwrap();
        fs::write(
            support
                .join("smart-previews")
                .join(id.to_string())
                .join("pixels.tsp"),
            bytes,
        )
        .unwrap();
        Self {
            dir,
            engine,
            id,
            original,
        }
    }
    fn bytes(&self) -> Vec<u8> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let response = self
                .engine
                .clone()
                .smart_preview_thumbnail(self.id.to_string(), 32)
                .unwrap();
            if let Some(bytes) = response.bytes {
                return bytes;
            }
            assert!(response.pending && Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn job(&self) -> Box<ThumbnailJob> {
        let identity = self.engine.thumbnail_local(self.id).unwrap().identity;
        self.engine
            .smart_thumbnail_states
            .lock()
            .unwrap()
            .entries
            .insert(
                (self.id, 32),
                Entry {
                    identity: identity.clone(),
                    state: State::Pending,
                },
            );
        Box::new(ThumbnailJob {
            engine: Arc::downgrade(&self.engine),
            slot: (self.id, 32),
            identity,
            completed: false,
        })
    }
}
#[test]
fn offline_thumbnail_renders_orientation_and_survives_new_engine_without_original() {
    let f = Fixture::new();
    let bytes = f.bytes();
    let image = image::load_from_memory(&bytes).unwrap();
    assert_eq!((image.width(), image.height()), (24, 32));
    assert!(!f.original.parent().unwrap().exists());
    let fresh = Engine::open(f.engine.support_dir().unwrap().to_string_lossy().into()).unwrap();
    let start = Instant::now();
    loop {
        let response = fresh
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
            .unwrap();
        if let Some(other) = response.bytes {
            assert_eq!(other, bytes);
            break;
        }
        assert!(start.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!f.original.parent().unwrap().exists());
}
#[test]
fn saved_edit_invalidates_ready_bytes_and_changes_render() {
    let f = Fixture::new();
    let before = f.bytes();
    let (mut journal, snapshot) =
        SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    let mut doc: sidecar::RecipeDocument = serde_json::from_slice(&snapshot.recipe).unwrap();
    let previous = doc.recipe.settings.clone();
    doc.recipe.settings.tone.exposure = 1.25;
    doc.recipe
        .history
        .record(
            &previous,
            &doc.recipe.settings,
            engine_api::recipe::EditMeta::user("edit", 1),
        )
        .unwrap();
    journal
        .save_recipe(serde_json::to_vec(&doc).unwrap())
        .unwrap();
    assert_ne!(before, f.bytes());
    assert!(!f.original.parent().unwrap().exists());
}
#[test]
fn corrupt_asset_cannot_reuse_ready_cache_and_missing_store_creates_nothing() {
    let f = Fixture::new();
    f.bytes();
    let local = f.engine.thumbnail_local(f.id).unwrap();
    fs::write(&local.path, b"corrupt").unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match f
            .engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
        {
            Err(_) => break,
            Ok(response) => {
                assert!(response.bytes.is_none());
                assert!(Instant::now() < deadline);
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    fs::remove_dir_all(local.path.parent().unwrap()).unwrap();
    assert!(
        f.engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
            .is_err()
    );
    assert!(!local.path.parent().unwrap().exists());
    assert!(!f.original.parent().unwrap().exists());
}
#[test]
fn replacement_incarnation_cannot_publish_old_job_even_with_equal_generation_and_recipe() {
    let f = Fixture::new();
    let mut job = f.job();
    let ctx = JobContext::new(JobId(0), CancellationToken::new(), None);
    let result = job.render(&f.engine, &ctx);
    assert!(result.is_ok());
    let (journal, snapshot) =
        SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    journal.discard_clean().unwrap();
    SmartPreviewJournal::create(
        f.engine.support_dir().unwrap(),
        f.id,
        snapshot.source_digest,
        snapshot.source_len,
        snapshot.recipe,
        None,
        None,
    )
    .unwrap();
    let now = f.engine.thumbnail_local(f.id).unwrap().identity;
    assert_eq!(now.generation, job.identity.generation);
    assert_ne!(now.incarnation, job.identity.incarnation);
    job.finish(&f.engine, result, false);
    assert!(matches!(
        f.engine.smart_thumbnail_states.lock().unwrap().entries[&(f.id, 32)].state,
        State::Failed(_)
    ));
    assert!(!f.bytes().is_empty());
}
#[test]
fn cancellation_and_drop_make_pending_terminal_and_size_bounds_fail() {
    let f = Fixture::new();
    let job = f.job();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(job.run(&JobContext::new(JobId(0), cancel, None)).is_err());
    assert!(matches!(
        f.engine.smart_thumbnail_states.lock().unwrap().entries[&(f.id, 32)].state,
        State::Interrupted
    ));
    drop(f.job());
    assert!(matches!(
        f.engine.smart_thumbnail_states.lock().unwrap().entries[&(f.id, 32)].state,
        State::Interrupted
    ));
    assert!(
        !f.bytes().is_empty(),
        "Cancellation must permit an unchanged-identity retry"
    );
    for size in [0, 2561, u32::MAX] {
        assert!(
            f.engine
                .clone()
                .smart_preview_thumbnail(f.id.to_string(), size)
                .is_err()
        );
    }
    assert!(!f.original.parent().unwrap().exists());
    assert!(f.dir.path().exists());
}

#[test]
fn incompatible_prefix_is_rejected_without_rendering_or_original_fallback() {
    let f = Fixture::new();
    let (mut journal, snapshot) =
        SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    let mut doc: sidecar::RecipeDocument = serde_json::from_slice(&snapshot.recipe).unwrap();
    let previous = doc.recipe.settings.clone();
    doc.recipe.settings.demosaic.method = engine_api::recipe::settings::DemosaicMethod::Bilinear;
    doc.recipe
        .history
        .record(
            &previous,
            &doc.recipe.settings,
            engine_api::recipe::EditMeta::user("incompatible prefix", 1),
        )
        .unwrap();
    journal
        .save_recipe(serde_json::to_vec(&doc).unwrap())
        .unwrap();
    let job = f.job();
    let ctx = JobContext::new(JobId(0), CancellationToken::new(), None);
    assert!(job.render(&f.engine, &ctx).is_err());
    assert!(!f.original.parent().unwrap().exists());
}

#[test]
fn source_identity_mismatch_is_rejected_even_for_a_well_formed_container() {
    let f = Fixture::new();
    let (journal, snapshot) =
        SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    journal.discard_clean().unwrap();
    SmartPreviewJournal::create(
        f.engine.support_dir().unwrap(),
        f.id,
        [8; 32],
        snapshot.source_len,
        snapshot.recipe,
        None,
        None,
    )
    .unwrap();
    let job = f.job();
    let ctx = JobContext::new(JobId(0), CancellationToken::new(), None);
    assert!(
        job.render(&f.engine, &ctx)
            .unwrap_err()
            .to_string()
            .contains("source identity mismatch")
    );
    assert!(!f.original.parent().unwrap().exists());
}

#[test]
fn completed_job_drop_does_not_clobber_reentrant_replacement_and_events_are_terminal() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Replace {
        engine: Weak<Engine>,
        slot: Slot,
        count: AtomicUsize,
    }
    impl crate::EngineEventListener for Replace {
        fn on_event(&self, _: EngineEvent) {
            self.count.fetch_add(1, Ordering::SeqCst);
            let engine = self.engine.upgrade().unwrap();
            engine
                .smart_thumbnail_states
                .lock()
                .unwrap()
                .entries
                .get_mut(&self.slot)
                .unwrap()
                .state = State::Pending;
        }
    }
    let f = Fixture::new();
    let listener = Arc::new(Replace {
        engine: Arc::downgrade(&f.engine),
        slot: (f.id, 32),
        count: AtomicUsize::new(0),
    });
    f.engine.set_event_listener(Some(listener.clone()));
    let job = f.job();
    job.run(&JobContext::new(JobId(0), CancellationToken::new(), None))
        .unwrap();
    assert_eq!(listener.count.load(Ordering::SeqCst), 1);
    assert!(matches!(
        f.engine.smart_thumbnail_states.lock().unwrap().entries[&(f.id, 32)].state,
        State::Pending
    ));
    drop(f.job());
    assert_eq!(listener.count.load(Ordering::SeqCst), 2);
}

#[test]
fn pending_jobs_are_bounded_and_repeated_edit_requests_do_not_add_work() {
    let f = Fixture::new();
    let identity = f.engine.thumbnail_local(f.id).unwrap().identity;
    {
        let mut states = f.engine.smart_thumbnail_states.lock().unwrap();
        for size in 1..=MAX_PENDING as u32 {
            states.entries.insert(
                (f.id, size),
                Entry {
                    identity: identity.clone(),
                    state: State::Pending,
                },
            );
        }
    }
    assert!(
        f.engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 1)
            .unwrap()
            .pending
    );
    assert!(
        f.engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
            .unwrap_err()
            .to_string()
            .contains("queue is full")
    );
    assert_eq!(
        f.engine
            .smart_thumbnail_states
            .lock()
            .unwrap()
            .entries
            .len(),
        MAX_PENDING
    );
}

#[test]
fn active_proxy_lease_allows_thumbnail_reads_and_corrupt_journal_blocks_warm_delivery() {
    let f = Fixture::new();
    let gate = crate::image_edit_admission::gate_for(f.id).unwrap();
    let _lease = gate
        .reserve_develop(crate::image_edit_admission::EditSource::SmartPreview)
        .unwrap();
    assert!(!f.bytes().is_empty());
    let local = f.engine.thumbnail_local(f.id).unwrap();
    fs::write(
        local.path.parent().unwrap().join("journal.json"),
        b"broken journal",
    )
    .unwrap();
    assert!(
        f.engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
            .is_err()
    );
    assert!(!f.original.parent().unwrap().exists());
}

#[cfg(unix)]
#[test]
fn symlinked_asset_journal_and_store_directories_are_rejected_before_content_read() {
    use std::os::unix::fs::symlink;
    for name in [
        "pixels.tsp",
        "journal.json",
        "image-directory",
        "store-directory",
    ] {
        let f = Fixture::new();
        f.bytes(); // Even a warm validated entry cannot bypass new local path admission.
        let source = f.dir.path().join("untouched-original.arw");
        fs::write(&source, b"private source bytes must remain untouched").unwrap();
        let before = fs::read(&source).unwrap();
        let directory = f
            .engine
            .support_dir()
            .unwrap()
            .join("smart-previews")
            .join(f.id.to_string());
        match name {
            "pixels.tsp" | "journal.json" => {
                let path = directory.join(name);
                fs::remove_file(&path).unwrap();
                symlink(&source, path).unwrap();
            }
            _ => {
                let path = if name == "image-directory" {
                    directory.clone()
                } else {
                    directory.parent().unwrap().to_path_buf()
                };
                let renamed = f.dir.path().join("relocated-store");
                fs::rename(&path, &renamed).unwrap();
                symlink(renamed, path).unwrap();
            }
        }
        let error = f
            .engine
            .clone()
            .smart_preview_thumbnail(f.id.to_string(), 32)
            .unwrap_err();
        assert!(error.to_string().contains("symlink"), "{name}: {error}");
        assert_eq!(fs::read(&source).unwrap(), before);
        assert!(!f.original.parent().unwrap().exists());
    }
}

#[cfg(unix)]
#[test]
fn unsafe_cache_paths_fail_without_external_writes_and_recover_without_identity_change() {
    use std::os::unix::fs::symlink;
    for kind in ["directory", "temporary", "jpeg"] {
        let f = Fixture::new();
        f.bytes();
        let key = {
            let states = f.engine.smart_thumbnail_states.lock().unwrap();
            match &states.entries[&(f.id, 32)].state {
                State::Ready(key) => key.clone(),
                _ => panic!("not ready"),
            }
        };
        let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
        let directory = f
            .engine
            .support_dir()
            .unwrap()
            .join("previews")
            .join(format!(
                "{}-{}-{}",
                hex(&key.file_hash),
                key.orientation,
                hex(&key.recipe_hash)
            ));
        let source_dir = f.dir.path().join("external-originals");
        fs::create_dir(&source_dir).unwrap();
        let source = source_dir.join("1.jpg");
        fs::write(&source, b"original target must never be overwritten").unwrap();
        let baseline = fs::read(&source).unwrap();
        let moved = f.dir.path().join("saved-cache");
        let bad_path = match kind {
            "directory" => {
                fs::rename(&directory, &moved).unwrap();
                symlink(&source_dir, &directory).unwrap();
                directory.clone()
            }
            "temporary" => {
                fs::remove_file(directory.join("1.jpg")).unwrap();
                let p = directory.join("8.tmp");
                symlink(&source, &p).unwrap();
                p
            }
            _ => {
                let p = directory.join("1.jpg");
                fs::remove_file(&p).unwrap();
                symlink(&source, &p).unwrap();
                p
            }
        };
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            match f
                .engine
                .clone()
                .smart_preview_thumbnail(f.id.to_string(), 32)
            {
                Err(error) => {
                    assert!(
                        error
                            .to_string()
                            .contains("unsafe local preview cache path"),
                        "{error}"
                    );
                    break;
                }
                Ok(response) => {
                    assert!(response.bytes.is_none());
                    assert!(Instant::now() < deadline);
                }
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(fs::read(&source).unwrap(), baseline);
        assert_eq!(fs::read_dir(&source_dir).unwrap().count(), 1);
        fs::remove_file(bad_path).unwrap();
        if kind == "directory" {
            fs::rename(&moved, &directory).unwrap();
        }
        assert!(
            !f.bytes().is_empty(),
            "An observed transient cache-write failure must be retryable"
        );
        assert_eq!(fs::read(&source).unwrap(), baseline);
    }
}

#[test]
fn hdr_saved_offline_recipe_keeps_policy_and_renders_sdr_thumbnail_without_mutation() {
    let f = Fixture::new();
    let expected = f.bytes();
    let session = f
        .engine
        .clone()
        .open_smart_preview_develop_session(f.id.to_string())
        .unwrap();
    session
        .set_settings(
            r#"{"output":{"hdr":true,"hdr_headroom_stops":2.0}}"#.into(),
            false,
        )
        .unwrap();
    session.flush().unwrap();
    session.close().unwrap();
    let (_, saved) = SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    let doc: sidecar::RecipeDocument = serde_json::from_slice(&saved.recipe).unwrap();
    assert!(doc.recipe.settings.output.hdr);
    assert_eq!(doc.recipe.settings.output.hdr_headroom_stops, 2.);
    assert!(saved.dirty);
    assert_eq!(
        f.bytes(),
        expected,
        "SDR thumbnail ignores only HDR presentation policy"
    );
    let (_, after) = SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    assert_eq!(
        saved.recipe, after.recipe,
        "thumbnail must not rewrite settings/history"
    );
    let reopened = f
        .engine
        .clone()
        .open_smart_preview_develop_session(f.id.to_string())
        .unwrap();
    let live: serde_json::Value =
        serde_json::from_str(&reopened.get_settings_json().unwrap()).unwrap();
    assert_eq!(live["output"]["hdr"], true);
    assert_eq!(live["output"]["hdr_headroom_stops"], 2.);
    reopened.close().unwrap();
    assert!(!f.original.parent().unwrap().exists());
}

#[test]
fn hdr_thumbnail_still_rejects_unsupported_proofing_without_journal_mutation() {
    let f = Fixture::new();
    let (mut journal, snapshot) =
        SmartPreviewJournal::open(f.engine.support_dir().unwrap(), f.id).unwrap();
    let mut doc: sidecar::RecipeDocument = serde_json::from_slice(&snapshot.recipe).unwrap();
    let previous = doc.recipe.settings.clone();
    doc.recipe.settings.output.hdr = true;
    doc.recipe.settings.output.hdr_headroom_stops = 2.;
    doc.recipe.settings.output.proof_profile = Some(engine_api::color::IccProfileHandle(
        engine_api::id::Digest([9; 32]),
    ));
    doc.recipe
        .history
        .record(
            &previous,
            &doc.recipe.settings,
            engine_api::recipe::EditMeta::user("unsupported proofing", 1),
        )
        .unwrap();
    let saved = serde_json::to_vec(&doc).unwrap();
    journal.save_recipe(saved.clone()).unwrap();
    let job = f.job();
    let ctx = JobContext::new(JobId(0), CancellationToken::new(), None);
    assert!(job.render(&f.engine, &ctx).is_err());
    assert_eq!(journal.snapshot().unwrap().recipe, saved);
    assert!(!f.original.parent().unwrap().exists());
}
