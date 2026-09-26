//! Opt-in performance probes, not ordinary CI. Set TESSERA_FILTER_MODEL_CACHE.
use anyhow::{Context, Result};
use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::{id::ModelRef, tile::Extent};
use ml_filters::{
    Cancel, Colorize, JpegArtifactRemoval, NeuralFilter, Params, PhotoRestoration, SkinSmoothing,
};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::time::Instant;

fn frame() -> Result<Raster> {
    let mut r = Raster::new(Extent::new(4000, 3000), 4, Depth::U8, 0.0);
    r.edit_region(Rect::new(0, 0, 4000, 3000), 1, |x, y, p| {
        let v = ((x + y) % 127) as f32 / 1270.0;
        *p = [0.65 + v, 0.4 + v, 0.3 + v, 1.0];
    })?;
    Ok(r)
}
fn registry(id: &str) -> Result<Option<ModelRegistry>> {
    let Some(cache) = std::env::var_os("TESSERA_FILTER_MODEL_CACHE") else {
        eprintln!("SKIP bench: no model cache");
        return Ok(None);
    };
    let r = ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        cache,
    )?;
    let m = r
        .models()
        .iter()
        .find(|m| m.id == id)
        .context("missing model entry")?;
    if r.resolve_cached_ref(&ModelRef {
        id: id.into(),
        version: m.version.clone(),
    })?
    .is_none()
    {
        eprintln!("SKIP bench: {id} absent");
        return Ok(None);
    }
    Ok(Some(r))
}
fn measure(filter: &dyn NeuralFilter, p: Params) -> Result<()> {
    let r = frame()?;
    let start = Instant::now();
    let out = filter.apply(&r, &p, &Cancel::new())?;
    assert_eq!(out.extent(), r.extent());
    println!(
        "{} 12 MP apply {:.3}s (load excluded)",
        filter.name(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
#[test]
#[ignore = "12 MP CPU frequency-separation benchmark; no neural model"]
fn skin_12mp() -> Result<()> {
    measure(
        &SkinSmoothing,
        Params {
            faces: vec![[1000.0, 500.0, 1800.0, 2000.0]],
            ..Params::default()
        },
    )
}
#[test]
#[ignore = "12 MP CPU-only DDColor benchmark; CoreML graph is unsupported"]
fn colorize_12mp_cpu() -> Result<()> {
    let Some(r) = registry("filters/ddcolor")? else {
        return Ok(());
    };
    let f = Colorize::load(&r, SessionOptions::default())?;
    measure(&f, Params::default())?;
    let report = f.partition_report()?;
    assert!(!report.nodes.is_empty());
    assert!(
        report
            .nodes
            .iter()
            .all(|n| n.provider == "CPUExecutionProvider")
    );
    assert!(report.require_coreml().is_err());
    println!(
        "DDColor: CPUExecutionProvider only ({} executed nodes); strict CoreML guard still rejects CPU",
        report.nodes.len()
    );
    Ok(())
}
#[test]
#[ignore = "12 MP CoreML partition-audited DRUNet benchmark; may be slow"]
fn jpeg_12mp_coreml() -> Result<()> {
    let Some(r) = registry(ml_enhance::DENOISE_MODEL_ID)? else {
        return Ok(());
    };
    let f = JpegArtifactRemoval::load(&r, SessionOptions::default())?;
    measure(&f, Params::default())?;
    let report = f.partition_report()?;
    println!("DRUNet partitions: {report:?}");
    report.require_coreml()
}
#[test]
#[ignore = "12 MP denoise-only restoration benchmark; GFPGAN blocked"]
fn restoration_denoise_12mp_coreml() -> Result<()> {
    let Some(r) = registry(ml_enhance::DENOISE_MODEL_ID)? else {
        return Ok(());
    };
    let f = PhotoRestoration::load(&r, SessionOptions::default())?;
    measure(&f, Params::default())?;
    let report = f.partition_report()?;
    println!("DRUNet partitions: {report:?}");
    report.require_coreml()
}
