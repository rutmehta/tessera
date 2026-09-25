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
