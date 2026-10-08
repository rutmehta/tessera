use engine_api::{EngineError, color::ColorMatrix3, recipe::ProcessVersion};
use pipeline_cpu::{CameraLinearProxy, RenderSource};
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
        catalog_orientation: None,
        baseline_exposure: 0.,
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
        maker_lens: None,
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
fn proxy() -> CameraLinearProxy {
    let (c, m) = fixture(8, 8);
    CameraLinearProxy::generate(
        &c,
        &m,
        &Default::default(),
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &Default::default(),
    )
    .unwrap()
}
#[test]
fn native_proxy_cannot_be_reinterpreted_as_adobe() {
    let p = proxy();
    let source = RenderSource::CameraLinear(&p);
    let s = Default::default();
    assert!(matches!(
        pipeline_adobe::render_linear_scaled(&s, &source, 1),
        Err(EngineError::Unsupported { .. })
    ));
    assert!(matches!(
        pipeline_adobe::render_linear_scaled_with_profile(&s, &source, 1, None),
        Err(EngineError::Unsupported { .. })
    ));
    assert!(matches!(
        pipeline_adobe::render_scaled(&s, &source, 1),
        Err(EngineError::Unsupported { .. })
    ));
    assert!(matches!(
        pipeline_adobe::render_scaled_with_profile(&s, &source, 1, None),
        Err(EngineError::Unsupported { .. })
    ));
}
