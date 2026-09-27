use engine_api::{jobs::CancellationToken, recipe::Recipe};
use export::*;
use pipeline_cpu::{Image, RenderSource};

#[test]
fn export_sharpens_after_resize_with_selected_strength_and_ppi() {
    let pixels = Image::new(
        96,
        64,
        vec![
            (0..96 * 64)
                .map(|i| if i % 96 > 48 { 0.3 } else { 0.1 })
                .collect();
            3
        ],
    )
    .unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "chart",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let recipe = Recipe::default();
    let token = CancellationToken::new();
    let resized = render_pixels(
        &source,
        &recipe,
        &RenderRequest {
            color_space: ColorSpace::Srgb,
            resize: Resize::LongEdge(48),
            sharpen_for: SharpenFor::None,
            scale: 1,
        },
        &token,
        None,
    )
    .unwrap();
    for medium in [SharpenFor::Screen, SharpenFor::Matte, SharpenFor::Glossy] {
        for amount in [
            SharpenAmount::Low,
            SharpenAmount::Standard,
            SharpenAmount::High,
        ] {
            for ppi in [150, 300] {
                let dir = tempfile::tempdir().unwrap();
                let settings = ExportSettings {
                    format: Format::Tiff { bits: 16 },
                    resize: Resize::LongEdge(48),
                    sharpen_for: medium,
                    sharpen_amount: amount,
                    dpi: Some(ppi),
                    output_dir: dir.path().into(),
                    ..Default::default()
                };
                let path = export_one(&source, &recipe, &settings).unwrap();
                let actual = image::open(path).unwrap().to_rgb16();
                let expected =
                    sharpen_output(resized.clone(), medium, amount, ppi, &token).unwrap();
                assert_eq!(actual.dimensions(), (48, 32));
                for (a, b) in actual.as_raw().iter().zip(expected.as_raw()) {
                    assert!((*a as f32 - b.clamp(0.0, 1.0) * 65535.0).abs() <= 2.0);
                }
            }
        }
    }
}
