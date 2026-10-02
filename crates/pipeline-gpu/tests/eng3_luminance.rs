use engine_api::{
    recipe::settings::{Curve, CurvePoint, ToneSettings},
    stage::StageId,
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;
fn patch(rgb: [f32; 3]) -> Tile {
    Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(3, 3),
            halo: 0,
            channels: 3,
        },
        rgb.into_iter().flat_map(|v| [v; 9]).collect(),
    )
    .unwrap()
}
#[test]
fn eng3_curve_one_ulp_sensitivity() {
    let rgb = [1., -0.2627_f32 / 0.678, 1e-6];
    let mut next = rgb;
    next[1] = next[1].next_up();
    let mut s = ToneSettings::default();
    s.curves.luminance = Curve(vec![
        CurvePoint { x: 0., y: 0.1 },
        CurvePoint { x: 1., y: 1. },
    ]);
    let a = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&s), patch(rgb))
        .unwrap();
    let b = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&s), patch(next))
        .unwrap();
    let gap = a
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(b.samples::<f32>().unwrap())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    eprintln!("ENG3 curve one-ulp gap={gap}");
    assert!(gap <= 1e-5);
}
#[test]
fn eng3_curve_near_zero_gpu_parity() {
    let mut s = ToneSettings::default();
    s.curves.luminance = Curve(vec![
        CurvePoint { x: 0., y: 0.1 },
        CurvePoint { x: 1., y: 1. },
    ]);
    let input = patch([1., -0.2627_f32 / 0.678, 1e-6]);
    let a = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&s), input.clone())
        .unwrap();
    let b = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))
        .run(StageId::Tone, &Op::ToneExtra(&s), input)
        .unwrap();
    let gap = a
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(b.samples::<f32>().unwrap())
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    eprintln!("ENG3 curve GPU gap={gap}");
    assert!(gap <= 1e-4);
}
#[path = "support/eng3_compute.rs"]
mod compute;
#[test]
fn eng3_curve_zero_delta_vs_one_ulp() {
    let rgb = [1., -0.2627_f32 / 0.678, 1e-6];
    let y = 0.2627 * rgb[0] + 0.678 * rgb[1] + 0.0593 * rgb[2];
    assert!(y > 0. && y < 1e-3);
    // Exact identity mapping supplies the zero-delta CPU reference.
    let s = ToneSettings::default();
    let cpu = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&s), patch(rgb))
        .unwrap();
    for (c, v) in rgb.iter().enumerate() {
        assert_eq!(cpu.samples::<f32>().unwrap()[c * 9], *v);
    }
    // Inject ONLY the mapped luminance, one ulp above Y. Production gain and
    // branch logic stay intact; other curves remain identity via p counts=0.
    let src = include_str!("../src/operators.wgsl");
    assert!(src.contains("curve_value(y, 4u)"));
    let source = src.replace("curve_value(y, 4u)", "bitcast<f32>(bitcast<u32>(y) + 1u)");
    let source = format!(
        "{source}\n@compute @workgroup_size(1) fn eng3_regression() {{ let v=curves(vec3(p[40],p[41],p[42])); dst[0]=v.x;dst[1]=v.y;dst[2]=v.z;dst[3]=0.0; }}"
    );
    let mut p = [0.; 43];
    p[23] = 1.;
    p[40..43].copy_from_slice(&rgb);
    let gpu = compute::run(source, &p, 2, 1);
    let gap = (0..3)
        .map(|c| (gpu[c] - cpu.samples::<f32>().unwrap()[c * 9]).abs())
        .fold(0f32, f32::max);
    eprintln!(
        "ENG3 curve zero/ulp L={y} delta={} gap={gap}",
        y.next_up() - y
    );
    assert!(gap <= 1e-8);
}
#[test]
fn eng3_curve_signed_floor_boundaries() {
    use engine_api::recipe::settings::ToneCurves;
    let s = ToneSettings {
        curves_extended: Some(ToneCurves {
            luminance: Curve(vec![
                CurvePoint { x: -1., y: 0.1 },
                CurvePoint { x: 1., y: 0.1 },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    // Flat luminance segment gives an independently known mapped luminance.
    let mapped = 0.18_f64 * ((0.1_f32 as f64) * (1.0_f64 / 0.18).ln_1p()).exp_m1();
    for lum in [
        -0.002_f32, -0.001001, -0.000999, -1e-6, 1e-6, 0.000999, 0.001001, 0.002,
    ] {
        let rgb = [lum; 3];
        let input = patch(rgb);
        let cpu = CpuStageOp
            .run(StageId::Tone, &Op::ToneExtra(&s), input.clone())
            .unwrap();
        let actual = gpu.run(StageId::Tone, &Op::ToneExtra(&s), input).unwrap();
        // Neutral signed greys do not cancel: retain the original ratio.
        let expected = mapped;
        for c in 0..3 {
            let a = cpu.samples::<f32>().unwrap()[c * 9];
            let b = actual.samples::<f32>().unwrap()[c * 9];
            assert!(
                (f64::from(a) - expected).abs() < 1e-7,
                "CPU L={lum}: {a} vs {expected}"
            );
            assert!((a - b).abs() < 1e-7, "GPU L={lum}: {a} vs {b}");
        }
    }
}

fn lifted() -> ToneSettings {
    let mut s = ToneSettings::default();
    s.curves.luminance = Curve(vec![
        CurvePoint { x: 0., y: 0.1 },
        CurvePoint { x: 1., y: 1. },
    ]);
    s
}
fn monotone_evidence(label: &str, pixels: &[[f32; 3]]) {
    let n = pixels.len();
    let input = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(n as u32, 1),
            halo: 0,
            channels: 3,
        },
        (0..3)
            .flat_map(|c| pixels.iter().map(move |p| p[c]))
            .collect(),
    )
    .unwrap();
    let s = lifted();
    let cpu = CpuStageOp
        .run(StageId::Tone, &Op::ToneExtra(&s), input.clone())
        .unwrap();
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()))
        .run(StageId::Tone, &Op::ToneExtra(&s), input)
        .unwrap();
    let a = cpu.samples::<f32>().unwrap();
    let b = gpu.samples::<f32>().unwrap();
    let ys: Vec<_> = (0..n)
        .map(|i| 0.2627 * a[i] + 0.678 * a[n + i] + 0.0593 * a[2 * n + i])
        .collect();
    let min_step = ys
        .windows(2)
        .map(|v| v[1] - v[0])
        .fold(f32::INFINITY, f32::min);
    let gap = a
        .iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0f32, f32::max);
    eprintln!(
        "ENG3b {label}: n={n} black={} first={} last={} min_step={min_step:e} gpu_gap={gap:e}",
        ys[0],
        ys[1],
        ys[n - 1]
    );
    assert!(min_step >= -1e-7, "near-black dip: {min_step}");
    assert!(gap < 2e-6);
    // Independent f64 oracle for a straight line in the curve's log domain.
    for (i, p) in pixels.iter().enumerate() {
        let y = 0.2627 * p[0] as f64 + 0.678 * p[1] as f64 + 0.0593 * p[2] as f64;
        let log_white = (1f64 / 0.18).ln_1p();
        let expected = 0.18 * (0.1 * log_white + 0.9 * (y / 0.18).ln_1p()).exp_m1();
        assert!((ys[i] as f64 - expected).abs() < 2e-7);
    }
}
#[test]
fn eng3b_lifted_black_gradient_monotone() {
    let pixels: Vec<_> = (0..256).map(|i| [i as f32 * 0.002 / 255.; 3]).collect();
    monotone_evidence("synthetic gradient 0..0.002", &pixels);
}
#[test]
fn eng3b_lifted_black_raw_monotone() {
    use pipeline_cpu::{RenderSource, render_linear_scaled};
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
    let mut raw = raw_decode::RawSource::open(&path).unwrap();
    let cfa = raw.decode_cfa().unwrap();
    let metadata = raw.metadata();
    let image = render_linear_scaled(
        &Default::default(),
        &RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        8,
    )
    .unwrap();
    let mut pixels = Vec::new();
    for coord in image.coords() {
        let tile = image.tile(coord, 0, 1).unwrap();
        let n = tile.layout().plane_len();
        let d = tile.samples::<f32>().unwrap();
        for i in 0..n {
            let p = [d[i], d[n + i], d[2 * n + i]];
            if p.iter().all(|v| v.is_finite() && *v > 0.) {
                pixels.push(p);
            }
        }
    }
    let luma = |p: &[f32; 3]| 0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2];
    pixels.sort_by(|a, b| luma(a).total_cmp(&luma(b)));
    assert!(pixels.len() > 256);
    // Preserve photographic chromaticities; expose the darkest 255 samples
    // into the regression interval with one uniform exposure multiplier.
    pixels.truncate(255);
    let scale = 0.0002 / luma(pixels.last().unwrap());
    for p in &mut pixels {
        for v in p {
            *v *= scale;
        }
    }
    pixels.sort_by(|a, b| luma(a).total_cmp(&luma(b)));
    pixels.insert(0, [0.; 3]);
    eprintln!("ENG3b RAW exposure multiplier={scale:e}");
    monotone_evidence("photographic RAW darkest 255 + black", &pixels);
}
#[test]
fn eng3b_coloured_curve_zero_crossing_documented() {
    use engine_api::recipe::settings::ToneCurves;
    let s = ToneSettings {
        curves_extended: Some(ToneCurves {
            luminance: Curve(vec![
                CurvePoint { x: -1., y: 0.1 },
                CurvePoint { x: 1., y: 0.1 },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mut gains = Vec::new();
    for delta in [-1e-6, 1e-6] {
        let rgb = [1., -0.2627_f32 / 0.678, delta / 0.0593];
        let input = patch(rgb);
        let a = CpuStageOp
            .run(StageId::Tone, &Op::ToneExtra(&s), input.clone())
            .unwrap();
        let b = gpu.run(StageId::Tone, &Op::ToneExtra(&s), input).unwrap();
        let gain = a.samples::<f32>().unwrap()[0];
        assert!((gain - b.samples::<f32>().unwrap()[0]).abs() < 1e-4);
        gains.push(gain);
    }
    eprintln!("ENG3b coloured curve crossing gains={gains:?}");
    assert!(gains[0] < -30. && gains[1] > 30.); // Retained signed-floor discontinuity.
}

#[test]
fn eng3b_curve_relative_cancellation_boundary() {
    use engine_api::recipe::settings::ToneCurves;
    let s = ToneSettings {
        curves_extended: Some(ToneCurves {
            luminance: Curve(vec![
                CurvePoint { x: -1., y: 0.1 },
                CurvePoint { x: 1., y: 0.1 },
            ]),
            ..Default::default()
        }),
        ..Default::default()
    };
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let mapped = 0.18_f64 * ((0.1_f32 as f64) * (1.0_f64 / 0.18).ln_1p()).exp_m1();
    for sign in [-1., 1.] {
        for ratio in [0.249, 0.251] {
            let r = sign * 0.002;
            let g = (ratio * r - 0.2627 * r) / 0.678;
            let rgb = [r, g, 0.];
            let y = 0.2627 * r + 0.678 * g;
            let gain = if ratio < 0.25 {
                1. + (mapped - y as f64) / 0.001_f64.copysign(y as f64)
            } else {
                mapped / y as f64
            };
            let input = patch(rgb);
            let a = CpuStageOp
                .run(StageId::Tone, &Op::ToneExtra(&s), input.clone())
                .unwrap();
            let b = gpu.run(StageId::Tone, &Op::ToneExtra(&s), input).unwrap();
            for c in 0..3 {
                let actual = a.samples::<f32>().unwrap()[c * 9];
                assert!((actual as f64 - rgb[c] as f64 * gain).abs() < 1e-7);
                assert!((actual - b.samples::<f32>().unwrap()[c * 9]).abs() < 1e-7);
            }
        }
    }
}
