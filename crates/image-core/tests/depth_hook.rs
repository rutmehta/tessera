use engine_api::recipe::DevelopSettings;
use image_core::{Renderer, RendererConfig, depth::DepthProvider, ml_depth::DepthMap};
use pipeline_cpu::Image;
use std::sync::Arc;

#[test]
fn depth_hook_applies_blur_and_visualises_without_weights() {
    let depth = DepthMap::from_prediction(
        8,
        8,
        (0..64).map(|i| if i < 32 { 1.0 } else { 0.0 }).collect(),
    )
    .unwrap();
    let provider = Arc::new(DepthProvider::from_map(depth));
    let renderer = Renderer::new(RendererConfig::default()).with_depth(provider);
    let input = Image::new(
        8,
        8,
        vec![
            (0..64)
                .map(|i| if i % 2 == 0 { 0.8 } else { 0.1 })
                .collect();
            3
        ],
    )
    .unwrap();
    let mut settings = DevelopSettings::default();
    settings.effects.lens_blur = Some(Default::default());
    let out = renderer.apply_depth_effects(&input, &settings).unwrap();
    assert_eq!(&out.planes()[0][..32], &input.planes()[0][..32]);
    assert_ne!(&out.planes()[0][32..], &input.planes()[0][32..]);
    let shown = renderer
        .with_depth_visualisation(true)
        .apply_depth_effects(&input, &settings)
        .unwrap();
    assert!(shown.planes()[0][..32].iter().all(|&v| v == 1.0));
    assert!(shown.planes()[0][32..].iter().all(|&v| v == 0.0));
}

#[test]
fn rgb_render_with_cached_depth_blurs_and_invalidates_visualisation_cache() {
    let input = Image::new(
        16,
        16,
        vec![
            (0..256)
                .map(|i| if i % 2 == 0 { 0.8 } else { 0.1 })
                .collect();
            3
        ],
    )
    .unwrap();
    let raw = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(49),
        image_core::RgbSource::from_linear_rec2020(input).unwrap(),
    )
    .unwrap();
    let depth = DepthMap::from_prediction(
        16,
        16,
        (0..256).map(|i| if i < 128 { 1.0 } else { 0.0 }).collect(),
    )
    .unwrap();
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(depth)));
    let mut settings = DevelopSettings::default();
    settings.effects.lens_blur = Some(Default::default());
    let rect = image_core::PixelRect::full(raw.active_extent());
    let shown = renderer
        .clone()
        .with_depth_visualisation(true)
        .render_region(&raw, &settings, 0, rect)
        .unwrap();
    let normal = renderer.render_region(&raw, &settings, 0, rect).unwrap();
    assert_ne!(
        shown[0].samples::<u8>().unwrap(),
        normal[0].samples::<u8>().unwrap()
    );
    let pixels = shown[0].samples::<u8>().unwrap();
    assert_eq!(pixels[0], 255);
    assert_eq!(pixels[255], 0);
}

#[test]
fn direct_rgb_linear_depth_and_visualisation_use_separate_memo_entries() {
    let input = Image::new(16, 16, vec![vec![0.2; 256]; 3]).unwrap();
    let raw = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(50),
        image_core::RgbSource::from_linear_rec2020(input).unwrap(),
    )
    .unwrap();
    let depth = DepthMap::from_prediction(
        16,
        16,
        (0..256).map(|i| if i < 128 { 1.0 } else { 0.0 }).collect(),
    )
    .unwrap();
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(Arc::new(DepthProvider::from_map(depth)));
    let mut settings = DevelopSettings::default();
    settings.effects.lens_blur = Some(Default::default());
    let cancel = engine_api::jobs::CancellationToken::new();
    let normal = renderer
        .render_rgb_linear(&raw, 0, &settings, &cancel)
        .unwrap();
    let shown = renderer
        .with_depth_visualisation(true)
        .render_rgb_linear(&raw, 0, &settings, &cancel)
        .unwrap();
    assert_ne!(normal.planes(), shown.planes());
    assert_eq!(shown.planes()[0][0], 1.0);
    assert_eq!(shown.planes()[0][255], 0.0);
}

#[test]
fn zero_blur_amount_is_exact_without_a_depth_provider() {
    let input = Image::new(3, 1, vec![vec![-0., -2., f32::MAX]; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    settings.effects.lens_blur = Some(engine_api::recipe::settings::LensBlur {
        amount: 0.,
        ..Default::default()
    });
    let renderer = Renderer::new(RendererConfig::default());
    let output = renderer.apply_depth_effects(&input, &settings).unwrap();
    for (a, b) in input
        .planes()
        .iter()
        .flatten()
        .zip(output.planes().iter().flatten())
    {
        assert_eq!(a.to_bits(), b.to_bits());
    }
    settings.effects.lens_blur.as_mut().unwrap().bokeh = "invalid".into();
    assert!(renderer.apply_depth_effects(&input, &settings).is_err());
}

#[test]
fn cached_provider_rejects_unknown_model_before_loading_weights() {
    let support = tempfile::tempdir().unwrap();
    let provider = Arc::new(DepthProvider::from_support(support.path()).unwrap());
    let renderer = Renderer::new(RendererConfig::default()).with_depth(provider);
    let input = Image::new(2, 1, vec![vec![0.2; 2]; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    settings.effects.lens_blur = Some(engine_api::recipe::settings::LensBlur {
        depth_model: Some(engine_api::id::ModelRef {
            id: "other/depth".into(),
            version: "1".into(),
        }),
        ..Default::default()
    });
    let error = renderer.apply_depth_effects(&input, &settings).unwrap_err();
    assert!(error.to_string().contains("provenance"), "{error}");
    assert_eq!(
        std::fs::read_dir(support.path().join("models/cache"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn cached_provider_histogram_tracks_source_and_clears_after_failed_estimate() {
    let support = tempfile::tempdir().unwrap();
    let provider = Arc::new(DepthProvider::from_support(support.path()).unwrap());
    assert!(provider.histogram().is_err());
    let input = Image::new(2, 1, vec![vec![0.2, 0.3]; 3]).unwrap();
    let rgb = image_core::depth::model_input(&input).unwrap();
    let store =
        image_core::ml_depth::DepthStore::new(support.path().join("previews/depth-cache"), 10000)
            .unwrap();
    let depth = DepthMap::from_prediction(2, 1, vec![1., 0.]).unwrap();
    depth
        .store(
            &store,
            &image_core::ml_depth::cache_key(&rgb, image_core::ml_depth::MODEL_VERSION),
        )
        .unwrap();
    assert_eq!(provider.estimate(&input).unwrap(), depth);
    assert_eq!(provider.histogram().unwrap(), depth.histogram());
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(provider.clone())
        .with_depth_visualisation(true);
    let shown = renderer
        .apply_depth_effects(&input, &DevelopSettings::default())
        .unwrap();
    assert_eq!(shown.planes(), &vec![vec![1., 0.]; 3]);
    let missing = Image::new(3, 1, vec![vec![0.1; 3]; 3]).unwrap();
    assert!(
        provider
            .estimate(&missing)
            .unwrap_err()
            .to_string()
            .contains(image_core::ml_depth::MISSING_MODEL_MESSAGE)
    );
    assert!(
        provider.histogram().is_err(),
        "failed estimate must not expose an older image's histogram"
    );
    assert_eq!(
        std::fs::read_dir(support.path().join("models/cache"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn replacing_depth_provider_invalidates_direct_rgb_memo() {
    let input = Image::new(2, 1, vec![vec![0.2; 2]; 3]).unwrap();
    let raw = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(51),
        image_core::RgbSource::from_linear_rec2020(input).unwrap(),
    )
    .unwrap();
    let first = Arc::new(DepthProvider::from_map(
        DepthMap::from_prediction(2, 1, vec![1., 0.]).unwrap(),
    ));
    let second = Arc::new(DepthProvider::from_map(
        DepthMap::from_prediction(2, 1, vec![0., 1.]).unwrap(),
    ));
    let renderer = Renderer::new(RendererConfig::default())
        .with_depth(first)
        .with_depth_visualisation(true);
    let cancel = engine_api::jobs::CancellationToken::new();
    let a = renderer
        .render_rgb_linear(&raw, 0, &DevelopSettings::default(), &cancel)
        .unwrap();
    let b = renderer
        .with_depth(second)
        .render_rgb_linear(&raw, 0, &DevelopSettings::default(), &cancel)
        .unwrap();
    assert_eq!(a.planes()[0], vec![1., 0.]);
    assert_eq!(b.planes()[0], vec![0., 1.]);
}
