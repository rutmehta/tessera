#[path = "../../image-core/tests/common/mod.rs"]
mod common;
use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
use engine_api::{
    recipe::DevelopSettings,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
use pipeline_gpu::{GpuContext, GpuOutputLut, GpuStageOp};
use std::sync::Arc;

fn transform(destination: Builtin) -> Transform {
    let mut registry = Registry::new();
    let source = registry.builtin(Builtin::LinearRec2020).unwrap();
    let destination = registry.builtin(destination).unwrap();
    Transform::new(&source, &destination, TransformOptions::default()).unwrap()
}

#[test]
fn typed_lut_rejects_inconsistent_dimensions() {
    let context = Arc::new(GpuContext::new().unwrap());
    for (size, count) in [(32, 33_usize.pow(3)), (33, 8), (0, 0)] {
        let lut = color_mgmt::Lut3d {
            size,
            values: vec![[0.; 3]; count],
        };
        assert!(GpuOutputLut::from_lut(context.clone(), &lut).is_err());
    }
}

#[test]
fn icc_lut_matches_cpu_sampling_and_direct_transform() {
    let context = Arc::new(GpuContext::new().unwrap());
    for destination in [Builtin::Srgb, Builtin::DisplayP3] {
        let transform = transform(destination);
        let cpu = transform.lut33();
        let gpu = GpuOutputLut::from_lut(context.clone(), &cpu).unwrap();
        let mut pixels = vec![
            [0.; 3],
            [1.; 3],
            [1., 0., 0.],
            [0., 1., 0.],
            [0., 0., 1.],
            [-1., 0.5, 2.],
        ];
        // Off-grid, interior colors avoid gamut-boundary / transfer-function knees
        // when measuring the interpolation approximation against the direct CMM.
        pixels.extend(
            (0..250).map(|i| {
                std::array::from_fn(|c| 0.2 + ((i * 73 + c * 31) % 257) as f32 / 257. * 0.6)
            }),
        );
        let n = pixels.len();
        let samples = (0..3)
            .flat_map(|c| pixels.iter().map(move |rgb| rgb[c]))
            .collect();
        let tile = Tile::from_samples(
            TileCoord::new(0, 0, 0),
            TileLayout {
                extent: Extent::new(n as u32, 1),
                halo: 0,
                channels: 3,
            },
            samples,
        )
        .unwrap();
        let result = gpu.apply(&tile).unwrap();
        let values = result.samples::<f32>().unwrap();
        let mut max_direct = 0.0_f32;
        for (i, &rgb) in pixels.iter().enumerate() {
            let expected = cpu.sample(rgb);
            for c in 0..3 {
                assert!(
                    (values[c * n + i] - expected[c]).abs() < 2e-6,
                    "{destination:?} pixel {i} channel {c}"
                );
            }
            if i >= 6 {
                let direct = transform.apply(rgb);
                for c in 0..3 {
                    max_direct = max_direct.max((values[c * n + i] - direct[c]).abs());
                }
            }
        }
        // A uniform 33^3 lattice is an approximation, not an exact CMM.
        assert!(
            max_direct < 0.01,
            "{destination:?}: direct max error {max_direct}"
        );
        eprintln!("{destination:?}: direct max error {max_direct}");
    }
}

#[test]
fn renderer_managed_output_bypasses_legacy_display_and_reuses_scene_cache() {
    let context = Arc::new(GpuContext::new().unwrap());
    let ops = Arc::new(GpuStageOp::new(context.clone()));
    let config = RendererConfig::default();
    let renderer = Renderer::with_ops(
        ops,
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    let image = common::synthetic(901, 67, 49, common::RGGB, [0, 0, 67, 49]);
    let mut settings = DevelopSettings::default();
    settings.tone.exposure = 0.4;
    settings.geometry.crop.angle = 7.0;
    let rect = PixelRect::full(image.level_extent(0));
    let legacy = renderer.render_region(&image, &settings, 0, rect).unwrap();
    let scene = renderer
        .render_region_as(&image, &settings, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    for destination in [Builtin::Srgb, Builtin::DisplayP3, Builtin::Srgb] {
        let cpu = transform(destination).lut33();
        let output = GpuOutputLut::from_lut(context.clone(), &cpu).unwrap();
        let managed = output
            .render_region(&renderer, &image, &settings, 0, rect)
            .unwrap();
        assert_eq!(managed.len(), scene.len());
        for (actual, input) in managed.iter().zip(&scene) {
            assert_eq!(actual.coord(), input.coord());
            assert_eq!(actual.layout(), input.layout());
            let n = input.layout().plane_len();
            let src = input.samples::<f32>().unwrap();
            let dst = actual.samples::<f32>().unwrap();
            for i in 0..n {
                let expected = cpu.sample([src[i], src[n + i], src[2 * n + i]]);
                for c in 0..3 {
                    assert!((dst[c * n + i] - expected[c]).abs() < 2e-6);
                }
            }
        }
    }
    let unchanged = renderer.render_region(&image, &settings, 0, rect).unwrap();
    for (before, after) in legacy.iter().zip(unchanged) {
        assert_eq!(
            before.samples::<u8>().unwrap(),
            after.samples::<u8>().unwrap()
        );
    }
}
