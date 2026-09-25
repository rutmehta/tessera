use engine_api::color::ColorMatrix3;
use merge::from_cfa;
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
#[test]
fn demosaic_preserves_camera_space_and_crops_active_area() {
    let layout = CfaLayout::Bayer([[0, 1], [1, 2]]);
    let rgb = [0.1, 0.25, 0.5];
    let plane = CfaImage::from_linear(
        8,
        8,
        (0..64)
            .map(|i| rgb[layout.channel_at(i % 8, i / 8)])
            .collect(),
    )
    .unwrap();
    let mut m = RawMetadata {
        make: "Synthetic".into(),
        model: "Bayer".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 2.,
        focal_mm: 35.,
        capture_time: 0,
        orientation: 1,
        width: 8,
        height: 8,
        cfa_layout: layout,
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [2., 1., 1.5, 1.],
        camera_to_xyz: ColorMatrix3([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]),
        cam_xyz: [[0.8, -0.1, 0.2], [0.1, 1., -0.1], [0., 0.1, 0.9], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [2, 2, 4, 4],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    };
    let out = from_cfa(&plane, &m).unwrap();
    assert_eq!((out.width, out.height), (4, 4));
    for p in out.pixels {
        for (a, b) in p.into_iter().zip(rgb) {
            assert!((a - b).abs() < 1e-6);
        }
    }
    assert_eq!(
        out.color_matrix,
        std::array::from_fn(|i| m.cam_xyz[i].map(f64::from))
    );
    assert!((out.as_shot_neutral[0] - 0.5).abs() < 1e-8);
    m.as_shot_wb[0] = 0.;
    assert!(from_cfa(&plane, &m).is_err());
}
