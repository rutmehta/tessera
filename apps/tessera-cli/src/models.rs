use anyhow::{Context, ensure};
use engine_api::id::ModelRef;
use ml_runtime::{ModelRegistry, Session, SessionOptions};
use serde_json::{Value, json};
use std::path::Path;

/// List app-local registrations, or audit executed nodes with real CoreML probes.
/// Models are pinned in `models.toml`; resolved ONNX files are cached in `models/`.
/// A probe covers manifest-shaped zero inputs, not every data-dependent branch.
pub fn run(app_dir: &Path, check: bool) -> anyhow::Result<Value> {
    let manifest = app_dir.join("models.toml");
    match std::fs::symlink_metadata(&manifest) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !check,
                "no registered models: {} is missing",
                manifest.display()
            );
            return Ok(json!({"models": [], "checked": false}));
        }
        result => {
            result.with_context(|| format!("inspect {}", manifest.display()))?;
        }
    }
    let registry = ModelRegistry::open(&manifest, app_dir.join("models"))
        .with_context(|| format!("open model registry {}", manifest.display()))?;
    if !check {
        return Ok(json!({"models": registry.models(), "checked": false}));
    }
    ensure!(
        !registry.models().is_empty(),
        "no registered models to check"
    );
    let mut models = Vec::new();
    for model in registry.models() {
        let checked = (|| -> anyhow::Result<Value> {
            let handle = registry.resolve_ref(&ModelRef {
                id: model.id.as_str().into(),
                version: model.version.clone(),
            })?;
            let mut session = Session::load(
                handle.path(),
                SessionOptions {
                    coreml: true,
                    ..SessionOptions::default()
                },
            )?;
            session.probe(&model.inputs)?;
            let report = session.partition_report()?;
            report
                .require_coreml()
                .with_context(|| match &session.fallback_reason {
                    Some(reason) => format!("CoreML initialization fell back to CPU: {reason}"),
                    None => "CoreML partition policy failed".to_owned(),
                })?;
            let mut entry = serde_json::to_value(model)?;
            entry["nodes"] = json!(
                report
                    .nodes
                    .iter()
                    .map(|node| json!({
                        "node": node.node, "provider": node.provider
                    }))
                    .collect::<Vec<_>>()
            );
            Ok(entry)
        })()
        .with_context(|| format!("check model {}@{}", model.id, model.version))?;
        models.push(checked);
    }
    Ok(json!({"models": models, "checked": true}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::PathBuf,
        sync::atomic::{AtomicU64, Ordering},
    };

    struct AppDir(PathBuf);
    impl AppDir {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "tessera-models-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for AppDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn manifest(dir: &AppDir) {
        fs::write(
            dir.0.join("models.toml"),
            r#"
[[models]]
id = "test/model"
version = "1"
task = "test"
dtype = "fp32"
sha256 = "0000000000000000000000000000000000000000000000000000000000000000"
download_url = "file:absent.onnx"
inputs = [{ name = "input", shape = [1], dtype = "fp32" }]
outputs = [{ name = "output", shape = [1], dtype = "fp32" }]
"#,
        )
        .unwrap();
    }

    #[test]
    fn listing_reads_registry_without_resolving_model_files() {
        let dir = AppDir::new();
        manifest(&dir);
        let result = run(&dir.0, false).unwrap();
        assert_eq!(result["models"][0]["id"], "test/model");
        assert_eq!(result["models"][0]["version"], "1");
        assert_eq!(result["models"].as_array().unwrap().len(), 1);
        assert_eq!(result["checked"], false);
    }

    #[test]
    fn check_rejects_missing_and_empty_registries() {
        let dir = AppDir::new();
        assert!(
            run(&dir.0, true)
                .unwrap_err()
                .to_string()
                .contains("no registered models")
        );
        fs::write(dir.0.join("models.toml"), "models = []\n").unwrap();
        assert!(
            run(&dir.0, true)
                .unwrap_err()
                .to_string()
                .contains("no registered models")
        );
    }

    #[test]
    fn check_resolves_registered_files_instead_of_claiming_success() {
        let dir = AppDir::new();
        manifest(&dir);
        let error = run(&dir.0, true).unwrap_err();
        assert!(error.to_string().contains("test/model@1"));
    }

    #[test]
    fn real_fixture_produces_assignments_or_explicit_coreml_policy_failure() {
        let dir = AppDir::new();
        fs::write(
            dir.0.join("models.toml"),
            include_str!("../../../crates/ml-runtime/models.toml")
                .replace("file:tests/data/conv.onnx", "file:conv.onnx"),
        )
        .unwrap();
        fs::write(
            dir.0.join("conv.onnx"),
            include_bytes!("../../../crates/ml-runtime/tests/data/conv.onnx"),
        )
        .unwrap();
        match run(&dir.0, true) {
            Ok(result) => {
                assert_eq!(result["checked"], true);
                let nodes = result["models"][0]["nodes"].as_array().unwrap();
                assert!(!nodes.is_empty());
                assert!(
                    nodes
                        .iter()
                        .all(|node| node["provider"] == "CoreMLExecutionProvider")
                );
            }
            Err(error) => {
                // CPU-only environments must fail the audit, never pass via fallback.
                assert!(
                    format!("{error:#}").contains("model has non-CoreML nodes"),
                    "{error:#}"
                );
            }
        }
    }

    #[test]
    fn malformed_registry_is_an_error() {
        let dir = AppDir::new();
        fs::write(dir.0.join("models.toml"), "not valid TOML").unwrap();
        assert!(run(&dir.0, false).is_err());
    }

    #[test]
    fn missing_manifest_lists_no_models_without_creating_files() {
        let dir = AppDir::new();
        let result = run(&dir.0, false).unwrap();
        assert_eq!(result["models"], serde_json::json!([]));
        assert_eq!(result["checked"], false);
        assert_eq!(fs::read_dir(&dir.0).unwrap().count(), 0);
    }
}
