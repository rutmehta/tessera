//! ENG-15 (REV-ENG-14 SHOULD-FIX 1): consecutive export bands of one worker
//! differ by a row or so (the band planner's bands map to 246 or 247 sensor
//! rows on the CR3). An exact size match then misses almost every recycled
//! buffer, so each band allocates its whole scratch afresh (17 of 17
//! buffers, ~150 MiB, zero-filled by wgpu): that, more than the band count,
//! is what made CR3 Web exports slower. Bands one row apart must recycle
//! like bands of one height, and recycled larger buffers must not change a
//! single sample.

use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::Arc;

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

const WIDTH: u32 = 2048;
const HEIGHT: u32 = 1536;

fn settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings.effects.vignette.amount = -30.;
    settings
}

fn renderer(scratch: u64) -> ManagedRenderer {
    let settings = settings();
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let output = Arc::new(
        GpuManagedOutput::new(
            Arc::new(GpuContext::new().unwrap()),
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Export(&target),
                proof: None,
                options: TransformOptions::default(),
            },
        )
        .unwrap(),
    );
    ManagedRenderer::new_export_budgeted(
        output,
        image_core::RendererConfig {
            cache_budget_bytes: 0,
            ..Default::default()
        },
        None,
        scratch,
    )
}

/// One worker renders `bands` in order, recycling its buffers: each band's
/// pixels, fresh allocation count and footprint (peak, or live plus the
/// readback staging copy).
fn render(
    base: &ManagedRenderer,
    image: &image_core::RawImage,
    bands: &[(u32, u32)],
) -> Vec<(Vec<f32>, u64, u64)> {
    let worker = base.export_band(None);
    bands
        .iter()
        .map(|&(top, rows)| {
            let renderer = worker.export_band_recycling(None);
            let mut dst = vec![0f32; (rows * WIDTH * 3) as usize];
            assert!(
                renderer
                    .render_export_rows(
                        image,
                        &settings(),
                        0,
                        top..top + rows,
                        None,
                        &mut dst,
                        &CancellationToken::new(),
                    )
                    .unwrap(),
                "band renderer declined rows {top}+{rows}"
            );
            let stats = renderer.stats();
            let footprint = stats
                .last_resident_peak_bytes
                .max(stats.last_resident_live_bytes + u64::from(rows * WIDTH * 12));
            (dst, stats.last_resident_buffers, footprint)
        })
        .collect()
}

fn bands(heights: &[u32]) -> Vec<(u32, u32)> {
    let mut top = 0;
    heights
        .iter()
        .cycle()
        .map_while(|&rows| {
            if top >= HEIGHT {
                return None;
            }
            let rows = rows.min(HEIGHT - top);
            top += rows;
            Some((top - rows, rows))
        })
        .collect()
}

#[test]
fn bands_a_row_apart_recycle_like_bands_of_one_height() {
    let image = common::synthetic(1501, WIDTH, HEIGHT, common::RGGB, [0, 0, WIDTH, HEIGHT]);
    // Each band alone: the scratch fits the largest with 1/32 to spare, so
    // a worker keeps only about one band's buffers, as export's workers do
    // (a roomy scratch would keep both heights' buffers and hide misses).
    let need = bands(&[129])
        .iter()
        .map(|&band| render(&renderer(1 << 30), &image, &[band])[0].2)
        .max()
        .unwrap();
    let scratch = need + need / 32;
    let base = renderer(scratch);
    // Fresh allocations of the interior bands (the first two fill the pool;
    // the first and last bands, at the frame's edges, have other shapes).
    let interior = |bands: &[(Vec<f32>, u64, u64)]| -> Vec<u64> {
        bands[2..bands.len() - 1].iter().map(|b| b.1).collect()
    };
    // Steady state of one height: what a band cannot recycle (its uploads).
    let uniform = render(&base, &image, &bands(&[128]));
    let floor = interior(&uniform).into_iter().max().unwrap();
    eprintln!(
        "RECYCLE one height: fresh buffers per band {:?}",
        uniform.iter().map(|b| b.1).collect::<Vec<_>>()
    );
    assert!(floor < uniform[0].1, "one height must recycle");
    // The planner's case: heights a row apart, larger first or smaller first.
    for heights in [[129, 128], [128, 129]] {
        let plan = bands(&heights);
        let recycled = render(&base, &image, &plan);
        eprintln!(
            "RECYCLE {heights:?}: fresh buffers per band {:?} (one height: {floor})",
            recycled.iter().map(|b| b.1).collect::<Vec<_>>()
        );
        let fresh = interior(&recycled);
        assert!(
            fresh.iter().all(|&n| n <= floor),
            "{heights:?}: bands a row apart allocate afresh: {fresh:?} (one height: {floor})"
        );
        // Within the scratch, and recycled (possibly larger) buffers change
        // no sample.
        for (&(top, rows), (pixels, _, footprint)) in plan.iter().zip(&recycled) {
            assert!(
                *footprint <= scratch,
                "rows {top}+{rows}: {footprint} B > {scratch} B"
            );
            let alone = render(&renderer(1 << 30), &image, &[(top, rows)]);
            assert!(
                alone[0]
                    .0
                    .iter()
                    .zip(pixels)
                    .all(|(a, b)| a.to_bits() == b.to_bits()),
                "rows {top}+{rows} differ from a band rendered alone"
            );
        }
    }
}
