use engine_api::{
    jobs::CancellationToken,
    recipe::settings::{Curve, CurvePoint, ToneSettings},
    stage::StageId,
    tile::TileCoord,
};
use image_core::{CpuStageOp, Op, StageOp};
use pipeline_cpu::Image;
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::sync::Arc;

#[test]
fn color_black_and_signed_neutrals_stay_finite() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let s = engine_api::recipe::settings::ColorSettings {
        vibrance: 35.0,
        ..Default::default()
    };
    for value in [0.0, -0.0, 1e-10, -0.1, 0.5, 4.0] {
        let image = Image::new(1, 1, vec![vec![value]; 3]).unwrap();
        let tile = image.tile(TileCoord::new(0, 0, 0), 0, 1).unwrap();
        let actual = gpu
            .run(StageId::Tone, &Op::Color(&s), tile.clone())
            .unwrap();
        let expected = CpuStageOp.run(StageId::Tone, &Op::Color(&s), tile).unwrap();
        for (a, b) in actual
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(expected.samples::<f32>().unwrap())
        {
            assert!(
                a.is_finite() && (a - b).abs() <= 1e-4,
                "input {value}, actual {a}, expected {b}"
            );
        }
    }
}

#[test]
fn color_controls_use_compute_and_match_reference() {
    use engine_api::recipe::settings::ColorSettings;
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let image = Image::new(
        127,
        3,
        (0..3)
            .map(|c| {
                (0..381)
                    .map(|i| ((i * 19 + c * 73) % 500) as f32 / 300.0 - 0.15)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    for amount in [-100.0, 0.0, 100.0] {
        let mut s = ColorSettings {
            vibrance: amount,
            saturation: amount / 2.0,
            ..Default::default()
        };
        s.hsl.hue.red = amount;
        s.hsl.saturation.blue = amount;
        s.hsl.luminance.green = amount;
        s.grading.shadows.hue = 35.0;
        s.grading.shadows.saturation = 45.0;
        s.grading.highlights.hue = 220.0;
        s.grading.highlights.saturation = 60.0;
        s.grading.global.luminance = amount / 3.0;
        let op = Op::Color(&s);
        let tile = image.tile(TileCoord::new(0, 0, 0), 2, 1).unwrap();
        let expected = CpuStageOp.run(StageId::Tone, &op, tile.clone()).unwrap();
        let actual = gpu.run(StageId::Tone, &op, tile.clone()).unwrap();
        let again = gpu.run(StageId::Tone, &op, tile).unwrap();
        assert_eq!(
            actual.samples::<f32>().unwrap(),
            again.samples::<f32>().unwrap()
        );
        for (a, b) in actual
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(expected.samples::<f32>().unwrap())
        {
            assert!((a - b).abs() <= 1e-4, "color {amount}: {a} != {b}");
        }
    }
    assert_eq!(gpu.stats().submissions, 6);
}

#[test]
fn curves_execute_on_gpu_for_tiles_and_images() {
    let gpu = GpuStageOp::new(Arc::new(GpuContext::new().unwrap()));
    let image = Image::new(
        513,
        3,
        (0..3)
            .map(|c| {
                (0..1539)
                    .map(|i| ((i * 17 + c * 97) % 1000) as f32 / 499.0 - 0.05)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let mut s = ToneSettings::default();
    s.curves.rgb = Curve(vec![
        CurvePoint { x: 0.0, y: 0.03 },
        CurvePoint { x: 0.4, y: 0.6 },
        CurvePoint { x: 1.0, y: 1.0 },
    ]);
    s.curves.blue = s.curves.rgb.clone();
    s.curves.luminance = s.curves.rgb.clone();
    s.curves.parametric.shadows = 75.0;
    s.curves.parametric.darks = -60.0;
    s.curves.parametric.lights = 40.0;
    s.curves.parametric.highlights = -80.0;
    let op = Op::ToneExtra(&s);
    let tile = image.tile(TileCoord::new(0, 0, 0), 2, 1).unwrap();
    let expected = CpuStageOp.run(StageId::Tone, &op, tile.clone()).unwrap();
    let actual = gpu.run(StageId::Tone, &op, tile).unwrap();
    for (a, b) in actual
        .samples::<f32>()
        .unwrap()
        .iter()
        .zip(expected.samples::<f32>().unwrap())
    {
        assert!((a - b).abs() <= 1e-4, "{a} != {b}");
    }
    assert_eq!(
        gpu.stats().submissions,
        1,
        "curves must not silently use CPU"
    );
    let token = CancellationToken::new();
    let expected = CpuStageOp
        .run_image(StageId::Tone, &op, image.clone(), &token)
        .unwrap();
    let actual = gpu
        .run_image(StageId::Tone, &op, image.clone(), &token)
        .unwrap();
    assert!(gpu.stats().submissions > 1);
    let again = gpu.run_image(StageId::Tone, &op, image, &token).unwrap();
    assert_eq!(actual.planes(), again.planes());
    for (a, b) in actual
        .planes()
        .iter()
        .flatten()
        .zip(expected.planes().iter().flatten())
    {
        assert!((a - b).abs() <= 1e-4, "{a} != {b}");
    }
}
