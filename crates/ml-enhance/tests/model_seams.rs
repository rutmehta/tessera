//! Real-weight seam checks, distinct from the convolution fixture tests.
//! Long, narrow images cross several tiles and exceed the complete halo so
//! interior patches really differ from full-frame inference.
use engine_api::id::{ModelId, ModelRef};
use ml_enhance::{SR_VERSION, SR_X2_SHA256, SR_X4_SHA256, SuperResolution};
use ml_runtime::{ModelRegistry, Session, SessionOptions, Tensor};

fn compare(factor: usize, sha: &str, width: usize) -> anyhow::Result<()> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache.join(format!("{sha}.onnx")).is_file() {
        eprintln!("SKIP: Real-ESRGAN x{factor} not cached for production seam test");
        return Ok(());
    }
    let radius = (23 * 3 * 5 + 7) * if factor == 2 { 2 } else { 1 };
    assert!(width > 128 + 2 * radius);
    let height = 4;
    let input = Tensor::new(
        3,
        height,
        width,
        (0..3 * height * width)
            .map(|i| {
                let x = i % width;
                let channel = i / (height * width);
                let edge = if (x / 127).is_multiple_of(2) {
                    0.15
                } else {
                    0.75
                };
                edge + 0.03 * channel as f32 + 0.01 * (i % 7) as f32
            })
            .collect(),
    )?;
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache)?;
    let handle = registry.resolve_ref(&ModelRef {
        id: ModelId::new(format!("enhance/realesrgan-x{factor}")),
        version: SR_VERSION.into(),
    })?;
    assert_eq!(handle.spec().sha256, sha);
    // CPU fp32 isolates the spatial contract from CoreML precision/partitioning.
    // Separate model tests audit real CoreML execution.
    let full = Session::load(handle.path(), SessionOptions::cpu())?.run(&input)?;
    let tiled = SuperResolution::load(&registry, factor, SessionOptions::cpu())?
        .super_resolution(&input, factor)?;
    assert_eq!(full.shape(), tiled.shape());
    assert_eq!(tiled.shape(), [1, 3, height * factor, width * factor]);
    let max_error = full
        .data()
        .iter()
        .zip(tiled.data())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    println!("Real-ESRGAN x{factor} full vs tiled max error: {max_error}");
    assert!(max_error <= 1e-4, "x{factor} seam error: {max_error}");
    Ok(())
}

#[test]
fn cached_x2_full_vs_tiled_with_partial_halo_patches() -> anyhow::Result<()> {
    compare(2, SR_X2_SHA256, 1664)
}

#[test]
fn cached_x4_full_vs_tiled_with_partial_halo_patches() -> anyhow::Result<()> {
    compare(4, SR_X4_SHA256, 896)
}
