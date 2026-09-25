use crate::{GpuContext, tone_local};
use engine_api::{jobs::CancellationToken, recipe::settings::ToneSettings, stage::StageId};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;

// Structural performance guard: numerical parity alone cannot catch a return
// to one global-memory window load per output pixel.
#[test]
fn local_means_use_cooperative_workgroup_storage() {
    let shader = include_str!("tone_local.wgsl");
    assert!(
        shader.contains("var<workgroup>"),
        "means must reuse shared halo loads"
    );
    assert!(
        shader.contains("workgroupBarrier()"),
        "halo loads must be synchronized"
    );
}

#[test]
fn local_pipelines_are_reused_and_device_scoped() {
    let first = GpuContext::new().unwrap();
    let second = GpuContext::new().unwrap();
    let a = tone_local::pipelines(&first).unwrap();
    let b = tone_local::pipelines(&first).unwrap();
    assert_eq!(a, b, "warm calls must not compile new pipelines");
    // wgpu resource equality is instance-local. Actual dispatch checks that
    // a second context never receives handles owned by the first instance.
    let input = Image::new(9, 9, vec![vec![0.25; 81]; 3]).unwrap();
    let settings = ToneSettings {
        clarity: 50.,
        ..Default::default()
    };
    tone_local::run(&second, &input, &settings).unwrap();
    assert_eq!(a, tone_local::pipelines(&first).unwrap());
}

#[test]
fn isolated_local_tone_matches_cpu() {
    let ctx = GpuContext::new().unwrap();
    for (w, h) in [
        (273, 19),
        (1, 1),
        (1, 29),
        (35, 1),
        (7, 9),
        (8, 8),
        (9, 17),
        (65, 63),
    ] {
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
