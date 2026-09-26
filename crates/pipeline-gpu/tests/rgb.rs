use engine_api::{id::ImageId, recipe::DevelopSettings};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn resident_rgb_tone_edits_match_cpu() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rgb.jpg");
    image::RgbImage::from_fn(270, 259, |x, y| {
        image::Rgb([((x + y) % 220) as u8, (x % 190) as u8, (y % 170) as u8])
    })
    .save(&path)
    .unwrap();
    let source = RawImage::open(ImageId(1), &path).unwrap();
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let renderer = Renderer::with_ops(
        gpu,
        Arc::new(TileCache::new(32 << 20)),
        RendererConfig::default(),
    );
    for ev in [0., 0.7, -0.4] {
        let mut settings = DevelopSettings::default();
        settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
        settings.lens.remove_chromatic_aberration = false;
        settings.tone.exposure = ev;
        let expected = pipeline_cpu::render(
            &settings,
            &pipeline_cpu::RenderSource::Rgb(source.rgb().unwrap().pixels()),
        )
        .unwrap();
        let tiles = renderer
            .render_resident_region(
                &source,
                &settings,
                0,
                PixelRect::full(source.active_extent()),
                &Default::default(),
            )
            .unwrap()
            .expect("RGB must use resident graph");
        for tile in tiles {
            let data = tile.samples::<u8>().unwrap();
            let e = tile.layout().extent;
            let n = e.area() as usize;
            let (ox, oy) = tile.coord().pixel_origin(256);
            for y in 0..e.height {
                for x in 0..e.width {
                    let i = (y * e.width + x) as usize;
                    let pixel = expected.get_pixel(ox + x, oy + y).0;
                    for c in 0..3 {
                        assert!(
                            data[c * n + i].abs_diff(pixel[c]) <= 3,
                            "ev={ev} ({x},{y}) channel {c}: {} vs {}",
                            data[c * n + i],
                            pixel[c]
                        );
                    }
                }
            }
        }
    }
}
