use engine_api::{
    jobs::CancellationToken,
    recipe::settings::{ColorSettings, Curve, CurvePoint, EffectsSettings, ToneSettings},
    stage::StageId,
    tile::Extent,
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn fused_output_levels_crop_styles_display_and_edits_match_cpu() {
    use engine_api::{
        recipe::settings::{Crop, GamutMapping, NormalizedRect, VignetteStyle},
        tile::{Tile, TileCoord, TileLayout},
    };
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let token = CancellationToken::new();
    let extent = Extent::new(1031, 539);
    let crop = Crop {
        rect: NormalizedRect {
            left: 0.1,
            top: 0.15,
            right: 0.95,
            bottom: 0.9,
        },
        angle: 13.,
        ..Default::default()
    };
    let mut tone = ToneSettings {
        exposure: 0.4,
        shadows: 20.,
        ..Default::default()
    };
    let mut color = ColorSettings {
        vibrance: 30.,
        ..Default::default()
    };
    color.grading.highlights.hue = 230.;
    color.grading.highlights.saturation = 35.;
    let mut effects = EffectsSettings::default();
    effects.vignette.amount = -35.;
    effects.grain.amount = 20.;
    for level in [0, 2, 5] {
        let e = extent.at_level(level);
        let coord = TileCoord::new(level, 0, 0);
        let layout = TileLayout {
            extent: Extent::new(e.width.min(61), e.height.min(37)),
            halo: 2,
            channels: 3,
        };
        let tile = Tile::from_samples(
            coord,
            layout,
            (0..layout.len())
                .map(|i| ((i * 31) % 900) as f32 / 500. - 0.08)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for style in [
            VignetteStyle::HighlightPriority,
            VignetteStyle::ColorPriority,
            VignetteStyle::PaintOverlay,
        ] {
            effects.vignette.style = style;
            tone.curves.parametric.lights += 3.;
            color.grading.highlights.hue += 7.;
            let chain = [
                (StageId::Tone, Op::Tone(&tone)),
                (StageId::Tone, Op::ToneExtra(&tone)),
                (StageId::Color, Op::Color(&color)),
                (StageId::Effects, Op::EffectsInCrop(&effects, extent, &crop)),
                (
                    StageId::Output,
                    Op::Display {
                        gamut: GamutMapping::default(),
                    },
                ),
            ];
            let expected = CpuStageOp
                .run_chain_batch(&chain, vec![tile.clone()], &token)
                .unwrap();
            let actual = gpu
                .run_chain_batch(&chain, vec![tile.clone()], &token)
                .unwrap();
            let again = gpu
                .run_chain_batch(&chain, vec![tile.clone()], &token)
                .unwrap();
            assert_eq!(actual[0].layout(), expected[0].layout());
            assert_eq!(
                actual[0].samples::<u8>().unwrap(),
                again[0].samples::<u8>().unwrap()
            );
            let max = actual[0]
                .samples::<u8>()
                .unwrap()
                .iter()
                .zip(expected[0].samples::<u8>().unwrap())
                .map(|(a, b)| a.abs_diff(*b))
                .max()
                .unwrap();
            assert!(max <= 1, "level {level}: display max {max}");
        }
    }
}

#[test]
fn fusion_validation_and_cancellation_do_not_submit() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let tone = ToneSettings::default();
    let color = ColorSettings {
        saturation: f32::NAN,
        ..Default::default()
    };
    let image = Image::new(3, 1, vec![vec![0.2; 3]; 3]).unwrap();
    let tile = image
        .tile(engine_api::tile::TileCoord::new(0, 0, 0), 0, 1)
        .unwrap();
    let chain = [
        (StageId::Tone, Op::Tone(&tone)),
        (StageId::Color, Op::Color(&color)),
    ];
    assert!(
        gpu.run_chain_batch(&chain, vec![tile.clone()], &CancellationToken::new())
            .is_err()
    );
    let token = CancellationToken::new();
    token.cancel();
    assert!(gpu.run_chain_batch(&chain, vec![tile], &token).is_err());
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn tone_curves_color_effects_share_one_transfer() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut tone = ToneSettings {
        exposure: 0.3,
        contrast: 15.,
        ..Default::default()
    };
    tone.curves.rgb = Curve(vec![
        CurvePoint { x: 0., y: 0.02 },
        CurvePoint { x: 0.4, y: 0.5 },
        CurvePoint { x: 1., y: 1. },
    ]);
    let mut color = ColorSettings {
        vibrance: 25.,
        saturation: -10.,
        ..Default::default()
    };
    color.grading.shadows.hue = 35.;
    color.grading.shadows.saturation = 20.;
    let mut effects = EffectsSettings::default();
    effects.vignette.amount = -25.;
    effects.grain.amount = 15.;
    let extent = Extent::new(513, 7);
    let image = Image::new(
        513,
        7,
        (0..3)
            .map(|c| {
                (0..3591)
                    .map(|i| ((i * 17 + c * 51) % 700) as f32 / 500. - 0.05)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let chain = [
        (StageId::Tone, Op::Tone(&tone)),
        (StageId::Tone, Op::ToneExtra(&tone)),
        (StageId::Color, Op::Color(&color)),
        (StageId::Effects, Op::Effects(&effects, extent)),
    ];
    let token = CancellationToken::new();
    let tiles = image
        .coords()
        .map(|c| image.tile(c, 0, 1).unwrap())
        .collect::<Vec<_>>();
    let expected = CpuStageOp
        .run_chain_batch(&chain, tiles.clone(), &token)
        .unwrap();
    let actual = gpu.run_chain_batch(&chain, tiles, &token).unwrap();
    for (a, b) in actual.iter().zip(expected.iter()) {
        for (a, b) in a
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(b.samples::<f32>().unwrap())
        {
            assert!((a - b).abs() <= 2e-3, "{a} != {b}");
        }
    }
    assert_eq!(
        gpu.stats().uploads,
        3,
        "one upload per output tile, not per operator"
    );
    assert_eq!(gpu.stats().readbacks, 3, "no effects boundary readback");
    assert_eq!(gpu.stats().submissions, 1, "one batched transaction");
    assert_eq!(
        gpu.stats().fused_dispatches,
        3,
        "one dispatch per output tile"
    );
}
