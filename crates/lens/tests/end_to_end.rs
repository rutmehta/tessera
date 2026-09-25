use lens::*;
#[test]
fn image_edges_recover_barrel_distortion() {
    let n = 192;
    let model = BrownConrady {
        k1: 0.12,
        ..Default::default()
    };
    let mut pixels = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let p = [
                2. * x as f64 / (n - 1) as f64 - 1.,
                2. * y as f64 / (n - 1) as f64 - 1.,
            ];
            let q = model.undistort(p).unwrap();
            let v = 0.5 + 0.5 * ((q[0].abs() - 0.53) * 150.).tanh();
            pixels.push(v);
        }
    }
    let image = GrayImage::new(n, n, pixels).unwrap();
    let lines = detect_lines(&image, 0.07, 80);
    let curves: Vec<_> = lines.into_iter().map(|l| l.points).collect();
    let e = estimate_k1(&curves, [-0.1, 0.3]).unwrap();
    assert!((e.value - 0.12).abs() < 0.02, "{e:?}");
}
