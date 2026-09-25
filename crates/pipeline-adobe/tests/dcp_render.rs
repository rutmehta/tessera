use engine_api::{
    color::ColorMatrix3,
    recipe::{DevelopSettings, settings::WhiteBalanceMode},
};
use pipeline_adobe::{
    RenderSource, dcp::DcpProfile, render_linear_scaled_with_profile, render_scaled_with_profile,
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

// Actual little-endian TIFF IFD, not a mocked parsed profile.
fn profile(matrix_scale: i32, tone: bool) -> DcpProfile {
    let count = if tone { 3 } else { 2 };
    let mut b = vec![0u8; 14 + count * 12];
    b[..8].copy_from_slice(&[73, 73, 82, 67, 8, 0, 0, 0]);
    b[8..10].copy_from_slice(&(count as u16).to_le_bytes());
    let mut entries = vec![
        (
            50721u16,
            10u16,
            9u32,
            (0..9)
                .flat_map(|i| {
                    let n = if i % 4 == 0 { matrix_scale } else { 0 };
                    [n.to_le_bytes(), 1i32.to_le_bytes()].concat()
                })
                .collect::<Vec<_>>(),
        ),
        (50778, 3, 1, 21u16.to_le_bytes().to_vec()),
    ];
    if tone {
        entries.push((
            50940,
            11,
            6,
            [0f32, 0., 0.5, 0.25, 1., 1.]
                .into_iter()
                .flat_map(f32::to_le_bytes)
                .collect(),
        ));
    }
    for (i, (tag, kind, n, data)) in entries.into_iter().enumerate() {
        let p = 10 + i * 12;
        b[p..p + 2].copy_from_slice(&tag.to_le_bytes());
        b[p + 2..p + 4].copy_from_slice(&kind.to_le_bytes());
        b[p + 4..p + 8].copy_from_slice(&n.to_le_bytes());
        if data.len() <= 4 {
            b[p + 8..p + 8 + data.len()].copy_from_slice(&data);
        } else {
            let offset = b.len() as u32;
            b[p + 8..p + 12].copy_from_slice(&offset.to_le_bytes());
            b.extend(data);
        }
    }
    DcpProfile::parse(&b).unwrap()
}
fn metadata() -> RawMetadata {
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
        width: 16,
        height: 16,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [0, 0, 16, 16],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}
#[test]
fn profile_detail_basic_tone_order_and_single_application() {
    let raw = CfaImage::from_linear(
        16,
        16,
        (0..256).map(|i| 0.08 + (i % 13) as f32 * 0.015).collect(),
    )
    .unwrap();
    let m = metadata();
    let source = RenderSource::Cfa {
        image: &raw,
        metadata: &m,
    };
    let mut s = DevelopSettings::default();
    s.white_balance.mode = WhiteBalanceMode::Custom;
    s.white_balance.temperature = 6504.;
    s.white_balance.tint = 0.;
    s.detail.sharpening.amount = 60.;
    s.detail.noise_reduction.color = 20.;
    s.tone.exposure = 0.75;
    s.tone.contrast = 30.;
    let p = profile(1, true);
    let plain = profile(1, false);
    let mut base = s.clone();
    base.detail.sharpening.amount = 0.;
    base.detail.noise_reduction.color = 0.;
    base.tone = Default::default();
    let before_detail = render_linear_scaled_with_profile(&base, &source, 1, Some(&plain)).unwrap();
    let mut expected = before_detail.clone();
    for coord in before_detail.coords() {
        let mut tile = before_detail
            .tile(coord, pipeline_cpu::detail_halo(&s.detail), 1)
            .unwrap();
        pipeline_cpu::detail(&mut tile, &s.detail).unwrap();
        pipeline_cpu::map_rgb(&mut tile, |v| {
            p.apply_tone(pipeline_adobe::basic_tone(v, &s.tone))
        })
        .unwrap();
        expected.put(&tile).unwrap();
    }
    let actual = render_linear_scaled_with_profile(&s, &source, 1, Some(&p)).unwrap();
    for (a, b) in actual
        .planes()
        .iter()
        .flatten()
        .zip(expected.planes().iter().flatten())
    {
        assert!((a - b).abs() < 0.00002, "{a} != {b}");
    }
}

#[test]
fn as_shot_resolves_temperature_tint_and_rgb_profile_is_rejected() {
    let raw = CfaImage::from_linear(16, 16, vec![0.2; 256]).unwrap();
    let m = metadata();
    let source = RenderSource::Cfa {
        image: &raw,
        metadata: &m,
    };
    let s = DevelopSettings::default();
    let p = profile(1, true);
    let a = render_linear_scaled_with_profile(&s, &source, 1, Some(&p)).unwrap();
    let mut custom = s.clone();
    let (t, tint) =
        pipeline_cpu::as_shot_temperature_tint(ColorMatrix3::IDENTITY, m.as_shot_wb).unwrap();
    custom.white_balance.mode = WhiteBalanceMode::Custom;
    custom.white_balance.temperature = t;
    custom.white_balance.tint = tint;
    let b = render_linear_scaled_with_profile(&custom, &source, 1, Some(&p)).unwrap();
    for (a, b) in a.planes().iter().flatten().zip(b.planes().iter().flatten()) {
        assert!((a - b).abs() < 0.00002);
    }
    let rgb = RenderSource::Rgb(&a);
    let error = render_scaled_with_profile(&s, &rgb, 1, Some(&p))
        .unwrap_err()
        .to_string();
    assert!(error.contains("CFA"), "{error}");
    let old = pipeline_adobe::render_linear_scaled(&s, &rgb, 2).unwrap();
    let none = render_linear_scaled_with_profile(&s, &rgb, 2, None).unwrap();
    assert_eq!(old.planes(), none.planes());
    assert!(render_scaled_with_profile(&s, &source, 0, Some(&p)).is_err());
}

#[test]
fn tiff_profile_changes_final_cfa_render() {
    let raw = CfaImage::from_linear(16, 16, vec![0.2; 256]).unwrap();
    let m = metadata();
    let source = RenderSource::Cfa {
        image: &raw,
        metadata: &m,
    };
    let mut s = DevelopSettings::default();
    s.white_balance.mode = WhiteBalanceMode::Custom;
    s.white_balance.temperature = 6504.;
    s.white_balance.tint = 0.;
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    let a = profile(1, false);
    let b = profile(2, false);
    let first = render_scaled_with_profile(&s, &source, 2, Some(&a)).unwrap();
    let second = render_scaled_with_profile(&s, &source, 2, Some(&b)).unwrap();
    assert_eq!(first.dimensions(), (8, 8));
    assert_ne!(first.as_raw(), second.as_raw());
    let linear = render_linear_scaled_with_profile(&s, &source, 1, Some(&a)).unwrap();
    let expected = a.apply([0.2; 3], 6504.);
    for (c, value) in expected.iter().enumerate() {
        assert!((linear.planes()[c][136] - value).abs() < 0.0001);
    }
}
