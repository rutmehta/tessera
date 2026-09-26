use vector::*;
#[test]
fn alignment_bounds_and_dashes() {
    let p = Shape::Rectangle {
        rect: Rect::new(0., 0., 10., 10.),
        radii: [0.; 4],
    }
    .path()
    .unwrap();
    for (alignment, edge) in [
        (Alignment::Center, -1.),
        (Alignment::Inside, 0.),
        (Alignment::Outside, -2.),
    ] {
        let s = Stroke {
            width: 2.,
            alignment,
            ..Stroke::default()
        };
        let b = s.outline(&p, 0.001).unwrap().bounds();
        assert!((b.x0 - edge).abs() < 1e-5);
        assert!((b.x1 - (10. - edge)).abs() < 1e-5);
    }
    let line = Path::polyline(&[Point::new(0., 0.), Point::new(10., 0.)], false);
    let s = Stroke {
        dashes: vec![2., 1.],
        ..Stroke::default()
    };
    let d = s.dashed(&line, 0.001).unwrap();
    let lengths: Vec<_> = d
        .flattened(0.001)
        .unwrap()
        .iter()
        .map(|(p, _)| p.windows(2).map(|p| p[0].distance(p[1])).sum::<f64>())
        .collect();
    assert_eq!(lengths, vec![2., 2., 2., 1.]);
}
