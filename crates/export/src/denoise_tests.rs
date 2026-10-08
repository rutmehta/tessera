//! Real-weight raw render regression, shared preview/export production adapters.
use crate::depth::{render, tone_map};
use engine_api::{
    id::{ImageId, ModelRef},
    jobs::CancellationToken,
    recipe::{DevelopSettings, settings::DenoiseMethod},
};
use std::sync::Arc;

#[test]
fn drunet_raw_is_nonblack_reduces_flat_noise_and_matches_preview() {
    let Some(cache) = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE").map(std::path::PathBuf::from)
    else {
        test_fixtures::models::skipped(
            &test_fixtures::current_test(),
            "set TESSERA_ENHANCE_MODEL_CACHE to a verified DRUNet cache",
        );
        return;
    };
    let weights = cache.join(format!("{}.onnx", ml_enhance::DENOISE_SHA256));
    if !weights.is_file() {
        test_fixtures::models::skipped(
            &test_fixtures::current_test(),
            "DRUNet weights absent from TESSERA_ENHANCE_MODEL_CACHE",
        );
        return;
    }
    let support = tempfile::tempdir().unwrap();
    let registry = Arc::new(ml_runtime::ModelRegistry::from_support(support.path()).unwrap());
    std::fs::copy(
        &weights,
        support
            .path()
            .join("models/cache")
            .join(weights.file_name().unwrap()),
    )
    .unwrap();
    let (_, mut metadata) = super::tests::raw_fixture();
    // A synthetic camera whose native channels are linear sRGB.
    let matrix = engine_api::color::WorkingSpace::LinearSrgb
        .to_xyz()
        .inverse()
        .unwrap();
    for c in 0..3 {
        metadata.cam_xyz[c] = matrix.0[c].map(|v| v as f32);
    }
    let mut state = 729u64;
    let values: Vec<f32> = (0..4096)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            0.18 + ((state >> 40) as f32 / (1u64 << 24) as f32 - 0.5) * 0.10
        })
        .collect();
    let cfa = Arc::new(raw_decode::CfaImage::from_linear(64, 64, values).unwrap());
    let raw =
        image_core::RawImage::new(ImageId(52), cfa.clone(), Arc::new(metadata.clone())).unwrap();
    let source = pipeline_cpu::RenderSource::Cfa {
        image: &cfa,
        metadata: &metadata,
    };
    let mut settings = DevelopSettings::default();
    let (before, _) = render(&source, &settings, 1, support.path(), None, None).unwrap();
    settings.denoise.method = DenoiseMethod::Neural {
        model: ModelRef {
            id: ml_enhance::DENOISE_MODEL_ID.into(),
            version: ml_enhance::DENOISE_VERSION.into(),
        },
        joint_demosaic: false,
    };
    settings.denoise.amount = 100.;
    let (export, warnings) = render(&source, &settings, 1, support.path(), None, None).unwrap();
    assert!(warnings.is_empty());
    assert!(export.as_raw().iter().all(|v| v.is_finite()));
    assert!(export.as_raw().iter().sum::<f32>() / export.as_raw().len() as f32 > 0.05);
    let variance = |im: &image::Rgb32FImage| {
        let mut total = 0.0f64;
        for c in 0..3 {
            let p: Vec<f64> = (8..56)
                .flat_map(|y| (8..56).map(move |x| f64::from(im.get_pixel(x, y).0[c])))
                .collect();
            let mean = p.iter().sum::<f64>() / p.len() as f64;
            total += p.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / p.len() as f64;
        }
        total / 3.0
    };
    assert!(
        variance(&export) < variance(&before) * 0.8,
        "noise before={} after={}",
        variance(&before),
        variance(&export)
    );
    let adapter = Arc::new(image_core::MlPostDemosaicDenoise::new(
        registry,
        Default::default(),
    ));
    let renderer =
        image_core::Renderer::new(Default::default()).with_post_demosaic_denoise(adapter);
    let mut preview = pipeline_cpu::Image::new(64, 64, vec![vec![0.; 4096]; 3]).unwrap();
    renderer
        .render_tiles(
            &raw,
            &settings,
            &preview.coords().collect::<Vec<_>>(),
            image_core::RenderOutput::SceneLinear,
            &CancellationToken::new(),
            &mut |t| preview.put(&t).unwrap(),
        )
        .unwrap();
    let preview = tone_map(preview);
    let l2 = (preview
        .as_raw()
        .iter()
        .zip(export.as_raw())
        .map(|(a, b)| f64::from(a - b).powi(2))
        .sum::<f64>()
        / export.as_raw().len() as f64)
        .sqrt();
    eprintln!(
        "DRUNet raw flat variance {} -> {}; preview/export RMS L2={l2}",
        variance(&before),
        variance(&export)
    );
    assert!(l2 < 0.002, "preview/export L2 {l2}");
}
