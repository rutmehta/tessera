use engine_api::recipe::Recipe;
use export::{ExportImage, ExportSettings, Format, Metadata, export_one_upscaled};
use ml_enhance::SuperResolution;
use ml_runtime::{ModelRegistry, SessionOptions};
use pipeline_cpu::{Image, RenderSource};

#[test]
fn cached_model_exports_doubled_png() -> Result<(), Box<dyn std::error::Error>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache
        .join(ml_enhance::SR_X2_SHA256.to_owned() + ".onnx")
        .is_file()
    {
        eprintln!("SKIP: Real-ESRGAN x2 not cached");
        return Ok(());
    }
    let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), cache)?;
    let mut sr = SuperResolution::load(&registry, 2, SessionOptions::cpu())?;
    let pixels = Image::new(8, 6, vec![vec![0.18; 48]; 3])?;
    let input = ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "enhanced",
        sequence: 1,
        date: "20260925",
        metadata: None,
    };
    let dir = tempfile::tempdir()?;
    let settings = ExportSettings {
        format: Format::Png,
        metadata: Metadata::None,
        output_dir: dir.path().into(),
        ..Default::default()
    };
    let output = export_one_upscaled(&input, &Recipe::default(), &settings, &mut sr)?;
    let mut decoder = png::Decoder::new(std::fs::File::open(output)?).read_info()?;
    assert_eq!((decoder.info().width, decoder.info().height), (16, 12));
    let mut data = vec![0; decoder.output_buffer_size()];
    decoder.next_frame(&mut data)?;
    assert!(export_one_upscaled(&input, &Recipe::default(), &settings, &mut sr).is_err());
    Ok(())
}
