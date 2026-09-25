use engine_api::{
    color::ColorMatrix3,
    jobs::CancellationToken,
    recipe::settings::{GamutMapping, HighlightReconstruction, ToneSettings},
    stage::StageId,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::DemosaicAlgorithm;
use pipeline_gpu::{GpuContext, GpuStageOp};
use raw_decode::CfaLayout;
use std::sync::Arc;

fn tile(channels: u8, halo: u16, value: f32) -> Tile {
    let layout = TileLayout {
        extent: Extent::new(1, 1),
        channels,
        halo,
    };
    Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![value; layout.len()]).unwrap()
}
fn gpu() -> GpuStageOp {
    GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))
}
const CFA: CfaLayout = CfaLayout::Bayer([[0, 1], [1, 2]]);

#[test]
fn invalid_layout_parameters_and_chain_fail_without_submission() {
    let gpu = gpu();
    let bad_tone = ToneSettings {
        exposure: f32::NAN,
        ..Default::default()
    };
    for (op, tile) in [
        (Op::Tone(&bad_tone), tile(3, 0, 0.1)),
        (
            Op::Matrix(ColorMatrix3([[f64::NAN; 3]; 3])),
            tile(3, 0, 0.1),
        ),
        (Op::Matrix(ColorMatrix3::IDENTITY), tile(1, 0, 0.1)),
        (
            Op::Demosaic {
                cfa: CFA,
                algorithm: DemosaicAlgorithm::Bilinear,
            },
            tile(1, 1, 0.1),
        ),
        (
            Op::Highlights {
                cfa: CFA,
                mode: HighlightReconstruction::ReconstructColor,
            },
            tile(1, 3, 0.1),
        ),
        (
            Op::Highlights {
                cfa: CfaLayout::Bayer([[0, 0], [0, 0]]),
                mode: HighlightReconstruction::Clip,
            },
            tile(1, 0, 0.1),
        ),
        (
            Op::Highlights {
                cfa: CFA,
                mode: HighlightReconstruction::Inpaint,
            },
            tile(1, 4, 0.1),
        ),
    ] {
        assert!(CpuStageOp.run(StageId::Tone, &op, tile.clone()).is_err());
        assert!(gpu.run(StageId::Tone, &op, tile).is_err());
    }
    let chain = [
        (
            StageId::Output,
            Op::Display {
                gamut: GamutMapping::Clip,
            },
        ),
        (StageId::Tone, Op::Matrix(ColorMatrix3::IDENTITY)),
    ];
    assert!(
        gpu.run_chain_batch(&chain, vec![tile(3, 0, 0.1)], &CancellationToken::new())
            .is_err()
    );
    let bad_type = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        tile(3, 0, 0.1).layout(),
        vec![0u8; 3],
    )
    .unwrap();
    assert!(
        gpu.run(StageId::Tone, &Op::Tone(&ToneSettings::default()), bad_type)
            .is_err()
    );
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn empty_and_cancelled_batches_do_not_submit() {
    let gpu = gpu();
    let t = tile(3, 0, 0.1);
    let output = gpu
        .run_chain_batch(&[], vec![t.clone()], &CancellationToken::new())
        .unwrap();
    assert_eq!(
        output[0].samples::<f32>().unwrap(),
        t.samples::<f32>().unwrap()
    );
    let chain = [(StageId::Tone, Op::Matrix(ColorMatrix3::IDENTITY))];
    assert!(
        gpu.run_chain_batch(&chain, vec![], &CancellationToken::new())
            .unwrap()
            .is_empty()
    );
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(gpu.run_chain_batch(&chain, vec![t], &cancelled).is_err());
    assert_eq!(gpu.stats().submissions, 0);
}

#[test]
fn black_clipped_and_exposure_extremes() {
    let gpu = gpu();
    for value in [-0.1, 0.0, 1e-8, 0.0031308, 0.18, 1.0, 1.2, 4.0] {
        for exposure in [-10.0, 0.0, 10.0] {
            let tone = ToneSettings {
                exposure,
                ..Default::default()
            };
            let input = tile(3, 0, value);
            let expected = CpuStageOp
                .run(StageId::Tone, &Op::Tone(&tone), input.clone())
                .unwrap();
            let actual = gpu.run(StageId::Tone, &Op::Tone(&tone), input).unwrap();
            assert_eq!(
                actual.samples::<f32>().unwrap(),
                expected.samples::<f32>().unwrap()
            );
            for gamut in [GamutMapping::Clip, GamutMapping::Perceptual] {
                let op = Op::Display { gamut };
                let a = gpu.run(StageId::Output, &op, actual.clone()).unwrap();
                let b = CpuStageOp
                    .run(StageId::Output, &op, actual.clone())
                    .unwrap();
                assert!(
                    a.samples::<u8>()
                        .unwrap()
                        .iter()
                        .zip(b.samples::<u8>().unwrap())
                        .all(|(a, b)| a.abs_diff(*b) <= 1)
                );
            }
        }
        let op = Op::Highlights {
            cfa: CFA,
            mode: HighlightReconstruction::ReconstructColor,
        };
        let a = gpu.run(StageId::Linearize, &op, tile(1, 4, value)).unwrap();
        let b = CpuStageOp
            .run(StageId::Linearize, &op, tile(1, 4, value))
            .unwrap();
        assert_eq!(a.samples::<f32>().unwrap(), b.samples::<f32>().unwrap());
    }
}
