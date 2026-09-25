use ml_enhance::{DENOISE_SHA256, Denoiser};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};

fn registry() -> anyhow::Result<Option<ModelRegistry>> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("tools/orchestrate/wp/M3-05/.cache"));
    if !cache.join(format!("{DENOISE_SHA256}.onnx")).is_file() {
        eprintln!("SKIP: DRUNet not cached");
        return Ok(None);
    }
    Ok(Some(ModelRegistry::open(
        root.join("crates/ml-runtime/models.toml"),
        cache,
    )?))
}

fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn gradient(h: usize, w: usize) -> anyhow::Result<(Tensor, Tensor)> {
    let mut state = 42_u32;
    let mut uniform = || {
        state = state.wrapping_mul(1664525).wrapping_add(1013904223);
        (state as f64 + 1.0) / (u32::MAX as f64 + 2.0)
    };
    let mut clean = Vec::new();
    let mut noisy = Vec::new();
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                let display =
                    0.25 + 0.35 * x as f32 / w as f32 + 0.1 * y as f32 / h as f32 + 0.02 * c as f32;
                let gaussian =
                    (-2.0 * uniform().ln()).sqrt() * (std::f64::consts::TAU * uniform()).cos();
                clean.push(decode(display));
                noisy.push(decode(
                    (display + ml_enhance::DENOISE_SIGMA * gaussian as f32).clamp(0.0, 1.0),
                ));
            }
        }
    }
    Ok((Tensor::new(3, h, w, clean)?, Tensor::new(3, h, w, noisy)?))
}

#[test]
fn cached_drunet_improves_noisy_gradient_by_three_db() -> anyhow::Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut denoiser = Denoiser::load(&registry, SessionOptions::cpu())?;
    let (clean, noisy) = gradient(65, 81)?;
    let output = denoiser.denoise(&noisy, 100.0, None)?;
    assert_eq!(output.shape(), noisy.shape());
    let psnr = |image: &Tensor| {
        let mse = image
            .data()
            .iter()
            .zip(clean.data())
            .map(|(a, b)| (*a as f64 - *b as f64).powi(2))
            .sum::<f64>()
            / clean.data().len() as f64;
        -10.0 * mse.log10()
    };
    let before = psnr(&noisy);
    let after = psnr(&output);
    println!(
        "DRUNet linear-light PSNR: before={before:.6} after={after:.6} gain={:.6} dB",
        after - before
    );
    assert!(after - before >= 3.0);
    Ok(())
}

#[test]
fn cached_denoiser_zero_and_mask_bypass_are_bit_exact() -> anyhow::Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut denoiser = Denoiser::load(&registry, SessionOptions::cpu())?;
    let input = Tensor::new(3, 1, 2, vec![-0.0, -2.0, 0.5, 2.0, 0.0, 1.0])?;
    for output in [
        denoiser.denoise(&input, 0.0, None)?,
        denoiser.denoise_masked(&input, 100.0, None, &[0.0, 0.0])?,
    ] {
        assert_eq!(
            input.data().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            output
                .data()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }
    assert!(
        denoiser.partition_report().is_err(),
        "bypass must not execute model"
    );
    Ok(())
}

#[test]
fn cached_drunet_full_vs_distinct_tiles() -> anyhow::Result<()> {
    use engine_api::id::ModelRef;
    use ml_enhance::{
        DENOISE_MODEL_ID, DENOISE_SIGMA, DENOISE_VERSION, SpatialContract, Tiling, run_tiled,
    };
    use ml_runtime::Session;
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let handle = registry.resolve_ref(&ModelRef {
        id: DENOISE_MODEL_ID.into(),
        version: DENOISE_VERSION.into(),
    })?;
    let mut session = Session::load(handle.path(), SessionOptions::cpu())?;
    let mut denoiser = Denoiser::load(&registry, SessionOptions::cpu())?;
    for (h, w) in [(16, 640), (640, 16)] {
        let (_, linear) = gradient(h, w)?;
        let display = Tensor::new(
            3,
            h,
            w,
            linear
                .data()
                .iter()
                .map(|&v| {
                    if v <= 0.0031308 {
                        12.92 * v
                    } else {
                        1.055 * v.powf(1.0 / 2.4) - 0.055
                    }
                })
                .collect(),
        )?;
        let mut infer = |patch: &Tensor| {
            let [_, _, ph, pw] = patch.shape();
            let mut values = patch.data().to_vec();
            values.resize(4 * ph * pw, DENOISE_SIGMA);
            session.run(&Tensor::new(4, ph, pw, values)?)
        };
        let full = infer(&display)?;
        let mut shapes = std::collections::BTreeSet::new();
        let tiled = run_tiled(
            &display,
            SpatialContract {
                scale: 1,
                radius: 185,
                alignment: 8,
            },
            Tiling {
                tile_size: 128,
                halo: 192,
            },
            |patch| {
                assert!(
                    patch.data().len() < display.data().len(),
                    "must not rerun full frame per tile"
                );
                shapes.insert(patch.shape());
                infer(patch)
            },
        )?;
        assert!(
            shapes.len() > 1,
            "genuinely distinct patch extents required"
        );
        let max_error = full
            .data()
            .iter()
            .zip(tiled.data())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        let adapted = denoiser.denoise(&linear, 100.0, None)?;
        let adapter_error = full
            .data()
            .iter()
            .zip(adapted.data())
            .map(|(&a, &b)| (decode(a.clamp(0.0, 1.0)) - b).abs())
            .fold(0.0_f32, f32::max);
        println!(
            "DRUNet {w}x{h} full/tiled: model={max_error:e} linear adapter={adapter_error:e}, patch shapes={shapes:?}"
        );
        assert!(max_error <= 1e-4);
        assert!(adapter_error <= 1e-4);
    }
    Ok(())
}

#[test]
fn cached_drunet_mask_blends_in_linear_light_and_hint_is_advisory() -> anyhow::Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut denoiser = Denoiser::load(&registry, SessionOptions::cpu())?;
    let (_, input) = gradient(9, 17)?;
    let full = denoiser.denoise(&input, 100.0, None)?;
    let hint = ml_enhance::NoiseModelHint {
        read: [0.01; 3],
        shot: [0.02; 3],
    };
    let mut mask = vec![0.5; 9 * 17];
    mask[0] = 0.0;
    let blended = denoiser.denoise_masked(&input, 50.0, Some(hint), &mask)?;
    for (i, (&src, &dst)) in input.data().iter().zip(full.data()).enumerate() {
        if i % (9 * 17) == 0 {
            assert_eq!(blended.data()[i].to_bits(), src.to_bits());
        } else {
            assert!((blended.data()[i] - (0.75 * src + 0.25 * dst)).abs() < 1e-7);
        }
    }
    assert!(
        denoiser
            .denoise(&Tensor::new(3, 1, 1, vec![2.0; 3])?, 1.0, None)
            .is_err()
    );
    assert!(denoiser.denoise(&input, f32::NAN, None).is_err());
    assert!(denoiser.denoise_masked(&input, 1.0, None, &[1.0]).is_err());
    Ok(())
}

#[test]
fn cached_drunet_reports_executed_coreml_partitions() -> anyhow::Result<()> {
    let Some(registry) = registry()? else {
        return Ok(());
    };
    let mut denoiser = Denoiser::load(&registry, SessionOptions::default())?;
    let (_, input) = gradient(16, 24)?;
    let output = denoiser.denoise(&input, 100.0, None)?;
    assert_eq!(output.shape(), input.shape());
    assert!(output.data().iter().all(|v| v.is_finite()));
    let report = denoiser.partition_report()?;
    println!("DRUNet partition report: {report:?}");
    #[cfg(target_os = "macos")]
    assert!(
        report
            .nodes
            .iter()
            .any(|n| n.provider == "CoreMLExecutionProvider")
    );
    Ok(())
}
