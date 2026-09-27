//! Metrics must count the full-resolution encoded output, not a resized image.
#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use engine_api::{jobs::CancellationToken, recipe::DevelopSettings};
use image_core::{PixelRect, Renderer, RendererConfig, TileCache, resident::OutputMetrics};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn reduction_counts_channel_endpoints_unions_and_partial_bands() {
    use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
    use image_core::StageOp;
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    for colour in [[255u8, 0, 64], [0, 0, 0], [255, 255, 255], [1, 254, 100]] {
        let mut batch = gpu.begin_resident().unwrap();
        assert!(batch.enable_metrics());
        let mut tiles = Vec::new();
        for (width, height) in [(253, 65), (7, 1)] {
            let layout = TileLayout {
                extent: Extent::new(width, height),
                halo: 0,
                channels: 3,
            };
            let input = Tile::from_samples(
                TileCoord::new(0, 0, 0),
                layout,
                colour
                    .iter()
                    .flat_map(|v| vec![f32::from(*v); (width * height) as usize])
                    .collect(),
            )
            .unwrap();
            tiles.push(batch.upload(&input).unwrap());
        }
        let output = batch
            .finish(tiles, true, None, &CancellationToken::new())
            .unwrap();
        assert!(output.tiles.is_empty());
        let m = output.metrics.unwrap();
        assert_eq!(m.pixels, 253 * 65 + 7);
        assert_eq!(
            m.clipped_shadows,
            if colour.contains(&0) { m.pixels } else { 0 }
        );
        assert_eq!(
            m.clipped_highlights,
            if colour.contains(&255) { m.pixels } else { 0 }
        );
        for (c, v) in colour.iter().enumerate() {
            assert_eq!(m.histogram[c][*v as usize], m.pixels);
            assert_eq!(m.histogram[c].iter().sum::<u64>(), m.pixels);
        }
    }
}

#[test]
fn full_resolution_reduction_matches_cpu_counting_and_reuses_stages() {
    let gpu = Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap())));
    let config = RendererConfig::default();
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    let image = common::synthetic(1414, 530, 291, common::RGGB, [0, 0, 530, 291]);
    let mut settings = DevelopSettings::default();
    for exposure in [0., 2., -3.] {
        settings.tone.exposure = exposure;
        let tiles = renderer
            .render_region(&image, &settings, 0, PixelRect::full(image.active_extent()))
            .unwrap();
        let mut cpu = OutputMetrics::default();
        for tile in tiles {
            let l = tile.layout();
            let samples = tile.samples::<u8>().unwrap();
            for y in 0..l.extent.height {
                for x in 0..l.extent.width {
                    cpu.add_pixel(std::array::from_fn(|c| {
                        samples[l.index(c as u8, x as i32, y as i32).unwrap()]
                    }));
                }
            }
        }
        let reduced = renderer
            .render_output_metrics(&image, &settings, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(reduced.histogram, cpu.histogram);
        assert_eq!(reduced.clipped_shadows, cpu.clipped_shadows);
        assert_eq!(reduced.clipped_highlights, cpu.clipped_highlights);
        assert_eq!(reduced.pixels, cpu.pixels);
        assert!((reduced.mean_luminance() - cpu.mean_luminance()).abs() < 1e-4);
        let before = gpu.stats();
        let again = renderer
            .render_output_metrics(&image, &settings, &CancellationToken::new())
            .unwrap()
            .unwrap();
        assert_eq!(again.histogram, reduced.histogram);
        assert_eq!(
            gpu.stats().last_resident_dispatches,
            1,
            "warm output needs only one reduction"
        );
        assert_eq!(gpu.stats().fused_dispatches, before.fused_dispatches);
        assert_eq!(gpu.stats().submissions - before.submissions, 1);
    }
}

#[test]
fn composed_optics_metrics_match_rendered_pixels_without_poisoning_wb_cache() {
    use engine_api::recipe::settings::{GuideLine, LensProfileSource, NormalizedRect, UprightMode};
    let gpu = Arc::new(GpuStageOp::new(Arc::new(
        GpuContext::new().expect("Metal adapter required"),
    )));
    let config = RendererConfig::default();
    let renderer = Renderer::with_ops(
        gpu,
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    let image = common::synthetic(1449, 320, 280, common::RGGB, [0, 0, 320, 280]);
    let mut settings = DevelopSettings::default();
    settings.lens.profile = LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings.lens.manual_distortion = 18.;
    settings.geometry.upright.mode = UprightMode::Guided;
    settings.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.1],
            end: [0.3, 0.9],
        },
        GuideLine {
            start: [0.8, 0.1],
            end: [0.7, 0.9],
        },
    ];
    settings.geometry.transform.rotate = 2.;
    settings.geometry.crop.rect = NormalizedRect {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    };
    for vignette in [35., -15.] {
        settings.lens.manual_vignetting = vignette;
        // Metrics runs first: its WB checkpoint must contain the same optics
        // as the subsequent display request, including after a lens edit.
        let reduced = renderer
            .render_output_metrics(&image, &settings, &CancellationToken::new())
            .unwrap()
            .unwrap();
        let extent = Renderer::output_extent(&image, &settings, 0).unwrap();
        let tiles = renderer
            .render_region(&image, &settings, 0, PixelRect::full(extent))
            .unwrap();
        let mut expected = OutputMetrics::default();
        for tile in tiles {
            let layout = tile.layout();
            let samples = tile.samples::<u8>().unwrap();
            for y in 0..layout.extent.height {
                for x in 0..layout.extent.width {
                    expected.add_pixel(std::array::from_fn(|c| {
                        samples[layout.index(c as u8, x as i32, y as i32).unwrap()]
                    }));
                }
            }
        }
        assert_eq!(reduced.pixels, extent.area());
        assert_eq!(reduced.histogram, expected.histogram);
        assert_eq!(reduced.clipped_shadows, expected.clipped_shadows);
        assert_eq!(reduced.clipped_highlights, expected.clipped_highlights);
        assert!((reduced.mean_luminance() - expected.mean_luminance()).abs() < 1e-4);
    }
}
