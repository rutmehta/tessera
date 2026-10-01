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
        compare(
            StageId::Output,
            Op::Display {
                gamut,
                headroom: None,
            },
            tile(3, 2),
        );
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
                headroom: None,
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

/// B5-32: sharpening can leave signed RGB with nearly cancelling luminance.
/// Tone's gain must approach its derivative at black, rather than magnifying
/// cancellation in softplus(z-center) - softplus(-center).
/// Diagnostic pending A/Codex engine work: clamp the texture/clarity signed
/// luminance divisor or evaluate presence before sharpening, then reassess tone
/// precision. Baseline tone errors: CPU 1.8405e-5, GPU 0.00511713; rich-chain max
/// 0.0205 and 24 MP full-frame max 6.14. The dropped tone fix changed Develop
/// output globally and worsened the 24 MP maximum to 16.49.
#[test]
#[ignore = "engine follow-up: signed-luminance conditioning; main exceeds f64-reference bound"]
fn tone_signed_rgb_near_zero_luminance_matches_f64_reference() {
    use engine_api::recipe::settings::ToneSettings;
    let settings = ToneSettings {
        exposure: 0.3,
        contrast: 11.,
        highlights: -17.,
        shadows: 9.,
        whites: 5.,
        blacks: -3.,
        ..Default::default()
    };
    let pixels: Vec<[f32; 3]> = [1e-7f32, 4e-7, 1e-6, 1e-5]
        .map(|y| {
            [
                -0.007,
                -0.006,
                (y + 0.2627 * 0.007 + 0.678 * 0.006) / 0.0593,
            ]
        })
        .to_vec();
    let layout = TileLayout {
        extent: Extent::new(4, 1),
        halo: 0,
        channels: 3,
    };
    let data = (0..3)
        .flat_map(|c| pixels.iter().map(move |p| p[c]))
        .collect();
    let input = Tile::from_samples(TileCoord::new(0, 0, 0), layout, data).unwrap();
    let expected: Vec<[f64; 3]> = pixels
        .iter()
        .map(|p| {
            // Isolate evaluation of the tone function from input rounding: the
            // exposure and luminance boundary is f32 in both real backends.
            let rgb = p.map(|v| v * settings.exposure.exp2());
            let y = (0.2627 * rgb[0] + 0.678 * rgb[1] + 0.0593 * rgb[2]) as f64;
            assert!(y > 0.);
            let initial = (y / 0.18).ln_1p();
            let slope = (f64::from(settings.contrast) / 100.).exp2();
            let z = slope * initial + (1. - slope) * 2. * 2f64.ln() * -(-initial).exp_m1();
            let softplus = |v: f64| v.max(0.) + (-v.abs()).exp().ln_1p();
            let mut out = z;
            for (amount, center, upper) in [
                (-3., 0.25, false),
                (9., 0.8, false),
                (-17., 1.5, true),
                (5., 2.5, true),
            ] {
                let integral = softplus(z - center) - softplus(-center);
                out += 0.2 * amount / 100. * if upper { integral } else { z - integral };
            }
            rgb.map(|v| f64::from(v) * 0.18 * out.exp_m1() / y)
        })
        .collect();
    let cpu = CpuStageOp
        .run(StageId::Tone, &Op::Tone(&settings), input.clone())
        .unwrap();
    let actual = gpu()
        .run(StageId::Tone, &Op::Tone(&settings), input)
        .unwrap();
    let mut failures = Vec::new();
    for (name, result) in [("CPU", cpu), ("GPU", actual)] {
        let samples = result.samples::<f32>().unwrap();
        let mut error = 0f64;
        for (i, rgb) in expected.iter().enumerate() {
            for c in 0..3 {
                error = error.max((f64::from(samples[c * 4 + i]) - rgb[c]).abs());
            }
        }
        eprintln!("near-zero signed tone {name}: max absolute={error}");
        if error > 1e-5 {
            failures.push((name, error));
        }
    }
    assert!(failures.is_empty(), "tone cancellation: {failures:?}");
}

#[test]
fn lr2b_monochrome_gpu_matches_cpu() {
    use engine_api::recipe::settings::{ColorSettings, HueBands, MonochromeSettings};
    let mut s = ColorSettings {
        monochrome: Some(MonochromeSettings {
            enabled: true,
            mixer: HueBands {
                red: 50.,
                orange: -20.,
                yellow: 30.,
                green: -40.,
                aqua: 70.,
                blue: -80.,
                purple: 90.,
                magenta: -10.,
            },
        }),
        ..Default::default()
    };
    compare(StageId::Color, Op::Color(&s), tile(3, 2));
    s.saturation = 20.;
    s.grading.highlights.saturation = 10.;
    compare(StageId::Color, Op::Color(&s), tile(3, 2));
}
