use transform::vanishing::*;
use transform::{Image, Kernel, Point};

#[test]
fn document_clone_crosses_planes_and_roundtrips_serde() {
    let a = PlaneSpace::from_quad(rect(0., 0., 10., 10.), [10., 10.]).unwrap();
    let b = PlaneSpace {
        canvas_quad: rect(10., 0., 20., 10.),
        plane_quad: rect(10., 0., 10., 10.),
    };
    let doc = VanishingPoint {
        planes: vec![a, b],
        camera: Camera::default(),
    };
    doc.validate().unwrap();
    close(doc.clone_source([9., 5.], [2., 0.]).unwrap(), [12., 5.]);
    close(doc.clone_source([12., 5.], [-2., 0.]).unwrap(), [9., 5.]);
    assert!(doc.clone_source([1., 5.], [-2., 0.]).is_none());
    assert!(doc.clone_source([f64::NAN, 5.], [0., 0.]).is_none());
    let decoded: VanishingPoint =
        serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    assert_eq!(doc, decoded);
    assert!(
        VanishingPoint {
            planes: vec![],
            camera: Camera::default()
        }
        .validate()
        .is_err()
    );
    let mut bad = doc.clone();
    bad.planes[1].canvas_quad[3][1] = 8.;
    assert!(bad.validate().is_err());
    let mut bad = doc.clone();
    bad.planes.push(bad.planes[0].clone());
    assert!(bad.validate().is_err());
    let mut bad = doc;
    bad.camera.focal_length = 0.;
    assert!(bad.validate().is_err());
}

#[test]
fn tear_off_uses_pinhole_rotation_and_preserves_entire_edge() {
    let mut doc = VanishingPoint {
        planes: vec![PlaneSpace::from_quad(rect(0., 0., 100., 100.), [100., 100.]).unwrap()],
        camera: Camera {
            focal_length: 100.,
            principal_point: [0., 0.],
        },
    };
    let child = doc.tear_off(0, 1, 25., 60.).unwrap();
    assert_eq!(child, 1);
    let theta = 60_f64.to_radians();
    close(
        doc.planes[1].canvas_quad[2],
        [
            (100. + 25. * theta.cos()) / (1. - 0.25 * theta.sin()),
            100. / (1. - 0.25 * theta.sin()),
        ],
    );
    let a = doc.planes[0].prepare().unwrap();
    let b = doc.planes[1].prepare().unwrap();
    for i in 0..=100 {
        let p = [100., i as f64];
        close(a.plane_to_canvas(p).unwrap(), b.plane_to_canvas(p).unwrap());
    }
    // Exactly 90 degrees is valid when the hinge is off the optical axis.
    let mut right = VanishingPoint {
        planes: vec![doc.planes[0].clone()],
        camera: doc.camera,
    };
    right.tear_off(0, 1, 25., 90.).unwrap();
    close(right.planes[1].canvas_quad[2], [100. / 0.75, 100. / 0.75]);
    let before = right.clone();
    assert!(right.tear_off(0, 1, 200., 90.).is_err()); // camera-plane crossing
    assert!(right.tear_off(0, 4, 2., 0.).is_err());
    assert!(right.tear_off(99, 1, 2., 0.).is_err());
    assert!(right.tear_off(0, 0, 0., 0.).is_err());
    assert!(right.tear_off(0, 0, 2., f64::NAN).is_err());
    assert_eq!(before, right); // failed edits are atomic
}

#[test]
fn tilted_plane_seam_is_projective_not_linearly_reparameterized() {
    let mut doc = VanishingPoint {
        planes: vec![
            PlaneSpace::from_quad([[0., 0.], [100., 0.], [80., 80.], [0., 100.]], [100., 100.])
                .unwrap(),
        ],
        camera: Camera::default(),
    };
    doc.tear_off(0, 1, 20., 20.).unwrap();
    let a = doc.planes[0].prepare().unwrap();
    let b = doc.planes[1].prepare().unwrap();
    for i in 0..=100 {
        let p = [100., i as f64];
        close(a.plane_to_canvas(p).unwrap(), b.plane_to_canvas(p).unwrap());
    }
    let p = a.plane_to_canvas([100., 50.]).unwrap();
    assert!((p[1] - 40.).abs() > 1.); // not the canvas edge midpoint
    close(doc.clone_source(p, [0., 0.]).unwrap(), p);
}

#[test]
fn resampled_paste_is_premultiplied_and_continuous_across_planes() {
    let doc = VanishingPoint {
        planes: vec![
            PlaneSpace::from_quad(rect(0., 0., 2., 2.), [2., 2.]).unwrap(),
            PlaneSpace {
                canvas_quad: rect(2., 0., 4., 2.),
                plane_quad: rect(2., 0., 2., 2.),
            },
        ],
        camera: Camera::default(),
    };
    let input = Image::new(
        4,
        2,
        [
            vec![0., 0.1, 0.2, 0.3, 0., 0.1, 0.2, 0.3],
            vec![0.; 8],
            vec![0.; 8],
            vec![0.5; 8],
        ],
    )
    .unwrap();
    let out = doc
        .paste(&input, 7, 2, [0., 0.], [1., 1.], Kernel::Bilinear)
        .unwrap();
    assert_eq!(out.planes[0][0], 0.);
    assert!((out.planes[0][2] - 0.175).abs() < 1e-6);
    assert!((out.planes[0][3] - 0.225).abs() < 1e-6);
    assert_eq!(out.planes[3][3], 0.5);
    assert_eq!(out.planes[3][6], 0.);
    assert!(
        doc.paste(&input, 0, 2, [0., 0.], [1., 1.], Kernel::Nearest)
            .is_err()
    );
    assert!(
        doc.paste(&input, 7, 2, [0., 0.], [0., 1.], Kernel::Nearest)
            .is_err()
    );
    assert!(
        doc.paste(&input, usize::MAX, 2, [0., 0.], [1., 1.], Kernel::Nearest)
            .is_err()
    );
}

#[test]
fn stroke_keeps_atlas_spacing_and_phase_across_multiple_planes() {
    let mut doc = VanishingPoint {
        planes: vec![PlaneSpace::from_quad(rect(0., 0., 100., 100.), [100., 100.]).unwrap()],
        camera: Camera::default(),
    };
    doc.tear_off(0, 1, 30., 30.).unwrap();
    doc.tear_off(1, 2, 30., -10.).unwrap();
    let ready = doc.prepare().unwrap();
    let samples = ready
        .plane_stroke(&[[90., 50.], [105., 50.], [145., 50.]], 4.)
        .unwrap();
    for (i, s) in samples.iter().enumerate().take(samples.len() - 1) {
        close(s.plane, [90. + i as f64 * 4., 50.]);
        close(ready.canvas_to_plane(s.canvas.unwrap()).unwrap(), s.plane);
    }
    close(samples.last().unwrap().plane, [145., 50.]);
    let gaps = ready
        .plane_stroke(&[[150., 50.], [170., 50.]], 10.)
        .unwrap();
    assert!(gaps.last().unwrap().canvas.is_none());
    assert!(ready.plane_stroke(&[[0., 0.], [1., 1.]], 0.).is_err());
    assert!(ready.plane_stroke(&[[0., 0.], [1., 1.]], 1e-20).is_err());
    assert!(ready.plane_stroke(&[[f64::NAN, 0.]], 1.).is_err());
}

#[test]
fn seam_endpoint_agreement_alone_is_not_enough() {
    let root = PlaneSpace::from_quad(rect(0., 0., 100., 100.), [100., 100.]).unwrap();
    let inconsistent = PlaneSpace {
        canvas_quad: [[100., 0.], [200., 0.], [180., 100.], [100., 100.]],
        plane_quad: rect(100., 0., 100., 100.),
    };
    let doc = VanishingPoint {
        planes: vec![root, inconsistent],
        camera: Camera::default(),
    };
    assert!(doc.validate().is_err());
    assert!(doc.clone_source([50., 50.], [0., 0.]).is_none());
}

#[test]
fn coplanar_tear_off_all_edges_and_both_windings() {
    for reversed in [false, true] {
        for edge in 0..4 {
            let mut plane = PlaneSpace::from_quad(rect(20., 30., 100., 80.), [100., 80.]).unwrap();
            if reversed {
                plane.canvas_quad.reverse();
                plane.plane_quad.reverse();
            }
            let original = plane.prepare().unwrap();
            let mut doc = VanishingPoint {
                planes: vec![plane],
                camera: Camera::default(),
            };
            doc.tear_off(0, edge, 10., 0.).unwrap();
            for p in doc.planes[1].plane_quad {
                close(
                    doc.prepare().unwrap().plane_to_canvas(p).unwrap(),
                    [p[0] + 20., p[1] + 30.],
                );
            }
            let a = doc.planes[0].plane_quad[edge];
            close(
                doc.prepare().unwrap().plane_to_canvas(a).unwrap(),
                original.plane_to_canvas(a).unwrap(),
            );
        }
    }
}

#[test]
fn folded_clone_has_no_seam_jump_and_survives_serialization() {
    let mut doc = VanishingPoint {
        planes: vec![
            PlaneSpace::from_quad([[0., 0.], [100., 0.], [80., 80.], [0., 100.]], [100., 100.])
                .unwrap(),
        ],
        camera: Camera {
            focal_length: 200.,
            principal_point: [30., 40.],
        },
    };
    doc.tear_off(0, 1, 30., 45.).unwrap();
    let doc: VanishingPoint = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
    let ready = doc.prepare().unwrap();
    for y in 1..100 {
        let left = ready.plane_to_canvas([100. - 1e-9, y as f64]).unwrap();
        let right = ready.plane_to_canvas([100. + 1e-9, y as f64]).unwrap();
        close(
            ready.clone_source(left, [-10., 0.]).unwrap(),
            ready.clone_source(right, [-10., 0.]).unwrap(),
        );
    }
}

fn close(a: Point, b: Point) {
    assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-7, "{a:?} != {b:?}");
}
fn rect(x: f64, y: f64, w: f64, h: f64) -> [Point; 4] {
    [[x, y], [x + w, y], [x + w, y + h], [x, y + h]]
}
#[test]
fn pasted_checker_has_projected_corners_and_projective_resampling() {
    // Independent H = [[2,0,2],[0,2,2],[0.1,0,1]].
    let project = |p: Point| {
        [
            (2. * p[0] + 2.) / (1. + 0.1 * p[0]),
            (2. * p[1] + 2.) / (1. + 0.1 * p[0]),
        ]
    };
    let corners = rect(0., 0., 4., 4.).map(project);
    let plane = PlaneSpace::from_quad(corners, [4., 4.]).unwrap();
    let ready = plane.prepare().unwrap();
    for (p, q) in rect(0., 0., 4., 4.).into_iter().zip(corners) {
        close(ready.plane_to_canvas(p).unwrap(), q);
    }
    let checker: Vec<f32> = (0..16).map(|i| ((i % 4 + i / 4) % 2) as f32).collect();
    let source = Image::new(
        4,
        4,
        [checker.clone(), checker.clone(), checker, vec![1.; 16]],
    )
    .unwrap();
    let doc = VanishingPoint {
        planes: vec![plane],
        camera: Camera::default(),
    };
    let out = doc
        .paste(&source, 12, 12, [0.; 2], [1.; 2], Kernel::Nearest)
        .unwrap();
    let mut checked = 0;
    for y in 0..12 {
        for x in 0..12 {
            let cx = x as f64 + 0.5;
            let cy = y as f64 + 0.5;
            let u = (cx - 2.) / (2. - 0.1 * cx);
            let v = (cy * (1. + 0.1 * u) - 2.) / 2.;
            let i = y * 12 + x;
            if (0. ..4.).contains(&u) && (0. ..4.).contains(&v) {
                assert_eq!(
                    out.planes[0][i],
                    ((u.floor() as usize + v.floor() as usize) % 2) as f32
                );
                assert_eq!(out.planes[3][i], 1.);
                checked += 1;
            } else {
                assert_eq!(out.planes[3][i], 0.);
            }
        }
    }
    assert!(checked > 20);
}

#[test]
fn quad_plane_roundtrips_with_pure_projective_mapping() {
    let quad = [[10., 5.], [180., 20.], [140., 110.], [20., 90.]];
    let plane = PlaneSpace::from_quad(quad, [100., 80.]).unwrap();
    let prepared = plane.prepare().unwrap();
    for (canvas, local) in quad.into_iter().zip(rect(0., 0., 100., 80.)) {
        close(prepared.canvas_to_plane(canvas).unwrap(), local);
        close(prepared.plane_to_canvas(local).unwrap(), canvas);
    }
    for p in [[1., 3.], [40., 50.], [100., 80.]] {
        close(
            prepared
                .canvas_to_plane(prepared.plane_to_canvas(p).unwrap())
                .unwrap(),
            p,
        );
    }
    assert!(prepared.canvas_to_plane([-100., 0.]).is_none());
    assert!(PlaneSpace::from_quad([[0.; 2]; 4], [1., 1.]).is_err());
    assert!(PlaneSpace::from_quad(quad, [0., 1.]).is_err());
    assert!(PlaneSpace::from_quad([[0., 0.], [1., 1.], [0., 1.], [1., 0.]], [1., 1.]).is_err());
}
