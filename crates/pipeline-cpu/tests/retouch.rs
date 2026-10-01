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
        camera: None, maker: "synthetic".into(), model: "synthetic".into(),
        samples: vec![lens::CalibrationSample { distortion: lens::BrownConrady { k1: 0.12, ..Default::default() }, ..Default::default() }],
    };
    let image = Image::new(80,48,vec![(0..3840).map(|i| (i%80) as f32/100.0).collect();3]).unwrap();
    let transplant = |w: u32,h: u32,p: &mut [Vec<f32>],_: &[RetouchOperation]| {
        assert_eq!((w,h),(80,48),"spot coordinate frame is unrotated active image");
        for plane in p { for y in 20..28 { for x in 16..24 { plane[y*80+x] = plane[y*80+x+40]; } } }
        Ok(())
    };
    let mut corrected = image.planes().to_vec();
    transplant(80,48,&mut corrected,&[]).unwrap();
    let corrected = Image::new(80,48,corrected).unwrap();
    for orientation in 5..=8 {
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.0;
        s.detail.noise_reduction.color = 0.0;
        s.tone.contrast = 20.0;
        s.geometry.orientation = orientation;
        s.geometry.crop.rect.left = 0.1;
        s.geometry.crop.rect.right = 0.9;
        s.geometry.crop.rect.top = 0.1;
        s.geometry.crop.rect.bottom = 0.9;
        let context = pipeline_cpu::LensContext { profile: Some(&profile), retouch: Some(Arc::new(transplant)), ..Default::default() };
        let expected = pipeline_cpu::render_linear_scaled_with_lens(&s,&RenderSource::Rgb(&corrected),1,&context).unwrap();
        s.locals.retouch.push(RetouchOperation {
            id:RetouchId(1), kind: RetouchKind::Clone{source_offset:[0.5,0.0]},
            target:RetouchTarget::Area{components:vec![]}, opacity:100.0, feather:0.0, enabled:true,
        });
        let actual = pipeline_cpu::render_linear_scaled_with_lens(&s,&RenderSource::Rgb(&image),1,&context).unwrap();
        assert_eq!(actual.planes(),expected.planes(),"orientation {orientation}");
    }
}

#[test]
fn lr3d_retouch_invalidates_detail_checkpoint() {
    let original = DevelopSettings::default();
    let mut edited = original.clone();
    edited.locals.retouch.push(RetouchOperation {
        id:RetouchId(1), kind: RetouchKind::Clone{source_offset:[0.5,0.0]},
        target:RetouchTarget::Area{components:vec![]}, opacity:100.0, feather:0.0, enabled:true,
    });
    assert_eq!(original.first_dirty_stage(&edited),Some(engine_api::stage::StageId::Detail));
}
