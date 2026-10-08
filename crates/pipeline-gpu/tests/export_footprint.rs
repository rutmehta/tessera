//! ENG-13: an export band's true device footprint (recycled buffers taken
//! in, plus fresh allocations, less buffers released) and its readback stay
//! within the scratch its renderer is given, also when consecutive bands of
//! one worker change shape and recycle each other's buffers.

use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings, tile::Extent};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{ExportResize, GpuContext, GpuManagedOutput, GpuStats, ManagedRenderer};
use std::sync::Arc;

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

const WIDTH: u32 = 2048;
const HEIGHT: u32 = 1024;

fn settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings
}

fn output(settings: &DevelopSettings) -> Arc<GpuManagedOutput> {
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    )
}

fn config() -> image_core::RendererConfig {
    image_core::RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    }
}

/// A band's true footprint: the larger of its peak and its footprint at
/// readback plus the readback staging copy.
fn footprint(stats: &GpuStats, readback: u64) -> u64 {
    (stats.last_resident_live_bytes + readback).max(stats.last_resident_peak_bytes)
}

/// Renders output rows `top..top + rows` with `renderer`: returns whether the
/// band renderer took it, and the readback size.
fn band(
    renderer: &ManagedRenderer,
    image: &image_core::RawImage,
    resize: Option<ExportResize>,
    top: u32,
    rows: u32,
) -> (bool, u64) {
    let (source, width) = match resize {
        Some(r) => {
            let rect = r.support_rect().unwrap();
            (rect.y..rect.y + rect.height, r.destination.width)
        }
        None => (top..top + rows, WIDTH),
    };
    let mut dst = vec![0f32; (rows * width * 3) as usize];
    let ok = renderer
        .render_export_rows(
            image,
            &settings(),
            0,
            source,
            None,
            &mut dst,
            &CancellationToken::new(),
        )
        .unwrap();
    assert!(ok, "band renderer declined rows {top}+{rows}");
    (ok, u64::from(rows) * u64::from(width) * 12)
}

/// Bands of one worker that change shape: tall, tall, short, medium, tall,
/// tiny, ... (output rows), covering `height`.
fn schedule(height: u32) -> Vec<(u32, u32)> {
    let pattern = [256, 256, 48, 128, 256, 16, 200, 64];
    let mut bands = Vec::new();
    let mut top = 0;
    for rows in pattern.into_iter().cycle() {
        if top >= height {
            break;
        }
        let rows = rows.min(height - top);
        bands.push((top, rows));
        top += rows;
    }
    bands
}

fn check(destination: Option<Extent>) {
    let settings = settings();
    let output = output(&settings);
    let image = common::synthetic(1301, WIDTH, HEIGHT, common::RGGB, [0, 0, WIDTH, HEIGHT]);
    let frame = image.active_extent();
    assert_eq!(frame, Extent::new(WIDTH, HEIGHT));
    let height = destination.map_or(HEIGHT, |d| d.height);
    let resize = |top, rows| {
        destination.map(|destination| ExportResize {
            source: frame,
            destination,
            top,
            rows,
        })
    };
    let bands = schedule(height);
    // Each band alone, without recycling, under an unconstrained scratch.
    let roomy = ManagedRenderer::new_export_budgeted(output.clone(), config(), None, 1 << 30);
    let mut need = 0;
    for &(top, rows) in &bands {
        let renderer = roomy.export_band(resize(top, rows));
        let (_, readback) = band(&renderer, &image, resize(top, rows), top, rows);
        let stats = renderer.stats();
        assert_eq!(stats.last_resident_recycled_bytes, 0);
        need = need.max(footprint(&stats, readback));
    }
    // The scratch fits the largest band alone, with 1/8 to spare: one
    // worker's bands, recycling each other's buffers, must stay within it.
    let scratch = need + need / 8;
    let base = ManagedRenderer::new_export_budgeted(output, config(), None, scratch);
    let worker = base.export_band(None);
    let mut recycled = 0;
    let mut over = Vec::new();
    for &(top, rows) in &bands {
        let renderer = worker.export_band_recycling(resize(top, rows));
        let (_, readback) = band(&renderer, &image, resize(top, rows), top, rows);
        let stats = renderer.stats();
        recycled += stats.last_resident_recycled_bytes;
        let used = footprint(&stats, readback);
        eprintln!(
            "FOOTPRINT {:?} rows {top}+{rows}: live {} + readback {readback} (peak {}, recycled {}, counted {}) of {scratch}",
            destination,
            stats.last_resident_live_bytes,
            stats.last_resident_peak_bytes,
            stats.last_resident_recycled_bytes,
            stats.last_resident_allocated_bytes,
        );
        if used > scratch {
            over.push(format!("rows {top}+{rows}: {used} B > {scratch} B"));
        }
    }
    assert!(recycled > 0, "bands must exercise recycling");
    assert!(over.is_empty(), "{destination:?}: {over:?}");
}

#[test]
fn shape_changing_bands_stay_within_their_scratch() {
    check(None);
}

#[test]
fn shape_changing_resized_bands_stay_within_their_scratch() {
    check(Some(Extent::new(1600, 800)));
}
