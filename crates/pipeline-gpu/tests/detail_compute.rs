use engine_api::{
    recipe::settings::DetailSettings,
    stage::StageId,
    tile::{Tile, TileCoord},
};
use image_core::{Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

// Compile the isolated production module without touching the parent's integration.
#[path = "../src/detail.rs"]
mod detail;

#[test]
fn isolated_detail_matches_reference_and_preserves_halo() {
    let ctx = GpuContext::new().unwrap();
    let input = sample();
    for mode in 0..8 {
        let mut s = DetailSettings::default();
        if mode & 1 != 0 {
            s.sharpening.amount = 150.0;
            s.sharpening.radius = 3.0;
            s.sharpening.detail = 83.0;
            s.sharpening.masking = 40.0;
        }
        if mode & 2 != 0 {
            s.noise_reduction.luminance = 85.0;
            s.noise_reduction.luminance_detail = 15.0;
            s.noise_reduction.luminance_contrast = 30.0;
        }
        if mode & 4 != 0 {
            s.noise_reduction.color = 100.0;
            s.noise_reduction.color_detail = 20.0;
            s.noise_reduction.color_smoothness = 100.0;
        }
        let expected = image_core::CpuStageOp
            .run(StageId::Tone, &Op::Detail(&s), input.clone())
            .unwrap();
        let actual = detail::run(&ctx, &input, &s).unwrap();
        let again = detail::run(&ctx, &input, &s).unwrap();
        assert_eq!(
            actual.samples::<f32>().unwrap(),
            again.samples::<f32>().unwrap()
        );
        let mut max_error = 0.0_f32;
        for (i, (&a, &b)) in actual
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(expected.samples::<f32>().unwrap())
            .enumerate()
        {
            max_error = max_error.max((a - b).abs());
            assert!(
                a.is_finite() && (a - b).abs() <= 1e-4,
                "mode {mode}, sample {i}: GPU {a}, CPU {b}"
            );
            let l = input.layout();
            let j = i % l.plane_len();
            let x = j % l.stride();
            let y = j / l.stride();
            if x < l.halo as usize
                || y < l.halo as usize
                || x >= l.halo as usize + l.extent.width as usize
                || y >= l.halo as usize + l.extent.height as usize
            {
                assert_eq!(a.to_bits(), input.samples::<f32>().unwrap()[i].to_bits());
            }
        }
        eprintln!("Detail mode {mode}: max error {max_error}");
    }
}

#[test]
fn isolated_validation_matches_cpu_before_any_write() {
    let ctx = GpuContext::new().unwrap();
    let input = sample();
    let compare_error = |tile: &Tile, s: &DetailSettings| {
        let expected = image_core::CpuStageOp
            .run(StageId::Tone, &Op::Detail(s), tile.clone())
            .unwrap_err();
        let actual = detail::run(&ctx, tile, s).unwrap_err();
        assert_eq!(actual.to_string(), expected.to_string());
    };
    for index in 0..10 {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0, 151.0] {
            let mut s = DetailSettings::default();
            let controls = [
                &mut s.sharpening.amount,
                &mut s.sharpening.radius,
                &mut s.sharpening.detail,
                &mut s.sharpening.masking,
                &mut s.noise_reduction.luminance,
                &mut s.noise_reduction.luminance_detail,
                &mut s.noise_reduction.luminance_contrast,
                &mut s.noise_reduction.color,
                &mut s.noise_reduction.color_detail,
                &mut s.noise_reduction.color_smoothness,
            ];
            *controls[index] = value;
            compare_error(&input, &s);
        }
    }
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut invalid = input.clone();
        invalid.samples_mut::<f32>().unwrap()[0] = value;
        compare_error(&invalid, &DetailSettings::default());
    }
    let coord = TileCoord::new(0, 0, 0);
    let mono = Image::new(1, 1, vec![vec![0.2]])
        .unwrap()
        .tile(coord, 0, 1)
        .unwrap();
    compare_error(&mono, &DetailSettings::default());
    let bytes = Tile::from_samples(
        coord,
        input.layout(),
        vec![0_u8; input.layout().plane_len() * 3],
    )
    .unwrap();
    compare_error(&bytes, &DetailSettings::default());
    let small = Image::new(1, 1, vec![vec![0.2]; 3])
        .unwrap()
        .tile(coord, 0, 1)
        .unwrap();
    let mut s = DetailSettings::default();
    s.sharpening.amount = 80.0;
    compare_error(&small, &s);
    assert_eq!(
        input.samples::<f32>().unwrap(),
        sample().samples::<f32>().unwrap()
    );
}

#[test]
fn isolated_neutral_signed_zero_bits_and_explicit_disable() {
    let ctx = GpuContext::new().unwrap();
    let input = Image::new(4, 1, vec![vec![-0.0, -0.1, 4.0, f32::from_bits(1)]; 3])
        .unwrap()
        .tile(TileCoord::new(0, 0, 0), 0, 1)
        .unwrap();
    let defaults = DetailSettings::default();
    let mut disabled = defaults.clone();
    disabled.sharpening.amount = 0.0;
    disabled.sharpening.radius = 3.0;
    disabled.noise_reduction.color = 0.0;
    disabled.noise_reduction.color_detail = 90.0;
    assert!(detail::run(&ctx, &input, &defaults).is_err()); // Active defaults require a halo.
    {
        let s = &disabled;
        let actual = detail::run(&ctx, &input, s).unwrap();
        assert_eq!(
            actual
                .samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            input
                .samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn isolated_signed_hdr_smooth_noise_and_control_endpoints() {
    let ctx = GpuContext::new().unwrap();
    for base in [-2.0, -0.1, 0.0, 0.3, 4.0, 10.0] {
        let input = Image::new(
            9,
            7,
            (0..3)
                .map(|c| {
                    (0..63)
                        .map(|i| base + ((i * 13 + c * 19) % 17) as f32 * 0.002 - 0.016)
                        .collect()
                })
                .collect(),
        )
        .unwrap()
        .tile(TileCoord::new(0, 0, 0), 9, 1)
        .unwrap();
        for endpoint in [0.0, 100.0] {
            let mut s = DetailSettings::default();
            s.sharpening.amount = 150.0;
            s.sharpening.radius = if endpoint == 0.0 { 0.5 } else { 3.0 };
            s.sharpening.detail = endpoint;
            s.sharpening.masking = endpoint;
            s.noise_reduction.luminance = 100.0;
            s.noise_reduction.luminance_detail = endpoint;
            s.noise_reduction.luminance_contrast = endpoint;
            s.noise_reduction.color = 100.0;
            s.noise_reduction.color_detail = endpoint;
            s.noise_reduction.color_smoothness = endpoint;
            let actual = detail::run(&ctx, &input, &s).unwrap();
            let expected = image_core::CpuStageOp
                .run(StageId::Tone, &Op::Detail(&s), input.clone())
                .unwrap();
            for (&a, &b) in actual
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(expected.samples::<f32>().unwrap())
            {
                assert!(
                    a.is_finite() && (a - b).abs() <= 1e-4,
                    "base {base}, endpoint {endpoint}: {a} != {b}"
                );
            }
        }
    }
}

#[test]
fn isolated_partition_invariance_with_real_neighbors() {
    use engine_api::tile::{Extent, TileLayout};
    let ctx = GpuContext::new().unwrap();
    let make = |ox: i32, width| {
        let l = TileLayout {
            extent: Extent::new(width, 3),
            halo: 9,
            channels: 3,
        };
        let mut data = Vec::new();
        for c in 0..3 {
            for y in -9..12 {
                for x in -9..width as i32 + 9 {
                    data.push(0.2 + ((x + ox) * 7 + y * 3 + c * 5).rem_euclid(11) as f32 * 0.01);
                }
            }
        }
        Tile::from_samples(TileCoord::new(0, 0, 0), l, data).unwrap()
    };
    let mut s = DetailSettings::default();
    s.sharpening.amount = 80.0;
    s.sharpening.radius = 3.0;
    s.sharpening.masking = 10.0;
    s.noise_reduction.luminance = 40.0;
    s.noise_reduction.color = 60.0;
    let full = detail::run(&ctx, &make(0, 16), &s).unwrap();
    for ox in [0, 8] {
        let part = detail::run(&ctx, &make(ox, 8), &s).unwrap();
        for c in 0..3 {
            for y in 0..3 {
                for x in 0..8 {
                    let a = part.samples::<f32>().unwrap()[part.layout().index(c, x, y).unwrap()];
                    let b =
                        full.samples::<f32>().unwrap()[full.layout().index(c, x + ox, y).unwrap()];
                    assert_eq!(a.to_bits(), b.to_bits());
                }
            }
        }
    }
}

fn sample() -> Tile {
    Image::new(
        17,
        5,
        (0..3)
            .map(|c| {
                (0..85)
                    .map(|i| ((i * 17 + c * 31) % 101) as f32 / 25.0 - 0.5)
                    .collect()
            })
            .collect(),
    )
    .unwrap()
    .tile(TileCoord::new(0, 0, 0), 9, 1)
    .unwrap()
}

#[test]
fn detail_public_route_submits_compute() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut settings = DetailSettings::default();
    settings.sharpening.amount = 80.0;
    gpu.run(StageId::Tone, &Op::Detail(&settings), sample())
        .unwrap();
    assert_eq!(
        gpu.stats().submissions,
        1,
        "Detail must submit compute, not CPU fallback"
    );
}
