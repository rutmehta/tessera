use engine_api::{jobs::CancellationToken, recipe::settings::ToneSettings, stage::StageId};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

// Compile and exercise the isolated module even before parent integration.
#[path = "../src/tone_local.rs"]
mod tone_local;

#[test]
fn isolated_local_tone_matches_cpu() {
    let ctx = GpuContext::new().unwrap();
    for (w, h) in [(273, 19), (1, 1), (1, 29), (35, 1)] {
        let input = Image::new(
            w,
            h,
            (0..3)
                .map(|c| {
                    (0..w * h)
                        .map(|i| {
                            0.2 + (i % w) as f32 * 0.002
                                + 0.025 * (i as f32 * 0.73 + c as f32).sin()
                        })
                        .collect()
                })
                .collect(),
        )
        .unwrap();
        for (texture, clarity, dehaze) in [
            (80., 0., 0.),
            (0., -75., 0.),
            (0., 0., 70.),
            (0., 0., -70.),
            (65., -45., 80.),
        ] {
            let s = ToneSettings {
                texture,
                clarity,
                dehaze,
                ..Default::default()
            };
            let actual = tone_local::run(&ctx, &input, &s).unwrap();
            let expected = CpuStageOp
                .run_image(
                    StageId::Tone,
                    &Op::ToneExtra(&s),
                    input.clone(),
                    &CancellationToken::new(),
                )
                .unwrap();
            let max = actual
                .planes()
                .iter()
                .flatten()
                .zip(expected.planes().iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0.0_f32, f32::max);
            assert!(
                max <= 1e-4,
                "{w}x{h} {texture}/{clarity}/{dehaze}: max error {max}"
            );
        }
    }
}

#[test]
fn isolated_local_tone_preserves_contracts() {
    use engine_api::recipe::settings::{Curve, CurvePoint};
    let ctx = GpuContext::new().unwrap();
    let input = Image::new(
        127,
        7,
        (0..3)
            .map(|c| {
                (0..889)
                    .map(|i| ((i * 19 + c * 73) % 500) as f32 / 300.0 - 0.15)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let mut s = ToneSettings::default();
    s.curves.red = Curve(vec![
        CurvePoint { x: 0.0, y: 0.2 },
        CurvePoint { x: 1.0, y: 1.0 },
    ]);
    assert_eq!(
        tone_local::run(&ctx, &input, &s).unwrap().planes(),
        input.planes(),
        "this module must not apply curves"
    );
    s.texture = f32::NAN;
    assert!(tone_local::run(&ctx, &input, &s).is_err());
    for amount in [-100.0, 100.0] {
        let s = ToneSettings {
            texture: amount,
            clarity: amount,
            dehaze: amount,
            ..Default::default()
        };
        let actual = tone_local::run(&ctx, &input, &s).unwrap();
        let expected = CpuStageOp
            .run_image(
                StageId::Tone,
                &Op::ToneExtra(&s),
                input.clone(),
                &CancellationToken::new(),
            )
            .unwrap();
        for (a, b) in actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
        {
            assert!(
                a.is_finite() && (a - b).abs() <= 1e-4,
                "signed/HDR {amount}: {a} vs {b}"
            );
        }
    }
}

#[test]
fn local_tone_uses_compute_and_matches_cpu() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let input = Image::new(
        273,
        19,
        (0..3)
            .map(|c| {
                (0..273 * 19)
                    .map(|i| {
                        0.2 + (i % 273) as f32 * 0.002 + 0.025 * (i as f32 * 0.73 + c as f32).sin()
                    })
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    for (texture, clarity, dehaze) in [
        (80., 0., 0.),
        (0., -75., 0.),
        (0., 0., 70.),
        (0., 0., -70.),
        (65., -45., 80.),
    ] {
        let s = ToneSettings {
            texture,
            clarity,
            dehaze,
            ..Default::default()
        };
        let before = gpu.stats().submissions;
        let actual = gpu
            .run_image(
                StageId::Tone,
                &Op::ToneExtra(&s),
                input.clone(),
                &CancellationToken::new(),
            )
            .unwrap();
        assert!(
            gpu.stats().submissions > before,
            "local tone must dispatch GPU compute"
        );
        let expected = CpuStageOp
            .run_image(
                StageId::Tone,
                &Op::ToneExtra(&s),
                input.clone(),
                &CancellationToken::new(),
            )
            .unwrap();
        for (a, b) in actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
        {
            assert!(
                a.is_finite() && (a - b).abs() <= 1e-4,
                "{texture}/{clarity}/{dehaze}: {a} != {b}"
            );
        }
    }
}
