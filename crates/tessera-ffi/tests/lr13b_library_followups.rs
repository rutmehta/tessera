//! LR-13b STEP 3 (A-LR8 M6, M7, library.json minor) on the synthetic fixture
//! catalog. Photo names are the fixture's synthetic names, never user data.
use import_lrcat::fixture;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};
use tessera_ffi::*;

struct Setup {
    temp: tempfile::TempDir,
    fixture: fixture::Fixture,
    engine: Arc<Engine>,
    import: Arc<LrcatImport>,
}

fn setup() -> Setup {
    let temp = tempfile::tempdir().unwrap();
    let fixture = fixture::write(&temp.path().join("fx")).unwrap();
    fixture::write_smart_previews(&fixture).unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    Setup {
        temp,
        fixture,
        engine,
        import,
    }
}

fn files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

#[test]
fn proxies_are_keyed_by_image_and_named_from_the_catalog() {
    for copy in [false, true] {
        let s = setup();
        // A second offline photo. The fixture's Smart Previews are
        // byte-identical, so a content-keyed copy (M7) or a content-keyed
        // protected recipe for in-place proxies (REV-SP-A S5, coordinator
        // ruling) would merge the two photos into one image, recipe and name.
        let wedding = s.fixture.photos.join("2026/wedding");
        std::fs::remove_file(wedding.join("ceremony-02.jpg")).unwrap();
        let expected: &[&str] = &["ceremony-02.jpg", "lost-01.jpg"];
        let mut options = s.import.default_options().unwrap();
        let photos = s.fixture.photos.canonicalize().unwrap();
        options.relocations[0].to = photos.to_string_lossy().into_owned();
        options.library_folder = photos.to_string_lossy().into_owned();
        options.import_smart_previews = true;
        options.copy_proxies = copy;
        assert_eq!(
            s.import
                .plan(options.clone())
                .unwrap()
                .offline_with_smart_preview as usize,
            expected.len()
        );
        let report = s.import.apply(options.clone(), None).unwrap();
        assert_eq!((report.imported, report.indexed), (6, 6), "copy={copy}");

        let rows = s.engine.list_images(ImageQuery::default()).unwrap();
        let proxies: Vec<_> = rows.iter().filter(|r| r.lightroom_smart_preview).collect();
        assert_eq!(
            proxies.len(),
            expected.len(),
            "copy={copy}: one image per photo"
        );
        assert_ne!(proxies[0].id, proxies[1].id);
        assert_ne!(proxies[0].path, proxies[1].path);
        // Separate photos keep separate recipes (their Lightroom edits).
        let recipes: Vec<String> = proxies
            .iter()
            .map(|r| s.engine.get_recipe(r.id.clone()).unwrap())
            .collect();
        assert_ne!(recipes[0], recipes[1], "copy={copy}: recipes merged");
        let mut names: Vec<_> = proxies
            .iter()
            .map(|r| r.display_name.clone().expect("catalog file name"))
            .collect();
        names.sort();
        assert_eq!(names, expected, "copy={copy}");
        for row in rows.iter().filter(|r| !r.lightroom_smart_preview) {
            assert_eq!(row.display_name, None, "ordinary rows keep their file name");
        }

        // The app lists photos through cull sessions; they carry the same name.
        let session = s
            .engine
            .open_cull_session(options.library_folder.clone())
            .unwrap();
        let mut session_names: Vec<_> = session
            .images()
            .unwrap()
            .into_iter()
            .filter(|r| r.lightroom_smart_preview)
            .map(|r| r.display_name.expect("catalog file name"))
            .collect();
        session_names.sort();
        assert_eq!(session_names, names);

        // Exports are named after the catalog file, not the proxy UUID or digest.
        let out = s.temp.path().join(format!("export-{copy}"));
        let exported = s
            .engine
            .export_batch(
                ExportTarget::Images {
                    image_ids: proxies.iter().map(|r| r.id.clone()).collect(),
                },
                serde_json::json!({"destination": out, "format": "png", "metadata": "none"})
                    .to_string(),
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            (exported.exported as usize, exported.failed),
            (expected.len(), 0),
            "{exported:?}"
        );
        let pngs: Vec<String> = expected.iter().map(|n| n.replace(".jpg", ".png")).collect();
        // Proxy exports also write a quality-warning note beside each image.
        let images: Vec<String> = files(&out)
            .into_iter()
            .filter(|n| n.ends_with(".png"))
            .collect();
        assert_eq!(images, pngs);
    }
}

#[test]
fn library_json_is_never_written_next_to_the_catalog() {
    let s = setup();
    let catalog_dir: PathBuf = s.fixture.catalog.parent().unwrap().to_path_buf();
    // The catalog's root is on an absent drive and it has Smart Previews:
    // the default library goes to app support, not beside the .lrcat.
    let options = s.import.default_options().unwrap();
    assert_ne!(Path::new(&options.library_folder), catalog_dir);
    assert_eq!(
        Path::new(&options.library_folder),
        s.temp.path().join("support/Imported Libraries")
    );
    let before = files(&catalog_dir);
    let mut explicit = options.clone();
    explicit.import_smart_previews = true;
    explicit.library_folder = catalog_dir.to_string_lossy().into_owned();
    let error = s.import.apply(explicit, None).unwrap_err();
    assert!(error.to_string().contains("library folder"), "{error}");
    assert_eq!(files(&catalog_dir), before);
    assert!(!catalog_dir.join("library.json").exists());
    // The default destination imports normally.
    let mut options = options;
    options.import_smart_previews = true;
    let report = s.import.apply(options, None).unwrap();
    assert!(report.imported > 0);
    assert!(!catalog_dir.join("library.json").exists());
    assert_eq!(files(&catalog_dir), before);
}
