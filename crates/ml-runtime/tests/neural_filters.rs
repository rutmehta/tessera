use ml_runtime::ModelRegistry;
#[test]
fn ddcolor_is_pinned_and_cache_only() -> anyhow::Result<()> {
    let cache = tempfile::tempdir()?;
    let r = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml"),
        cache.path(),
    )?;
    let m = r
        .models()
        .iter()
        .find(|m| m.id == "filters/ddcolor")
        .unwrap();
    assert!(
        r.models()
            .iter()
            .all(|m| !m.id.to_lowercase().contains("gfpgan"))
    );
    assert_eq!(m.version, "4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0");
    assert_eq!(
        m.sha256,
        "2653da00dc15e54a45e5200b61dbf82ee9ceaf56b02bb9b9657569ac775e82e6"
    );
    assert_eq!(m.outputs[0].shape, [1, 2, 512, 512]);
    assert!(
        r.resolve_cached_ref(&engine_api::id::ModelRef {
            id: m.id.as_str().into(),
            version: m.version.clone()
        })?
        .is_none()
    );
    assert_eq!(std::fs::read_dir(cache.path())?.count(), 0);
    Ok(())
}
