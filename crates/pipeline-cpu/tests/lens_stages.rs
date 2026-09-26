use engine_api::{
    color::ColorMatrix3,
    recipe::{
        DevelopSettings,
        settings::{DemosaicMethod, LensProfileSource},
    },
};
use pipeline_cpu::{LensContext, RenderSource, render_linear_scaled_with_lens};
use raw_decode::{CfaLayout, RawMetadata};
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
        width: 32,
        height: 24,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [3, 1, 26, 22],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}
#[test]
fn list2_aligns_camera_planes_before_matrix_and_nonlinear_tone() {
    use engine_api::color::WorkingSpace;
    use pipeline_cpu::{DemosaicAlgorithm, Image};
    for layout in [
        CfaLayout::Bayer([[0, 1], [3, 2]]),
        CfaLayout::XTrans([
            [1, 2, 1, 1, 0, 1],
            [0, 1, 0, 2, 1, 2],
            [1, 2, 1, 1, 0, 1],
            [1, 0, 1, 1, 2, 1],
            [2, 1, 2, 0, 1, 0],
            [1, 0, 1, 1, 2, 1],
        ]),
    ] {
        let mut m = metadata();
        m.cfa_layout = layout;
        let period = if matches!(layout, CfaLayout::XTrans(_)) {
            6
        } else {
            2
        };
        m.default_crop = [0, 0, 32, 24];
        let mut bytes: Vec<u8> = [1_u32, 1, 0x01030000, 0, 164, 3]
            .into_iter()
            .flat_map(u32::to_be_bytes)
            .collect();
        for scale in [0.91_f64, 1., 0.95] {
            for v in [scale, 0., 0., 0., 0., 0.] {
                bytes.extend(v.to_be_bytes());
            }
        }
        for v in [0.5_f64, 0.5] {
            bytes.extend(v.to_be_bytes());
        }
        m.opcode_lists[1] = Some(bytes);
        let raw = Image::new(
            32,
            24,
            vec![
                (0..768)
                    .map(|i| 0.15 + ((i * 17 + i / 32 * 11) % 31) as f32 * 0.012)
                    .collect(),
            ],
        )
        .unwrap();
        let mut camera = Image::new(32, 24, vec![vec![0.; 768]; 3]).unwrap();
        for coord in raw.coords() {
            camera
                .put(
                    &pipeline_cpu::demosaic(
                        &raw.tile(coord, 3, period).unwrap(),
                        m.cfa_layout,
                        DemosaicAlgorithm::MalvarHeCutler,
                    )
                    .unwrap(),
                )
                .unwrap();
        }
        let mut planes = camera.planes().to_vec();
        for c in [0, 2] {
            for y in 0..24usize {
                for x in 0..32usize {
                    let scale = [0.91, 1., 0.95][c];
                    let u = (16. + (x as f64 + 0.5 - 16.) * scale - 0.5).clamp(0., 31.);
                    let v = (12. + (y as f64 + 0.5 - 12.) * scale - 0.5).clamp(0., 23.);
                    let (a, b) = (u.floor() as usize, v.floor() as usize);
                    let at = |xx: usize, yy: usize| {
                        camera.planes()[c][yy.min(23) * 32 + xx.min(31)] as f64
                    };
                    planes[c][y * 32 + x] = ((at(a, b) * (1. - u.fract())
                        + at(a + 1, b) * u.fract())
                        * (1. - v.fract())
                        + (at(a, b + 1) * (1. - u.fract()) + at(a + 1, b + 1) * u.fract())
                            * v.fract()) as f32;
                }
            }
        }
        let mut reference = Image::new(32, 24, planes).unwrap();
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.;
        s.detail.noise_reduction.color = 0.;
        s.tone.contrast = 42.;
        let xyz = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))
        .unwrap();
        let matrix = WorkingSpace::LinearRec2020.to_xyz().inverse().unwrap() * xyz;
        let wb = pipeline_cpu::white_balance_matrix(&s.white_balance, xyz, m.as_shot_wb).unwrap();
        for coord in reference.coords() {
            let mut t = reference.tile(coord, 0, 1).unwrap();
            pipeline_cpu::apply_matrix(&mut t, matrix).unwrap();
            pipeline_cpu::apply_matrix(&mut t, wb).unwrap();
            pipeline_cpu::tone(&mut t, &s.tone).unwrap();
            reference.put(&t).unwrap();
        }
        let cfa = raw_decode::CfaImage::from_linear(32, 24, raw.planes()[0].clone()).unwrap();
        let actual = render_linear_scaled_with_lens(
            &s,
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &m,
            },
            1,
            &LensContext::default(),
        )
        .unwrap();
        let error = actual
            .planes()
            .iter()
            .flatten()
            .zip(reference.planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0f32, f32::max);
        assert!(error < 2e-6, "camera-channel fallback ordering: {error}");
    }
}

#[test]
fn exact_profile_ca_precedes_bayer_demosaic_and_channel_mixing() {
    for pattern in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 1], [3, 0]],
        [[1, 2], [0, 3]],
    ] {
        let mut m = metadata();
        m.cfa_layout = CfaLayout::Bayer(pattern);
        let raw: Vec<f32> = (0..768)
            .map(|i| {
                let c = m.cfa_layout.channel_at(i % 32, i / 32);
                [0.1, 0.3, 0.5, 0.3][c] + ((i * 17 + i / 32 * 11) % 31) as f32 * 0.009
            })
            .collect();
        // Independent bilinear interpolation on each Bayer phase, never adjacent colours.
        let mut expected = raw.clone();
        for y in 0..24usize {
            for x in 0..32usize {
                let c = m.cfa_layout.channel_at(x as u32, y as u32);
                let scale = match c {
                    0 => 0.91,
                    2 => 0.95,
                    _ => continue,
                };
                let sx = 16.0 + (x as f64 + 0.5 - 16.0) * scale - 0.5;
                let sy = 12.0 + (y as f64 + 0.5 - 12.0) * scale - 0.5;
                let u = ((sx - (x % 2) as f64) / 2.).clamp(0., 15.);
                let v = ((sy - (y % 2) as f64) / 2.).clamp(0., 11.);
                let (a, b) = (u.floor() as usize, v.floor() as usize);
                let at = |xx: usize, yy: usize| {
                    raw[(yy.min(11) * 2 + y % 2) * 32 + xx.min(15) * 2 + x % 2] as f64
                };
                expected[y * 32 + x] = ((at(a, b) * (1. - u.fract()) + at(a + 1, b) * u.fract())
                    * (1. - v.fract())
                    + (at(a, b + 1) * (1. - u.fract()) + at(a + 1, b + 1) * u.fract()) * v.fract())
                    as f32;
            }
        }
        let p = lens::Profile {
            camera: None,
            maker: "test".into(),
            model: "test".into(),
            samples: vec![lens::CalibrationSample {
                ca_red: [0.91, 0., 0.],
                ca_blue: [0.95, 0., 0.],
                ..Default::default()
            }],
        };
        let context = LensContext {
            profile: Some(&p),
            ..Default::default()
        };
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.;
        s.detail.noise_reduction.color = 0.;
        s.tone.contrast = 37.;
        for method in [DemosaicMethod::Auto, DemosaicMethod::Bilinear] {
            s.demosaic.method = method;
            let cfa = raw_decode::CfaImage::from_linear(32, 24, raw.clone()).unwrap();
            let corrected = raw_decode::CfaImage::from_linear(32, 24, expected.clone()).unwrap();
            let actual = render_linear_scaled_with_lens(
                &s,
                &RenderSource::Cfa {
                    image: &cfa,
                    metadata: &m,
                },
                1,
                &context,
            )
            .unwrap();
            let mut off = s.clone();
            off.lens.profile = LensProfileSource::None;
            off.lens.remove_chromatic_aberration = false;
            let reference = render_linear_scaled_with_lens(
                &off,
                &RenderSource::Cfa {
                    image: &corrected,
                    metadata: &m,
                },
                1,
                &LensContext::default(),
            )
            .unwrap();
            let error = actual
                .planes()
                .iter()
                .flatten()
                .zip(reference.planes().iter().flatten())
                .map(|(a, b)| (a - b).abs())
                .fold(0f32, f32::max);
            assert!(
                error < 2e-6,
                "CA must precede demosaic and mixed nonlinear output: {error}"
            );
        }
    }
}
