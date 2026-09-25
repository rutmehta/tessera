use lens::*;
#[test]
fn independent_calibration_grids_preserve_measured_focal_lengths() {
    let xml = r#"<lensdatabase><lens><maker>A</maker><model>Zoom</model><calibration>
      <distortion model="poly5" focal="20" k1="0.1"/>
      <distortion model="poly5" focal="60" k1="0.3"/>
      <tca model="linear" focal="20" kr="1.002" kb="0.998"/>
      <tca model="linear" focal="40" kr="1.008" kb="0.992"/>
      <tca model="linear" focal="60" kr="1.004" kb="0.996"/>
      <vignetting model="pa" focal="30" aperture="2" distance="1" k1="-0.2"/>
      <vignetting model="pa" focal="50" aperture="8" distance="10" k1="-0.1"/>
    </calibration></lens></lensdatabase>"#;
    let db = ProfileDatabase::from_lensfun(xml).unwrap();
    let p = &db.profiles[0];
    for (focal, k1) in [(20., 0.1), (60., 0.3)] {
        for (aperture, distance) in [(2., 1.), (8., 10.)] {
            let sample = p.sample(focal, aperture, distance).unwrap();
            assert!((sample.distortion.k1 - k1).abs() < 1e-12);
        }
    }
    let middle = p.sample(40., 2., 1.).unwrap();
    assert!((middle.ca_red[0] - 1.008).abs() < 1e-12);
    assert!((middle.ca_blue[0] - 0.992).abs() < 1e-12);
    assert_eq!(p.sample(30., 2., 1.).unwrap().vignette[0], -0.2);
    assert_eq!(p.sample(50., 8., 10.).unwrap().vignette[0], -0.1);
}

#[test]
fn lensfun_ptlens_ca_vignette_samples() {
    let xml = r#"<lensdatabase><lens><maker>A</maker><model>B</model><calibration><distortion model="ptlens" focal="35" a="0.02" b="-0.01" c="0.03"/><tca model="linear" focal="35" kr="1.002" kb="0.999"/><vignetting model="pa" focal="35" aperture="2" distance="1" k1="-0.2" k2="0.03" k3="0"/><vignetting model="pa" focal="35" aperture="8" distance="10" k1="-0.1"/></calibration></lens></lensdatabase>"#;
    let db = ProfileDatabase::from_lensfun(xml).unwrap();
    let p = &db.profiles[0];
    let a = p.sample(35., 2., 1.).unwrap();
    let b = p.sample(35., 8., 10.).unwrap();
    assert_eq!(a.vignette[0], -0.2);
    assert_eq!(b.vignette[0], -0.1);
    assert_eq!(a.ca_red[0], 1.002);
    let q = a.distort([0.5, 0.]);
    let expected = 0.5 * (0.96 + 0.03 * 0.5 - 0.01 * 0.25 + 0.02 * 0.125);
    assert!((q[0] - expected).abs() < 1e-10, "{q:?}");
}
