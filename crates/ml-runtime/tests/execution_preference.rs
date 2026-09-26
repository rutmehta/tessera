use ml_runtime::{ExecutionPreference, Session, SessionOptions, Tensor};

#[test]
fn cpu_only_overrides_coreml_without_weakening_partition_guard() -> anyhow::Result<()> {
    let options = SessionOptions {
        coreml: true,
        ..SessionOptions::default()
    }
    .with_execution_preference(ExecutionPreference::CpuOnly);
    assert!(!options.coreml);
    let mut session = Session::load(
        concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/conv.onnx"),
        options,
    )?;
    assert!(
        session.fallback_reason.is_none(),
        "CPU is intentional, not fallback"
    );
    session.run(&Tensor::new(3, 64, 64, vec![0.25; 3 * 64 * 64])?)?;
    let report = session.partition_report()?;
    assert!(!report.nodes.is_empty());
    assert!(
        report
            .nodes
            .iter()
            .all(|n| n.provider == "CPUExecutionProvider")
    );
    assert!(report.require_coreml().is_err());
    assert!(
        SessionOptions::cpu()
            .with_execution_preference(ExecutionPreference::PreferCoreMl)
            .coreml
    );
    assert_eq!(SessionOptions::default().coreml, cfg!(target_os = "macos"));
    Ok(())
}
