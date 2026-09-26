use engine_api::id::ModelRef;
use ml_runtime::ModelRegistry;

#[test]
fn cached_resolution_never_fetches_and_still_verifies() -> anyhow::Result<()> {
    let cache = tempfile::tempdir()?;
    let registry = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml"),
        cache.path(),
    )?;
    let model = ModelRef {
        id: "test/conv".into(),
        version: "1".into(),
    };
    assert!(registry.resolve_cached_ref(&model)?.is_none());
    assert_eq!(std::fs::read_dir(cache.path())?.count(), 0);
    let fetched = registry.resolve_ref(&model)?;
    assert_eq!(
        registry.resolve_cached_ref(&model)?.unwrap().path(),
        fetched.path()
    );
    std::fs::write(fetched.path(), b"corrupt")?;
    assert!(registry.resolve_cached_ref(&model).is_err());
    assert!(
        registry
            .resolve_cached_ref(&ModelRef {
                id: "unknown".into(),
                version: "1".into()
            })
            .is_err()
    );
    Ok(())
}

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
