use ml_enhance::{CfaDenoiser, CfaNoise, Tiling};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};
use std::{path::PathBuf, process::Command, time::Instant};

#[test]
fn trained_model_quality_tiling_and_partition_report() -> anyhow::Result<()> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let python = std::env::var_os("TESSERA_TRAIN_PYTHON")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-16/.venv/bin/python"));
    anyhow::ensure!(
        python.exists(),
        "Install tools/orchestrate/wp/M3-16/requirements.txt; set TESSERA_TRAIN_PYTHON"
    );
    let dir = tempfile::tempdir()?;
    let start = Instant::now();
    let fit_status = Command::new(&python)
        .current_dir(&root)
        .args([
            "tools/orchestrate/wp/M3-16/test_training.py",
            "TrainingTests.test_fit_recovers_poisson_gaussian_parameters",
        ])
        .status()?;
    anyhow::ensure!(fit_status.success(), "noise calibration test failed");
    let status = Command::new(&python)
        .current_dir(&root)
        .arg("tools/train_cfa_denoise.py")
        .args([
            "--synthetic",
            "--steps",
            "300",
            "--device",
            "cpu",
            "--output",
        ])
        .arg(dir.path())
        .status()?;
    anyhow::ensure!(status.success(), "tiny training failed");
    assert!(
        start.elapsed().as_secs() < 180,
        "tiny training exceeded three minutes"
    );
    let status = Command::new(&python)
        .current_dir(&root)
        .arg("tools/export_cfa_denoise.py")
        .arg(dir.path().join("cfa.pt"))
        .arg("--output")
        .arg(dir.path())
        .status()?;
    anyhow::ensure!(status.success(), "ONNX export failed");
    let registry = ModelRegistry::open(dir.path().join("models.toml"), dir.path().join("cache"))?;
    let noise = CfaNoise {
        shot: [0.003; 4],
        read: [0.0008; 4],
    };
    let samples = |name: &str| -> anyhow::Result<Vec<f32>> {
        Ok(std::fs::read(dir.path().join(name))?
            .as_chunks::<4>()
            .0
            .iter()
            .map(|x| f32::from_le_bytes(*x))
            .collect())
    };
    let clean = samples("clean.f32")?;
    let noisy = samples("noisy.f32")?;
    assert_eq!(clean.len(), 24 * 4 * 32 * 32);
    assert_eq!(clean.len(), noisy.len());
    for dtype in ["fp32", "fp16"] {
        let handle = registry.resolve(&format!("enhance/cfa-unet-{dtype}"))?;
        let mut model =
            CfaDenoiser::load(&registry, &handle.model_ref(), SessionOptions::default())?;
        let mut output = Vec::new();
        for crop in noisy.as_chunks::<4096>().0 {
            let input = Tensor::new(4, 32, 32, crop.to_vec())?;
            output.extend_from_slice(
                model
                    .apply(
                        &input,
                        noise,
                        100.0,
                        None,
                        Tiling {
                            tile_size: 64,
                            halo: 16,
                        },
                    )?
                    .data(),
            );
        }
        let mse = |data: &[f32]| {
            data.iter()
                .zip(&clean)
                .map(|(a, b)| f64::from(a - b).powi(2))
                .sum::<f64>()
                / clean.len() as f64
        };
        let gain = 10.0 * (mse(&noisy) / mse(&output)).log10();
        assert!(gain >= 3.0, "{dtype}: PSNR gain {gain}");
        // Larger than several tiles, sharp and smooth content, all four planes.
        let large = Tensor::new(
            4,
            66,
            70,
            (0..4 * 66 * 70).map(|i| (i % 113) as f32 / 113.0).collect(),
        )?;
        let whole = model.apply(
            &large,
            noise,
            100.0,
            None,
            Tiling {
                tile_size: 128,
                halo: 16,
            },
        )?;
        let tiled = model.apply(
            &large,
            noise,
            100.0,
            None,
            Tiling {
                tile_size: 16,
                halo: 16,
            },
        )?;
        let error = whole
            .data()
            .iter()
            .zip(tiled.data())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(
            error < if dtype == "fp16" { 0.003 } else { 0.0001 },
            "seam error: {error}"
        );
        let report = model.partition_report()?;
        assert!(!report.nodes.is_empty());
        println!("{dtype}: gain={gain}, tiling_error={error}, partition={report:?}");
    }
    Ok(())
}
