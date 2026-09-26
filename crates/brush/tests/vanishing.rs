use brush::api::vanishing_point_stroke;
use brush::{Brush, CloneSource, InputPoint, PaintMode, Tip};
use compositor::{Depth, Raster, Rect};
use engine_api::tile::Extent;
use transform::vanishing::{Camera, PlaneSpace, VanishingPoint};

fn document() -> VanishingPoint {
    VanishingPoint {
        camera: Camera::default(),
        planes: vec![
            PlaneSpace::from_quad([[0., 0.], [8., 0.], [8., 8.], [0., 8.]], [8., 8.]).unwrap(),
            PlaneSpace {
                canvas_quad: [[8., 0.], [24., 0.], [24., 8.], [8., 8.]],
                plane_quad: [[8., 0.], [16., 0.], [16., 8.], [8., 8.]],
            },
        ],
    }
}
fn base() -> Raster {
    Raster::new(Extent::new(24, 8), 4, Depth::F32, 0.)
}
fn brush(source: Raster) -> Brush {
    Brush {
        size: 8.,
        tip: Tip::round(1.),
        spacing: 0.1,
        mode: PaintMode::Clone(CloneSource {
            offset: [0.; 2],
            source: Some(source),
        }),
        ..Brush::default()
    }
}
#[test]
fn engine_stroke_samples_through_plane_mapping_across_shared_edge() {
    let mut source = base();
    source
        .edit_region(Rect::new(0, 0, 24, 8), 1, |x, _, p| {
            *p = [x as f32 / 24., 0., 0., 1.]
        })
        .unwrap();
    let mut target = base();
    let mut stroke =
        vanishing_point_stroke(&target, None, brush(source), &document(), [2., 0.], 1).unwrap();
    stroke.add_point(InputPoint::at(5., 4.)).unwrap();
    stroke.add_point(InputPoint::at(17., 4.)).unwrap();
    stroke.finish().unwrap();
    stroke
        .apply(&mut target, Rect::new(0, 0, 24, 8), 2)
        .unwrap();
    for x in 5..17 {
        let src = document()
            .clone_source([x as f64 + 0.5, 4.5], [2., 0.])
            .unwrap();
        assert!(
            (target.pixel(x, 4)[0] - (src[0] as f32 - 0.5) / 24.).abs() < 1e-5,
            "x={x}"
        );
        assert!((target.pixel(x, 4)[3] - 1.).abs() < 1e-5);
    }
}
#[test]
fn adapter_rejects_invalid_inputs() {
    let target = base();
    assert!(
        vanishing_point_stroke(&target, None, Brush::default(), &document(), [0.; 2], 1).is_err()
    );
    assert!(
        vanishing_point_stroke(&target, None, brush(base()), &document(), [f64::NAN, 0.], 1)
            .is_err()
    );
    let mut b = brush(base());
    if let PaintMode::Clone(ref mut c) = b.mode {
        c.offset = [1., 0.];
    }
    assert!(vanishing_point_stroke(&target, None, b, &document(), [0.; 2], 1).is_err());
}

#[test]
fn mapped_clone_filters_premultiplied_alpha_and_respects_selection() {
    let mut source = base();
    source
        .edit_region(Rect::new(0, 0, 24, 8), 1, |x, _, p| {
            *p = if x % 2 == 0 {
                [1., 0., 0., 1.]
            } else {
                [0., 100., 0., 0.]
            };
        })
        .unwrap();
    let mut selection = Raster::new(Extent::new(24, 8), 1, Depth::F32, 1.);
    selection
        .edit_region(Rect::new(0, 0, 3, 8), 1, |_, _, p| p[0] = 0.)
        .unwrap();
    let target = base();
    let mut stroke = vanishing_point_stroke(
        &target,
        Some(&selection),
        brush(source),
        &document(),
        [0.5, 0.],
        1,
    )
    .unwrap();
    stroke.add_point(InputPoint::at(4., 4.)).unwrap();
    stroke.finish().unwrap();
    let mut pixel = [0.; 4];
    stroke.shade(4, 4, &mut pixel);
    assert!((pixel[0] - 1.).abs() < 1e-6);
    assert_eq!(pixel[1], 0.); // Hidden transparent green must not bleed.
    assert!((pixel[3] - 0.5).abs() < 1e-6);
    stroke.shade(2, 4, &mut pixel);
    assert_eq!(pixel, [0.; 4]);
}

#[test]
fn heal_adapter_is_finite_and_masks_out_unmapped_planes() {
    let mut target = base();
    target
        .edit_region(Rect::new(0, 0, 24, 8), 1, |_, _, p| {
            *p = [0.2, 0.3, 0.4, 1.]
        })
        .unwrap();
    let mut b = brush(target.clone());
    if let PaintMode::Clone(c) = b.mode {
        b.mode = PaintMode::Heal(c);
    }
    let mut stroke =
        vanishing_point_stroke(&target, None, b.clone(), &document(), [1., 0.], 1).unwrap();
    stroke.add_point(InputPoint::at(8., 4.)).unwrap();
    stroke.finish().unwrap();
    let mut p = [0.; 4];
    stroke.shade(8, 4, &mut p);
    for (a, b) in p.into_iter().zip([0.2, 0.3, 0.4, 1.]) {
        assert!((a - b).abs() < 1e-5);
    }
    let mut stroke = vanishing_point_stroke(&target, None, b, &document(), [1000., 0.], 1).unwrap();
    stroke.add_point(InputPoint::at(8., 4.)).unwrap();
    stroke.finish().unwrap();
    stroke.shade(8, 4, &mut p);
    assert_eq!(p, target.pixel(8, 4));
}
