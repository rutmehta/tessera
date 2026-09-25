use ml_runtime::{Session, SessionOptions, Tensor};

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/data")
        .join(name)
}

#[test]
fn fp16_matches_reference() -> anyhow::Result<()> {
    let input = Tensor::new(
        3,
        64,
        64,
        (0..12288).map(|i| (i % 31) as f32 / 31. - 0.5).collect(),
    )?;
    for options in [SessionOptions::cpu(), SessionOptions::default()] {
        let mut session = Session::load(fixture("conv-fp16.onnx"), options)?;
        let output = session.run(&input)?;
        for (a, b) in output.data().iter().zip(reference(&input)) {
            assert!((a - b).abs() < 1e-2);
        }
    }
    Ok(())
}

#[test]
fn cpu_conv_matches_reference() -> anyhow::Result<()> {
    let mut session = Session::load(fixture("conv.onnx"), SessionOptions::cpu())?;
    let input = Tensor::new(
        3,
        64,
        64,
        (0..3 * 64 * 64)
            .map(|i| (i % 31) as f32 / 31.0 - 0.5)
            .collect(),
    )?;
    let output = session.run(&input)?;
    let reference = reference(&input);
    for (a, b) in output.data().iter().zip(reference) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }
    let report = session.partition_report()?;
    assert!(!report.nodes.is_empty());
    assert!(
        report
            .nodes
            .iter()
            .all(|n| n.provider == "CPUExecutionProvider")
    );
    Ok(())
}

fn reference(input: &Tensor) -> Vec<f32> {
    let [_, _, h, w] = input.shape();
    let mut out = vec![0.; 3 * h * w];
    for c in 0..3 {
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                for ic in 0..3 {
                    for ky in 0..3 {
                        for kx in 0..3 {
                            let iy = y as isize + ky as isize - 1;
                            let ix = x as isize + kx as isize - 1;
                            if iy >= 0 && ix >= 0 && iy < h as isize && ix < w as isize {
                                let weight =
                                    (((c * 27 + ic * 9 + ky * 3 + kx) % 7) as f32 - 3.) / 64.;
                                sum += input.data()[ic * h * w + iy as usize * w + ix as usize]
                                    * weight;
                            }
                        }
                    }
                }
                out[c * h * w + y * w + x] = sum.max(0.);
            }
        }
    }
    out
}

#[test]
fn tiled_matches_whole_image() -> anyhow::Result<()> {
    let mut session = Session::load(fixture("conv-dynamic.onnx"), SessionOptions::cpu())?;
    let input = Tensor::new(
        3,
        300,
        300,
        (0..270000).map(|i| (i % 97) as f32 / 97. - 0.5).collect(),
    )?;
    let whole = session.run(&input)?;
    let tiled = session.run_tiled(&input, 64, 1)?;
    assert_eq!(whole.shape(), tiled.shape());
    for (a, b) in whole.data().iter().zip(tiled.data()) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }
    assert!(session.run_tiled(&input, 0, 1).is_err());
    Ok(())
}

#[cfg(target_os = "macos")]
#[test]
fn coreml_runs_and_reports_real_nodes() -> anyhow::Result<()> {
    let mut session = Session::load(fixture("conv.onnx"), SessionOptions::default())?;
    assert!(
        session.fallback_reason.is_none(),
        "{:?}",
        session.fallback_reason
    );
    let input = Tensor::new(
        3,
        64,
        64,
        (0..12288).map(|i| (i % 31) as f32 / 31. - 0.5).collect(),
    )?;
    let output = session.run(&input)?;
    for (a, b) in output.data().iter().zip(reference(&input)) {
        assert!((a - b).abs() < 1e-4, "{a} != {b}");
    }
    let report = session.partition_report()?;
    println!("CoreML partition: {report:?}");
    report.require_coreml()?;
    Ok(())
}
