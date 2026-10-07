//! A-LR8 minor: the Smart Preview location override is test-only. Integration
//! tests link the library as production code does (no `cfg(test)`). This file
//! holds a single test because it changes the process environment.
use import_lrcat::fixture;
use tessera_ffi::*;

#[test]
fn smart_preview_location_override_is_ignored_in_production() {
    let temp = tempfile::tempdir().unwrap();
    let decoy = temp.path().join("decoy-bundle");
    std::fs::create_dir(&decoy).unwrap();
    // SAFETY: the only test in this binary; no other thread reads the environment.
    unsafe { std::env::set_var("TESSERA_LRCAT_SMART_PREVIEWS", &decoy) };
    let fixture = fixture::write(&temp.path().join("fx")).unwrap();
    fixture::write_smart_previews(&fixture).unwrap();
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    let photos = fixture.photos.canonicalize().unwrap();
    options.relocations[0].to = photos.to_string_lossy().into_owned();
    options.library_folder = photos.to_string_lossy().into_owned();
    options.import_smart_previews = true;
    let plan = import.plan(options).unwrap();
    assert_eq!(
        (
            plan.offline_with_smart_preview,
            plan.offline_without_smart_preview
        ),
        (1, 0),
        "the catalog's own Smart Previews are used whatever the environment says"
    );
}
