use engine_api::{
    jobs::CancellationToken,
    recipe::settings::{GeometrySettings, NormalizedRect},
    stage::StageId,
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

// Compile the isolated module before parent routing is integrated.
#[path = "../src/geometry.rs"]
mod isolated_geometry;

#[test]
fn isolated_geometry_matches_cpu() {
    let ctx = GpuContext::new().unwrap();
    for (w, h) in [(1, 1), (2, 7), (37, 29), (513, 17)] {
        for channels in [1, 3] {
            for angle in [0., 13.7, -31., 45., -45.] {
                let input = fixture(w, h, channels);
                let mut s = GeometrySettings::default();
                s.crop.angle = angle;
                if angle == 0. || angle == 13.7 {
                    s.crop.rect = NormalizedRect {
                        left: 0.037,
                        top: 0.081,
                        right: 0.913,
                        bottom: 0.957,
                    };
                }
                let expected = CpuStageOp
                    .run_image(
                        StageId::Geometry,
                        &Op::Geometry(&s),
                        input.clone(),
                        &CancellationToken::new(),
                    )
                    .unwrap();
                let actual = isolated_geometry::run(&ctx, &input, &s).unwrap();
                assert_eq!(
                    (actual.width(), actual.height()),
                    (expected.width(), expected.height())
                );
                let error = actual
                    .planes()
                    .iter()
                    .flatten()
                    .zip(expected.planes().iter().flatten())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    error <= 1e-4,
                    "{w}x{h} channels={channels} angle={angle}: max error={error}"
                );
            }
        }
    }
}

#[test]
fn isolated_geometry_identity_validation_and_extremes() {
    use engine_api::recipe::settings::{GuideLine, UprightMode};
    let ctx = GpuContext::new().unwrap();
    let input = Image::new(2, 2, vec![vec![-0.0, f32::MIN_POSITIVE, -2., f32::MAX]; 3]).unwrap();
    let mut s = GeometrySettings::default();
    s.crop.aspect = Some([16, 9]);
    let actual = isolated_geometry::run(&ctx, &input, &s).unwrap();
    assert_eq!(
        actual
            .planes()
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>(),
        input
            .planes()
            .iter()
            .flatten()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>()
    );
    for case in 0..12 {
        let mut s = GeometrySettings::default();
        match case {
            0 => s.crop.angle = f32::NAN,
            1 => s.crop.angle = f32::INFINITY,
            2 => s.crop.angle = 45.01,
            3 => s.crop.angle = -45.01,
            4 => s.crop.rect.left = s.crop.rect.right,
            5 => s.crop.rect.top = -0.01,
            6 => s.crop.aspect = Some([0, 1]),
            7 => s.orientation = 2,
            8 => s.upright.mode = UprightMode::Auto,
            9 => s.upright.guides.push(GuideLine::default()),
            10 => s.transform.scale = 99.,
            _ => {
                s.constrain_crop = true;
                s.crop.angle = f32::NAN;
            }
        }
        let cpu = CpuStageOp
            .run_image(
                StageId::Geometry,
                &Op::Geometry(&s),
                input.clone(),
                &CancellationToken::new(),
            )
            .unwrap_err();
        let gpu = isolated_geometry::run(&ctx, &input, &s).unwrap_err();
        assert_eq!(format!("{cpu:?}"), format!("{gpu:?}"));
    }
    let input = Image::new(9, 9, vec![vec![f32::MAX; 81]]).unwrap();
    let mut s = GeometrySettings::default();
    s.crop.angle = 13.;
    let actual = isolated_geometry::run(&ctx, &input, &s).unwrap();
    assert!(actual.planes()[0].iter().all(|v| v.is_finite()));
    assert!(actual.planes()[0][40] > f32::MAX * 0.99);
}

fn fixture(w: u32, h: u32, channels: usize) -> Image {
    Image::new(
        w,
        h,
        (0..channels)
            .map(|c| {
                (0..w * h)
                    .map(|i| ((i * 37 + c as u32 * 19) % 103) as f32 / 31.0 - 0.5)
                    .collect()
            })
            .collect(),
    )
    .unwrap()
}

#[test]
fn active_geometry_submits_once_and_matches_reference() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    for channels in [1, 3] {
        for angle in [0., 13.7, -31., 45., -45.] {
            let input = fixture(37, 29, channels);
            let mut s = GeometrySettings::default();
            s.crop.angle = angle;
            if angle == 0. || angle == 13.7 {
                s.crop.rect = NormalizedRect {
                    left: 0.037,
                    top: 0.081,
                    right: 0.913,
                    bottom: 0.957,
                };
            }
            let cancel = CancellationToken::new();
            let expected = CpuStageOp
                .run_image(StageId::Geometry, &Op::Geometry(&s), input.clone(), &cancel)
                .unwrap();
            let before = gpu.stats();
            let actual = gpu
                .run_image(StageId::Geometry, &Op::Geometry(&s), input, &cancel)
                .unwrap();
            assert_eq!(
                gpu.stats().submissions - before.submissions,
                1,
                "active geometry must dispatch compute"
            );
            assert_eq!(gpu.stats().uploads - before.uploads, 1);
            assert_eq!(gpu.stats().readbacks - before.readbacks, 1);
            assert_eq!(
                (actual.width(), actual.height()),
                (expected.width(), expected.height())
            );
            for (a, b) in actual
                .planes()
                .iter()
                .flatten()
                .zip(expected.planes().iter().flatten())
            {
                assert!(
                    (a - b).abs() <= 1e-4,
                    "angle={angle}, channels={channels}: {a} != {b}"
                );
            }
        }
    }
}
