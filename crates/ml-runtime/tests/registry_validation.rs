use ml_runtime::ModelRegistry;

fn source_manifest() -> String {
    // Version-mutation tests use only the local fixture, not production entries
    // whose revision strings are independent of the fixture's version counter.
    let fixture = include_str!("../models.toml")
        .split("[[models]]")
        .nth(1)
        .expect("local convolution fixture");
    format!("[[models]]{fixture}").replace(
        "file:tests/data/conv.onnx",
        &format!("file:{}/tests/data/conv.onnx", env!("CARGO_MANIFEST_DIR")),
    )
}

#[test]
fn malformed_and_duplicate_manifests_are_rejected() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("models.toml");
    let valid = source_manifest();
    for bad in [
        valid.replace("dtype = \"fp32\"", "dtype = \"float\""),
        valid.replace("shape = [1, 3, 64, 64]", "shape = [1, 3, 0, 64]"),
        valid.replace(
            "c64f58321fa5cfeca15daf11a4db55e9546e057d9d812acbf4f23e38f860d901",
            "../evil",
        ),
        format!("{valid}\n{valid}"),
        valid.replace("file:", "http:"),
    ] {
        std::fs::write(&path, bad)?;
        assert!(ModelRegistry::open(&path, dir.path().join("cache")).is_err());
    }
    Ok(())
}

#[test]
fn bad_download_never_enters_cache() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("models.toml");
    std::fs::write(
        &path,
        source_manifest().replace(
            "c64f58321fa5cfeca15daf11a4db55e9546e057d9d812acbf4f23e38f860d901",
            &"0".repeat(64),
        ),
    )?;
    let cache = dir.path().join("cache");
    let registry = ModelRegistry::open(&path, &cache)?;
    assert!(registry.resolve("test/conv").is_err());
    assert_eq!(std::fs::read_dir(cache)?.count(), 0);
    Ok(())
}

#[test]
fn multiple_versions_require_explicit_reference() -> anyhow::Result<()> {
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("models.toml");
    let valid = source_manifest();
    std::fs::write(
        &path,
        format!(
            "{valid}\n{}",
            valid.replace("version = \"1\"", "version = \"2\"")
        ),
    )?;
    let registry = ModelRegistry::open(&path, dir.path().join("cache"))?;
    assert!(registry.resolve("test/conv").is_err());
    let handle = registry.resolve_ref(&engine_api::id::ModelRef {
        id: "test/conv".into(),
        version: "2".into(),
    })?;
    assert_eq!(handle.model_ref().version, "2");
    let again = registry.resolve_ref(&handle.model_ref())?;
    assert_eq!(handle.path(), again.path());
    Ok(())
}
