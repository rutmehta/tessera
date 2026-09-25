use ml_runtime::{ModelRegistry, Session, SessionOptions, Tensor};

#[test]
fn registered_models_coreml_guard() -> anyhow::Result<()> {
    if std::env::var("TESSERA_REQUIRE_COREML").as_deref() != Ok("1") {
        return Ok(());
    }
    let cache = tempfile::tempdir()?;
    let manifest = std::env::var_os("TESSERA_MODELS_MANIFEST")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| concat!(env!("CARGO_MANIFEST_DIR"), "/models.toml").into());
    let registry = ModelRegistry::open(manifest, cache.path())?;
    anyhow::ensure!(
        !registry.models().is_empty(),
        "CoreML guard refuses an empty registry"
    );
    for model in registry.models() {
        let handle = registry.resolve_ref(&engine_api::id::ModelRef {
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
        println!("{}@{}: {report:?}", model.id, model.version);
        report.require_coreml()?;
    }
    Ok(())
}

#[test]
fn cpu_and_empty_reports_fail_coreml_policy() -> anyhow::Result<()> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/conv.onnx");
    let mut session = Session::load(path, SessionOptions::cpu())?;
    assert!(session.partition_report().is_err());
    session.run(&Tensor::new(3, 64, 64, vec![0.; 12288])?)?;
    assert!(session.partition_report()?.require_coreml().is_err());
    assert!(
        ml_runtime::PartitionReport { nodes: vec![] }
            .require_coreml()
            .is_err()
    );
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
#[ignore = "manual timing benchmark"]
fn bench_twenty_runs_coreml_vs_cpu() -> anyhow::Result<()> {
    let input = Tensor::new(3, 64, 64, vec![0.25; 12288])?;
    for (name, options) in [
        ("CPU", SessionOptions::cpu()),
        ("CoreML", SessionOptions::default()),
    ] {
        let mut session = Session::load(
            concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/conv.onnx"),
            options,
        )?;
        session.run(&input)?;
        let start = std::time::Instant::now();
        for _ in 0..20 {
            std::hint::black_box(session.run(&input)?);
        }
        println!("{name}: 20 runs {:?}", start.elapsed());
        if name == "CoreML" {
            session.partition_report()?.require_coreml()?;
        }
    }
    Ok(())
}
