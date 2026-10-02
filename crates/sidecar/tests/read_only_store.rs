#[test]
fn explicitly_read_only_originals_publish_only_to_app_store() {
    let temp = std::env::temp_dir().join(format!(
        "tessera-readonly-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let source = temp.join("source/photo.raw");
    let app = temp.join("app");
    std::fs::create_dir_all(source.parent().unwrap()).unwrap();
    std::fs::write(&source, b"synthetic source").unwrap();
    std::fs::create_dir_all(&app).unwrap();
    let app = app.canonicalize().unwrap();
    sidecar::Sidecar::register_read_only_store(&source, &app);
    let paths = sidecar::Sidecar::paths(&source);
    assert!(paths.recipe.starts_with(&app));
    assert!(paths.xmp.starts_with(&app));
    sidecar::Sidecar::write_recipe(&paths.recipe, &Default::default()).unwrap();
    assert_eq!(
        std::fs::read_dir(source.parent().unwrap()).unwrap().count(),
        1
    );
    assert_eq!(std::fs::read(source).unwrap(), b"synthetic source");
    std::fs::remove_dir_all(temp).unwrap();
}
