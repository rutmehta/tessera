//! Measured develop-backend selection. Explicit overrides skip calibration.

use engine_api::{
    EngineResult,
    recipe::{DevelopSettings, settings::WhiteBalanceMode},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
use std::{sync::Arc, time::Instant};

fn gpu_is_faster(cpu: [f64; 3], gpu: [f64; 3]) -> bool {
    cpu.iter().chain(&gpu).all(|v| v.is_finite() && *v > 0.) && gpu[1] < cpu[1] && gpu[2] < cpu[2]
}
fn measure(renderer: &Renderer, image: &RawImage) -> EngineResult<[f64; 3]> {
    let mut s = DevelopSettings::default();
    let extent = image.level_extent(2);
    let rect = PixelRect::full(extent);
    // Match the actual develop sink on both backends. Timing GPU pixel
    // readback here can reject a faster zero-copy presentation path.
    // Allocation is not a per-frame cost and stays outside the timer.
    let surface = crate::surface::Surface::create_rgba8(extent.width, extent.height)
        .map_err(engine_api::EngineError::internal)?;
    let cancel = engine_api::jobs::CancellationToken::new();
    let mut times = [0.; 3];
    for (i, time) in times.iter_mut().enumerate() {
        if i == 1 {
            s.tone.exposure = 0.25;
        }
        if i == 2 {
            s.white_balance.mode = WhiteBalanceMode::Daylight;
        }
        let start = Instant::now();
        let histogram = match renderer.render_surface(image, &s, 2, surface.id(), &cancel)? {
            Some(histogram) => histogram,
            None => {
                let tiles = renderer.render_region(image, &s, 2, rect)?;
                crate::develop::write_level(Some(&surface), &tiles)
                    .map_err(engine_api::EngineError::internal)?
            }
        };
        std::hint::black_box(histogram);
        *time = start.elapsed().as_secs_f64() * 1000.;
    }
    Ok(times)
}
pub(crate) fn select(image: &RawImage) -> (Arc<Renderer>, String) {
    let config = RendererConfig::default();
    let cpu = Arc::new(Renderer::new(config.clone()));
    let cpu_name = format!("CPU ×{}", config.threads);
    let preference = std::env::var("TESSERA_RENDER_BACKEND").unwrap_or_default();
    if preference.eq_ignore_ascii_case("cpu") {
        return (cpu, cpu_name);
    }
    let ctx = match pipeline_gpu::GpuContext::new() {
        Ok(ctx) => Arc::new(ctx),
        Err(e) => {
            eprintln!("develop: Metal unavailable, using CPU: {e}");
            return (cpu, cpu_name);
        }
    };
    let name = format!("Metal ({})", ctx.adapter_info.name);
    let ops = Arc::new(pipeline_gpu::GpuStageOp::with_cache_budget(
        ctx,
        config.cache_budget_bytes,
    ));
    let gpu = Arc::new(Renderer::with_ops(
        ops,
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    ));
    if preference.eq_ignore_ascii_case("gpu") {
        return (gpu, name);
    }
    match (measure(&cpu, image), measure(&gpu, image)) {
        (Ok(c), Ok(g)) => {
            eprintln!("develop L2 calibration [first, tone, WB] ms: CPU {c:?}; GPU {g:?}");
            if gpu_is_faster(c, g) {
                return (gpu, name);
            }
        }
        (_, Err(e)) => eprintln!("develop: GPU calibration failed, using CPU: {e}"),
        (Err(e), _) => eprintln!("develop: CPU calibration failed, retaining CPU: {e}"),
    }
    (cpu, cpu_name)
}

#[cfg(test)]
#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_os = "macos")]
    fn calibration_measures_surface_presentation_without_pixel_readback() {
        use super::*;
        let image = common::synthetic(401, 269, 261, common::RGGB, [3, 5, 263, 251]);
        let ops = Arc::new(pipeline_gpu::GpuStageOp::new(Arc::new(
            pipeline_gpu::GpuContext::new().unwrap(),
        )));
        let renderer = Renderer::with_ops(
            ops.clone(),
            Arc::new(TileCache::new(16 << 20)),
            RendererConfig::default(),
        );
        let times = measure(&renderer, &image).unwrap();
        assert!(times.iter().all(|t| t.is_finite() && *t > 0.));
        assert_eq!(ops.stats().pixel_readback_bytes, 0);
        assert_eq!(ops.stats().histogram_readbacks, 3);
        assert_eq!(ops.stats().submissions, 3);
        let cpu = Renderer::new(RendererConfig::default());
        let times = measure(&cpu, &image).unwrap();
        assert!(times.iter().all(|t| t.is_finite() && *t > 0.));
    }

    #[test]
    fn selection_requires_measured_gpu_advantage() {
        // Interactive edits recur for the entire session. Do not reject the
        // faster interactive backend because its one-time cold fill costs more.
        assert!(super::gpu_is_faster([230., 9., 74.], [335., 6., 11.]));
        assert!(super::gpu_is_faster([10., 12., 40.], [9., 4., 20.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [9., 4., 80.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [10., 12., 40.]));
        assert!(!super::gpu_is_faster([10., 12., 40.], [f64::NAN, 4., 20.]));
    }
}
