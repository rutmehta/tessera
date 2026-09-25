#[path = "../../image-core/tests/common/mod.rs"]
mod common;

use engine_api::{
    jobs::CancellationToken,
    tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout},
};
use image_core::{Op, StageOp};
use pipeline_cpu::DemosaicAlgorithm;
use pipeline_gpu::{GpuContext, GpuStageOp};
use raw_decode::CfaLayout;
use std::sync::Arc;

#[test]
fn resident_xtrans_gather_matches_cpu_at_frame_edges_and_tile_seams() {
    use std::collections::HashMap;
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let cfa = common::xtrans();
    for frame in [Extent::new(515, 259), Extent::new(2, 1)] {
        let pixels = common::samples(frame.width, frame.height, cfa);
        let mut batch = gpu.begin_resident().unwrap();
        let mut sources = HashMap::new();
        for ty in 0..frame.height.div_ceil(TILE_SIZE) {
            for tx in 0..frame.width.div_ceil(TILE_SIZE) {
                let coord = TileCoord::new(0, tx, ty);
                let (ox, oy) = coord.pixel_origin(TILE_SIZE);
                let layout = TileLayout {
                    extent: Extent::new(
                        (frame.width - ox).min(TILE_SIZE),
                        (frame.height - oy).min(TILE_SIZE),
                    ),
                    halo: 0,
                    channels: 1,
                };
                let mut data = Vec::new();
                for y in 0..layout.extent.height {
                    for x in 0..layout.extent.width {
                        data.push(pixels[((oy + y) * frame.width + ox + x) as usize]);
                    }
                }
                let input = Tile::from_samples(coord, layout, data).unwrap();
                sources.insert(coord, batch.upload(&input).unwrap());
            }
        }
        let mut coords: Vec<_> = sources.keys().copied().collect();
        coords.sort();
        let mut outputs = Vec::new();
        let mut expected = Vec::new();
        for coord in coords {
            let raw = batch.gather(frame, coord, 3, 6, &sources).unwrap();
            let l = raw.layout;
            let (ox, oy) = coord.pixel_origin(TILE_SIZE);
            // Independent nearest in-frame sample with matching CFA phase.
            let clamp = |v: i64, n: u32| -> u32 {
                if (0..i64::from(n)).contains(&v) {
                    return v as u32;
                }
                (0..n)
                    .filter(|&i| i as i64 % 6 == v.rem_euclid(6))
                    .min_by_key(|&i| (i as i64 - v).abs())
                    .unwrap_or(v.clamp(0, n as i64 - 1) as u32)
            };
            let mut data = Vec::new();
            for y in -3..l.extent.height as i64 + 3 {
                for x in -3..l.extent.width as i64 + 3 {
                    let sx = clamp(ox as i64 + x, frame.width);
                    let sy = clamp(oy as i64 + y, frame.height);
                    data.push(pixels[(sy * frame.width + sx) as usize]);
                }
            }
            let input = Tile::from_samples(coord, l, data).unwrap();
            let algorithm = DemosaicAlgorithm::MalvarHeCutler;
            expected.push(pipeline_cpu::demosaic(&input, cfa, algorithm).unwrap());
            outputs.push(batch.run(&Op::Demosaic { cfa, algorithm }, &raw).unwrap());
        }
        let result = batch
            .finish(outputs, false, None, &CancellationToken::new())
            .unwrap();
        for (actual, expected) in result.tiles.iter().zip(expected) {
            assert_eq!(actual.layout(), expected.layout());
            for (a, b) in actual
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(expected.samples::<f32>().unwrap())
            {
                assert!(
                    a.is_finite() && (a - b).abs() <= 1e-4,
                    "frame={frame:?} coord={:?}: GPU={a}, CPU={b}",
                    actual.coord()
                );
            }
        }
    }
}

#[test]
fn resident_xtrans_highlights_match_cpu_without_intermediate_readback() {
    use engine_api::recipe::settings::HighlightReconstruction::{Clip, ReconstructColor};
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut batch = gpu.begin_resident().unwrap();
    let cfa = common::xtrans();
    let mut outputs = Vec::new();
    let mut expected = Vec::new();
    for mode in [Clip, ReconstructColor] {
        for phase in 0..3 {
            for flat in [false, true] {
                let layout = TileLayout {
                    extent: Extent::new(19, 17),
                    halo: if mode == Clip { 0 } else { 4 },
                    channels: 1,
                };
                let input = Tile::from_samples(
                    TileCoord::new(0, phase, 2 - phase),
                    layout,
                    (0..layout.len())
                        .map(|i| {
                            if flat {
                                1.2
                            } else {
                                ((i * 37 % 251) as f32 - 20.0) / 151.0
                            }
                        })
                        .collect(),
                )
                .unwrap();
                expected.push(pipeline_cpu::reconstruct_highlights(&input, cfa, mode).unwrap());
                let raw = batch.upload(&input).unwrap();
                outputs.push(batch.run(&Op::Highlights { cfa, mode }, &raw).unwrap());
            }
        }
    }
    assert_eq!(gpu.stats().submissions, 0);
    assert_eq!(gpu.stats().readbacks, 0);
    let result = batch
        .finish(outputs, false, None, &CancellationToken::new())
        .unwrap();
    for (actual, expected) in result.tiles.iter().zip(expected) {
        assert_eq!(actual.layout(), expected.layout());
        assert!(
            actual
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(expected.samples::<f32>().unwrap())
                .all(|(a, b)| a.is_finite() && (a - b).abs() <= 1e-4)
        );
    }
    assert_eq!(gpu.stats().submissions, 1);
    assert_eq!(gpu.stats().readbacks, 1);
}

#[test]
fn resident_xtrans_rejects_invalid_cfa_and_layout_before_dispatch() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut batch = gpu.begin_resident().unwrap();
    let mut bad_channel = [[1; 6]; 6];
    bad_channel[0][0] = 0;
    bad_channel[0][1] = 2;
    bad_channel[0][2] = 3;
    for (cfa, halo, channels) in [
        (common::xtrans(), 2, 1),
        (common::xtrans(), 3, 3),
        (CfaLayout::XTrans([[1; 6]; 6]), 3, 1),
        (CfaLayout::XTrans(bad_channel), 3, 1),
    ] {
        let layout = TileLayout {
            extent: Extent::new(1, 1),
            halo,
            channels,
        };
        let input =
            Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![0.2_f32; layout.len()])
                .unwrap();
        let op = Op::Demosaic {
            cfa,
            algorithm: DemosaicAlgorithm::Bilinear,
        };
        assert!(pipeline_cpu::demosaic(&input, cfa, DemosaicAlgorithm::Bilinear).is_err());
        let raw = batch.upload(&input).unwrap();
        assert!(
            batch.run(&op, &raw).is_err(),
            "accepted cfa={cfa:?} layout={layout:?}"
        );
    }
    batch
        .finish(vec![], false, None, &CancellationToken::new())
        .unwrap();
    assert_eq!(gpu.stats().last_resident_dispatches, 0);
}

#[test]
fn resident_xtrans_matches_cpu_at_all_tile_phases_and_preserves_known_samples() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut batch = gpu.begin_resident().unwrap();
    let mut expected = Vec::new();
    let mut outputs = Vec::new();
    // Sparse but valid CFA forces the 7x7 fallback, including at halo edges.
    let mut sparse = [[1; 6]; 6];
    sparse[0][0] = 0;
    sparse[5][5] = 2;
    for cfa in [common::xtrans(), CfaLayout::XTrans(sparse)] {
        for algorithm in [
            DemosaicAlgorithm::Bilinear,
            DemosaicAlgorithm::MalvarHeCutler,
        ] {
            for y in 0..3 {
                for x in 0..3 {
                    let coord = TileCoord::new(0, x, y);
                    let layout = TileLayout {
                        extent: Extent::new(17, 13),
                        halo: 3,
                        channels: 1,
                    };
                    let input = Tile::from_samples(
                        coord,
                        layout,
                        (0..layout.len())
                            .map(|i| ((i * 37 % 251) as f32 - 30.0) / 89.0)
                            .collect(),
                    )
                    .unwrap();
                    let cpu = pipeline_cpu::demosaic(&input, cfa, algorithm).unwrap();
                    let raw = batch.upload(&input).unwrap();
                    outputs.push(batch.run(&Op::Demosaic { cfa, algorithm }, &raw).unwrap());
                    expected.push((input, cpu, cfa));
                }
            }
        }
    }
    // Encoding must neither submit nor read back intermediate pixels.
    assert_eq!(gpu.stats().submissions, 0);
    assert_eq!(gpu.stats().readbacks, 0);
    let result = batch
        .finish(outputs, false, None, &CancellationToken::new())
        .unwrap();
    assert_eq!(result.tiles.len(), expected.len());
    for (actual, (input, cpu, cfa)) in result.tiles.iter().zip(expected) {
        assert_eq!(actual.layout(), cpu.layout());
        assert_eq!(actual.coord(), cpu.coord());
        let values = actual.samples::<f32>().unwrap();
        for (i, (&a, &b)) in values.iter().zip(cpu.samples::<f32>().unwrap()).enumerate() {
            assert!(
                a.is_finite() && (a - b).abs() <= 1e-4,
                "coord={:?} sample={i}: GPU={a}, CPU={b}",
                actual.coord()
            );
        }
        let (ox, oy) = actual.coord().pixel_origin(TILE_SIZE);
        for y in 0..actual.layout().extent.height {
            for x in 0..actual.layout().extent.width {
                let c = cfa.channel_at(ox + x, oy + y);
                assert_eq!(
                    values[actual.layout().index(c as u8, x as i32, y as i32).unwrap()],
                    input.samples::<f32>().unwrap()
                        [input.layout().index(0, x as i32, y as i32).unwrap()]
                );
            }
        }
    }
    assert_eq!(gpu.stats().submissions, 1);
    assert_eq!(gpu.stats().readbacks, 1);
}
