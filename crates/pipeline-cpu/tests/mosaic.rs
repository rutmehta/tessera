use engine_api::{
    recipe::settings::HighlightReconstruction,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::{DemosaicAlgorithm, demosaic, inverse_linearize, reconstruct_highlights};
use raw_decode::CfaLayout;

fn tile(data: Vec<f32>, halo: u16) -> Tile {
    Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo,
            channels: 1,
        },
        data,
    )
    .unwrap()
}
#[test]
fn mhc_impulse_has_paper_gradient_coefficients() {
    let mut data = vec![0.0; 25];
    data[12] = 1.0;
    let t = tile(data, 2);
    for (pattern, expected) in [
        ([[0, 1], [3, 2]], [1.0, 0.5, 0.75]),
        ([[1, 0], [2, 3]], [0.625, 1.0, 0.625]),
        ([[2, 1], [3, 0]], [0.75, 0.5, 1.0]),
    ] {
        let out = demosaic(
            &t,
            CfaLayout::Bayer(pattern),
            DemosaicAlgorithm::MalvarHeCutler,
        )
        .unwrap();
        assert_eq!(out.samples::<f32>().unwrap(), expected);
    }
}
#[test]
fn both_algorithms_preserve_flat_colours_all_bayer_phases() {
    for pattern in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 1], [3, 0]],
        [[1, 2], [0, 3]],
    ] {
        let layout = CfaLayout::Bayer(pattern);
        let data: Vec<f32> = (0..25)
            .map(|i| [0.2, 0.4, 0.6, 0.4][layout.channel_at(i % 5, i / 5)])
            .collect();
        for algorithm in [
            DemosaicAlgorithm::Bilinear,
            DemosaicAlgorithm::MalvarHeCutler,
        ] {
            let out = demosaic(&tile(data.clone(), 2), layout, algorithm).unwrap();
            for (a, b) in out.samples::<f32>().unwrap().iter().zip([0.2, 0.4, 0.6]) {
                assert!((a - b).abs() < 1e-6);
            }
        }
    }
}
#[test]
fn clipping_and_channel_propagation_are_distinct() {
    let layout = CfaLayout::Bayer([[0, 1], [3, 2]]);
    let mut data: Vec<_> = (0..81)
        .map(|i| {
            if layout.channel_at(i % 9, i / 9) == 0 {
                0.8
            } else {
                0.4
            }
        })
        .collect();
    // Bright central patch: R clips, neighbouring G/B stay below white.
    for y in 3..=5 {
        for x in 3..=5 {
            data[y * 9 + x] = 0.7;
        }
    }
    data[40] = 1.0;
    let t = tile(data, 4);
    let clip = reconstruct_highlights(&t, layout, HighlightReconstruction::Clip).unwrap();
    let recover =
        reconstruct_highlights(&t, layout, HighlightReconstruction::ReconstructColor).unwrap();
    assert_eq!(clip.samples::<f32>().unwrap()[0], 1.0);
    assert!(recover.samples::<f32>().unwrap()[0] > 1.0);
    assert_eq!(inverse_linearize(0.5, 100.0, 1100.0).unwrap(), 600.0);
}
