use vector::*;
fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Shape::Rectangle {
        rect: Rect::new(x, y, x + w, y + h),
        radii: [0.; 4],
    }
    .path()
    .unwrap()
}
#[test]
fn fill_rules_holes_touching_and_empty() {
    let mut p = rect(0., 0., 4., 4.);
    p.subpaths.extend(rect(1., 1., 2., 2.).subpaths);
    assert!((p.area(1e-5).unwrap() - 16.).abs() < 1e-8);
    p.fill_rule = FillRule::EvenOdd;
    assert!((p.area(1e-5).unwrap() - 12.).abs() < 1e-8);
    assert!(!p.contains(Point::new(2., 2.)));
    let r = VectorRenderer::default()
        .coverage(
            &p,
            Viewport {
                width: 4,
                height: 4,
                origin: Point::ZERO,
                level: 0,
            },
        )
        .unwrap();
    assert_eq!(r.data.iter().sum::<f32>(), 12.);
    assert_eq!(r.data[5], 0.);
    assert_eq!(Path::default().area(1e-5).unwrap(), 0.);
    assert_eq!(
        p.boolean(&p, Operation::Exclude, 1e-5)
            .unwrap()
            .area(1e-5)
            .unwrap(),
        0.
    );
    assert!(
        (rect(0., 0., 1., 1.)
            .boolean(&rect(1., 0., 1., 1.), Operation::Combine, 1e-5)
            .unwrap()
            .area(1e-5)
            .unwrap()
            - 2.)
            .abs()
            < 1e-8
    );
}
#[test]
fn nonfinite_primitives_rejected() {
    assert!(
        Shape::Polygon {
            center: Point::new(f64::NAN, 0.),
            radius: 1.,
            sides: 3,
            rotation: 0.,
            inner_radius: None
        }
        .path()
        .is_err()
    );
    assert!(
        Shape::Line {
            start: Point::ZERO,
            end: Point::new(f64::INFINITY, 0.)
        }
        .path()
        .is_err()
    );
    assert!(
        Shape::Ellipse {
            center: Point::ZERO,
            radii: Vec2::new(f64::NAN, 1.)
        }
        .path()
        .is_err()
    );
}
#[test]
fn zero_stroke_and_bad_inputs() {
    let p = rect(0., 0., 2., 2.);
    assert_eq!(
        Stroke {
            width: 0.,
            ..Stroke::default()
        }
        .outline(&p, 1e-3)
        .unwrap()
        .area(1e-3)
        .unwrap(),
        0.
    );
    assert!(
        Stroke {
            dashes: vec![0., 1.],
            ..Stroke::default()
        }
        .outline(&p, 1e-3)
        .is_err()
    );
    assert!(
        Stroke {
            alignment: Alignment::Inside,
            ..Stroke::default()
        }
        .outline(
            &Path::polyline(&[Point::ZERO, Point::new(1., 1.)], false),
            1e-3
        )
        .is_err()
    );
    assert!(
        VectorRenderer { tolerance: 0. }
            .coverage(
                &p,
                Viewport {
                    width: 1,
                    height: 1,
                    origin: Point::ZERO,
                    level: 0
                }
            )
            .is_err()
    );
    assert!(content_aware_scale(&p, Vec2::new(2., 2.)).is_err());
}
#[test]
fn fractional_rectangle_coverage() {
    let r = VectorRenderer::default()
        .coverage(
            &rect(0.25, 0.25, 0.5, 0.5),
            Viewport {
                width: 1,
                height: 1,
                origin: Point::ZERO,
                level: 0,
            },
        )
        .unwrap();
    assert_eq!(r.data, [0.25]);
}
#[test]
fn caps_joins_odd_dashes() {
    let line = Path::polyline(&[Point::ZERO, Point::new(10., 0.)], false);
    for (cap, x) in [
        (LineCap::Butt, 0.),
        (LineCap::Square, -1.),
        (LineCap::Round, -1.),
    ] {
        let b = Stroke {
            width: 2.,
            cap,
            ..Stroke::default()
        }
        .outline(&line, 1e-4)
        .unwrap()
        .bounds();
        assert!((b.x0 - x).abs() < 1e-3);
        assert!((b.y0 + 1.).abs() < 1e-3);
    }
    let s = Stroke {
        dashes: vec![2.],
        dash_offset: 1.,
        ..Stroke::default()
    };
    let d = s.dashed(&line, 1e-3).unwrap();
    assert_eq!(d.subpaths.len(), 3);
    assert_eq!(
        d.subpaths[0].anchors.last().unwrap().point,
        Point::new(1., 0.)
    );
    for join in [LineJoin::Miter, LineJoin::Bevel, LineJoin::Round] {
        assert!(
            Stroke {
                join,
                ..Stroke::default()
            }
            .outline(&rect(0., 0., 2., 2.), 1e-3)
            .unwrap()
            .area(1e-3)
            .unwrap()
                > 0.
        );
    }
}
#[test]
fn all_gradient_kinds() {
    for (kind, end) in [
        (GradientKind::Linear, Point::new(2., 0.)),
        (GradientKind::Radial, Point::new(0., 2.)),
        (GradientKind::Reflected, Point::new(-2., 0.)),
        (GradientKind::Diamond, Point::new(1., 1.)),
    ] {
        let g = Gradient::new(
            kind,
            Point::ZERO,
            Point::new(2., 0.),
            vec![
                Stop {
                    position: 0.,
                    color: [0.; 4],
                },
                Stop {
                    position: 1.,
                    color: [1.; 4],
                },
            ],
            true,
        )
        .unwrap();
        assert_eq!(g.sample(Point::ZERO), [0.; 4]);
        assert_eq!(g.sample(end), [1.; 4]);
    }
    let g = Gradient::new(
        GradientKind::Angle,
        Point::ZERO,
        Point::new(1., 0.),
        vec![
            Stop {
                position: 0.,
                color: [0.; 4],
            },
            Stop {
                position: 1.,
                color: [1.; 4],
            },
        ],
        false,
    )
    .unwrap();
    assert_eq!(g.sample(Point::new(-1., 0.)), [0.5; 4]);
}
#[test]
fn live_properties_and_layer() {
    let mut shape = Shape::Rectangle {
        rect: Rect::new(0., 0., 4., 4.),
        radii: [0., 1., 2., 0.],
    };
    let a = shape.path().unwrap().area(1e-4).unwrap();
    if let Shape::Rectangle { radii, .. } = &mut shape {
        *radii = [0.; 4];
    }
    assert!(shape.path().unwrap().area(1e-4).unwrap() > a);
    let star = Shape::Polygon {
        center: Point::ZERO,
        radius: 2.,
        sides: 5,
        rotation: 0.,
        inner_radius: Some(1.),
    }
    .path()
    .unwrap();
    assert_eq!(star.subpaths[0].anchors.len(), 10);
    let layer = ShapeLayer {
        shape: Shape::Custom(rect(0., 0., 2., 2.)),
        fill: Some(Fill::Solid([1., 0., 0., 1.])),
        stroke: Some((
            Stroke {
                width: 1.,
                alignment: Alignment::Inside,
                ..Stroke::default()
            },
            Fill::Solid([0., 0., 1., 1.]),
        )),
        transform: Affine::IDENTITY,
    };
    let r = VectorRenderer::default()
        .layer(
            &layer,
            Viewport {
                width: 2,
                height: 2,
                origin: Point::ZERO,
                level: 0,
            },
        )
        .unwrap();
    assert_eq!(r.data, vec![[0., 0., 1., 1.]; 4]);
}
