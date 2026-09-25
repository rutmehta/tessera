use engine_api::recipe::Recipe;
use export::*;
use pipeline_cpu::{Image, RenderSource};

#[test]
fn resize_modes_and_sharpening() {
    let image = Image::new(
        120,
        80,
        vec![
            (0..9600)
                .map(|i| if i % 120 < 60 { 0.1 } else { 0.5 })
                .collect();
            3
        ],
    )
    .unwrap();
    let source = ExportImage {
        source: RenderSource::Rgb(&image),
        name: "step",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let recipe = Recipe::default();
    for (resize, expected) in [
        (Resize::None, (120, 80)),
        (Resize::LongEdge(60), (60, 40)),
        (Resize::Fit(50, 20), (30, 20)),
        (Resize::Percent(25.0), (30, 20)),
        (Resize::Percent(200.0), (240, 160)),
    ] {
        let mut outputs = Vec::new();
        for sharpen_for in [
            SharpenFor::None,
            SharpenFor::Screen,
            SharpenFor::Matte,
            SharpenFor::Glossy,
        ] {
            let dir = tempfile::tempdir().unwrap();
            let settings = ExportSettings {
                format: Format::Tiff { bits: 16 },
                resize,
                sharpen_for,
                output_dir: dir.path().into(),
                ..Default::default()
            };
            let path = export_one(&source, &recipe, &settings).unwrap();
            let mut decoder =
                tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
            assert_eq!(decoder.dimensions().unwrap(), expected);
            let tiff::decoder::DecodingResult::U16(data) = decoder.read_image().unwrap() else {
                panic!("not u16")
            };
            outputs.push(data);
        }
        assert_ne!(outputs[0], outputs[1]);
        assert_ne!(outputs[1], outputs[2]);
        assert_ne!(outputs[2], outputs[3]);
    }
    for resize in [
        Resize::LongEdge(0),
        Resize::Fit(0, 1),
        Resize::Percent(f64::NAN),
        Resize::Percent(-1.0),
        Resize::Percent(f64::INFINITY),
        Resize::Percent(1e20),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            resize,
            output_dir: dir.path().into(),
            ..Default::default()
        };
        assert!(export_one(&source, &recipe, &settings).is_err());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
