//! Lightroom import over the bridge (M2-13b) on the synthetic fixture catalog:
//! inspect → plan (relocation, selection/mark mapping, keywords) → fidelity →
//! apply (sidecars, library.json, index) → resume, cancel, and safety.
use import_lrcat::fixture;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};
use tessera_ffi::*;

fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.clone(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

struct Setup {
    _temp: tempfile::TempDir,
    fixture: fixture::Fixture,
    engine: Arc<Engine>,
    import: Arc<LrcatImport>,
}

fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture::write(&temp.path().join("fx")).unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    Setup {
        _temp: temp,
        fixture,
        engine,
        import,
    }
}

fn relocated(s: &Setup) -> LrcatOptions {
    let mut options = s.import.default_options();
    let photos = s.fixture.photos.canonicalize().unwrap();
    options.relocations[0].to = photos.to_string_lossy().into_owned();
    options.library_folder = photos.to_string_lossy().into_owned();
    options
}

#[derive(Default)]
struct Recorder {
    events: Mutex<Vec<LrcatProgress>>,
    cancel_on_first_write: Mutex<Option<Weak<LrcatImport>>>,
}
impl LrcatProgressListener for Recorder {
    fn on_progress(&self, progress: LrcatProgress) {
        if progress.phase == LrcatPhase::WritingEdits
            && let Some(import) = self.cancel_on_first_write.lock().unwrap().take()
            && let Some(import) = import.upgrade()
        {
            import.cancel();
        }
        self.events.lock().unwrap().push(progress);
    }
}

#[test]
fn inspect_counts_and_unsupported_reasons() {
    let s = setup();
    let summary = inspect_lrcat(s.fixture.catalog.to_string_lossy().into_owned()).unwrap();
    assert_eq!(summary, s.import.summary());
    assert_eq!(summary.schema_version, "1300025");
    assert_eq!(summary.roots, vec![fixture::MOVED_ROOT.to_string()]);
    assert_eq!(
        (summary.images, summary.virtual_copies, summary.edited),
        (7, 1, 4)
    );
    assert_eq!((summary.folders, summary.keywords), (3, 7));
    assert_eq!(
        (
            summary.collections,
            summary.collection_sets,
            summary.smart_collections
        ),
        (2, 1, 2)
    );
    assert_eq!((summary.stacks, summary.faces, summary.previews), (1, 1, 6));
    assert!(summary.estimated_bytes > 1000);
    assert!(!summary.catalog_locked);
    let reasons: Vec<String> = summary
        .unsupported
        .iter()
        .map(|i| format!("{}: {}", i.category, i.reason))
        .collect();
    let has = |needle: &str| reasons.iter().any(|r| r.contains(needle));
    assert!(has("FutureKnob"), "{reasons:#?}");
    assert!(has("Virtual copies: Tessera keeps one edit per file"));
    assert!(has("Smart collections: rule is kept but cannot run"));
    assert!(has("“Paris” appears 2 times"));
    assert!(has("Stacks") && has("Faces") && has("History"));
    let optional = summary
        .unsupported
        .iter()
        .find(|i| i.reason.starts_with("optional tables"))
        .unwrap();
    assert_eq!(
        optional.examples,
        ["AgLibraryCollectionStack", "AgLibraryCollectionStackImage"]
    );
    let copies = summary
        .unsupported
        .iter()
        .find(|i| i.category == "Virtual copies")
        .unwrap();
    assert_eq!(copies.examples, vec!["ceremony-01.jpg (Black & White)"]);
    // A lock file beside the catalog is reported, not touched.
    let lock = s.fixture.catalog.with_extension("lrcat.lock");
    std::fs::write(&lock, b"lr").unwrap();
    assert!(
        inspect_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
            .unwrap()
            .catalog_locked
    );
    assert_eq!(std::fs::read(&lock).unwrap(), b"lr");
}

#[test]
fn plan_relocates_maps_selection_and_marks_without_writing() {
    let s = setup();
    let before = snapshot(s.fixture.photos.parent().unwrap());
    let defaults = s.import.default_options();
    assert_eq!(defaults.library_folder, "/Volumes/Old Drive/Photos");
    assert_eq!(
        defaults
            .marks
            .iter()
            .map(|m| m.label.as_str())
            .collect::<Vec<_>>(),
        ["Blue", "Client", "Red"]
    );
    let moved = s.import.plan(defaults).unwrap();
    assert!(!moved.roots[0].exists);
    assert_eq!((moved.roots[0].images, moved.roots[0].missing), (6, 6));
    assert_eq!((moved.to_import, moved.missing), (0, 6));

    let mut options = relocated(&s);
    options.marks = vec![
        LrcatMarkMapping {
            label: "Red".into(),
            mark: "Needs Retouch".into(),
        },
        LrcatMarkMapping {
            label: "Client".into(),
            mark: "".into(),
        },
    ];
    let plan = s.import.plan(options.clone()).unwrap();
    assert!(plan.roots[0].exists);
    assert_eq!(
        (
            plan.to_import,
            plan.missing,
            plan.virtual_copies,
            plan.conflicts
        ),
        (5, 1, 1, 0)
    );
    assert_eq!(plan.skipped.len(), 1);
    assert!(plan.skipped[0].path.ends_with("lost-01.jpg"));
    assert_eq!(plan.outside_library, 0);
    assert!(!plan.library_exists);
    let folders: Vec<(String, u32, u32, u32, bool)> = plan
        .folders
        .iter()
        .map(|f| {
            (
                f.catalog_path
                    .trim_start_matches(fixture::MOVED_ROOT)
                    .to_string(),
                f.images,
                f.virtual_copies,
                f.missing,
                f.exists,
            )
        })
        .collect();
    assert_eq!(
        folders,
        [
            ("2026/".into(), 0, 0, 0, true),
            ("2026/portraits/".into(), 3, 0, 1, true),
            ("2026/wedding/".into(), 3, 1, 0, true),
        ]
    );
    let rows: Vec<(String, Decision, Option<u8>, u32)> = plan
        .selection_rows
        .iter()
        .map(|r| (r.lightroom.clone(), r.decision, r.grade, r.count))
        .collect();
    assert_eq!(
        rows,
        [
            ("Rejected".into(), Decision::Reject, None, 1),
            ("Picked, 5 stars".into(), Decision::Keep, Some(3), 1),
            ("Unflagged, 4 stars".into(), Decision::Keep, Some(2), 1),
            ("Unflagged, 3 stars".into(), Decision::Keep, Some(2), 1),
            ("Unflagged, 2 stars".into(), Decision::Keep, Some(1), 1),
            ("Unflagged, no stars".into(), Decision::Undecided, None, 1),
        ]
    );
    assert_eq!(
        plan.selection,
        LrcatSelectionCounts {
            rejects: 1,
            keeps: 4,
            undecided: 1,
            grade1: 1,
            grade2: 2,
            grade3: 1,
            marked: 2
        }
    );
    let marks: Vec<(String, String, u32)> = plan
        .marks
        .iter()
        .map(|m| (m.label.clone(), m.mark.clone(), m.count))
        .collect();
    assert_eq!(
        marks,
        [
            ("Client".into(), "".into(), 1),
            ("Red".into(), "Needs Retouch".into(), 2)
        ]
    );
    let keywords: Vec<(String, u32, u32, bool)> = plan
        .keywords
        .iter()
        .map(|k| (k.name.clone(), k.depth, k.images, k.merged))
        .collect();
    assert_eq!(
        keywords,
        [
            ("Places".into(), 0, 0, false),
            ("NYC".into(), 1, 2, false),
            ("Paris".into(), 1, 0, false),
            ("People".into(), 0, 0, false),
            ("Alice".into(), 1, 1, false),
            ("Trips".into(), 0, 0, false),
            ("Paris".into(), 1, 1, true),
        ]
    );
    assert_eq!(plan.keywords[1].synonyms, ["New York"]);
    // A library folder that excludes a subfolder is flagged.
    options.library_folder = s
        .fixture
        .photos
        .join("2026/wedding")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(s.import.plan(options).unwrap().outside_library, 3);
    assert_eq!(
        snapshot(s.fixture.photos.parent().unwrap()),
        before,
        "plan wrote nothing"
    );
}

#[test]
fn fidelity_sample_compares_with_lightroom_previews() {
    let s = setup();
    let fidelity = s.import.fidelity_sample(relocated(&s), 3, 128).unwrap();
    assert_eq!(fidelity.renderer, "recipe-selected (native/adobe-compat)");
    assert!(fidelity.previews_available);
    assert_eq!(fidelity.samples.len(), 3);
    for sample in &fidelity.samples {
        assert_eq!(sample.status, LrcatFidelityStatus::Compared, "{sample:?}");
        assert!(sample.delta_e_mean.is_finite() && sample.delta_e_mean > 0.0);
        assert!(sample.delta_e_p95 >= sample.delta_e_mean);
        for jpeg in [&sample.lightroom_jpeg, &sample.tessera_jpeg] {
            assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
            let (w, h) = import_lrcat::previews::jpeg_dimensions(jpeg).unwrap();
            assert_eq!(w.max(h), 128);
        }
    }
    // Edited photos first.
    assert!(fidelity.samples.iter().any(|s| s.name == "ceremony-01.jpg"));
    // Without relocation nothing can be rendered.
    let moved = s
        .import
        .fidelity_sample(s.import.default_options(), 3, 128)
        .unwrap();
    assert!(moved.samples.is_empty());
}

#[test]
fn apply_writes_sidecars_library_and_index_and_resumes() {
    let s = setup();
    let catalog_dir = s.fixture.catalog.parent().unwrap().to_path_buf();
    let catalog_before = snapshot(&catalog_dir);
    let mut options = relocated(&s);
    options.marks = vec![LrcatMarkMapping {
        label: "Red".into(),
        mark: "Needs Retouch".into(),
    }];
    let recorder = Arc::new(Recorder::default());
    let report = s
        .import
        .apply(options.clone(), Some(recorder.clone()))
        .unwrap();
    assert!(!report.cancelled);
    assert_eq!(
        (report.imported, report.resumed, report.virtual_copies),
        (5, 0, 1)
    );
    assert_eq!(
        (
            report.albums,
            report.album_groups,
            report.smart_albums,
            report.keywords
        ),
        (2, 1, 2, 6)
    );
    assert_eq!(report.skipped.len(), 1);
    assert!(report.skipped[0].reason.contains("original not found"));
    assert_eq!(report.selection.keeps, 3);
    assert_eq!(report.indexed, 5);
    let phases: Vec<LrcatPhase> = recorder
        .events
        .lock()
        .unwrap()
        .iter()
        .map(|e| e.phase)
        .collect();
    for phase in [
        LrcatPhase::Preparing,
        LrcatPhase::WritingEdits,
        LrcatPhase::Library,
        LrcatPhase::Indexing,
        LrcatPhase::Finished,
    ] {
        assert!(phases.contains(&phase), "{phases:?}");
    }

    // Sidecars beside the originals.
    let photos = s.fixture.photos.canonicalize().unwrap();
    let one = photos.join("2026/wedding/ceremony-01.jpg");
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&one).recipe).unwrap();
    assert_eq!(doc.last_writer.machine_id, "lightroom-import");
    assert_eq!(doc.recipe.settings.tone.exposure, 0.5);
    assert_eq!(
        doc.recipe.selection.mark.as_ref().unwrap().0,
        "Needs Retouch"
    );
    let xmp = std::fs::read_to_string(sidecar::Sidecar::paths(&one).xmp).unwrap();
    assert!(xmp.contains("NYC") && xmp.contains("Places|NYC"), "{xmp}");

    // Index: the engine lists the five photos with their decisions.
    let rows = s.engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(rows.len(), 5);
    let by_name = |n: &str| rows.iter().find(|r| r.path.ends_with(n)).unwrap();
    assert_eq!(by_name("ceremony-01.jpg").selection.grade, Some(3));
    assert_eq!(
        by_name("ceremony-02.jpg").selection.decision,
        Decision::Reject
    );
    assert_eq!(
        doc.recipe.image_id.unwrap().to_string(),
        by_name("ceremony-01.jpg").id
    );

    // library.json: albums keyed to the app's identities (the virtual copy
    // folds into its master), groups, smart albums, keywords, roots.
    let store = s
        .engine
        .clone()
        .open_library(report.library_path.clone())
        .unwrap();
    let nodes = store.nodes().unwrap();
    let selects = nodes.iter().find(|n| n.name == "Selects").unwrap();
    assert_eq!(selects.image_count, 2);
    assert_eq!(
        selects.parent,
        nodes.iter().find(|n| n.name == "Wedding").map(|n| n.id)
    );
    assert_eq!(
        nodes
            .iter()
            .find(|n| n.name == "Client review")
            .unwrap()
            .image_count,
        2
    );
    let library: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&report.library_path).unwrap()).unwrap();
    assert_eq!(library["roots"][0], photos.to_string_lossy().as_ref());
    assert_eq!(library["people"][0], "Alice");
    assert!(
        Path::new(&report.bundle_path)
            .join("import-plan.json")
            .is_file()
    );

    // Safe by construction: nothing in the catalog folder changed.
    assert_eq!(snapshot(&catalog_dir), catalog_before);

    // Re-running resumes: nothing rewritten, library merged once.
    let again = s.import.apply(options, None).unwrap();
    assert_eq!((again.imported, again.resumed, again.albums), (0, 5, 0));
    let nodes = store.nodes().unwrap();
    assert_eq!(
        nodes
            .iter()
            .filter(|n| n.name.starts_with("Selects"))
            .count(),
        1
    );
}

#[test]
fn cancel_then_resume_and_existing_edits_are_kept() {
    let s = setup();
    let options = relocated(&s);
    let photos = s.fixture.photos.canonicalize().unwrap();
    // Tessera edits made before the import win unless overwrite is chosen.
    let kept = photos.join("2026/portraits/portrait-02.jpg");
    s.engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let id = s
        .engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .find(|r| r.path.ends_with("portrait-02.jpg"))
        .unwrap()
        .id;
    s.engine
        .set_selection(
            id,
            Selection {
                decision: Decision::Keep,
                grade: Some(1),
                mark: None,
            },
        )
        .unwrap();
    let plan = s.import.plan(options.clone()).unwrap();
    assert_eq!((plan.to_import, plan.conflicts), (4, 1));

    let recorder = Arc::new(Recorder::default());
    *recorder.cancel_on_first_write.lock().unwrap() = Some(Arc::downgrade(&s.import));
    let first = s.import.apply(options.clone(), Some(recorder)).unwrap();
    assert!(first.cancelled);
    assert_eq!((first.imported, first.albums, first.keywords), (1, 0, 0));
    assert!(
        !Path::new(&first.library_path).exists(),
        "library is merged only on completion"
    );

    let second = s.import.apply(options.clone(), None).unwrap();
    assert!(!second.cancelled);
    assert_eq!((second.imported, second.resumed), (3, 1));
    assert!(
        second
            .skipped
            .iter()
            .any(|k| k.reason.contains("already has Tessera edits"))
    );
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&kept).recipe).unwrap();
    assert_eq!(doc.last_writer.machine_id, "tessera-mac");

    let mut overwrite = options;
    overwrite.overwrite_existing_edits = true;
    let third = s.import.apply(overwrite, None).unwrap();
    assert_eq!(
        third.imported, 5,
        "new options rewrite the importer's own sidecars too"
    );
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&kept).recipe).unwrap();
    assert_eq!(doc.last_writer.machine_id, "lightroom-import");
}
