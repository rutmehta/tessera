//! Diagnostic for the pinned LaMa export: path, CPU threads, compute units.
use ml_runtime::{ComputeUnits, Session, SessionOptions, TensorInput};
fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    anyhow::ensure!(
        args.len() == 4,
        "usage: lama_profile MODEL THREADS all|gpu|cpu"
    );
    let options = SessionOptions {
        coreml: args[3] != "cpu",
        compute_units: if args[3] == "gpu" {
            ComputeUnits::CPUAndGPU
        } else {
            ComputeUnits::All
        },
        ..SessionOptions::default()
    };
    let mut model = Session::load_with_dimensions_and_threads(
        &args[1],
        options,
        &[("batch", 1)],
        args[2].parse()?,
    )?;
    let image = TensorInput::F32 {
        shape: vec![1, 3, 512, 512],
        data: vec![0.5; 3 * 512 * 512],
    };
    let mut mask = vec![0.0; 512 * 512];
    for y in 192..320 {
        mask[y * 512 + 192..y * 512 + 320].fill(1.0);
    }
    let mask = TensorInput::F32 {
        shape: vec![1, 1, 512, 512],
        data: mask,
    };
    let inputs = [("image", image), ("mask", mask)];
    for i in 0..3 {
        let now = std::time::Instant::now();
        let outputs = model.run_tensors(&inputs)?;
        println!(
            "{} threads={} run={i} elapsed={:?} shape={:?}",
            args[3],
            args[2],
            now.elapsed(),
            outputs[0].shape
        );
    }
    let report = model.partition_report()?;
    let mut providers = std::collections::BTreeMap::new();
    for n in &report.nodes {
        *providers.entry(&n.provider).or_insert(0) += 1;
    }
    println!("{providers:?}");
    println!("fallback={:?}", model.fallback_reason);
    Ok(())
}
