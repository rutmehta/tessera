use engine_api::{
    color::ColorMatrix3,
    stage::StageId,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::{Arc, OnceLock};

fn gpu() -> GpuStageOp {
    static CONTEXT: OnceLock<Arc<GpuContext>> = OnceLock::new();
    GpuStageOp::new(
        CONTEXT
            .get_or_init(|| Arc::new(GpuContext::new().expect("Metal GPU required")))
            .clone(),
    )
}
fn tile(channels: u8, halo: u16) -> Tile {
    let l = TileLayout {
        extent: Extent::new(29, 17),
        halo,
        channels,
    };
    let data = (0..l.plane_len() * channels as usize)
        .map(|i| ((i * 179 + 31) % 1024) as f32 / 731.0 - 0.03)
        .collect();
    Tile::from_samples(TileCoord::new(0, 1, 2), l, data).unwrap()
}
fn compare(stage: StageId, op: Op<'_>, input: Tile) {
    let gpu = gpu();
    let expected = CpuStageOp.run(stage, &op, input.clone()).unwrap();
    let actual = gpu.run(stage, &op, input.clone()).unwrap();
    let repeated = gpu.run(stage, &op, input).unwrap();
    assert_eq!(actual.layout(), expected.layout());
    if let Ok(a) = actual.samples::<f32>() {
        let b = expected.samples::<f32>().unwrap();
        let diff = a
            .iter()
            .zip(b)
            .map(|(a, b)| {
                assert!(a.is_finite() && b.is_finite());
                (a - b).abs()
            })
            .fold(0.0f32, f32::max);
        assert!(diff <= 1e-4, "{stage:?} {op:?}: max error {diff}");
        assert_eq!(
            a.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            repeated
                .samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    } else {
        let a = actual.samples::<u8>().unwrap();
        let b = expected.samples::<u8>().unwrap();
        assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(*b) <= 1));
        assert_eq!(a, repeated.samples::<u8>().unwrap());
    }
}
#[test]
fn matrix_matches_cpu_and_is_deterministic() {
    compare(
        StageId::CameraProfile,
        Op::Matrix(ColorMatrix3([
            [1.3, -0.2, -0.1],
            [-0.02, 0.97, 0.05],
            [0.11, -0.21, 1.1],
        ])),
        tile(3, 2),
    );
}

#[test]
fn bayer_operators_all_phases() {
    use engine_api::recipe::settings::HighlightReconstruction::*;
    use pipeline_cpu::DemosaicAlgorithm::*;
    for pattern in [
        [[0, 1], [1, 2]],
        [[1, 0], [2, 1]],
        [[1, 2], [0, 1]],
        [[2, 1], [1, 0]],
        [[0, 1], [3, 2]],
    ] {
        let cfa = raw_decode::CfaLayout::Bayer(pattern);
        for mode in [Clip, ReconstructColor] {
            compare(StageId::Linearize, Op::Highlights { cfa, mode }, tile(1, 4));
        }
        for algorithm in [Bilinear, MalvarHeCutler] {
            compare(
                StageId::Demosaic,
                Op::Demosaic { cfa, algorithm },
                tile(1, 2),
            );
        }
    }
}

#[test]
fn tone_and_display() {
    use engine_api::recipe::settings::{GamutMapping, ToneSettings};
    for amount in [-100.0, -35.0, 0.0, 30.0, 100.0] {
        let s = ToneSettings {
            exposure: 0.5,
            contrast: amount,
            highlights: -amount,
            shadows: amount,
            whites: amount,
            blacks: -amount,
            ..Default::default()
        };
        compare(StageId::Tone, Op::Tone(&s), tile(3, 2));
    }
    for gamut in [GamutMapping::Clip, GamutMapping::Perceptual] {
        compare(StageId::Output, Op::Display { gamut }, tile(3, 2));
    }
}

#[test]
fn cat16_white_balance() {
    use engine_api::{
        color::WorkingSpace,
        recipe::settings::{WhiteBalanceMode, WhiteBalanceSettings},
    };
    for temperature in [2000.0, 5500.0, 12000.0] {
        let settings = WhiteBalanceSettings {
            mode: WhiteBalanceMode::Custom,
            temperature,
            tint: 35.0,
        };
        let m = pipeline_cpu::white_balance_matrix(
            &settings,
            WorkingSpace::LinearRec2020.to_xyz(),
            [1.0; 4],
        )
        .unwrap();
        compare(StageId::WhiteBalance, Op::Matrix(m), tile(3, 0));
    }
}

#[test]
fn batch_chain_matches_cpu() {
    use engine_api::{
        jobs::CancellationToken,
        recipe::settings::{GamutMapping, ToneSettings},
    };
    let gpu = gpu();
    let s = ToneSettings {
        exposure: 0.7,
        shadows: 35.0,
        ..Default::default()
    };
    let chain = [
        (StageId::Tone, Op::Tone(&s)),
        (
            StageId::Output,
            Op::Display {
                gamut: GamutMapping::Perceptual,
            },
        ),
    ];
    let inputs: Vec<_> = (0..19)
        .map(|i| {
            let mut t = tile(3, 0);
            for v in t.samples_mut::<f32>().unwrap() {
                *v *= 0.1 + i as f32 / 19.0;
            }
            Tile::from_samples(
                TileCoord::new(0, i, 0),
                t.layout(),
                t.samples::<f32>().unwrap().to_vec(),
            )
            .unwrap()
        })
        .collect();
    let expected = CpuStageOp
        .run_chain_batch(&chain, inputs.clone(), &CancellationToken::new())
        .unwrap();
    let actual = gpu
        .run_chain_batch(&chain, inputs.clone(), &CancellationToken::new())
        .unwrap();
    assert_eq!(actual.len(), 19);
    let stats = gpu.stats();
    assert_eq!(stats.uploads, 19);
    assert_eq!(stats.readbacks, 19);
    assert_eq!(stats.submissions, 2);
    let repeat = gpu
        .run_chain_batch(&chain, inputs, &CancellationToken::new())
        .unwrap();
    for (a, b) in actual.iter().zip(repeat) {
        assert_eq!(a.samples::<u8>().unwrap(), b.samples::<u8>().unwrap());
    }
    for (a, b) in actual.iter().zip(expected) {
        assert_eq!(a.coord(), b.coord());
        assert!(
            a.samples::<u8>()
                .unwrap()
                .iter()
                .zip(b.samples::<u8>().unwrap())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
    }
}
