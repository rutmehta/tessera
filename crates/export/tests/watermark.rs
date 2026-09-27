use engine_api::jobs::CancellationToken;
use export::{Anchor, Watermark, apply_watermark};

#[test]
fn graphic_anchor_alpha_and_inset_are_deterministic() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mark.png");
    let mut encoder = png::Encoder::new(std::fs::File::create(&path).unwrap(), 2, 2);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&[255, 0, 0, 128].repeat(4))
        .unwrap();
    let mark = Watermark::Graphic {
        path,
        scale: 0.2,
        opacity: 0.5,
        anchor: Anchor::BottomRight,
        inset: 0.1,
    };
    let mut first = image::Rgb32FImage::new(10, 10);
    apply_watermark(&mut first, &mark, &CancellationToken::new()).unwrap();
    assert!((first.get_pixel(7, 7)[0] - 128.0 / 255.0 * 0.5).abs() < 1e-6);
    assert_eq!(first.get_pixel(6, 7).0, [0.0; 3]);
    assert_eq!(first.get_pixel(9, 9).0, [0.0; 3]);
    let mut second = image::Rgb32FImage::new(10, 10);
    apply_watermark(&mut second, &mark, &CancellationToken::new()).unwrap();
    assert_eq!(first, second);

    // The public export path must place the mark after resizing/sharpening,
    // using the final short edge rather than the source dimensions.
    let source = pipeline_cpu::Image::new(20, 20, vec![vec![0.18; 400]; 3]).unwrap();
    let input = export::ExportImage {
        source: pipeline_cpu::RenderSource::Rgb(&source),
        name: "marked",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let settings = export::ExportSettings {
        format: export::Format::Png,
        output_dir: dir.path().into(),
        resize: export::Resize::LongEdge(10),
        sharpen_for: export::SharpenFor::Screen,
        metadata: export::Metadata::None,
        ..Default::default()
    };
    let baseline = export::export_one(&input, &Default::default(), &settings).unwrap();
    let marked = export::export_one(
        &input,
        &Default::default(),
        &export::ExportSettings {
            watermark: Some(mark),
            naming: "marked-watermark".into(),
            ..settings
        },
    )
    .unwrap();
    let baseline = image::open(baseline).unwrap().into_rgb8();
    let marked = image::open(marked).unwrap().into_rgb8();
    assert_eq!(marked.dimensions(), (10, 10));
    assert_eq!(marked.get_pixel(6, 7), baseline.get_pixel(6, 7));
    let a = 128.0 / 255.0 * 0.5;
    for c in 0..3 {
        let expected = (if c == 0 { 255.0 * a } else { 0.0 })
            + f32::from(baseline.get_pixel(7, 7)[c]) * (1.0 - a);
        assert!((f32::from(marked.get_pixel(7, 7)[c]) - expected).abs() <= 1.0);
    }
}

#[test]
fn text_rasterization_rotation_and_validation() {
    let dir = tempfile::tempdir().unwrap();
    let font = dir.path().join("font.ttf");
    std::fs::write(
        &font,
        include_bytes!("../../typography/tests/fonts/NotoSans-Regular.ttf"),
    )
    .unwrap();
    let mark = Watermark::Text {
        text: "Tessera".into(),
        font,
        size: 0.2,
        color: [1.0, 0.0, 0.0],
        opacity: 1.0,
        anchor: Anchor::Center,
        inset: 0.0,
        rotation: 90.0,
    };
    let mut first = image::Rgb32FImage::new(100, 100);
    apply_watermark(&mut first, &mark, &CancellationToken::new()).unwrap();
    assert!(first.pixels().any(|p| p[0] > 0.5));
    assert!(first.pixels().all(|p| p[1] == 0.0 && p[2] == 0.0));
    let mut second = image::Rgb32FImage::new(100, 100);
    apply_watermark(&mut second, &mark, &CancellationToken::new()).unwrap();
    assert_eq!(first, second);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(apply_watermark(&mut second, &mark, &cancel).is_err());
}
