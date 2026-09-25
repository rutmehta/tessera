use engine_api::{
    color::{ColorMatrix3, WorkingSpace},
    recipe::{
        DevelopSettings,
        settings::{
            DemosaicMethod, HighlightReconstruction, ToneSettings, WhiteBalanceMode,
            WhiteBalanceSettings,
        },
    },
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::*;
use raw_decode::CfaLayout;
fn patch(v: [f32; 3]) -> Tile {
    Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels: 3,
        },
        v.to_vec(),
    )
    .unwrap()
}

#[test]
fn tone_extremes_remain_finite_and_monotone() {
    for mask in 0..32 {
        let control = |i| if mask & (1 << i) == 0 { -100.0 } else { 100.0 };
        let s = ToneSettings {
            exposure: 10.0,
            contrast: control(0),
            highlights: control(1),
            shadows: control(2),
            whites: control(3),
            blacks: control(4),
            ..Default::default()
        };
        let mut last = 0.0;
        for i in 0..1025 {
            let mut t = patch([i as f32 / 256.0; 3]);
            tone(&mut t, &s).unwrap();
            let v = t.samples::<f32>().unwrap()[0];
            assert!(
                v.is_finite() && v >= last,
                "mask={mask}, i={i}, previous={last}, current={v}"
            );
            last = v;
        }
    }
}
#[test]
fn neutral_tone_is_bit_exact_and_copy_on_write() {
    let mut t = patch([-0.1, 0.18, 2.0]);
    let original = t.clone();
    tone(&mut t, &ToneSettings::default()).unwrap();
    assert_eq!(
        t.samples::<f32>().unwrap(),
        original.samples::<f32>().unwrap()
    );
    assert_eq!(original.samples::<f32>().unwrap(), &[-0.1, 0.18, 2.0]);
}
#[test]
fn positive_tint_moves_output_toward_magenta() {
    let transform = |tint| {
        white_balance_matrix(
            &WhiteBalanceSettings {
                mode: WhiteBalanceMode::Custom,
                temperature: 6500.0,
                tint,
            },
            WorkingSpace::LinearRec2020.to_xyz(),
            [1.0; 4],
        )
        .unwrap()
        .apply([1.0; 3])
    };
    let a = transform(0.0);
    let b = transform(100.0);
    assert!(
        b[0] / b[1] > a[0] / a[1] && b[2] / b[1] > a[2] / a[1],
        "neutral={a:?}, positive={b:?}"
    );
    assert!(temperature_white(f32::NAN, 0.0).is_err());
    assert!(
        white_balance_matrix(
            &WhiteBalanceSettings::default(),
            ColorMatrix3::IDENTITY,
            [0.0; 4]
        )
        .is_err()
    );
}
#[test]
fn unsupported_algorithms_are_not_silently_accepted() {
    let im = Image::new(1, 1, vec![vec![0.2]; 3]).unwrap();
    let src = RenderSource::Rgb(&im);
    let mut s = DevelopSettings::default();
    s.demosaic.method = DemosaicMethod::Rcd;
    assert!(render(&s, &src).is_err());
    s = DevelopSettings::default();
    s.linearize.highlight_reconstruction = HighlightReconstruction::Inpaint;
    assert!(render(&s, &src).is_err());
    s = DevelopSettings::default();
    s.tone.clarity = 1.0;
    assert!(render(&s, &src).is_ok());
    s.output.hdr = true;
    assert!(render(&s, &src).is_err());
}
#[test]
fn xtrans_mean_uses_global_phase_across_tile_boundaries() {
    let p = [
        [1, 0, 1, 1, 2, 1],
        [2, 1, 2, 0, 1, 0],
        [1, 0, 1, 1, 2, 1],
        [1, 2, 1, 1, 0, 1],
        [0, 1, 0, 2, 1, 2],
        [1, 2, 1, 1, 0, 1],
    ];
    let cfa = CfaLayout::XTrans(p);
    let values = [0.2, 0.4, 0.6];
    let data = (0..259 * 7)
        .map(|i| values[cfa.channel_at(i % 259, i / 259)])
        .collect();
    let im = Image::new(259, 7, vec![data]).unwrap();
    for coord in im.coords() {
        let t = demosaic(
            &im.tile(coord, 3, 6).unwrap(),
            cfa,
            DemosaicAlgorithm::MalvarHeCutler,
        )
        .unwrap();
        for (c, value) in values.iter().enumerate() {
            for v in t.plane::<f32>(c as u8).unwrap() {
                assert!((v - value).abs() < 1e-6);
            }
        }
    }
}
#[test]
fn bayer_mhc_all_kernel_taps_match_paper() {
    // Fig. 2: all coefficients are divided by eight. Independent impulse oracle.
    let green = [
        0., 0., -1., 0., 0., 0., 0., 2., 0., 0., -1., 2., 4., 2., -1., 0., 0., 2., 0., 0., 0., 0.,
        -1., 0., 0.,
    ];
    let opposite = [
        0., 0., -1.5, 0., 0., 0., 2., 0., 2., 0., -1.5, 0., 6., 0., -1.5, 0., 2., 0., 2., 0., 0.,
        0., -1.5, 0., 0.,
    ];
    let horizontal = [
        0., 0., 0.5, 0., 0., 0., -1., 0., -1., 0., -1., 4., 5., 4., -1., 0., -1., 0., -1., 0., 0.,
        0., 0.5, 0., 0.,
    ];
    for pattern in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 1], [3, 0]],
        [[1, 2], [0, 3]],
    ] {
        let cfa = CfaLayout::Bayer(pattern);
        let known = cfa.channel_at(0, 0);
        let known = if known == 3 { 1 } else { known };
        for tap in 0..25 {
            let mut data = vec![0.; 25];
            data[tap] = 1.;
            let t = Tile::from_samples(
                TileCoord::new(0, 0, 0),
                TileLayout {
                    extent: Extent::new(1, 1),
                    halo: 2,
                    channels: 1,
                },
                data,
            )
            .unwrap();
            let out = demosaic(&t, cfa, DemosaicAlgorithm::MalvarHeCutler).unwrap();
            for c in 0..3 {
                let expected = if c == known {
                    if tap == 12 { 1. } else { 0. }
                } else if c == 1 {
                    green[tap] / 8.
                } else if known != 1 {
                    opposite[tap] / 8.
                } else if cfa.channel_at(1, 0) == c {
                    horizontal[tap] / 8.
                } else {
                    horizontal[(tap % 5) * 5 + tap / 5] / 8.
                };
                assert_eq!(
                    out.plane::<f32>(c as u8).unwrap()[0],
                    expected,
                    "phase={pattern:?}, tap={tap}, channel={c}"
                );
            }
        }
    }
}
