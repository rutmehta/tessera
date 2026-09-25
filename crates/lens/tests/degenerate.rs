use lens::*;
#[test]
fn full_requires_two_valid_vanishing_families() {
    let make = |a, b| LineSegment {
        start: a,
        end: b,
        points: vec![],
        strength: 1.,
    };
    let lines = vec![
        make([-0.5, -1.], [-0.5, 1.]),
        make([-0.5, -1.], [-0.5, 1.]),
        make([-1., -0.5], [1., -0.5]),
        make([-1., 0.5], [1., 0.5]),
    ];
    assert!(estimate_upright(&lines, UprightMode::Full).is_none());
    assert!(estimate_upright(&lines, UprightMode::Auto).is_some());
}
#[test]
fn malformed_and_nonfinite_are_rejected() {
    assert!(BrownConrady::default().undistort([f64::NAN, 0.]).is_none());
    assert!(GrayImage::new(3, 3, vec![f64::INFINITY; 9]).is_err());
    for xml in ["<a><b></a>","<!DOCTYPE a><a/>","<lensdatabase><lens><model>A</model><calibration><distortion model='poly3' k1='NaN'/></calibration></lens></lensdatabase>"]{assert!(ProfileDatabase::from_lensfun(xml).is_err());}
    assert!(Homography([[0.; 3]; 3]).inverse().is_none());
}
