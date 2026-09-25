use ml_enhance::SuperResolution;
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};

#[test]
fn cached_realesrgan_x4_scales_and_reports_coreml() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache
        .join(ml_enhance::SR_X4_SHA256.to_owned() + ".onnx")
        .is_file()
    {
        eprintln!("SKIP: Real-ESRGAN x4 not cached");
        return Ok(());
    }
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache)?;
    let mut sr = SuperResolution::load(&registry, 4, SessionOptions::default())?;
    let input = Tensor::new(3, 9, 7, vec![0.5; 3 * 9 * 7])?;
    let output = sr.super_resolution(&input, 4)?;
    assert_eq!(output.shape(), [1, 3, 36, 28]);
    assert!(output.data().iter().all(|v| v.is_finite()));
    let report = sr.partition_report()?;
    println!("Real-ESRGAN x4 partition: {report:?}");
    #[cfg(target_os = "macos")]
    assert!(
        report
            .nodes
            .iter()
            .any(|n| n.provider == "CoreMLExecutionProvider")
    );
    Ok(())
}

#[test]
fn cached_realesrgan_doubles_dimensions_preserves_edge_and_reports_coreml() -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache
        .join(ml_enhance::SR_X2_SHA256.to_owned() + ".onnx")
        .is_file()
    {
        eprintln!("SKIP: Real-ESRGAN x2 not cached");
        return Ok(());
    }
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache)?;
    let input = Tensor::new(
        3,
        16,
        16,
        (0..3 * 16 * 16)
            .map(|i| if i % 16 < 8 { 0.1 } else { 0.9 })
            .collect(),
    )?;
    let mut sr = SuperResolution::load(&registry, 2, SessionOptions::default())?;
    let output = sr.super_resolution(&input, 2)?;
    assert_eq!(output.shape(), [1, 3, 32, 32]);
    let mut left = 0.0;
    let mut right = 0.0;
    for y in 8..24 {
        for x in 4..12 {
            left += output.data()[y * 32 + x];
            right += output.data()[y * 32 + x + 16];
        }
    }
    assert!(
        right / 128.0 - left / 128.0 > 0.5,
        "sharp edge lost: {left} {right}"
    );
    let report = sr.partition_report()?;
    println!("Real-ESRGAN x2 partition: {report:?}");
    #[cfg(target_os = "macos")]
    assert!(
        report
            .nodes
            .iter()
            .any(|n| n.provider == "CoreMLExecutionProvider")
    );
    assert!(sr.super_resolution(&input, 4).is_err());
    Ok(())
}
