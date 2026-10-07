//! SP-INT3 (REV2-SP NB1): re-keying in-place Smart Preview recipes never loses
//! Tessera edits made before the key changed. Synthetic catalog and proxies.
use super::*;

const EDITED: f32 = 1.25;
const FOREIGN: &str = "tessera-app-machine";

struct Setup {
    _temp: tempfile::TempDir,
    support: PathBuf,
    engine: Arc<Engine>,
    fixture: import_lrcat::fixture::Fixture,
    options: LrcatOptions,
    proxies: Vec<PathBuf>,
}

/// An in-place import of two offline photos whose Smart Previews are
/// byte-identical, as the INT/SP-INT builds left it: content-keyed protected
/// recipes, here carrying a Tessera edit from the app (another machine id).
fn legacy(two_photos: bool) -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
    import_lrcat::fixture::write_smart_previews(&fixture).unwrap();
    if two_photos {
        std::fs::remove_file(fixture.photos.join("2026/wedding/ceremony-02.jpg")).unwrap();
    }
    let support = temp.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    let photos = fixture.photos.canonicalize().unwrap();
    options.relocations[0].to = photos.to_string_lossy().into_owned();
    options.library_folder = photos.to_string_lossy().into_owned();
    options.import_smart_previews = true;
    options.copy_proxies = false;
    let proxies: Vec<PathBuf> = resolve(&import.plan, &options)
        .unwrap()
        .into_iter()
        .filter(|r| r.outcome == Outcome::OfflineProxy)
        .map(|r| {
            Sidecar::register_store(&Sidecar::resolved_destination(&r.folder), &support);
            r.path
        })
        .collect();
    assert_eq!(proxies.len(), if two_photos { 2 } else { 1 });
    assert!(proxies.iter().all(Sidecar::is_lightroom_owned));
    for path in &proxies {
        let mut doc = RecipeDocument::default();
        doc.recipe.image_id = app_image_id(path);
        doc.recipe
            .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
                s.tone.exposure = EDITED
            })
            .unwrap();
        doc.record_write(FOREIGN, 1).unwrap();
        Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc).unwrap();
    }
    if two_photos {
        assert_eq!(
            Sidecar::paths(&proxies[0]).recipe,
            Sidecar::paths(&proxies[1]).recipe,
            "legacy content keying shared one recipe"
        );
    }
    Setup {
        _temp: temp,
        support,
        engine,
        fixture,
        options,
        proxies,
    }
}

fn exposure(path: &Path) -> f32 {
    Sidecar::read_recipe(Sidecar::paths(path).recipe)
        .unwrap()
        .recipe
        .settings
        .tone
        .exposure
}

fn kept(skipped: &[LrcatSkip], path: &Path) -> bool {
    skipped
        .iter()
        .any(|s| Path::new(&s.path) == path && s.reason.contains("kept them"))
}

#[test]
fn sp_int3_reimport_without_overwrite_keeps_legacy_tessera_edits() {
    let s = legacy(false);
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let plan = import.plan(s.options.clone()).unwrap();
    assert!(kept(&plan.skipped, &s.proxies[0]), "{:?}", plan.skipped);
    let report = import.apply(s.options.clone(), None).unwrap();
    assert!(
        kept(&report.skipped, &s.proxies[0]),
        "plan and apply must agree: {:?}",
        report.skipped
    );
    assert_eq!(exposure(&s.proxies[0]), EDITED, "Tessera edits survive");
}

#[test]
fn sp_int3_reimport_with_overwrite_replaces_the_legacy_recipe_without_orphaning_it() {
    let s = legacy(false);
    let old = Sidecar::paths(&s.proxies[0]).recipe;
    assert!(old.is_file());
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = s.options.clone();
    options.overwrite_existing_edits = true;
    let report = import.apply(options, None).unwrap();
    assert!(!kept(&report.skipped, &s.proxies[0]));
    let doc = Sidecar::read_recipe(Sidecar::paths(&s.proxies[0]).recipe).unwrap();
    assert_eq!(
        doc.last_writer.machine_id, MACHINE,
        "Lightroom recipe written"
    );
    assert_ne!(doc.recipe.settings.tone.exposure, EDITED);
    assert_ne!(Sidecar::paths(&s.proxies[0]).recipe, old);
    assert!(!old.exists(), "the replaced legacy recipe is not orphaned");
}

#[test]
fn sp_int3_byte_identical_proxies_with_shared_legacy_edits_both_keep_them() {
    let s = legacy(true);
    let shared = Sidecar::paths(&s.proxies[0]).recipe;
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let report = import.apply(s.options.clone(), None).unwrap();
    for path in &s.proxies {
        assert!(kept(&report.skipped, path), "{:?}", report.skipped);
        assert_eq!(exposure(path), EDITED);
    }
    assert_ne!(
        Sidecar::paths(&s.proxies[0]).recipe,
        Sidecar::paths(&s.proxies[1]).recipe,
        "each photo now owns its own copy"
    );
    assert!(!shared.exists(), "the shared legacy object was migrated");
}

#[test]
fn sp_int3_renamed_and_moved_catalog_reimport_keeps_edits() {
    let s = legacy(false);
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    import.apply(s.options.clone(), None).unwrap();
    assert_eq!(exposure(&s.proxies[0]), EDITED);
    // Rename and move the catalog together with its Smart Previews bundle.
    let moved = s.fixture.catalog.parent().unwrap().join("moved");
    std::fs::create_dir(&moved).unwrap();
    let catalog = moved.join("Renamed.lrcat");
    std::fs::rename(&s.fixture.catalog, &catalog).unwrap();
    std::fs::rename(
        import_lrcat::smart_previews::SmartPreviewIndex::new(&s.fixture.catalog).root(),
        import_lrcat::smart_previews::SmartPreviewIndex::new(&catalog).root(),
    )
    .unwrap();
    let import = s
        .engine
        .clone()
        .open_lrcat(catalog.to_string_lossy().into_owned())
        .unwrap();
    let rows: Vec<PathBuf> = resolve(&import.plan, &s.options)
        .unwrap()
        .into_iter()
        .filter(|r| r.outcome == Outcome::OfflineProxy)
        .map(|r| {
            Sidecar::register_store(&Sidecar::resolved_destination(&r.folder), &s.support);
            r.path
        })
        .collect();
    let report = import.apply(s.options.clone(), None).unwrap();
    assert!(kept(&report.skipped, &rows[0]), "{:?}", report.skipped);
    assert_eq!(exposure(&rows[0]), EDITED, "edits follow the photo");
}

/// N3: the new key is durable as soon as the row is processed, even when the
/// row is skipped or resumed without a write; a fresh process resolves it.
#[test]
fn sp_int3_new_keys_survive_a_restart_without_a_recipe_write() {
    const CHILD: &str = "TESSERA_SP_INT3_REKEY_CHILD";
    if let Some(spec) = std::env::var_os(CHILD) {
        let spec = spec.into_string().unwrap();
        let mut parts = spec.split('\n');
        let support = PathBuf::from(parts.next().unwrap());
        for line in parts {
            let (path, recipe) = line.split_once('\t').unwrap();
            let path = PathBuf::from(path);
            Sidecar::register_store(&path.parent().unwrap().canonicalize().unwrap(), &support);
            assert_eq!(Sidecar::paths(&path).recipe, PathBuf::from(recipe));
            assert_eq!(exposure(&path), EDITED);
        }
        return;
    }
    let s = legacy(true);
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    import.apply(s.options.clone(), None).unwrap();
    let mut spec = s.support.to_string_lossy().into_owned();
    for path in &s.proxies {
        spec.push('\n');
        spec.push_str(&format!(
            "{}\t{}",
            path.display(),
            Sidecar::paths(path).recipe.display()
        ));
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "lrcat::rekey_tests::sp_int3_new_keys_survive_a_restart_without_a_recipe_write",
        ])
        .env(CHILD, spec)
        .status()
        .unwrap();
    assert!(
        status.success(),
        "a restarted process fell back to another key"
    );
}

/// A catalog in `name` with a legacy (content-keyed) recipe on its one offline
/// Smart Preview; `rebuilt` changes the preview's bytes (a rebuilt preview).
fn catalog(
    root: &Path,
    name: &str,
    support: &Path,
    engine: &Arc<Engine>,
    rebuilt: bool,
    exposure: Option<f32>,
) -> (PathBuf, LrcatOptions, PathBuf) {
    let fixture = import_lrcat::fixture::write(&root.join(name)).unwrap();
    import_lrcat::fixture::write_smart_previews(&fixture).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    let photos = fixture.photos.canonicalize().unwrap();
    options.relocations[0].to = photos.to_string_lossy().into_owned();
    options.library_folder = photos.to_string_lossy().into_owned();
    options.import_smart_previews = true;
    options.copy_proxies = false;
    let row = resolve(&import.plan, &options)
        .unwrap()
        .into_iter()
        .find(|r| r.outcome == Outcome::OfflineProxy)
        .unwrap();
    Sidecar::register_store(&Sidecar::resolved_destination(&row.folder), support);
    if rebuilt {
        let mut bytes = std::fs::read(&row.path).unwrap();
        bytes.extend_from_slice(b"rebuilt");
        std::fs::write(&row.path, bytes).unwrap();
    }
    if let Some(exposure) = exposure {
        let mut doc = RecipeDocument::default();
        doc.recipe.image_id = app_image_id(&row.path);
        doc.recipe
            .edit(engine_api::recipe::EditMeta::user("Exposure", 1), |s| {
                s.tone.exposure = exposure
            })
            .unwrap();
        doc.record_write(FOREIGN, 1).unwrap();
        Sidecar::write_recipe(Sidecar::paths(&row.path).recipe, &doc).unwrap();
    }
    (fixture.catalog, options, row.path)
}

fn issue<'a>(issues: &'a [LrcatIssue], category: &str) -> Option<&'a LrcatIssue> {
    issues.iter().find(|i| i.category == category)
}

/// REV3-SP NB2 (coordinator ruling): two catalogs share a Lightroom file id
/// but carry different edits; the second keeps its own, visibly, in plan and
/// report, and nothing is deleted or merged.
#[test]
fn sp_int4_second_catalog_with_different_edits_keeps_them_and_reports_a_conflict() {
    let temp = tempfile::tempdir().unwrap();
    let support = temp.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let (a, a_options, a_proxy) = catalog(temp.path(), "a", &support, &engine, false, Some(1.25));
    let (b, b_options, b_proxy) = catalog(temp.path(), "b", &support, &engine, true, Some(2.5));
    assert_ne!(
        Sidecar::paths(&a_proxy).recipe,
        Sidecar::paths(&b_proxy).recipe
    );
    let open = |c: &Path| {
        engine
            .clone()
            .open_lrcat(c.to_string_lossy().into_owned())
            .unwrap()
    };
    open(&a).apply(a_options, None).unwrap();
    let plan = open(&b).plan(b_options.clone()).unwrap();
    assert!(
        issue(&plan.unsupported, "Edits conflict").is_some(),
        "plan: {:?}",
        plan.unsupported
    );
    let report = open(&b).apply(b_options, None).unwrap();
    let conflict = issue(&report.unsupported, "Edits conflict").expect("reported");
    assert!(conflict.reason.contains("kept separate"), "{conflict:?}");
    assert_eq!(exposure(&a_proxy), 1.25);
    assert_eq!(exposure(&b_proxy), 2.5, "B keeps its own edits");
}

/// NS4: the same photo in two catalogs (identical Smart Preview and edits)
/// shares one recipe, and the import report says so.
#[test]
fn sp_int4_second_catalog_with_identical_edits_shares_and_reports_it() {
    let temp = tempfile::tempdir().unwrap();
    let support = temp.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let (a, a_options, a_proxy) = catalog(temp.path(), "a", &support, &engine, false, Some(1.25));
    let (b, b_options, b_proxy) = catalog(temp.path(), "b", &support, &engine, false, None);
    assert_eq!(exposure(&b_proxy), 1.25, "legacy content key shared");
    let open = |c: &Path| {
        engine
            .clone()
            .open_lrcat(c.to_string_lossy().into_owned())
            .unwrap()
    };
    open(&a).apply(a_options, None).unwrap();
    let report = open(&b).apply(b_options, None).unwrap();
    assert!(
        issue(&report.unsupported, "Shared with another catalog").is_some(),
        "{:?}",
        report.unsupported
    );
    assert_eq!(
        Sidecar::paths(&a_proxy).recipe,
        Sidecar::paths(&b_proxy).recipe
    );
    assert_eq!(exposure(&b_proxy), 1.25);
}

/// REV3-SP NS3: a photo whose edits are reserved (open in Develop) is not
/// re-keyed under it; it keeps its recipe and the report says why.
#[test]
fn sp_int4_photo_open_in_develop_is_not_rekeyed_under_it() {
    let s = legacy(false);
    let legacy_recipe = Sidecar::paths(&s.proxies[0]).recipe;
    let id = app_image_id(&s.proxies[0]).unwrap();
    let held = crate::original_write::OriginalWriteReservation::acquire(
        &s.support,
        &[(id, s.proxies[0].clone())],
    )
    .unwrap();
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let report = import.apply(s.options.clone(), None).unwrap();
    drop(held);
    let reserved = issue(&report.unsupported, "Edit key not updated").expect("reported");
    // REV4-SP N10: the actual reason, not a guess.
    assert!(
        reserved
            .examples
            .iter()
            .any(|e| e.contains("another import or export is writing its edits")),
        "{reserved:?}"
    );
    assert!(
        !reserved
            .reason
            .contains("open in Develop or being exported")
    );
    assert_eq!(Sidecar::paths(&s.proxies[0]).recipe, legacy_recipe);
    assert_eq!(exposure(&s.proxies[0]), EDITED);
}

/// REV4-SP N11: cancelling part-way through the re-key pass and importing
/// again loses no edits.
#[test]
fn sp_int5_cancel_during_rekey_then_rerun_keeps_every_edit() {
    let s = legacy(true);
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    CANCEL_REKEY_AFTER.with(|c| c.set(Some(1)));
    let report = import.apply(s.options.clone(), None).unwrap();
    CANCEL_REKEY_AFTER.with(|c| c.set(None));
    assert!(report.cancelled, "cancelled during the re-key pass");
    for path in &s.proxies {
        assert_eq!(exposure(path), EDITED, "after cancel");
    }
    let import = s
        .engine
        .clone()
        .open_lrcat(s.fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let report = import.apply(s.options.clone(), None).unwrap();
    assert!(!report.cancelled);
    for path in &s.proxies {
        assert!(kept(&report.skipped, path), "{:?}", report.skipped);
        assert_eq!(exposure(path), EDITED, "after the re-run");
    }
    assert_ne!(
        Sidecar::paths(&s.proxies[0]).recipe,
        Sidecar::paths(&s.proxies[1]).recipe
    );
}
