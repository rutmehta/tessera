use transform::Point;
use transform::adaptive::{self, Adaptive, CameraModel, Projection};
use transform::displacement::Displacement;

#[test]
fn camera_sphere_reprojection_recovers_synthetic_fisheye_grid() {
    let camera = CameraModel::Manual {
        focal_px: 85.,
        center: [100., 80.],
        projection: Projection::Equidistant,
    };
    let a = Adaptive::new(200, 160, camera);
    let d = a.solve().unwrap();
    // Independent equidistant projection of rectilinear horizontal/vertical grid.
    for y in [25., 50., 80., 110., 135.] {
        for x in [25., 50., 75., 100., 125., 150., 175.] {
            let ideal = [x, y];
            let dx: f64 = x - 100.;
            let dy: f64 = y - 80.;
            let r = dx.hypot(dy);
            let factor = if r == 0. {
                1.
            } else {
                85. * (r / 85.).atan() / r
            };
            let observed = [100. + factor * dx, 80. + factor * dy];
            let recovered = a.project(observed).unwrap();
            assert!((recovered[0] - ideal[0]).hypot(recovered[1] - ideal[1]) < 1e-8);
            let source = d.inverse(ideal).unwrap();
            assert!((source[0] - observed[0]).hypot(source[1] - observed[1]) < 0.01);
        }
    }
    let identity = Adaptive::new(
        40,
        30,
        CameraModel::Manual {
            focal_px: 50.,
            center: [20., 15.],
            projection: Projection::Rectilinear,
        },
    );
    assert_eq!(
        identity.solve().unwrap().inverse([10.5, 12.5]),
        Some([10.5, 12.5])
    );
    let mut bad = identity;
    bad.output_focal_px = 0.;
    assert!(bad.solve().is_err());
}

#[test]
fn profile_native_terms_and_crop_scale_round_trip() {
    let sample = lens::CalibrationSample {
        distortion: lens::BrownConrady {
            k1: -0.08,
            p1: 0.001,
            ..Default::default()
        },
        distortion_scale: 0.99,
        radial_odd: [0.004, -0.002],
        coordinate_scale: [1.2, 0.9],
        ..Default::default()
    };
    let profile = lens::Profile {
        model: "test".into(),
        samples: vec![sample.clone()],
        ..Default::default()
    };
    let camera = CameraModel::from_profile(&profile, 50., 4., 10., 36., [180, 120]).unwrap();
    let mut a = Adaptive::new(180, 120, camera);
    a.scale = 1.2;
    a.crop_factor = 1.5;
    a.output_focal_px *= 0.8;
    a.crop = [12., 8.];
    a.output_width = 150;
    a.output_height = 100;
    let d = a.solve().unwrap();
    for p in [[40., 30.], [90., 60.], [135., 85.]] {
        let n = [p[0] / 90. - 1., p[1] / 60. - 1.];
        let distorted = sample.distort(n);
        let source = [(distorted[0] + 1.) * 90., (distorted[1] + 1.) * 60.];
        let expected = [
            90. + 1.44 * (p[0] - 90.) - 12.,
            60. + 1.44 * (p[1] - 60.) - 8.,
        ];
        let q = a.project(source).unwrap();
        assert!((q[0] - expected[0]).hypot(q[1] - expected[1]) < 1e-6);
        let s = d.inverse(q).unwrap();
        assert!((s[0] - source[0]).hypot(s[1] - source[1]) < 0.02);
    }
    let mut invalid = profile;
    invalid.samples[0].coordinate_scale = [0., 1.];
    assert!(CameraModel::from_profile(&invalid, 50., 4., 10., 36., [180, 120]).is_err());
}

fn rectilinear() -> Adaptive {
    Adaptive::new(
        160,
        120,
        CameraModel::Manual {
            focal_px: 100.,
            center: [80., 60.],
            projection: Projection::Rectilinear,
        },
    )
}

#[test]
fn degenerate_uncovered_and_nonconvergent_recipes_report_errors() {
    use adaptive::{LineConstraint, LineOrientation};
    let mut a = rectilinear();
    a.crop = [10000., 10000.];
    assert!(a.solve().unwrap_err().to_string().contains("coverage"));
    a = rectilinear();
    a.lines.push(LineConstraint {
        points: vec![[20., 30.], [20., 30.]],
        orientation: LineOrientation::Straight,
        weight: 1.,
    });
    assert!(a.solve().unwrap_err().to_string().contains("degenerate"));
    a.lines[0].points = vec![[20., 30.], [80., 34.], [140., 30.]];
    a.max_iterations = 1;
    assert!(a.solve().unwrap_err().to_string().contains("converge"));
    a = rectilinear();
    a.mesh_size = [usize::MAX, 17];
    assert!(a.solve().is_err());
    a = rectilinear();
    a.output_width = usize::MAX;
    assert!(a.solve().is_err());
    a = rectilinear();
    a.scale = f64::NAN;
    assert!(a.solve().is_err());
    a = rectilinear();
    a.regularization = 0.;
    assert!(a.solve().is_err());
    let camera = CameraModel::Manual {
        focal_px: 20.,
        center: [80., 60.],
        projection: Projection::Equidistant,
    };
    a = Adaptive::new(160, 120, camera);
    assert!(a.project([140., 60.]).is_none());
    a.lines.push(LineConstraint {
        points: vec![[80., 60.], [140., 60.]],
        orientation: LineOrientation::Straight,
        weight: 1.,
    });
    assert!(a.solve().unwrap_err().to_string().contains("horizon"));
}

#[test]
fn straight_and_vertical_constraints_and_source_segments() {
    use adaptive::{LineConstraint, LineOrientation};
    for orientation in [LineOrientation::Straight, LineOrientation::Vertical] {
        let mut a = rectilinear();
        let points: Vec<_> = (0..=40)
            .map(|i| {
                let t = i as f64 / 40.;
                if orientation == LineOrientation::Vertical {
                    [65. + 3. * (t * std::f64::consts::PI).sin(), 20. + 80. * t]
                } else {
                    [
                        20. + 120. * t,
                        30. + 45. * t + 3. * (t * std::f64::consts::PI).sin(),
                    ]
                }
            })
            .collect();
        a.lines.push(LineConstraint {
            points: points.clone(),
            orientation,
            weight: 1.,
        });
        let d = a.solve().unwrap();
        let q: Vec<_> = points.iter().map(|p| destination(&d, *p, *p)).collect();
        if orientation == LineOrientation::Vertical {
            let range = q.iter().map(|p| p[0]).fold(f64::NEG_INFINITY, f64::max)
                - q.iter().map(|p| p[0]).fold(f64::INFINITY, f64::min);
            assert!(range < 0.5, "vertical range {range}");
        } else {
            let v = [q[40][0] - q[0][0], q[40][1] - q[0][1]];
            for p in q.iter() {
                let error =
                    ((p[0] - q[0][0]) * v[1] - (p[1] - q[0][1]) * v[0]).abs() / v[0].hypot(v[1]);
                assert!(error < 0.5, "straight error {error}");
            }
        }
    }
    let mut a = Adaptive::new(
        160,
        120,
        CameraModel::Manual {
            focal_px: 100.,
            center: [80., 60.],
            projection: Projection::Equidistant,
        },
    );
    a.lines.push(LineConstraint {
        points: vec![[30., 40.], [130., 40.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    let d = a.solve().unwrap();
    let ys: Vec<_> = (0..=40)
        .map(|i| {
            let p = [30. + 2.5 * i as f64, 40.];
            destination(&d, p, a.project(p).unwrap())[1]
        })
        .collect();
    let range = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - ys.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(range < 0.5, "segment range {range}");
}

#[test]
fn imperfect_fisheye_model_grid_is_straight_below_half_pixel() {
    use adaptive::{LineConstraint, LineOrientation};
    // Deliberately inaccurate manual focal estimate: constraints, not just the
    // analytical radial model, must remove the remaining curved grid lines.
    let mut a = Adaptive::new(
        200,
        160,
        CameraModel::Manual {
            focal_px: 95.,
            center: [100., 80.],
            projection: Projection::Equidistant,
        },
    );
    let observed = |p: Point| {
        let x = p[0] - 100.;
        let y = p[1] - 80.;
        let r = x.hypot(y);
        let k = if r < 1e-12 {
            1.
        } else {
            85. * (r / 85.).atan() / r
        };
        [100. + k * x, 80. + k * y]
    };
    for axis in 0..2 {
        for fixed in [35., 60., 85., 110., 135.] {
            let points = (0..=40)
                .map(|i| {
                    let t = 25. + 3. * i as f64;
                    observed(if axis == 0 { [fixed, t] } else { [t, fixed] })
                })
                .collect();
            a.lines.push(LineConstraint {
                points,
                orientation: if axis == 0 {
                    LineOrientation::Vertical
                } else {
                    LineOrientation::Horizontal
                },
                weight: 1.,
            });
        }
    }
    let d = a.solve().unwrap();
    let baseline = a.clone();
    let mut worst = 0.0f64;
    let mut worst_before = 0.0f64;
    for axis in 0..2 {
        for fixed in [35., 60., 85., 110., 135.] {
            let mut lo = f64::INFINITY;
            let mut hi = f64::NEG_INFINITY;
            let mut before_lo = f64::INFINITY;
            let mut before_hi = f64::NEG_INFINITY;
            // Independent inter-sample positions, not the fitted input vertices.
            for i in 0..80 {
                let t = 25.75 + 1.5 * i as f64;
                let p = observed(if axis == 0 { [fixed, t] } else { [t, fixed] });
                let base = baseline.project(p).unwrap();
                before_lo = before_lo.min(base[axis]);
                before_hi = before_hi.max(base[axis]);
                let q = destination(&d, p, base);
                lo = lo.min(q[axis]);
                hi = hi.max(q[axis]);
            }
            worst = worst.max(hi - lo);
            worst_before = worst_before.max(before_hi - before_lo);
        }
    }
    assert!(
        worst_before > 0.5,
        "fixture must actually need constraint correction"
    );
    assert!(worst < 0.5, "corrected grid worst axis range {worst}");
    println!("fisheye grid max axis range: before={worst_before:.6}px, after={worst:.6}px");
}

#[test]
fn incompatible_constraints_fail_instead_of_folding() {
    use adaptive::{LineConstraint, LineOrientation};
    let mut a = rectilinear();
    a.lines.push(LineConstraint {
        points: vec![[80., 20.], [80., 100.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    assert!(a.solve().is_err());
    a = rectilinear();
    a.lines.push(LineConstraint {
        points: vec![[20., 30.], [80., 45.], [140., 30.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    a.lines.push(LineConstraint {
        points: vec![[20., 30.], [80., 45.], [140., 30.]],
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    assert!(a.solve().is_err());
}

// Invert the emitted field, independently of the solver's mesh, to measure
// straightness in final destination pixels (including field interpolation).
fn destination(d: &Displacement, source: Point, mut q: Point) -> Point {
    for _ in 0..40 {
        let p = d.inverse(q).expect("interior field coverage");
        let e = [p[0] - source[0], p[1] - source[1]];
        if e[0].hypot(e[1]) < 1e-7 {
            return q;
        }
        let h = 0.001;
        let x = d.inverse([q[0] + h, q[1]]).unwrap();
        let y = d.inverse([q[0], q[1] + h]).unwrap();
        let j = [
            (x[0] - p[0]) / h,
            (y[0] - p[0]) / h,
            (x[1] - p[1]) / h,
            (y[1] - p[1]) / h,
        ];
        let det = j[0] * j[3] - j[1] * j[2];
        q[0] -= (j[3] * e[0] - j[1] * e[1]) / det;
        q[1] -= (-j[2] * e[0] + j[0] * e[1]) / det;
    }
    panic!("test inverse did not converge");
}

#[test]
fn mesh_constraints_really_straighten_curved_lines_and_are_deterministic() {
    use adaptive::{LineConstraint, LineOrientation};
    let mut a = Adaptive::new(
        160,
        120,
        CameraModel::Manual {
            focal_px: 100.,
            center: [80., 60.],
            projection: Projection::Rectilinear,
        },
    );
    let points: Vec<_> = (0..=40)
        .map(|i| {
            let x = 20. + 3. * i as f64;
            [x, 40. + 5. * ((x - 80.) / 60.).powi(2)]
        })
        .collect();
    a.lines.push(LineConstraint {
        points: points.clone(),
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    let d = a.solve().unwrap();
    let ys: Vec<_> = points.iter().map(|p| destination(&d, *p, *p)[1]).collect();
    let range = ys.iter().copied().fold(f64::NEG_INFINITY, f64::max)
        - ys.iter().copied().fold(f64::INFINITY, f64::min);
    assert!(range < 0.5, "horizontal residual {range}");
    assert!(
        (d.inverse([80., 40.]).unwrap()[1] - 40.).abs() > 0.5,
        "constraints must change the radial-only result"
    );
    assert_eq!(d, a.solve().unwrap());
    let json = serde_json::to_string(&a).unwrap();
    let b: Adaptive = serde_json::from_str(&json).unwrap();
    assert_eq!(a, b);
    assert_eq!(d, b.solve().unwrap());
}

#[test]
fn displacement_interpolates_absolute_pixel_centers_and_rejects_bad_payloads() {
    let mut d = Displacement {
        width: 2,
        height: 2,
        coordinates: (0..=2)
            .flat_map(|y| (0..=2).map(move |x| Some([x as f64 + 3., y as f64 + 4.])))
            .collect(),
    };
    d.validate().unwrap();
    assert_eq!(d.inverse([0.5, 1.5]), Some([3.5, 5.5]));
    assert_eq!(d.inverse([2., 2.]), Some([5., 6.]));
    assert_eq!(d.inverse([-0.1, 1.]), None);
    assert_eq!(d.inverse([f64::NAN, 1.]), None);
    d.coordinates[0] = None;
    assert_eq!(d.inverse([0.5, 0.5]), None);
    let json = serde_json::to_string(&d).unwrap();
    assert_eq!(d, serde_json::from_str(&json).unwrap());
    d.coordinates.pop();
    assert!(d.validate().is_err());
    assert_eq!(d.inverse([2., 2.]), None);
}
