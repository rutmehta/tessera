use lens::*;
#[test]
fn brown_round_trip() {
    let m = BrownConrady {
        k1: 0.12,
        k2: -0.01,
        p1: 0.003,
        p2: -0.002,
        ..Default::default()
    };
    let p = [0.7, -0.6];
    let q = m.distort(p);
    assert!((q[0] - p[0]).abs() > 0.01);
    let r = m.undistort(q).unwrap();
    assert!((r[0] - p[0]).abs() < 1e-9 && (r[1] - p[1]).abs() < 1e-9);
}
