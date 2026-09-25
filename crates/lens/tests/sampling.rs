use lens::*;
#[test]
fn sample_clamps_outside_domain_and_rejects_bad_data() {
    let mut a = CalibrationSample {
        focal: 20.,
        ..Default::default()
    };
    a.distortion.k1 = 0.1;
    let mut b = a.clone();
    b.focal = 40.;
    b.distortion.k1 = 0.3;
    let p = Profile {
        maker: "A".into(),
        model: "B".into(),
        samples: vec![a, b],
        camera: None,
    };
    assert_eq!(p.sample(5., 4., 10.).unwrap().distortion.k1, 0.1);
    assert_eq!(p.sample(80., 4., 10.).unwrap().distortion.k1, 0.3);
    assert!(p.sample(f64::NAN, 4., 10.).is_none());
    let mut invalid = p;
    invalid.samples[0].focal = f64::NAN;
    assert!(invalid.sample(30., 4., 10.).is_none());
}
