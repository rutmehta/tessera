use lens::*;
#[test]
fn estimate_channel_radial_alignment() {
    let n = 80;
    let mut data = Vec::new();
    let f = |x: f64, y: f64| 0.5 + 0.2 * (19. * x).sin() + 0.2 * (23. * y).sin();
    for y in 0..n {
        for x in 0..n {
            let a = 2. * x as f64 / (n - 1) as f64 - 1.;
            let b = 2. * y as f64 / (n - 1) as f64 - 1.;
            data.push([f(a / 1.012, b / 1.012), f(a, b), f(a / 0.989, b / 0.989)]);
        }
    }
    let e = estimate_ca(&RgbImage::new(n, n, data).unwrap(), 0.025).unwrap();
    assert!((e.value.red[0] - 1.012).abs() < 0.003, "{e:?}");
    assert!((e.value.blue[0] - 0.989).abs() < 0.003, "{e:?}");
    assert!(estimate_ca(&RgbImage::new(20, 20, vec![[1.; 3]; 400]).unwrap(), 0.025).is_none());
}
