use vector::*;
#[test]
fn perspective_corners_and_mesh_handles() {
    let a = [
        Point::new(0., 0.),
        Point::new(2., 0.),
        Point::new(2., 2.),
        Point::new(0., 2.),
    ];
    let b = [
        Point::new(1., 1.),
        Point::new(4., 0.),
        Point::new(3., 5.),
        Point::new(-1., 3.),
    ];
    let h = Perspective::from_quads(a, b).unwrap();
    for i in 0..4 {
        assert!(h.map(a[i]).unwrap().distance(b[i]) < 1e-12);
        assert!(h.inverse().unwrap().map(b[i]).unwrap().distance(a[i]) < 1e-12);
    }
    let mut m = MeshWarp::identity(Rect::new(0., 0., 2., 2.), 2, 2).unwrap();
    assert!(
        m.map(Point::new(0.7, 1.3))
            .unwrap()
            .distance(Point::new(0.7, 1.3))
            < 1e-12
    );
    m.patches[0][1][1].y += 0.2;
    let p = Point::new(0.4, 0.4);
    let q = m.map(p).unwrap();
    assert!(q.y > p.y);
    assert!(m.inverse_map(q, 1e-8).unwrap().distance(p) < 1e-7);
    let path = Path::polyline(&a, true);
    let mapped = path.warp(&h, 1e-4).unwrap();
    assert!(mapped.bounds().width() > 4.9);
}
#[test]
fn anchor_editing_and_curvature() {
    let mut p = Path::polyline(&[Point::new(0., 0.), Point::new(10., 0.)], false);
    p.split_segment(0, 0, 0.5).unwrap();
    assert_eq!(p.subpaths[0].anchors.len(), 3);
    assert_eq!(p.subpaths[0].anchors[1].point, Point::new(5., 0.));
    p.move_anchor(0, 1, Point::new(5., 2.)).unwrap();
    p.set_handle(0, 1, Handle::Outgoing, Point::new(7., 2.), true)
        .unwrap();
    assert_eq!(p.subpaths[0].anchors[1].incoming, Point::new(3., 2.));
    assert_eq!(p.hit_anchor(Point::new(5., 2.), 0.1), Some((0, 1)));
    let curve = Path::curvature_pen(
        &[Point::new(0., 0.), Point::new(1., 2.), Point::new(3., 0.)],
        false,
    )
    .unwrap();
    assert_ne!(curve.subpaths[0].anchors[1].outgoing, Point::new(1., 2.));
    assert!(p.hit_path(Point::new(5., 2.), 0.01).is_some());
}
