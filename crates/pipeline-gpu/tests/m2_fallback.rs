//! Mixed GPU operators and CPU tile fallback preserve reference semantics.
use engine_api::{
    jobs::CancellationToken,
    recipe::settings::{
        ColorSettings, Crop, Curve, CurvePoint, DetailSettings, EffectsSettings, GeometrySettings,
        ToneSettings,
    },
    stage::StageId,
    tile::{Extent, TileCoord},
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

fn image() -> Image {
    Image::new(
        17,
        9,
        (0..3)
            .map(|c| {
                (0..153)
                    .map(|i| 0.2 + ((i * 7 + c * 11) % 51) as f32 / 100.0)
                    .collect()
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn m2_tiles_and_image_barriers_match_cpu() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut detail = DetailSettings::default();
    detail.sharpening.amount = 75.0;
    detail.noise_reduction.luminance = 25.0;
    detail.noise_reduction.color = 50.0;
    let tone = ToneSettings {
        texture: 40.0,
        clarity: -20.0,
        dehaze: 15.0,
        ..Default::default()
    };
    let mut effects = EffectsSettings::default();
    effects.grain.amount = 30.0;
    effects.vignette.amount = -40.0;
    let crop = Crop {
        angle: 12.0,
        ..Default::default()
    };
    let input = image();
    for op in [
        Op::Detail(&detail),
        Op::ToneExtra(&tone),
        Op::Effects(&effects, Extent::new(17, 9)),
        Op::EffectsInCrop(&effects, Extent::new(17, 9), &crop),
    ] {
        let tile = input.tile(TileCoord::new(0, 0, 0), 9, 1).unwrap();
        let expected = CpuStageOp.run(StageId::Tone, &op, tile.clone()).unwrap();
        let actual = gpu.run(StageId::Tone, &op, tile).unwrap();
        assert_eq!(actual.layout(), expected.layout());
        for (a, b) in actual
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(expected.samples::<f32>().unwrap())
        {
            assert!(a.is_finite() && (a - b).abs() <= 1e-4, "tile {a} != {b}");
        }
    }
    let geometry = GeometrySettings {
        crop,
        ..Default::default()
    };
    for op in [
        Op::Geometry(&geometry),
        Op::ToneExtra(&tone),
        Op::Detail(&detail),
    ] {
        let token = CancellationToken::new();
        let expected = CpuStageOp
            .run_image(StageId::Tone, &op, input.clone(), &token)
            .unwrap();
        let actual = gpu
            .run_image(StageId::Tone, &op, input.clone(), &token)
            .unwrap();
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height())
        );
        for (a, b) in actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
        {
            assert!(a.is_finite() && (a - b).abs() <= 1e-4, "image {a} != {b}");
        }
        token.cancel();
        assert!(
            gpu.run_image(StageId::Tone, &op, input.clone(), &token)
                .is_err()
        );
    }
    assert!(gpu.stats().submissions > 0);
    let before = gpu.stats().submissions;
    let tile = input.tile(TileCoord::new(0, 0, 0), 9, 1).unwrap();
    let expected = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&tone), tile.clone())
        .unwrap();
    let actual = gpu.run(StageId::Tone, &Op::ToneExtra(&tone), tile).unwrap();
    assert_eq!(
        actual.samples::<f32>().unwrap(),
        expected.samples::<f32>().unwrap()
    );
    assert_eq!(
        gpu.stats().submissions,
        before,
        "local tone tile fallback remains CPU"
    );
}

#[test]
fn curve_color_validation_and_neutral_bits() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut tile = image().tile(TileCoord::new(0, 0, 0), 1, 1).unwrap();
    tile.samples_mut::<f32>().unwrap()[0] = -0.0;
    let tone = ToneSettings::default();
    let color = ColorSettings::default();
    for op in [Op::ToneExtra(&tone), Op::Color(&color)] {
        let actual = gpu.run(StageId::Tone, &op, tile.clone()).unwrap();
        assert_eq!(
            actual
                .samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            tile.samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }
    let mut tone = ToneSettings::default();
    tone.curves.rgb = Curve(vec![
        CurvePoint { x: 0.5, y: 0.3 },
        CurvePoint { x: 0.5, y: 0.7 },
    ]);
    assert!(
        gpu.run(StageId::Tone, &Op::ToneExtra(&tone), tile.clone())
            .is_err()
    );
    tone = ToneSettings::default();
    tone.curves.parametric.shadow_split = 90.0;
    assert!(
        gpu.run(StageId::Tone, &Op::ToneExtra(&tone), tile.clone())
            .is_err()
    );
    let mut color = ColorSettings {
        vibrance: f32::NAN,
        ..Default::default()
    };
    assert!(
        gpu.run(StageId::Tone, &Op::Color(&color), tile.clone())
            .is_err()
    );
    color = ColorSettings::default();
    color.point_colors.push(Default::default());
    assert!(
        gpu.run(StageId::Tone, &Op::Color(&color), tile.clone())
            .is_err()
    );
    tile.samples_mut::<f32>().unwrap()[0] = f32::INFINITY;
    for op in [
        Op::ToneExtra(&ToneSettings::default()),
        Op::Color(&ColorSettings::default()),
    ] {
        assert!(gpu.run(StageId::Tone, &op, tile.clone()).is_err());
    }
}

#[test]
fn lr2c_legacy_tone_gpu_session_falls_back_to_cpu() {
    use engine_api::recipe::settings::LegacyPv2010;
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let tone = ToneSettings {
        legacy_pv2010: Some(LegacyPv2010 {
            brightness: Some(50.),
            blacks: Some(5.),
            ..Default::default()
        }),
        ..Default::default()
    };
    let input = image().tile(TileCoord::new(0, 0, 0), 0, 1).unwrap();
    let op = Op::Tone(&tone);
    let expected = CpuStageOp.run(StageId::Tone, &op, input.clone()).unwrap();
    let actual = gpu.run(StageId::Tone, &op, input.clone()).unwrap();
    assert_eq!(
        actual.samples::<f32>().unwrap(),
        expected.samples::<f32>().unwrap()
    );
    let batch = gpu
        .run_chain_batch(
            &[(StageId::Tone, op)],
            vec![input],
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        batch[0].samples::<f32>().unwrap(),
        expected.samples::<f32>().unwrap()
    );
}

#[path = "../../image-core/tests/common/mod.rs"]
mod common;

#[test]
fn lr2c_native_and_adobe_gpu_sessions_match_cpu_for_legacy_and_bw() {
    use engine_api::recipe::{
        DevelopSettings, ProcessVersion,
        settings::{Curve, CurvePoint, LegacyPv2010, MonochromeSettings},
    };
    use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
    let image = common::synthetic(2903, 24, 24, common::RGGB, [0, 0, 24, 24]);
    let mut s = DevelopSettings::default();
    s.tone.legacy_pv2010 = Some(LegacyPv2010 {
        brightness: Some(50.),
        blacks: Some(5.),
        ..Default::default()
    });
    s.color.monochrome = Some(MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    s.tone.curves.red = Curve(vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 1., y: 0.5 },
    ]);
    for pv in [ProcessVersion::NATIVE_CURRENT, ProcessVersion::adobe(2)] {
        let config = RendererConfig {
            process_version: pv,
            ..Default::default()
        };
        let cpu = Renderer::with_ops(
            Arc::new(CpuStageOp),
            Arc::new(TileCache::new(16 << 20)),
            config.clone(),
        );
        let gpu = Renderer::with_ops(
            Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))),
            Arc::new(TileCache::new(16 << 20)),
            config,
        );
        let rect = PixelRect::full(image.level_extent(0));
        let expected = cpu
            .render_region_as(&image, &s, 0, rect, RenderOutput::SceneLinear)
            .unwrap();
        let actual = gpu
            .render_region_as(&image, &s, 0, rect, RenderOutput::SceneLinear)
            .unwrap();
        for (a, b) in actual.iter().zip(&expected) {
            for (a, b) in a
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(b.samples::<f32>().unwrap())
            {
                assert!((a - b).abs() < 1e-4, "{pv:?}: {a} != {b}");
            }
            let samples = a.samples::<f32>().unwrap();
            let n = a.layout().plane_len();
            assert!(
                (samples[n / 2] - samples[n + n / 2]).abs() > 0.001,
                "channel toning lost"
            );
        }
    }
}

#[test]
fn lr2c_resident_bw_preserves_toning_with_and_without_local_tone() {
    use engine_api::recipe::{
        DevelopSettings,
        settings::{Curve, CurvePoint, MonochromeSettings},
    };
    use image_core::{PixelRect, RenderOutput, Renderer, RendererConfig, TileCache};
    let image = common::synthetic(2904, 24, 24, common::RGGB, [0, 0, 24, 24]);
    let cpu = Renderer::new(RendererConfig::default());
    let gpu = Renderer::with_ops(
        Arc::new(GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))),
        Arc::new(TileCache::new(16 << 20)),
        RendererConfig::default(),
    );
    let mut s = DevelopSettings::default();
    s.color.monochrome = Some(MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    s.tone.curves.red = Curve(vec![
        CurvePoint { x: 0., y: 0. },
        CurvePoint { x: 1., y: 0.5 },
    ]);
    let rect = PixelRect::full(image.level_extent(0));
    for clarity in [0., 20.] {
        s.tone.clarity = clarity;
        assert!(gpu.can_render_resident(&image, &s).unwrap());
        let expected = cpu
            .render_region_as(&image, &s, 0, rect, RenderOutput::SceneLinear)
            .unwrap();
        let actual = gpu
            .render_region_as(&image, &s, 0, rect, RenderOutput::SceneLinear)
            .unwrap();
        for (a, b) in actual.iter().zip(&expected) {
            let worst = a
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(b.samples::<f32>().unwrap())
                .map(|(a, b)| (a - b).abs())
                .fold(0., f32::max);
            // Existing resident f16 checkpoint tolerance (resident_fusion.rs).
            assert!(worst <= 0.002, "clarity {clarity}: {worst}");
            let samples = a.samples::<f32>().unwrap();
            let n = a.layout().plane_len();
            assert!(
                (samples[n / 2] - samples[n + n / 2]).abs() > 0.001,
                "channel toning lost"
            );
        }
    }
}
