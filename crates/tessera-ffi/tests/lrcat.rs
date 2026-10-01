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
                out.insert(p.clone(), Vec::new());
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

#[test]
fn catalog_import_indexes_only_references_but_open_folder_indexes_everything() {
    let s = setup();
    let photos = s.fixture.photos.canonicalize().unwrap();
    let source = photos.join("2026/wedding/ceremony-01.jpg");
    // Include siblings and descendants of a catalog folder, plus another root child.
    for relative in [
        "2026/wedding/unrelated.jpg",
        "2026/wedding/extra/nested.jpg",
        "other/unrelated.jpg",
    ] {
        let path = photos.join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::copy(&source, path).unwrap();
    }
    let report = s.import.apply(relocated(&s), None).unwrap();
    let imported = s.engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(
        imported.len(),
        5,
        "unrelated originals must not enter the catalog index"
    );
    assert_eq!(report.indexed, 5);
    assert!(
        imported
            .iter()
            .all(|image| !image.path.contains("unrelated") && !image.path.contains("nested"))
    );
    assert_eq!(s.import.apply(relocated(&s), None).unwrap().indexed, 0);

    // Exercise the exact FFI entry point used by normal Open Folder.
    s.engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let all = s.engine.list_images(ImageQuery::default()).unwrap();
    assert_eq!(all.len(), 8);
}

/// Run only against an explicitly supplied catalog COPY. Stage referenced originals
/// under disposable mapped roots so apply cannot write beside personal originals.
#[test]
#[ignore = "requires TESSERA_LRCAT_COPY and eight accessible originals"]
fn catalog_copy_indexes_eight_accessible_references() {
    let catalog = PathBuf::from(std::env::var("TESSERA_LRCAT_COPY").unwrap());
    assert!(catalog.canonicalize().unwrap().starts_with("/private/tmp"));
    let before = std::fs::read(&catalog).unwrap();
    let source =
        rusqlite::Connection::open_with_flags(&catalog, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let catalog_photos: i64 = source
        .query_row("SELECT COUNT(*) FROM Adobe_images", [], |r| r.get(0))
        .unwrap();
    let mut statement = source
        .prepare(
            "SELECT r.absolutePath, d.pathFromRoot, f.baseName, f.extension
        FROM Adobe_images i JOIN AgLibraryFile f ON i.rootFile=f.id_local
        JOIN AgLibraryFolder d ON f.folder=d.id_local
        JOIN AgLibraryRootFolder r ON d.rootFolder=r.id_local
        WHERE COALESCE(i.masterImage,0)=0",
        )
        .unwrap();
    let paths = statement
        .query_map([], |r| {
            let root: String = r.get(0)?;
            let folder: String = r.get(1)?;
            let base: String = r.get(2)?;
            let ext: Option<String> = r.get(3)?;
            let ext = ext.unwrap_or_default();
            let name = if ext.is_empty() {
                base
            } else {
                format!("{base}.{ext}")
            };
            Ok(PathBuf::from(root).join(folder).join(name))
        })
        .unwrap();
    let accessible: std::collections::BTreeSet<PathBuf> =
        paths.map(|p| p.unwrap()).filter(|p| p.is_file()).collect();
    assert_eq!(accessible.len(), 8, "accessible catalog originals changed");
    println!(
        "catalog_photos={catalog_photos} accessible={}",
        accessible.len()
    );
    let temp = tempfile::tempdir().unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options();
    // Longest root wins, matching importer relocation semantics.
    options
        .relocations
        .sort_by_key(|r| std::cmp::Reverse(r.from.len()));
    let original_roots = options.relocations.clone();
    for (n, relocation) in options.relocations.iter_mut().enumerate() {
        relocation.to = temp
            .path()
            .join(format!("root-{n}"))
            .to_string_lossy()
            .into_owned();
        std::fs::create_dir_all(&relocation.to).unwrap();
    }
    for path in &accessible {
        let n = original_roots
            .iter()
            .position(|r| path.starts_with(&r.from))
            .unwrap();
        let target = Path::new(&options.relocations[n].to)
            .join(path.strip_prefix(&original_roots[n].from).unwrap());
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::copy(path, target).unwrap();
    }
    options.library_folder = temp.path().join("library").to_string_lossy().into_owned();
    let preview = import.plan(options.clone()).unwrap();
    assert_eq!(preview.to_import, 8);
    let report = import.apply(options, None).unwrap();
    assert_eq!(report.indexed, 8);
    assert_eq!(engine.list_images(ImageQuery::default()).unwrap().len(), 8);
    assert_eq!(std::fs::read(catalog).unwrap(), before);
    println!(
        "catalog_photos={} accessible={} imported={} indexed={} missing={} seconds={:.3}",
        catalog_photos,
        accessible.len(),
        report.imported,
        report.indexed,
        preview.missing,
        report.seconds
    );
}

/// B5-29c: the plan report groups per-image develop warnings ("N images
/// (first: image ID): reason"); the summary still counts every image and
/// names the first one as an example.
#[test]
fn grouped_develop_report_entries_keep_their_image_counts() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture::write(&temp.path().join("fx")).unwrap();
    let c = rusqlite::Connection::open(&fixture.catalog).unwrap();
    let changed = c
        .execute(
            "UPDATE Adobe_imageDevelopSettings SET text='s = { Exposure2012 = 1, GroupedFutureKey = 2 }', processVersion='15.4' WHERE image IN (SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT 3)",
            [],
        )
        .unwrap();
    assert_eq!(changed, 3);
    drop(c);
    let summary = inspect_lrcat(fixture.catalog.to_string_lossy().into_owned()).unwrap();
    let issue = summary
        .unsupported
        .iter()
        .find(|i| i.reason.contains("GroupedFutureKey"))
        .unwrap_or_else(|| panic!("{:#?}", summary.unsupported));
    assert_eq!(issue.category, "Develop settings");
    assert_eq!(issue.count, 3);
    assert_eq!(issue.examples.len(), 1, "{issue:?}");
    assert!(!issue.examples[0].starts_with("image "), "{issue:?}");
}

#[test]
fn unedited_summary_lists_each_image() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture::write(&temp.path().join("fx")).unwrap();
    let c = rusqlite::Connection::open(&fixture.catalog).unwrap();
    c.execute("UPDATE Adobe_imageDevelopSettings SET text='garbage'", [])
        .unwrap();
    let summary = inspect_lrcat(fixture.catalog.to_string_lossy().into_owned()).unwrap();
    let issues: Vec<_> = summary
        .unsupported
        .iter()
        .filter(|i| i.reason.contains("imported as unedited"))
        .collect();
    assert_eq!(issues.len(), summary.images as usize);
    assert!(
        issues
            .iter()
            .all(|i| i.count == 1 && i.reason.contains("image "))
    );
}

#[test]
fn lightroom_owned_photos_import_without_adjacent_files() {
    for folder in ["X.lrdata", "Foo.lrcat-data"] {
        let s = setup();
        let protected = s._temp.path().join(folder);
        std::fs::rename(&s.fixture.photos, &protected).unwrap();
        let before = snapshot(&protected);
        let mut options = s.import.default_options();
        options.relocations[0].to = protected.to_string_lossy().into_owned();
        options.library_folder = s
            ._temp
            .path()
            .join("library")
            .to_string_lossy()
            .into_owned();
        let report = s.import.apply(options.clone(), None).unwrap();
        assert_eq!(snapshot(&protected), before, "import modified {folder}");
        assert_eq!(report.imported, 5);
        let issue = report
            .unsupported
            .iter()
            .find(|i| i.category == "Read-only originals")
            .unwrap();
        assert!(!issue.examples.is_empty());
        assert!(
            report
                .unsupported
                .iter()
                .any(|issue| issue.reason.contains("Lightroom")
                    && issue.reason.contains("sidecar")
                    && issue.count == 5)
        );
        let rows = s.engine.list_images(ImageQuery::default()).unwrap();
        let row = rows
            .iter()
            .find(|r| r.path.ends_with("ceremony-01.jpg"))
            .unwrap();
        let recipe: engine_api::recipe::Recipe =
            serde_json::from_str(&s.engine.get_recipe(row.id.clone()).unwrap()).unwrap();
        assert_eq!(recipe.settings.tone.exposure, 0.5);
        assert_eq!(
            recipe.selection.grade,
            Some(engine_api::recipe::Grade::Three)
        );
        let resumed = s.import.apply(options, None).unwrap();
        assert_eq!(resumed.resumed, 5);
        assert!(
            !resumed
                .unsupported
                .iter()
                .any(|i| i.category == "Read-only originals")
        );
        assert_eq!(snapshot(&protected), before);
        let adjacent = std::path::Path::new(&row.path).with_extension("xmp");
        std::fs::write(&adjacent, b"Lightroom owns this packet").unwrap();
        let before = snapshot(&protected);
        assert!(
            sidecar::Sidecar::paths(&row.path)
                .recipe
                .starts_with(s._temp.path().join("support").canonicalize().unwrap())
        );
        let mut edited = recipe;
        edited
            .edit(
                engine_api::recipe::EditMeta::user("Exposure", 1),
                |settings| {
                    settings.tone.exposure = 1.25;
                },
            )
            .unwrap();
        s.engine
            .set_recipe_json(row.id.clone(), serde_json::to_string(&edited).unwrap())
            .unwrap();
        let refreshed = s.engine.list_images(ImageQuery::default()).unwrap();
        assert_eq!(
            refreshed
                .iter()
                .find(|r| r.id == row.id)
                .unwrap()
                .recipe_hash,
            edited.recipe_hash().to_string()
        );
        assert_eq!(snapshot(&protected), before);
    }
}

#[test]
fn lightroom_owned_library_destination_is_rejected_before_creation() {
    let s = setup();
    let mut options = relocated(&s);
    let protected = s._temp.path().join("X.lrdata/new-library");
    options.library_folder = protected.to_string_lossy().into_owned();
    assert!(
        s.import
            .apply(options, None)
            .unwrap_err()
            .to_string()
            .contains("library folder")
    );
    assert!(!s._temp.path().join("X.lrdata").exists());
}

#[test]
fn protected_default_library_import_succeeds_without_custom_folder() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture::write(&temp.path().join("Fixture.lrdata")).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    db.execute(
        "UPDATE AgLibraryRootFolder SET absolutePath=?",
        [format!("{}/", fixture.photos.display())],
    )
    .unwrap();
    drop(db);
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into()).unwrap();
    let import = engine
        .open_lrcat(fixture.catalog.to_string_lossy().into())
        .unwrap();
    let options = import.default_options();
    assert_eq!(
        std::path::Path::new(&options.library_folder),
        temp.path().join("support/Imported Libraries")
    );
    assert!(!sidecar::Sidecar::is_lightroom_owned(
        &options.library_folder
    ));
    let report = import.apply(options, None).unwrap();
    assert_eq!(report.imported, 5);
}

#[test]
fn protected_edit_resolves_after_remount_and_engine_reopen() {
    let s = setup();
    let parent = s._temp.path().join("Photos");
    std::fs::create_dir(&parent).unwrap();
    let protected = parent.join("X.lrdata");
    std::fs::rename(&s.fixture.photos, &protected).unwrap();
    let mut options = s.import.default_options();
    options.relocations[0].to = protected.to_string_lossy().into();
    options.library_folder = s._temp.path().join("library").to_string_lossy().into();
    assert_eq!(s.import.apply(options, None).unwrap().imported, 5);
    let renamed = s._temp.path().join("Photos 1");
    std::fs::rename(parent, &renamed).unwrap();
    let renamed = renamed.canonicalize().unwrap();
    let engine = Engine::open(s._temp.path().join("support").to_string_lossy().into()).unwrap();
    engine
        .index_folder(renamed.to_string_lossy().into())
        .unwrap();
    let row = engine
        .list_images(ImageQuery::default())
        .unwrap()
        .into_iter()
        .find(|i| {
            i.path.starts_with(renamed.to_str().unwrap()) && i.path.ends_with("ceremony-01.jpg")
        })
        .unwrap();
    let recipe: engine_api::recipe::Recipe =
        serde_json::from_str(&engine.get_recipe(row.id).unwrap()).unwrap();
    assert_eq!(recipe.settings.tone.exposure, 0.5);
}

#[test]
fn read_only_report_counts_successful_writes_not_failed_candidates() {
    let s = setup();
    let protected = s._temp.path().join("X.lrdata");
    std::fs::rename(&s.fixture.photos, &protected).unwrap();
    let mut options = s.import.default_options();
    options.relocations[0].to = protected.to_string_lossy().into();
    options.library_folder = s._temp.path().join("library").to_string_lossy().into();
    let failed = snapshot(&protected)
        .keys()
        .find(|p| p.ends_with("ceremony-01.jpg"))
        .cloned()
        .unwrap();
    std::fs::write(failed.with_extension("xmp"), b"not valid XMP").unwrap();
    let report = s.import.apply(options, None).unwrap();
    assert_eq!(report.imported, 4);
    let issue = report
        .unsupported
        .iter()
        .find(|i| i.category == "Read-only originals")
        .unwrap();
    assert_eq!(issue.count, report.imported);
    assert!(!issue.examples.is_empty());
    assert!(
        !issue
            .examples
            .iter()
            .any(|p| p.ends_with("ceremony-01.jpg"))
    );
}

#[test]
fn streaming_resume_rejects_protected_publication_symlinks() {
    for child in ["bundle", "large", "import-plan.json"] {
        let s = setup();
        let options = relocated(&s);
        s.import.apply(options.clone(), None).unwrap();
        let import_root = Path::new(&options.library_folder).join(".tessera-import");
        let bundle = std::fs::read_dir(&import_root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let protected = s._temp.path().join("Retained.lrdata");
        std::fs::create_dir(&protected).unwrap();
        let destination = if child == "bundle" {
            bundle.clone()
        } else {
            bundle.join(child)
        };
        if destination.is_dir() {
            std::fs::rename(&destination, s._temp.path().join("saved-bundle")).unwrap();
        } else if destination.exists() {
            std::fs::remove_file(&destination).unwrap();
        }
        let target = if child == "import-plan.json" {
            let target = protected.join("plan.json");
            std::fs::write(&target, b"untouched").unwrap();
            target
        } else {
            protected.clone()
        };
        std::os::unix::fs::symlink(target, destination).unwrap();
        let before = snapshot(&protected);
        let error = s.import.apply(options, None).unwrap_err();
        assert!(error.to_string().contains("import bundle"), "{error}");
        assert_eq!(snapshot(&protected), before);
    }
}
