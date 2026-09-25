#![cfg(feature = "ml-denoise")]
use pipeline_cpu::{Image, PostDemosaicDenoise};
use std::sync::Arc;
mod common;

#[test]
fn cached_model_runs_through_renderer_and_reference_pipeline() {
    use engine_api::{
        id::ModelRef,
        recipe::{DevelopSettings, settings::DenoiseMethod},
    };
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../tools/orchestrate/wp/M3-05/.cache"
            ))
        });
    if !cache
        .join(format!("{}.onnx", ml_enhance::DENOISE_SHA256))
        .is_file()
    {
        eprintln!("SKIP: DRUNet not cached");
        return;
    }
    let registry = Arc::new(
        ml_runtime::ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            cache,
        )
        .unwrap(),
    );
    let adapter = Arc::new(image_core::MlPostDemosaicDenoise::new(
        registry,
        ml_runtime::SessionOptions {
            coreml: false,
            ..Default::default()
        },
    ));
    let image = common::synthetic(701, 64, 48, common::RGGB, [0, 0, 64, 48]);
    let mut settings = DevelopSettings::default();
    settings.denoise.method = DenoiseMethod::Neural {
        model: ModelRef {
            id: ml_enhance::DENOISE_MODEL_ID.into(),
            version: ml_enhance::DENOISE_VERSION.into(),
        },
        joint_demosaic: false,
    };
    settings.denoise.amount = 100.0;
    let source = pipeline_cpu::RenderSource::Cfa {
        image: image.cfa(),
        metadata: image.metadata(),
    };
    let reference = pipeline_cpu::render_linear_scaled_with_denoise(
        &settings,
        &source,
        1,
        &Default::default(),
        Some(adapter.as_ref()),
    )
    .unwrap();
    assert!(reference.planes().iter().flatten().all(|v| v.is_finite()));
    let renderer =
        image_core::Renderer::new(Default::default()).with_post_demosaic_denoise(adapter);
    let rect = image_core::PixelRect::full(image.level_extent(0));
    let cold = renderer.render_region(&image, &settings, 0, rect).unwrap();
    let warm = renderer.render_region(&image, &settings, 0, rect).unwrap();
    assert_eq!(cold.len(), warm.len());
    for (a, b) in cold.iter().zip(&warm) {
        // Existing renderer memo buffers are f16, including WhiteBalance.
        // Their quantization may move a display sample by one code value.
        assert!(
            a.samples::<u8>()
                .unwrap()
                .iter()
                .zip(b.samples::<u8>().unwrap())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }
}
#[test]
fn zero_sensor_mask_skips_download_and_changes_cache_revision() {
    let cache = std::env::temp_dir().join(format!("tessera-denoise-mask-{}", std::process::id()));
    let registry = Arc::new(
        ml_runtime::ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            &cache,
        )
        .unwrap(),
    );
    let adapter = image_core::MlPostDemosaicDenoise::new(registry, Default::default());
    let revision = adapter.adapter_revision().to_owned();
    let adapter = adapter.with_mask(2, 1, vec![0.0; 2]).unwrap();
    assert_ne!(revision, adapter.adapter_revision());
    let input = Image::new(2, 1, vec![vec![0.2, 0.7]; 3]).unwrap();
    assert_eq!(
        adapter.denoise(&input, 100.0).unwrap().planes(),
        input.planes()
    );
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    std::fs::remove_dir(cache).unwrap();
}
#[test]
fn lazy_adapter_zero_never_creates_model_cache() {
    let cache = std::env::temp_dir().join(format!(
        "tessera-denoise-no-download-{}",
        std::process::id()
    ));
    assert!(!cache.exists());
    let registry = ml_runtime::ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        &cache,
    )
    .unwrap();
    let adapter = image_core::MlPostDemosaicDenoise::new(
        Arc::new(registry),
        ml_runtime::SessionOptions::default(),
    );
    let input = Image::new(1, 1, vec![vec![0.25]; 3]).unwrap();
    assert_eq!(
        adapter.denoise(&input, 0.0).unwrap().planes(),
        input.planes()
    );
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 0);
    std::fs::remove_dir(&cache).unwrap();
    assert_eq!(
        pipeline_cpu::POST_DENOISE_MODEL_ID,
        ml_enhance::DENOISE_MODEL_ID
    );
    assert_eq!(
        pipeline_cpu::POST_DENOISE_VERSION,
        ml_enhance::DENOISE_VERSION
    );
    assert!(
        adapter
            .adapter_revision()
            .contains(ml_enhance::DENOISE_ADAPTER_VERSION)
    );
}
