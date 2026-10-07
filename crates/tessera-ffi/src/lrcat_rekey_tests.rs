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
    assert!(proxies.iter().all(|p| Sidecar::is_lightroom_owned(p)));
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
