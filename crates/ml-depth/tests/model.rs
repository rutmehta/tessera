use ml_depth::{DepthEstimator, DepthStore, MODEL_SHA256};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::PathBuf;
#[test]
fn cached_model_fixture_and_partition() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_DEPTH_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-06/.cache/depth-registry"));
    if !cache.join(format!("{MODEL_SHA256}.onnx")).is_file() {
        eprintln!("SKIP offline: run tools/orchestrate/wp/M3-06/fetch_depth.py");
        return;
    }
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let store = DepthStore::new(dir.path(), 8 * 1024 * 1024).unwrap();
    let options = if std::env::var_os("TESSERA_DEPTH_COREML").is_some() {
        SessionOptions::default()
    } else {
        SessionOptions::cpu()
    };
    let mut model = DepthEstimator::load(&registry, options, store).unwrap();
    let image =
        image::RgbImage::from_fn(64, 48, |x, y| image::Rgb([x as u8 * 3, y as u8 * 4, 128]));
    let depth = model.estimate(&image).unwrap();
    assert_eq!((depth.width(), depth.height()), image.dimensions());
    assert!(
        depth
            .inverse_depth()
            .iter()
            .all(|v| v.is_finite() && (0. ..=1.).contains(v))
    );
    assert_eq!(depth, model.estimate(&image).unwrap());
    if let Some(fixture) = std::env::var_os("TESSERA_DEPTH_FIXTURE") {
        let image = image::open(fixture).unwrap().to_rgb8();
        let depth = model.estimate(&image).unwrap();
        assert_eq!((depth.width(), depth.height()), image.dimensions());
    } else {
        eprintln!("SKIP photograph: set TESSERA_DEPTH_FIXTURE to a JPEG");
    }
    let report = model.partition_report().unwrap();
    assert!(!report.nodes.is_empty());
    let mut counts = std::collections::BTreeMap::new();
    for node in report.nodes {
        *counts.entry(node.provider).or_insert(0usize) += 1;
    }
    println!("depth partition: {counts:?}");
}
