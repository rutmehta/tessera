use transform::free::FreeTransform;

#[test]
fn reference_helpers_preserve_pivot() {
    let pivot = [23., -9.];
    for t in [
        FreeTransform::scale(2., 3., pivot),
        FreeTransform::rotate(0.7, pivot),
        FreeTransform::skew(0.2, -0.1, pivot),
        FreeTransform::flip(true, false, pivot),
    ] {
        let t = t.unwrap();
        let q = t.map(pivot).unwrap();
        assert!((q[0] - pivot[0]).abs() < 1e-12 && (q[1] - pivot[1]).abs() < 1e-12);
    }
    assert_eq!(FreeTransform::identity().map(pivot), Some(pivot));
    assert_eq!(
        FreeTransform::translate(2., 3.).unwrap().map(pivot),
        Some([25., -6.])
    );
    assert_eq!(
        FreeTransform::flip(true, true, pivot)
            .unwrap()
            .map([24., -8.]),
        Some([22., -10.])
    );
    assert!(FreeTransform::scale(0., 1., pivot).is_err());
    assert!(FreeTransform::rotate(f64::NAN, pivot).is_err());
}

#[test]
fn homography_inverse_and_exact_bounds() {
    let t = FreeTransform {
        matrix: [[2., 0., 3.], [0., 3., -2.], [0., 0., 1.]],
    };
    t.validate().unwrap();
    assert_eq!(t.map([2., 4.]), Some([7., 10.]));
    let p = t.inverse().unwrap().map([7., 10.]).unwrap();
    assert!((p[0] - 2.).abs() < 1e-12 && (p[1] - 4.).abs() < 1e-12);
    assert_eq!(t.bounds(10., 20.).unwrap(), [[3., -2.], [23., 58.]]);
    let pole = FreeTransform {
        matrix: [[1., 0., 0.], [0., 1., 0.], [1., 0., -5.]],
    };
    assert!(pole.bounds(10., 20.).is_err());
    assert!(
        FreeTransform {
            matrix: [[0.; 3]; 3]
        }
        .validate()
        .is_err()
    );
}
