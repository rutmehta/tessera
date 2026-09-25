use engine_api::{color::ColorMatrix3, recipe::DevelopSettings};
use pipeline_cpu::{
    CorrectionSource, Image, LensContext, RenderSource, render_linear_scaled_with_lens,
    resolve_lens,
};
use raw_decode::{CfaLayout, RawMetadata};
fn metadata(bytes: Vec<u8>) -> RawMetadata {
    RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        width: 32,
        height: 24,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: ColorMatrix3([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]),
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [0, 0, 32, 24],
        has_gain_map: false,
        has_opcode_list: true,
        opcode_lists: [None, None, Some(bytes)],
    }
}
fn vignette() -> Vec<u8> {
    let mut b = Vec::new();
    for x in [1_u32, 3, 0x01030000, 0, 56] {
        b.extend(x.to_be_bytes());
    }
    for x in [0.5_f64, 0., 0., 0., 0., 0.5, 0.5] {
        b.extend(x.to_be_bytes());
    }
    b
}
#[test]
fn embedded_has_priority_and_applies_gain() {
    let m = metadata(vignette());
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    let profile = lens::Profile {
        camera: None,
        maker: "test".into(),
        model: "test".into(),
        samples: vec![Default::default()],
    };
    let c = LensContext {
        profile: Some(&profile),
        ..Default::default()
    };
    let mut s = DevelopSettings::default();
    let r = resolve_lens(&image, &s.lens, Some(&m), &c).unwrap();
    assert_eq!(r.source(), CorrectionSource::Embedded);
    // CFA integration uses the selected raw metadata; no external opcode override.
    let cfa = raw_decode::CfaImage::from_linear(32, 24, vec![0.25; 768]).unwrap();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let a = render_linear_scaled_with_lens(
        &s,
        &RenderSource::Cfa {
            image: &cfa,
            metadata: &m,
        },
        1,
        &c,
    )
    .unwrap();
    assert!(a.planes()[1][0] > a.planes()[1][12 * 32 + 16]);
}
#[test]
fn wrong_stage_and_malformed_embedded_are_errors() {
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    let mut m = metadata(vignette());
    m.opcode_lists.swap(1, 2);
    assert!(
        resolve_lens(
            &image,
            &DevelopSettings::default().lens,
            Some(&m),
            &LensContext::default()
        )
        .is_err()
    );
    m.opcode_lists = [None, None, Some(vec![0, 0, 0, 1])];
    assert!(
        resolve_lens(
            &image,
            &DevelopSettings::default().lens,
            Some(&m),
            &LensContext::default()
        )
        .is_err()
    );
}

#[test]
fn auto_database_matching_and_manual_fallback() {
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    let mut m = metadata(vec![0, 0, 0, 0]);
    m.lens = Some("Test Prime 50".into());
    let database = lens::ProfileDatabase {
        profiles: vec![lens::Profile {
            camera: None,
            maker: "test".into(),
            model: "Test Prime 50".into(),
            samples: vec![Default::default()],
        }],
    };
    let context = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let r = resolve_lens(&image, &DevelopSettings::default().lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    let r = resolve_lens(
        &image,
        &DevelopSettings::default().lens,
        Some(&m),
        &LensContext::default(),
    )
    .unwrap();
    assert_eq!(r.source(), CorrectionSource::Manual);
}

#[test]
fn database_resolves_third_party_lens_for_the_actual_camera() {
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    let mut m = metadata(vec![0, 0, 0, 0]);
    m.make = "Canon".into();
    m.model = "EOS R5".into();
    m.lens = Some("35 mm F1.4 DG".into());
    let profile = |camera: &str, k1| lens::Profile {
        maker: "Sigma".into(),
        model: "35mm F1.4 DG".into(),
        camera: Some(lens::CameraIdentity {
            maker: "Canon".into(),
            model: camera.into(),
        }),
        samples: vec![lens::CalibrationSample {
            distortion: lens::BrownConrady {
                k1,
                ..Default::default()
            },
            ..Default::default()
        }],
    };
    let database = lens::ProfileDatabase {
        profiles: vec![profile("EOS R6", 0.2), profile("EOS R5", 0.1)],
    };
    let context = LensContext {
        database: Some(&database),
        ..Default::default()
    };
    let settings = DevelopSettings::default();
    let r = resolve_lens(&image, &settings.lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Database);
    assert_eq!(r.sample().unwrap().distortion.k1, 0.1);
    m.model = "EOS R7".into();
    let r = resolve_lens(&image, &settings.lens, Some(&m), &context).unwrap();
    assert_eq!(r.source(), CorrectionSource::Manual);
}
#[test]
fn three_plane_embedded_warp_does_not_average_ca() {
    let mut bytes = Vec::new();
    for n in [1_u32, 1, 0x01030000, 0, 164, 3] {
        bytes.extend(n.to_be_bytes());
    }
    for scale in [0.98_f64, 1., 1.02] {
        for n in [scale, 0., 0., 0., 0., 0.] {
            bytes.extend(n.to_be_bytes());
        }
    }
    for n in [0.5_f64, 0.5] {
        bytes.extend(n.to_be_bytes());
    }
    let m = metadata(bytes);
    let cfa = raw_decode::CfaImage::from_linear(
        32,
        24,
        (0..768).map(|i| 0.1 + (i % 32) as f32 / 40.).collect(),
    )
    .unwrap();
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let a = render_linear_scaled_with_lens(
        &s,
        &RenderSource::Cfa {
            image: &cfa,
            metadata: &m,
        },
        1,
        &LensContext::default(),
    )
    .unwrap();
    s.lens.remove_chromatic_aberration = false;
    let b = render_linear_scaled_with_lens(
        &s,
        &RenderSource::Cfa {
            image: &cfa,
            metadata: &m,
        },
        1,
        &LensContext::default(),
    )
    .unwrap();
    let i = 12 * 32 + 24;
    assert!((a.planes()[0][i] - b.planes()[0][i]).abs() > 1e-4);
    assert!((a.planes()[2][i] - b.planes()[2][i]).abs() > 1e-4);
    // Working green mixes camera red/blue; it must change after sensor-channel CA.
    assert!((a.planes()[1][i] - b.planes()[1][i]).abs() > 1e-4);
}

#[test]
fn required_bad_pixel_operations_are_deliberately_ignored() {
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    for id in [4_u32, 5] {
        for stage in 0..3 {
            let bytes = [1_u32, id, 0x01030000, 0, 0]
                .into_iter()
                .flat_map(u32::to_be_bytes)
                .collect();
            let mut m = metadata(vec![0, 0, 0, 0]);
            m.opcode_lists = [None, None, None];
            m.opcode_lists[stage] = Some(bytes);
            assert!(
                resolve_lens(
                    &image,
                    &DevelopSettings::default().lens,
                    Some(&m),
                    &LensContext::default()
                )
                .is_ok(),
                "id {id}, stage {stage}"
            );
        }
    }
}

#[test]
fn unknown_required_opcode_is_not_silently_ignored() {
    let image = Image::new(32, 24, vec![vec![0.25; 768]; 3]).unwrap();
    let bytes = [1_u32, 999, 0x01030000, 0, 0]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    let m = metadata(bytes);
    assert!(
        resolve_lens(
            &image,
            &DevelopSettings::default().lens,
            Some(&m),
            &LensContext::default()
        )
        .is_err()
    );
    let bytes = [1_u32, 999, 0x01030000, 1, 0]
        .into_iter()
        .flat_map(u32::to_be_bytes)
        .collect();
    assert!(
        resolve_lens(
            &image,
            &DevelopSettings::default().lens,
            Some(&metadata(bytes)),
            &LensContext::default()
        )
        .is_ok()
    );
}
