use color_mgmt::*;
use std::sync::Arc;
#[test]
fn lut_is_cached_red_fastest_and_interpolates_identity() {
    let p = Registry::new().builtin(Builtin::Srgb).unwrap();
    let t = Transform::new(&p, &p, Default::default()).unwrap();
    let lut = t.lut33();
    assert!(Arc::ptr_eq(&lut, &t.lut33()));
    assert_eq!(lut.size, 33);
    assert_eq!(lut.values.len(), 33 * 33 * 33);
    assert!((lut.values[1][0] - 1. / 32.).abs() < 0.0001);
    for rgb in [[0.; 3], [1.; 3], [0.231, 0.731, 0.492], [-1., 2., 0.5]] {
        let result = lut.sample(rgb);
        for c in 0..3 {
            assert!((result[c] - rgb[c].clamp(0., 1.)).abs() < 0.0001);
        }
    }
}
#[test]
fn lut_matches_direct_at_grid_points_for_non_identity() {
    let mut r = Registry::new();
    let w = r.builtin(Builtin::LinearRec2020).unwrap();
    let d = r.builtin(Builtin::DisplayP3).unwrap();
    let t = Transform::new(&w, &d, Default::default()).unwrap();
    let lut = t.lut33();
    for rgb in [[0.5, 0.25, 0.75], [1., 0., 0.], [0., 0., 0.]] {
        let direct = t.apply(rgb);
        let sampled = lut.sample(rgb);
        for c in 0..3 {
            assert!((direct[c] - sampled[c]).abs() < 0.00001);
        }
    }
}
