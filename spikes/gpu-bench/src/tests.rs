use crate::data;

#[test]
fn timing_rejects_invalid_samples_before_taking_median() {
    for (gpu, wall) in [
        (vec![], vec![]),
        (vec![1.0], vec![1.0, 2.0]),
        (vec![0.0, 1.0, 2.0], vec![3.0; 3]),
        (vec![-1.0, 1.0, 2.0], vec![3.0; 3]),
        (vec![f64::INFINITY, 1.0, 2.0], vec![3.0; 3]),
        (vec![1.0; 3], vec![f64::NAN, 2.0, 3.0]),
    ] {
        assert!(std::panic::catch_unwind(|| crate::Timing::from_samples(gpu, wall)).is_err());
    }
}

#[test]
fn accuracy_includes_alpha() {
    let reference = [[0.0, 0.0, 0.0, 1.0]];
    let (max, mean) = data::errors(&[0.0, 0.0, 0.0, 0.0], &reference);
    assert_eq!(max, 1.0);
    assert_eq!(mean, 0.25);
}

#[test]
#[should_panic(expected = "non-finite")]
fn accuracy_rejects_nonfinite_reference() {
    data::errors(&[0.0; 4], &[[f32::NAN, 0.0, 0.0, 1.0]]);
}

#[test]
#[should_panic(expected = "non-finite")]
fn accuracy_rejects_nonfinite_output() {
    data::errors(&[0.0, 0.0, 0.0, f32::INFINITY], &[[0.0; 4]]);
}

#[test]
fn medians_use_middle_pair() {
    assert_eq!(crate::median(vec![9.0, 1.0, 7.0, 3.0]), 5.0);
}

#[test]
fn oklab_roundtrip() {
    for c in [[0.0; 3], [1.0; 3], [0.9, 0.1, 0.3], [-0.1, 0.2, 1.4]] {
        let r = data::from_oklab(data::to_oklab(c));
        for i in 0..3 {
            assert!((r[i] - c[i]).abs() < 1e-5);
        }
    }
}
