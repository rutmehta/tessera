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
    assert!(gap <= 1e-3);
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
    let source = include_str!("../src/operators.wgsl")
        .replace("curve_value(y, 4u)", "bitcast<f32>(bitcast<u32>(y) + 1u)");
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
