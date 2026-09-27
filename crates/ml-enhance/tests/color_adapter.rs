use engine_api::color::{ColorMatrix3, WorkingSpace};
use ml_enhance::CameraSrgb;
use ml_runtime::Tensor;
#[test]
fn calibrated_camera_roundtrip_and_neutral() {
    let rgb_xyz = ColorMatrix3::rgb_to_xyz(&WorkingSpace::LinearSrgb.primaries()).unwrap();
    let xyz_cam = ColorMatrix3::diagonal([0.5, 1.0, 0.7]) * rgb_xyz.inverse().unwrap();
    let adapter = CameraSrgb::new(xyz_cam.0, [0.5, 1.0, 0.7]).unwrap();
    let camera = Tensor::new(3, 1, 2, vec![0.1, 0.15, 0.2, 0.4, 0.14, 0.35]).unwrap();
    let rgb = adapter.to_srgb(&camera).unwrap();
    for i in [0, 2, 4] {
        assert!((rgb.data()[i] - 0.2).abs() < 1e-6);
    }
    let restored = adapter.to_camera(&rgb).unwrap();
    for (a, b) in camera.data().iter().zip(restored.data()) {
        assert!((a - b).abs() < 1e-6);
    }
}
#[test]
fn hdr_and_singular_calibration_are_rejected() {
    assert!(CameraSrgb::new([[0.0; 3]; 3], [1.0; 3]).is_err());
    let xyz_cam = ColorMatrix3::rgb_to_xyz(&WorkingSpace::LinearSrgb.primaries())
        .unwrap()
        .inverse()
        .unwrap();
    let adapter = CameraSrgb::new(xyz_cam.0, [1.0; 3]).unwrap();
    for v in [-0.01, 1.01, f32::NAN] {
        assert!(
            adapter
                .to_srgb(&Tensor::new(3, 1, 1, vec![v; 3]).unwrap())
                .is_err()
        );
    }
}
