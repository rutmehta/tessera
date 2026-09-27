use ml_enhance::{Denoiser, SuperResolution};
use ml_runtime::{ModelRegistry, SessionOptions};
#[test]
fn absent_models_never_resolve_online() {
    let dir = tempfile::tempdir().unwrap();
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml");
    let registry = ModelRegistry::open(manifest, dir.path()).unwrap();
    assert!(
        Denoiser::load_cached(&registry, SessionOptions::default())
            .err()
            .unwrap()
            .to_string()
            .contains("missing")
    );
    assert!(
        SuperResolution::load_cached(&registry, 2, SessionOptions::default())
            .err()
            .unwrap()
            .to_string()
            .contains("missing")
    );
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn corrupt_cache_is_not_repaired_or_downloaded() {
    let dir = tempfile::tempdir().unwrap();
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml");
    let registry = ModelRegistry::open(manifest, dir.path()).unwrap();
    let path = dir
        .path()
        .join(format!("{}.onnx", ml_enhance::DENOISE_SHA256));
    std::fs::write(&path, b"not weights").unwrap();
    assert!(Denoiser::load_cached(&registry, SessionOptions::default()).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"not weights");
    std::fs::remove_file(path).unwrap();
    assert!(Denoiser::load_cached(&registry, SessionOptions::default()).is_err());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
}

#[test]
fn optional_cached_models_execute_without_downloads() {
    let Some(cache) = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE") else {
        eprintln!("SKIP: set TESSERA_ENHANCE_MODEL_CACHE for cached model execution");
        return;
    };
    let manifest =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../ml-runtime/models.toml");
    let registry = ModelRegistry::open(manifest, &cache).unwrap();
    let input = ml_runtime::Tensor::new(3, 8, 8, vec![0.2; 3 * 8 * 8]).unwrap();
    let cache = std::path::Path::new(&cache);
    if cache
        .join(format!("{}.onnx", ml_enhance::DENOISE_SHA256))
        .is_file()
    {
        let mut model = Denoiser::load_cached(&registry, SessionOptions::default()).unwrap();
        let output = model.denoise(&input, 50.0, None).unwrap();
        assert_eq!(output.shape(), input.shape());
        assert!(output.data().iter().all(|v| v.is_finite()));
    } else {
        eprintln!("SKIP: DRUNet weights absent");
    }
    if cache
        .join(format!("{}.onnx", ml_enhance::SR_X2_SHA256))
        .is_file()
    {
        let mut model =
            SuperResolution::load_cached(&registry, 2, SessionOptions::default()).unwrap();
        let output = model.super_resolution(&input, 2).unwrap();
        assert_eq!(output.shape(), [1, 3, 16, 16]);
        assert!(output.data().iter().all(|v| v.is_finite()));
    } else {
        eprintln!("SKIP: Real-ESRGAN x2 weights absent");
    }
}
