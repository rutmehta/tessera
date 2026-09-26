use vector::*;
#[test]
fn unit_circle_coverage_and_level() {
    let p = Shape::Ellipse {
        center: Point::new(1., 1.),
        radii: Vec2::new(1., 1.),
    }
    .path()
    .unwrap();
    let r = VectorRenderer::default();
    let v = Viewport {
        width: 2,
        height: 2,
        origin: Point::ZERO,
        level: 0,
    };
    let mask = r.coverage(&p, v).unwrap();
    let area: f64 = mask.data.iter().map(|v| f64::from(*v)).sum();
    assert!(
        (area - std::f64::consts::PI).abs() / std::f64::consts::PI < 0.001,
        "{area}"
    );
    let low = r
        .coverage(
            &p,
            Viewport {
                width: 1,
                height: 1,
                level: 1,
                ..v
            },
        )
        .unwrap();
    assert!((f64::from(low.data[0]) * 4. - area).abs() < 1e-4);
}
#[test]
fn gradient_endpoints_and_pattern() {
    let g = Gradient::new(
        GradientKind::Linear,
        Point::ZERO,
        Point::new(10., 0.),
        vec![
            Stop {
                position: 0.,
                color: [1., 0., 0., 1.],
            },
            Stop {
                position: 1.,
                color: [0., 0., 1., 1.],
            },
        ],
        true,
    )
    .unwrap();
    assert_eq!(g.sample(Point::ZERO), [1., 0., 0., 1.]);
    assert_eq!(g.sample(Point::new(10., 0.)), [0., 0., 1., 1.]);
    let tile = Pattern::new(2, 1, vec![[1.; 4], [0.; 4]], Affine::IDENTITY).unwrap();
    assert_eq!(tile.sample(Point::new(-1., 0.)), [0.; 4]);
    let p = Shape::Rectangle {
        rect: Rect::new(0., 0., 1., 1.),
        radii: [0.; 4],
    }
    .path()
    .unwrap();
    let rgba = VectorRenderer::default()
        .rgba(
            &p,
            &Fill::Solid([1., 0., 0., 0.5]),
            Viewport {
                width: 1,
                height: 1,
                origin: Point::ZERO,
                level: 0,
            },
        )
        .unwrap();
    assert_eq!(rgba.data[0], [0.5, 0., 0., 0.5]);
}
