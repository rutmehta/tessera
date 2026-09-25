use color_mgmt::{Builtin, Registry, Transform, TransformOptions};

#[test]
fn linearized_matrix_profile_preserves_signed_values_and_reuses_registry() {
    let mut registry = Registry::new();
    let source = registry.builtin(Builtin::LinearRec2020).unwrap();
    let target = registry.builtin(Builtin::AdobeRgb).unwrap();
    let linear = registry.linearized_rgb(&target).unwrap().unwrap();
    let again = registry.linearized_rgb(&target).unwrap().unwrap();
    assert!(std::sync::Arc::ptr_eq(&linear, &again));
    let to_linear = Transform::new(&source, &linear, TransformOptions::default()).unwrap();
    let transfer = Transform::new(&linear, &target, TransformOptions::default()).unwrap();
    let direct = Transform::new(&source, &target, TransformOptions::default()).unwrap();
    let green = to_linear.apply([0.0, 1.0, 0.0]);
    assert!(green.iter().any(|v| *v < 0.0), "{green:?}");
    for rgb in [[0.18; 3], [0.3, 0.2, 0.1], [0.0, 1.0, 0.0]] {
        let actual = transfer.apply(to_linear.apply(rgb));
        let expected = direct.apply(rgb);
        for c in 0..3 {
            assert!((actual[c] - expected[c]).abs() < 1e-4);
        }
    }
}
