mod common;

use std::collections::BTreeSet;

use common::{RGGB, assemble_f32, metadata, samples};
use engine_api::recipe::settings::{DemosaicMethod, LensProfileSource};
use engine_api::recipe::{DevelopSettings, ProcessFamily, ProcessVersion};
use engine_api::stage::StageId;
use engine_api::tile::{Extent, Tile};
use image_core::{PipelineGraph, PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::{RenderSource, SigmoidSettings, display_float, render_linear_scaled};
use raw_decode::CfaImage;
use std::sync::Arc;

fn candidate_settings() -> DevelopSettings {
    let mut settings = DevelopSettings::default();
    settings.demosaic.method = DemosaicMethod::Bilinear;
    settings.demosaic.model = None;
    settings.lens.profile = LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings
}

fn candidate_renderer() -> Renderer {
    let graph = StageId::ALL
        .into_iter()
        .fold(PipelineGraph::m2(), |graph, stage| {
            graph.with_cacheable(stage, false)
        });
    Renderer::new(RendererConfig {
        cache_budget_bytes: 0,
        threads: 1,
        process_version: ProcessVersion::NATIVE_CURRENT,
        graph,
        preview_approximations: false,
    })
}

fn synthetic_bayer_with_samples(values: Vec<f32>) -> RawImage {
    const SENSOR_WIDTH: u32 = 288;
    const SENSOR_HEIGHT: u32 = 280;
    let cfa = Arc::new(CfaImage::from_linear(SENSOR_WIDTH, SENSOR_HEIGHT, values).unwrap());
    let metadata = Arc::new(metadata(
        SENSOR_WIDTH,
        SENSOR_HEIGHT,
        RGGB,
        [8, 6, 280, 276],
    ));
    RawImage::new(
        engine_api::id::ImageId(0x0050_524f_4649_4c45),
        cfa,
        metadata,
    )
    .unwrap()
}

fn synthetic_bayer() -> RawImage {
    synthetic_bayer_with_samples(samples(288, 280, RGGB))
}

fn rect(extent: Extent) -> PixelRect {
    PixelRect::full(extent)
}

fn convert_scene_tiles(
    tiles: &[Tile],
    extent: Extent,
    gamut: engine_api::recipe::settings::GamutMapping,
) -> Vec<[u16; 4]> {
    let mut pixels = vec![[0u16; 4]; extent.area() as usize];
    let mut coverage = vec![0u8; extent.area() as usize];
    for scene in tiles {
        let display = display_float(scene, SigmoidSettings::default(), gamut).unwrap();
        let layout = display.layout();
        let plane = layout.plane_len();
        let samples = display.samples::<f32>().unwrap();
        let (ox, oy) = display.coord().pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..layout.extent.height {
            for x in 0..layout.extent.width {
                let i = layout.index(0, x as i32, y as i32).unwrap();
                let px = ox + x;
                let py = oy + y;
                assert!(
                    px < extent.width && py < extent.height,
                    "tile outside active extent"
                );
                let out = (py * extent.width + px) as usize;
                coverage[out] = coverage[out]
                    .checked_add(1)
                    .expect("duplicate tile coverage");
                let rgb = [samples[i], samples[plane + i], samples[2 * plane + i]];
                let mut rgba = [0u16; 4];
                for (channel, value) in rgb.into_iter().enumerate() {
                    assert!(value.is_finite(), "non-finite display channel");
                    rgba[channel] = (value.clamp(0.0, 1.0) * 65535.0).round() as u16;
                }
                rgba[3] = u16::MAX;
                pixels[out] = rgba;
            }
        }
    }
    assert!(
        coverage.iter().all(|count| *count == 1),
        "tile coverage has gaps or overlaps"
    );
    pixels
}

#[test]
fn pinned_native2_bilinear_profile_has_float_scene_and_u16_display_contract() {
    let image = synthetic_bayer();
    assert_eq!(image.metadata().orientation, 1);
    assert_eq!(image.active_extent(), Extent::new(272, 270));

    let settings = candidate_settings();
    assert_eq!(settings.detail.sharpening.amount, 40.0);
    let renderer = candidate_renderer();
    assert_eq!(
        renderer.config().process_version.family,
        ProcessFamily::Native
    );
    assert_eq!(renderer.config().process_version.revision, 2);
    let extent = image.level_extent(0);
    let scene_tiles = renderer
        .render_region_as(
            &image,
            &settings,
            0,
            rect(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    assert!(!scene_tiles.is_empty());
    assert!(
        scene_tiles
            .iter()
            .flat_map(|tile| tile.samples::<f32>().unwrap())
            .all(|v| v.is_finite())
    );
    let got_scene = assemble_f32(extent, &scene_tiles);

    // Independent scalar CPU path from the same synthetic CFA and pinned settings.
    let source = RenderSource::Cfa {
        image: image.cfa(),
        metadata: image.metadata(),
    };
    let reference = render_linear_scaled(&settings, &source, 1).unwrap();
    let mut reference_planes = vec![vec![0.0f32; extent.area() as usize]; 3];
    let mut reference_tiles = Vec::new();
    for coord in reference.coords() {
        let tile = reference.tile(coord, 0, 1).unwrap();
        let layout = tile.layout();
        let plane = layout.plane_len();
        let data = tile.samples::<f32>().unwrap();
        let (ox, oy) = coord.pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..layout.extent.height {
            for x in 0..layout.extent.width {
                let i = layout.index(0, x as i32, y as i32).unwrap();
                let out = ((oy + y) * extent.width + ox + x) as usize;
                for channel in 0..3 {
                    reference_planes[channel][out] = data[channel * plane + i];
                }
            }
        }
        reference_tiles.push(tile);
    }
    let max_diff = got_scene
        .iter()
        .zip(&reference_planes)
        .flat_map(|(actual, expected)| actual.iter().zip(expected))
        .map(|(actual, expected)| (actual - expected).abs())
        .fold(0.0f32, f32::max);
    assert!(
        max_diff <= 1.0e-5,
        "scene-linear CPU reference diff {max_diff}"
    );

    let rgba16 = convert_scene_tiles(&scene_tiles, extent, settings.output.gamut_mapping);
    let reference_rgba16 =
        convert_scene_tiles(&reference_tiles, extent, settings.output.gamut_mapping);
    assert_eq!(rgba16.len(), extent.area() as usize);
    assert_eq!(rgba16.len(), reference_rgba16.len());
    let max_u16_diff = rgba16
        .iter()
        .zip(&reference_rgba16)
        .flat_map(|(actual, expected)| actual.iter().zip(expected))
        .map(|(actual, expected)| actual.abs_diff(*expected))
        .max()
        .unwrap_or(0);
    assert!(max_u16_diff <= 1, "U16 reference diff {max_u16_diff}");
    assert!(rgba16.iter().all(|pixel| pixel[3] == u16::MAX));
    let distinct: BTreeSet<u16> = rgba16
        .iter()
        .flat_map(|pixel| pixel[..3].iter().copied())
        .collect();
    assert!(
        distinct.len() > 256,
        "U16 output had only {} distinct channel codes",
        distinct.len()
    );

    // Detail remains the pinned recipe value; this contrast control proves that
    // the renderer path did not silently strip it while preparing display output.
    let mut no_sharpening = settings.clone();
    no_sharpening.detail.sharpening.amount = 0.0;
    let no_detail_tiles = candidate_renderer()
        .render_region_as(
            &image,
            &no_sharpening,
            0,
            rect(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let no_detail = assemble_f32(extent, &no_detail_tiles);
    let detail_delta = got_scene
        .iter()
        .zip(&no_detail)
        .flat_map(|(a, b)| a.iter().zip(b))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(
        detail_delta > 1.0e-5,
        "default detail setting had no observable effect"
    );

    // A fresh private renderer must not inherit cache history or output changes.
    let repeated_tiles = candidate_renderer()
        .render_region_as(
            &image,
            &settings,
            0,
            rect(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    assert_eq!(assemble_f32(extent, &repeated_tiles), got_scene);
    assert_eq!(
        convert_scene_tiles(&repeated_tiles, extent, settings.output.gamut_mapping),
        rgba16
    );

    // Reuse the same ImageId with different synthetic source bytes. A render
    // history for that identity must not affect subsequent pixels for A.
    let alternate = synthetic_bayer_with_samples(
        samples(288, 280, RGGB)
            .into_iter()
            .map(|value| value * 0.4 + 0.1)
            .collect(),
    );
    assert_eq!(alternate.id(), image.id());
    let alternate_tiles = renderer
        .render_region_as(
            &alternate,
            &settings,
            0,
            rect(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let alternate_scene = assemble_f32(extent, &alternate_tiles);
    assert_ne!(
        alternate_scene, got_scene,
        "distinct source bytes must render distinctly"
    );
    let after_history = renderer
        .render_region_as(
            &image,
            &settings,
            0,
            rect(extent),
            RenderOutput::SceneLinear,
        )
        .unwrap();
    assert_eq!(assemble_f32(extent, &after_history), got_scene);
}
