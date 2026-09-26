use engine_api::{id::ImageId, recipe::DevelopSettings};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig};

#[test]
fn rgb_skips_raw_denoise_in_cpu_reference_and_graph() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
        .save(&path)
        .unwrap();
    let source = RawImage::open(ImageId(2), path).unwrap();
    let rgb = pipeline_cpu::RenderSource::Rgb(source.rgb().unwrap().pixels());
    let mut settings = DevelopSettings::default();
    let expected = pipeline_cpu::render(&settings, &rgb).unwrap();
    settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
        model: engine_api::id::ModelRef {
            id: pipeline_cpu::POST_DENOISE_MODEL_ID.into(),
            version: pipeline_cpu::POST_DENOISE_VERSION.into(),
        },
        joint_demosaic: false,
    };
    assert_eq!(pipeline_cpu::render(&settings, &rgb).unwrap(), expected);
    let renderer = Renderer::new(RendererConfig::default());
    let tiles = renderer
        .render_region(
            &source,
            &settings,
            0,
            PixelRect::full(source.active_extent()),
        )
        .unwrap();
    assert!(!tiles.is_empty());
}

#[test]
fn jpeg_opens_and_renders_nonblack() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
        .save(&path)
        .unwrap();
    let source = RawImage::open(ImageId(1), &path).unwrap();
    let renderer = Renderer::new(RendererConfig::default());
    let tiles = renderer
        .render_region(
            &source,
            &DevelopSettings::default(),
            0,
            PixelRect::full(source.active_extent()),
        )
        .unwrap();
    assert!(
        tiles
            .iter()
            .any(|t| t.samples::<u8>().unwrap().iter().any(|v| *v > 30))
    );
}
