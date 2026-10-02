//! LR-3b: supported heal/clone recipes must reach the Develop CPU renderer.
use engine_api::{
    id::RetouchId,
    recipe::{
        DevelopSettings, MaskComponent, MaskKind,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
    },
};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

#[test]
fn spots_without_registered_renderer_fail_explicitly() {
    let plane: Vec<f32> = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 0.8 } else { 0.1 }))
        .collect();
    let image = Image::new(128, 80, vec![plane; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    let baseline = render_linear_scaled(&settings, &RenderSource::Rgb(&image), 1).unwrap();
    settings.locals.retouch = [
        RetouchKind::Clone {
            source_offset: [0.5, 0.0],
        },
        RetouchKind::Heal {
            source_offset: [0.5, 0.0],
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| RetouchOperation {
        id: RetouchId(i as u32),
        kind,
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.3 + i as f32 * 0.4, 1.0]],
                    radius: 0.0625,
                    feather: 50.0,
                    ..BrushStroke::default()
                }],
            })],
        },
        opacity: 50.0,
        feather: 0.0,
        enabled: true,
    })
    .collect();
    let error = render_linear_scaled(&settings, &RenderSource::Rgb(&image), 1).unwrap_err();
    assert!(
        matches!(error, engine_api::EngineError::InvalidArgument { name, .. } if name == "retouch")
    );
    assert_eq!((baseline.width(), baseline.height()), (128, 80));
}

#[test]
fn lr3d_sensor_coordinates_precede_rotation_crop_and_lens_profile() {
    use std::sync::Arc;
    let profile = lens::Profile {
        camera: None,
        maker: "synthetic".into(),
        model: "synthetic".into(),
        samples: vec![lens::CalibrationSample {
            distortion: lens::BrownConrady {
                k1: 0.12,
                ..Default::default()
            },
            ..Default::default()
        }],
    };
    let image = Image::new(
        80,
        48,
        vec![(0..3840).map(|i| (i % 80) as f32 / 100.0).collect(); 3],
    )
    .unwrap();
    let transplant = |w: u32, h: u32, p: &mut [Vec<f32>], _: &[RetouchOperation]| {
        assert_eq!(
            (w, h),
            (80, 48),
            "spot coordinate frame is unrotated active image"
        );
        for plane in p {
            for y in 20..28 {
                for x in 16..24 {
                    plane[y * 80 + x] = plane[y * 80 + x + 40];
                }
            }
        }
        Ok(())
    };
    let mut corrected = image.planes().to_vec();
    transplant(80, 48, &mut corrected, &[]).unwrap();
    let corrected = Image::new(80, 48, corrected).unwrap();
    for angle in [-12.0, 9.0] {
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.0;
        s.detail.noise_reduction.color = 0.0;
        s.tone.contrast = 20.0;
        s.geometry.crop.angle = angle;
        s.geometry.crop.rect.left = 0.1;
        s.geometry.crop.rect.right = 0.9;
        s.geometry.crop.rect.top = 0.1;
        s.geometry.crop.rect.bottom = 0.9;
        let context = pipeline_cpu::LensContext {
            profile: Some(&profile),
            retouch: Some(Arc::new(transplant)),
            ..Default::default()
        };
        let expected = pipeline_cpu::render_linear_scaled_with_lens(
            &s,
            &RenderSource::Rgb(&corrected),
            1,
            &context,
        )
        .unwrap();
        s.locals.retouch.push(RetouchOperation {
            id: RetouchId(1),
            kind: RetouchKind::Clone {
                source_offset: [0.5, 0.0],
            },
            target: RetouchTarget::Area { components: vec![] },
            opacity: 100.0,
            feather: 0.0,
            enabled: true,
        });
        let actual = pipeline_cpu::render_linear_scaled_with_lens(
            &s,
            &RenderSource::Rgb(&image),
            1,
            &context,
        )
        .unwrap();
        assert_eq!(actual.planes(), expected.planes(), "crop rotation {angle}");
    }
}

#[test]
fn lr3d_retouch_invalidates_detail_checkpoint() {
    let original = DevelopSettings::default();
    let mut edited = original.clone();
    edited.locals.retouch.push(RetouchOperation {
        id: RetouchId(1),
        kind: RetouchKind::Clone {
            source_offset: [0.5, 0.0],
        },
        target: RetouchTarget::Area { components: vec![] },
        opacity: 100.0,
        feather: 0.0,
        enabled: true,
    });
    assert_eq!(
        original.first_dirty_stage(&edited),
        Some(engine_api::stage::StageId::Detail)
    );
}

#[test]
fn lr3e_scaled_spot_preserves_pixels_outside_support() {
    use std::sync::Arc;
    let (w, h) = (257, 193);
    let image = Image::new(
        w,
        h,
        vec![
            (0..w * h)
                .map(|i| 0.1 + ((i * 73 % 997) as f32 / 1300.))
                .collect();
            3
        ],
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.tone.contrast = 23.;
    s.tone.clarity = 17.;
    s.tone.texture = 11.;
    s.color.vibrance = 21.;
    for scale in [2, 4] {
        let context = pipeline_cpu::LensContext {
            retouch: Some(Arc::new(
                move |rw: u32, rh: u32, planes: &mut [Vec<f32>], _: &[RetouchOperation]| {
                    assert_eq!((rw, rh), (w.div_ceil(scale), h.div_ceil(scale)));
                    for p in planes {
                        for y in rh / 2 - 1..=rh / 2 + 1 {
                            for x in rw / 2 - 1..=rw / 2 + 1 {
                                p[(y * rw + x) as usize] = 0.9;
                            }
                        }
                    }
                    Ok(())
                },
            )),
            ..Default::default()
        };
        let baseline = pipeline_cpu::render_linear_scaled_with_lens(
            &s,
            &RenderSource::Rgb(&image),
            scale,
            &context,
        )
        .unwrap();
        let mut edited = s.clone();
        edited.locals.retouch.push(RetouchOperation {
            id: RetouchId(1),
            kind: RetouchKind::Clone {
                source_offset: [0.2, 0.],
            },
            target: RetouchTarget::Area { components: vec![] },
            opacity: 100.,
            feather: 0.,
            enabled: true,
        });
        let actual = pipeline_cpu::render_linear_scaled_with_lens(
            &edited,
            &RenderSource::Rgb(&image),
            scale,
            &context,
        )
        .unwrap();
        let mut changed = false;
        let mut checked = 0;
        for (a, b) in actual.planes().iter().zip(baseline.planes()) {
            for y in 0..actual.height() {
                for x in 0..actual.width() {
                    let i = (y * actual.width() + x) as usize;
                    changed |= a[i].to_bits() != b[i].to_bits();
                    // 3x3 target-level support, plus 32 input pixels for the
                    // downstream Detail/presence filter halo and one sampling cell.
                    let margin = 2 + 32_u32.div_ceil(scale);
                    if x.abs_diff(actual.width() / 2) > margin
                        || y.abs_diff(actual.height() / 2) > margin
                    {
                        assert_eq!(a[i].to_bits(), b[i].to_bits(), "scale {scale} ({x},{y})");
                        checked += 1;
                    }
                }
            }
        }
        assert!(changed && checked > 1000);
    }
}
