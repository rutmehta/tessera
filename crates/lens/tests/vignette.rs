use lens::*;
#[test]
fn fit_smooth_radial_illumination() {
    let n = 81;
    let mut data = Vec::new();
    for y in 0..n {
        for x in 0..n {
            let a = 2. * x as f64 / 80. - 1.;
            let b = 2. * y as f64 / 80. - 1.;
            let r = a * a + b * b;
            data.push(0.8 * (1. - 0.22 * r + 0.025 * r * r));
        }
    }
    let e = estimate_vignette(&GrayImage::new(n, n, data).unwrap()).unwrap();
    assert!((e.value[0] + 0.22).abs() < 0.005, "{e:?}");
    assert!((e.value[1] - 0.025).abs() < 0.005);
    assert!(e.residual < 1e-6);
    assert!(estimate_vignette(&GrayImage::new(10, 10, vec![0.; 100]).unwrap()).is_none());
}
