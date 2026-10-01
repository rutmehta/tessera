use engine_api::recipe::settings::GeometrySettings;

#[test]
fn stored_upright_matrix_bypasses_image_analysis() {
    let s: GeometrySettings = serde_json::from_value(serde_json::json!({"upright": {"mode":"auto", "homography":[[1.,0.,0.],[0.,1.,0.],[0.2,0.,1.]]}})).unwrap();
    let input = pipeline_cpu::Image::new(
        128,
        128,
        vec![(0..16384).map(|i| (i % 128) as f32).collect(); 3],
    )
    .unwrap();
    let output = pipeline_cpu::geometry(&input, &s).unwrap();
    // u=64.5/128; inverse source x=128*u/(1-.2*u)-.5 = 71.228062...
    assert!((output.planes()[0][64 * 128 + 64] - 71.228065).abs() < 0.08);
}

#[test]
fn invalid_stored_matrix_is_rejected() {
    let s: GeometrySettings = serde_json::from_value(serde_json::json!({"upright": {"mode":"auto", "homography":[[1.,0.,0.],[0.,0.,0.],[0.,0.,1.]]}})).unwrap();
    let input = pipeline_cpu::Image::new(8, 8, vec![vec![0.; 64]; 3]).unwrap();
    assert!(pipeline_cpu::geometry(&input, &s).is_err());
}

#[test]
fn rotated_matrix_on_non_square_ramps_has_correct_direction() {
    for (w, h) in [(160, 80), (80, 160)] {
        let s: GeometrySettings=serde_json::from_value(serde_json::json!({"upright":{"mode":"auto","homography_mode":"auto","homography":[[0.,-1.,1.],[1.,0.,0.],[0.,0.,1.]]}})).unwrap();
        let x = (0..w * h).map(|i| (i % w) as f32).collect();
        let y = (0..w * h).map(|i| (i / w) as f32).collect();
        let input = pipeline_cpu::Image::new(w, h, vec![x, y, vec![0.; (w * h) as usize]]).unwrap();
        let out = pipeline_cpu::geometry(&input, &s).unwrap();
        for (px, py) in [(w / 4, h / 3), (w / 2, h / 2)] {
            let i = (py * w + px) as usize;
            let sx = (py as f32 + 0.5) / h as f32 * w as f32 - 0.5;
            let sy = (1. - (px as f32 + 0.5) / w as f32) * h as f32 - 0.5;
            assert!((out.planes()[0][i] - sx).abs() < 0.08);
            assert!((out.planes()[1][i] - sy).abs() < 0.08);
        }
    }
}
