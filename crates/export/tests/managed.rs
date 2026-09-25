use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::recipe::Recipe;
use export::{ColorSpace, ExportImage, ExportSettings, Format, Metadata, export_one};
use pipeline_cpu::{Image, OutputContext, OutputTarget, RenderSource, render_managed_scaled};

#[test]
fn tiff_preserves_wide_gamut_float_render_until_final_quantization() {
    let image = Image::new(
        3,
        1,
        vec![
            vec![0.7, 0.1801, 0.1802],
            vec![0.02, 0.1801, 0.1802],
            vec![0.01, 0.1801, 0.1802],
        ],
    )
    .unwrap();
    let recipe = Recipe::default();
    let source = RenderSource::Rgb(&image);
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::ProPhoto).unwrap();
    let expected = render_managed_scaled(
        &recipe.settings,
        &source,
        1,
        &mut OutputContext {
            registry: &mut registry,
            target: OutputTarget::Export(&target),
            proof: None,
            options: TransformOptions::default(),
        },
    )
    .unwrap()
    .pixels;
    let dir = tempfile::tempdir().unwrap();
    let path = export_one(
        &ExportImage {
            source,
            name: "wide",
            sequence: 1,
            date: "20260925",
            metadata: None,
        },
        &recipe,
        &ExportSettings {
            format: Format::Tiff { bits: 16 },
            color_space: ColorSpace::ProPhoto,
            metadata: Metadata::None,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let mut decoder = tiff::decoder::Decoder::new(std::fs::File::open(path).unwrap()).unwrap();
    let tiff::decoder::DecodingResult::U16(actual) = decoder.read_image().unwrap() else {
        panic!("expected 16-bit TIFF")
    };
    let expected: Vec<_> = expected
        .as_raw()
        .iter()
        .map(|v| (v.clamp(0., 1.) * 65535.).round() as u16)
        .collect();
    assert_ne!(
        &expected[3..6],
        &expected[6..9],
        "fixture must resolve sub-8-bit differences"
    );
    assert_eq!(actual, expected);
}
