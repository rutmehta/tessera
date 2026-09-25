use engine_api::recipe::Recipe;
use export::{ExportImage, ExportSettings, Format, Metadata, export_one_upscaled};
use ml_enhance::SuperResolution;
use ml_runtime::{ModelRegistry, SessionOptions};
use pipeline_cpu::{Image, RenderSource};

#[test]
fn cached_models_batch_cancellation_resume_and_duplicate_preflight()
-> Result<(), Box<dyn std::error::Error>> {
    use engine_api::{EngineError, jobs::CancellationToken};
    use export::{ExportItem, export_batch_upscaled};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    for (factor, sha) in [(2, ml_enhance::SR_X2_SHA256), (4, ml_enhance::SR_X4_SHA256)] {
        if !cache.join(format!("{sha}.onnx")).is_file() {
            eprintln!("SKIP: Real-ESRGAN x{factor} not cached");
            continue;
        }
        let registry = ModelRegistry::open(root.join("crates/ml-runtime/models.toml"), &cache)?;
        let mut sr = SuperResolution::load(&registry, factor, SessionOptions::cpu())?;
        let pixels = Image::new(8, 6, vec![vec![0.18; 48]; 3])?;
        let recipe = Recipe::default();
        let items: Vec<_> = (1..=2)
            .map(|sequence| ExportItem {
                image: ExportImage {
                    source: RenderSource::Rgb(&pixels),
                    name: "photo",
                    sequence,
                    date: "",
                    metadata: None,
                },
                recipe: &recipe,
            })
            .collect();
        let dir = tempfile::tempdir()?;
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            format: Format::Png,
            ..Default::default()
        };
        let cancel = CancellationToken::new();
        cancel.cancel();
        let report = export_batch_upscaled(
            &items,
            &settings,
            |_| panic!("already cancelled"),
            &cancel,
            &mut sr,
        )?;
        assert_eq!(report.remaining(), vec![0, 1]);
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        let duplicate = ExportSettings {
            naming: "same".into(),
            ..settings.clone()
        };
        let error = export_batch_upscaled(
            &items,
            &duplicate,
            |_| panic!("duplicate preflight"),
            &CancellationToken::new(),
            &mut sr,
        )
        .unwrap_err();
        assert!(error.to_string().contains("duplicate output names"));
        assert_eq!(std::fs::read_dir(dir.path())?.count(), 0);
        let cancel = CancellationToken::new();
        let report = export_batch_upscaled(
            &items,
            &settings,
            |p| {
                assert_eq!((p.index, p.completed, p.total), (0, 1, 2));
                assert!(p.result.is_ok());
                cancel.cancel();
            },
            &cancel,
            &mut sr,
        )?;
        assert!(report.results[0].is_ok());
        assert!(matches!(report.results[1], Err(EngineError::Cancelled)));
        assert_eq!(report.remaining(), vec![1]);
        assert!(!dir.path().join("photo-2.png").exists());
        assert!(!dir.path().join("photo-2.png.xmp").exists());
        let resumed = export_batch_upscaled(
            &items[1..],
            &settings,
            |_| {},
            &CancellationToken::new(),
            &mut sr,
        )?;
        assert!(resumed.remaining().is_empty());
        for sequence in 1..=2 {
            let output = dir.path().join(format!("photo-{sequence}.png"));
            assert_eq!(
                image::image_dimensions(output)?,
                (8 * factor as u32, 6 * factor as u32)
            );
        }
    }
    Ok(())
}

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
