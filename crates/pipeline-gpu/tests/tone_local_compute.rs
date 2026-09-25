use engine_api::{jobs::CancellationToken, recipe::settings::ToneSettings, stage::StageId};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

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
        assert_eq!(
            gpu.stats().submissions - before,
            if dehaze != 0. { 3 } else { 1 },
            "neutral curves must not add a pixel roundtrip after presence"
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
