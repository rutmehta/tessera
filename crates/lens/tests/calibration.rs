use lens::*;
#[test]
fn recover_radial_straightness() {
    let m = BrownConrady {
        k1: 0.13,
        ..Default::default()
    };
    let lines: Vec<Vec<Point>> = [-0.7, -0.4, 0.3, 0.65]
        .iter()
        .map(|x| {
            (0..41)
                .map(|i| m.distort([*x, -0.8 + i as f64 * 0.04]))
                .collect()
        })
        .collect();
    let e = estimate_k1(&lines, [-0.2, 0.3]).unwrap();
    assert!((e.value - 0.13).abs() < 0.001, "{e:?}");
    assert!(e.residual < 1e-8);
    assert!(estimate_k1(&[], [-0.2, 0.3]).is_none());
}
