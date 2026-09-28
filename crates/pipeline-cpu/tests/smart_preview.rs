use engine_api::{
    color::ColorMatrix3,
    recipe::{DevelopSettings, ProcessVersion, settings::WhiteBalanceMode},
};
use pipeline_cpu::{
    CameraLinearProxy, LensContext, RenderSource, render_linear_scaled,
    render_linear_scaled_with_lens,
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
fn fixture(w: u32, h: u32) -> (CfaImage, RawMetadata) {
    let m = RawMetadata {
        make: "synthetic".into(),
        model: "camera".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 6,
        width: w,
        height: h,
        cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.1, 0.2, 0.7], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, w, h],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let c = CfaImage::from_linear(
        w,
        h,
        (0..w * h)
            .map(|i| 0.1 + (i % w) as f32 / (w as f32) * 0.15 + ((i / w) % 2) as f32 * 0.1)
            .collect(),
    )
    .unwrap();
    (c, m)
}
fn close(a: &pipeline_cpu::Image, b: &pipeline_cpu::Image) {
    assert_eq!((a.width(), a.height()), (b.width(), b.height()));
    for (x, y) in a.planes().iter().flatten().zip(b.planes().iter().flatten()) {
        assert!((x - y).abs() < 2e-5, "{x} != {y}");
    }
}
#[test]
fn camera_linear_scale_one_preserves_calibration_wb_and_exposure() {
    let (c, m) = fixture(32, 24);
    let mut s = DevelopSettings::default();
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap();
    for custom in [false, true] {
        if custom {
            s.white_balance.mode = WhiteBalanceMode::Custom;
            s.white_balance.temperature = 4200.;
            s.white_balance.tint = 13.;
        }
        let a = render_linear_scaled(
            &s,
            &RenderSource::Cfa {
                image: &c,
                metadata: &m,
            },
            1,
        )
        .unwrap();
        let b = render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap();
        close(&a, &b);
    }
    let a = render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap();
    s.tone.exposure = 1.;
    let b = render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap();
    assert_ne!(a.planes(), b.planes());
    let wrong = render_linear_scaled(
        &DevelopSettings::default(),
        &RenderSource::Rgb(p.pixels()),
        1,
    )
    .unwrap();
    assert_ne!(a.planes(), wrong.planes());
    s.demosaic.method = engine_api::recipe::settings::DemosaicMethod::Bilinear;
    assert!(render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).is_err());
}
#[test]
fn default_auto_lens_correction_is_applied_once() {
    let (c, m) = fixture(32, 24);
    let s = DevelopSettings::default();
    let profile = lens::Profile {
        camera: None,
        maker: "test".into(),
        model: "lens".into(),
        samples: vec![lens::CalibrationSample {
            ca_red: [0.94, 0., 0.],
            vignette: [-0.2, 0., 0.],
            distortion: lens::BrownConrady {
                k1: 0.02,
                ..Default::default()
            },
            ..Default::default()
        }],
    };
    let context = LensContext {
        profile: Some(&profile),
        ..Default::default()
    };
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [8; 32],
        &context,
    )
    .unwrap();
    close(
        &render_linear_scaled_with_lens(
            &s,
            &RenderSource::Cfa {
                image: &c,
                metadata: &m,
            },
            1,
            &context,
        )
        .unwrap(),
        &render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap(),
    );
}
#[test]
fn odd_crop_is_bounded_and_keeps_sensor_orientation_and_hdr() {
    let (_, mut m) = fixture(2564, 6);
    m.default_crop = [1, 1, 2561, 3];
    m.opcode_lists[1] = Some(vignette_opcode());
    let c = CfaImage::from_linear(2564, 6, vec![2.; 2564 * 6]).unwrap();
    let s = DevelopSettings::default();
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [9; 32],
        &Default::default(),
    )
    .unwrap();
    assert_eq!((p.pixels().width(), p.pixels().height()), (1281, 2));
    assert_eq!(p.original_metadata().orientation, 6);
    assert_eq!(p.original_metadata().default_crop, m.default_crop);
    assert!(p.pixels().planes().iter().flatten().all(|v| v.is_finite()));
    assert!(p.pixels().planes().iter().flatten().any(|v| *v > 1.));
    assert!(
        CameraLinearProxy::generate(
            &c,
            &m,
            &s,
            ProcessVersion::adobe(6),
            [9; 32],
            &Default::default()
        )
        .is_err()
    );
}

fn vignette_opcode() -> Vec<u8> {
    let mut bytes: Vec<u8> = [1_u32, 3, 0x01030000, 0, 56]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    for v in [2.0_f64, 0., 0., 0., 0., 0.5, 0.5] {
        bytes.extend(v.to_be_bytes());
    }
    bytes
}
#[test]
fn embedded_prefix_stages_agree_and_late_stage_requires_original() {
    let (c, mut m) = fixture(32, 24);
    let s = DevelopSettings::default();
    for stage in [0, 1] {
        m.opcode_lists = [None, None, None];
        m.opcode_lists[stage] = Some(vignette_opcode());
        let p = CameraLinearProxy::generate(
            &c,
            &m,
            &s,
            ProcessVersion::NATIVE_CURRENT,
            [4; 32],
            &Default::default(),
        )
        .unwrap();
        close(
            &render_linear_scaled(
                &s,
                &RenderSource::Cfa {
                    image: &c,
                    metadata: &m,
                },
                1,
            )
            .unwrap(),
            &render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap(),
        );
        close(p.pixels(), p.clone().pixels());
    }
    m.opcode_lists[2] = Some(vignette_opcode());
    assert!(matches!(
        CameraLinearProxy::generate(
            &c,
            &m,
            &s,
            ProcessVersion::NATIVE_CURRENT,
            [4; 32],
            &Default::default()
        ),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

#[test]
fn odd_crop_area_bins_match_independent_demosaic_and_keep_negative_values() {
    let (c, mut m) = fixture(2564, 6);
    m.default_crop = [1, 1, 2561, 3];
    let mut s = DevelopSettings::default();
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    let raw = pipeline_cpu::Image::from_pyramid(c.pyramid()).unwrap();
    let mut camera = pipeline_cpu::Image::new(2564, 6, vec![vec![0.; 2564 * 6]; 3]).unwrap();
    for coord in raw.coords() {
        camera
            .put(
                &pipeline_cpu::demosaic(
                    &raw.tile(coord, 3, 2).unwrap(),
                    m.cfa_layout,
                    pipeline_cpu::DemosaicAlgorithm::MalvarHeCutler,
                )
                .unwrap(),
            )
            .unwrap();
    }
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [2; 32],
        &Default::default(),
    )
    .unwrap();
    for channel in 0..3 {
        for y in 0..2 {
            for x in 0..1281 {
                let mut sum = 0.;
                let mut n = 0;
                for sy in (y * 2)..((y * 2 + 2).min(3)) {
                    for sx in (x * 2)..((x * 2 + 2).min(2561)) {
                        sum += camera.planes()[channel][(sy + 1) * 2564 + sx + 1];
                        n += 1;
                    }
                }
                assert!((sum / n as f32 - p.pixels().planes()[channel][y * 1281 + x]).abs() < 1e-6);
            }
        }
    }
    let negative = CfaImage::from_linear(2564, 6, vec![-0.1; 2564 * 6]).unwrap();
    let p = CameraLinearProxy::generate(
        &negative,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [2; 32],
        &Default::default(),
    )
    .unwrap();
    assert!(p.pixels().planes().iter().flatten().all(|v| *v < 0.));
}

#[test]
fn geometry_remains_editable_but_lens_and_raw_denoise_require_original() {
    let (c, m) = fixture(32, 24);
    let mut s = DevelopSettings::default();
    let p = CameraLinearProxy::generate(
        &c,
        &m,
        &s,
        ProcessVersion::NATIVE_CURRENT,
        [3; 32],
        &Default::default(),
    )
    .unwrap();
    s.geometry.crop.angle = 10.;
    close(
        &render_linear_scaled(
            &s,
            &RenderSource::Cfa {
                image: &c,
                metadata: &m,
            },
            1,
        )
        .unwrap(),
        &render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1).unwrap(),
    );
    s.lens.chromatic_aberration_scale = 75.;
    assert!(matches!(
        render_linear_scaled(&s, &RenderSource::CameraLinear(&p), 1),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
    s.lens = Default::default();
    s.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
        model: Default::default(),
        joint_demosaic: false,
    };
    assert!(matches!(
        CameraLinearProxy::generate(
            &c,
            &m,
            &s,
            ProcessVersion::NATIVE_CURRENT,
            [3; 32],
            &Default::default()
        ),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

#[test]
fn hdr_presentation_policy_does_not_change_proxy_prefix_or_relax_legacy_validation() {
    let (cfa, metadata) = fixture(32, 24);
    let baseline = DevelopSettings::default();
    let generate = |settings: &DevelopSettings| {
        CameraLinearProxy::generate(
            &cfa,
            &metadata,
            settings,
            ProcessVersion::NATIVE_CURRENT,
            [91; 32],
            &LensContext::default(),
        )
    };
    let expected = generate(&baseline).unwrap().encode_persistent(100).unwrap();
    let mut hdr = baseline.clone();
    hdr.output.hdr = true;
    hdr.output.hdr_headroom_stops = 2.;
    let preserved = hdr.clone();
    let actual = generate(&hdr).unwrap().encode_persistent(100).unwrap();
    assert_eq!(
        actual, expected,
        "presentation policy is not baked into the prefix"
    );
    assert_eq!(hdr, preserved);
    assert!(
        pipeline_cpu::validate_settings(&hdr).is_err(),
        "legacy scalar validation remains strict"
    );
    hdr.output.proof_profile = Some(engine_api::color::IccProfileHandle(engine_api::id::Digest(
        [9; 32],
    )));
    assert!(
        generate(&hdr).is_err(),
        "unrelated unsupported output controls remain rejected"
    );
}
