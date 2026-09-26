//! Opt-in, cache-only integration: no downloaded/synthetic substitute model.
use anyhow::{Context, Result};
use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::{id::ModelRef, tile::Extent};
use ml_filters::{Cancel, ColorHint, Colorize, NeuralFilter, Params};
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::PathBuf;

#[test]
fn real_cached_ddcolor_cpu() -> Result<()> {
    let Some(cache) = std::env::var_os("TESSERA_FILTER_MODEL_CACHE") else {
        eprintln!("SKIP DDColor: set TESSERA_FILTER_MODEL_CACHE to a SHA-addressed model cache");
        return Ok(());
    };
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let registry = ModelRegistry::open(root.join("../ml-runtime/models.toml"), cache)?;
    let spec = registry
        .models()
        .iter()
        .find(|m| m.id == "filters/ddcolor")
        .context("missing verified DDColor entry")?;
    let model = ModelRef {
        id: spec.id.as_str().into(),
        version: spec.version.clone(),
    };
    if registry.resolve_cached_ref(&model)?.is_none() {
        eprintln!("SKIP DDColor: model absent from requested cache");
        return Ok(());
    }
    // Corrupt/unreadable cached bytes propagate an error above, never a skip.
    // DDColor opts into CPU-only even when the caller normally prefers CoreML.
    let filter = Colorize::load(
        &registry,
        SessionOptions {
            coreml: true,
            ..SessionOptions::default()
        },
    )?;
    assert!(filter.requires_weights());
    assert!(
        filter.partition_report().is_err(),
        "no partition report before inference"
    );
    let mut src = Raster::new(Extent::new(37, 19), 4, Depth::F32, 0.0);
    src.edit_region(Rect::new(0, 0, 37, 19), 1, |x, y, p| {
        let g = 0.15 + 0.7 * x as f32 / 36.0;
        *p = [g, g, g, y as f32 / 18.0];
    })?;
    let p = Params {
        artifact_reduction: 0.6,
        hints: vec![ColorHint {
            position: [18.0, 9.0],
            rgb: [0.6, 0.4, 0.4],
            radius: 4.0,
            strength: 0.8,
        }],
        ..Params::default()
    };
    let out = filter.apply(&src, &p, &Cancel::new())?;
    assert_eq!(out.extent(), src.extent());
    let mut chromatic = false;
    for y in 0..19 {
        for x in 0..37 {
            let px = out.pixel(x, y);
            assert!(px.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
            assert_eq!(px[3], src.pixel(x, y)[3]);
            chromatic |= (px[0] - px[2]).abs() > 0.01;
        }
    }
    assert!(chromatic, "real model/hint path should produce chroma");
    let report = filter.partition_report()?;
    assert!(!report.nodes.is_empty());
    assert!(
        report
            .nodes
            .iter()
            .all(|n| n.provider == "CPUExecutionProvider")
    );
    assert!(report.require_coreml().is_err());
    let cancel = Cancel::new();
    cancel.cancel();
    assert!(filter.apply(&src, &p, &cancel).is_err());
    assert!(
        filter
            .apply(
                &src,
                &Params {
                    saturation: f32::NAN,
                    ..p
                },
                &Cancel::new()
            )
            .is_err()
    );
    Ok(())
}
