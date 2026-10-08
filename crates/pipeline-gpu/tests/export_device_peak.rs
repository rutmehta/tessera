//! ENG-14: the Metal device's own allocation counter, not the export meter,
//! must stay within the scratch export band workers are given. Two workers
//! render bands concurrently, as export does, each recycling its own
//! buffers under a scratch share, while another thread samples
//! `MTLDevice.currentAllocatedSize`. Every byte a band transaction makes
//! the device hold (payloads, wgpu's staging copies of uploads and
//! parameter arenas, parameters, the readback staging copy) must be
//! inside the shares.
//!
//! The counter is process-wide, so this file holds exactly one test (cargo
//! runs test binaries one at a time).

use color_mgmt::{Builtin, Registry, TransformOptions};
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{GpuContext, GpuManagedOutput, ManagedRenderer};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

const WIDTH: u32 = 3072;
const HEIGHT: u32 = 2048;
const WORKERS: usize = 2;

fn settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings.effects.vignette.amount = -40.;
    settings.effects.grain.amount = 30.;
    settings
}

fn config() -> image_core::RendererConfig {
    image_core::RendererConfig {
        cache_budget_bytes: 0,
        ..Default::default()
    }
}

/// Bands of varying height covering the frame, in export's order.
fn bands() -> Vec<(u32, u32)> {
    let mut bands = Vec::new();
    let mut top = 0;
    for rows in [192u32, 192, 64, 128, 192, 32, 160].into_iter().cycle() {
        if top >= HEIGHT {
            break;
        }
        let rows = rows.min(HEIGHT - top);
        bands.push((top, rows));
        top += rows;
    }
    bands
}

/// Renders every band on `WORKERS` threads, each with its own recycling
/// worker renderer from `base` (as export's band workers do).
fn export(base: &ManagedRenderer, image: &image_core::RawImage, settings: &DevelopSettings) {
    let queue = Mutex::new(bands().into_iter());
    std::thread::scope(|scope| {
        for _ in 0..WORKERS {
            scope.spawn(|| {
                let worker = base.export_band(None);
                loop {
                    let Some((top, rows)) = queue.lock().unwrap().next() else {
                        return;
                    };
                    let renderer = worker.export_band_recycling(None);
                    let mut dst = vec![0f32; (rows * WIDTH * 3) as usize];
                    assert!(
                        renderer
                            .render_export_rows(
                                image,
                                settings,
                                0,
                                top..top + rows,
                                None,
                                &mut dst,
                                &CancellationToken::new(),
                            )
                            .unwrap(),
                        "band renderer declined rows {top}+{rows}"
                    );
                }
            });
        }
    });
}

#[test]
fn concurrent_export_bands_stay_within_their_shares_on_the_device() {
    let settings = settings();
    let context = Arc::new(GpuContext::new().unwrap());
    if context.device_allocated_bytes().is_none() {
        panic!("Metal allocation counter unavailable");
    }
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let output = Arc::new(
        GpuManagedOutput::new(
            context.clone(),
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
    let image = common::synthetic(1403, WIDTH, HEIGHT, common::RGGB, [0, 0, WIDTH, HEIGHT]);
    // The largest band's own footprint (counted by the meter), alone.
    let roomy = ManagedRenderer::new_export_budgeted(output.clone(), config(), None, 1 << 30);
    let mut need = 0;
    for (top, rows) in bands() {
        let renderer = roomy.export_band(None);
        let mut dst = vec![0f32; (rows * WIDTH * 3) as usize];
        renderer
            .render_export_rows(
                &image,
                &settings,
                0,
                top..top + rows,
                None,
                &mut dst,
                &CancellationToken::new(),
            )
            .unwrap();
        let stats = renderer.stats();
        need = need.max(
            stats
                .last_resident_peak_bytes
                .max(stats.last_resident_live_bytes + u64::from(rows * WIDTH * 12)),
        );
    }
    drop(roomy);
    let share = need + need / 64;
    let base = ManagedRenderer::new_export_budgeted(output, config(), None, share);
    // Warm-up: pipelines compile on first use and stay with the context.
    export(&base, &image, &settings);
    let baseline = context.idle_device_allocated_bytes().unwrap();
    let (done, peak) = (AtomicBool::new(false), AtomicU64::new(baseline));
    std::thread::scope(|scope| {
        scope.spawn(|| {
            while !done.load(Ordering::Acquire) {
                peak.fetch_max(context.device_allocated_bytes().unwrap(), Ordering::Relaxed);
                std::thread::sleep(std::time::Duration::from_micros(300));
            }
        });
        export(&base, &image, &settings);
        done.store(true, Ordering::Release);
    });
    let peak = peak.into_inner() - baseline;
    let mib = |b: u64| b as f64 / (1 << 20) as f64;
    eprintln!(
        "DEVICE_PEAK synthetic {WIDTH}x{HEIGHT} workers={WORKERS}: {:.1} MiB, shares {:.1} MiB (band need {:.1} MiB)",
        mib(peak),
        mib(share * WORKERS as u64),
        mib(need)
    );
    assert!(
        peak <= share * WORKERS as u64,
        "device peak {:.1} MiB > {WORKERS} x {:.1} MiB",
        mib(peak),
        mib(share)
    );
}
