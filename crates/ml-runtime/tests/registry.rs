use engine_api::id::ModelRef;
use ml_runtime::ModelRegistry;

#[test]
fn registry_resolves_version_and_rejects_corruption() -> anyhow::Result<()> {
    let cache = tempfile::tempdir()?;
    let registry = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml"),
        cache.path(),
    )?;
    let handle = registry.resolve("test/conv")?;
    assert_eq!(handle.model_ref().version, "1");
    assert!(registry.resolve("absent").is_err());
    assert!(
        registry
            .resolve_ref(&ModelRef {
                id: "test/conv".into(),
                version: "missing".into()
            })
            .is_err()
    );
    std::fs::write(handle.path(), b"corrupt")?;
    assert!(registry.resolve("test/conv").is_err());
    Ok(())
}
