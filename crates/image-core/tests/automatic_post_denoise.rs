#![cfg(feature = "ml-denoise")]

use image_core::MlPostDemosaicDenoise;
use ml_enhance::{DENOISE_SHA256, Denoiser};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};
use pipeline_cpu::{Image, PostDemosaicDenoise};
use std::sync::Arc;

#[test]
fn cached_post_adapter_uses_automatic_sigma_for_masked_and_unmasked_images() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache.join(format!("{DENOISE_SHA256}.onnx")).is_file() {
        eprintln!("SKIP: DRUNet not cached");
        return;
    }
    let registry =
        Arc::new(ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache).unwrap());
    let (h, w) = (16, 24);
    let mut seed = 17u32;
    let values = (0..3 * h * w)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            0.25 + 0.02 * (2.0 * (seed >> 8) as f32 / 16777216.0 - 1.0)
        })
        .collect::<Vec<_>>();
    let tensor = Tensor::new(3, h, w, values.clone()).unwrap();
    let image = Image::new(
        w as u32,
        h as u32,
        values.chunks_exact(h * w).map(|p| p.to_vec()).collect(),
    )
    .unwrap();
    let mut direct = Denoiser::load_cached(&registry, SessionOptions::cpu()).unwrap();
    let mut mask = vec![0.5; h * w];
    mask[0] = 0.0;
    for masked in [false, true] {
        let adapter = MlPostDemosaicDenoise::new(registry.clone(), SessionOptions::cpu());
        let adapter = if masked {
            adapter.with_mask(w as u32, h as u32, mask.clone()).unwrap()
        } else {
            adapter
        };
        let expected = direct
            .denoise_automatic(&tensor, 50.0, masked.then_some(mask.as_slice()))
            .unwrap();
        let actual = adapter.denoise(&image, 50.0).unwrap();
        for (&a, &b) in actual.planes().iter().flatten().zip(expected.data()) {
            assert!(
                (a - b).abs() < 1e-7,
                "post adapter must use automatic sigma: {a} vs {b}"
            );
        }
    }
}
