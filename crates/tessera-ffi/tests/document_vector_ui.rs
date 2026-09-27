//! Live shapes, Pen / Direct Selection path edits and vector masks over the
//! bridge (WP B5-11): Shape summary dispatch, live primitive regeneration,
//! custom path persistence, inverse-affine / stroke / fill-rule hits, anchor
//! splitting and handles, boolean one-node history, bounds and thumbnail
//! revisions, masks next to raster masks, density / feather / disabled masks,
//! fixed masks on move versus the explicit linked Batch, invalid inputs and
//! locks, conversion with exact undo, and native / PSD editability (standard
//! tags, cached pixels, tvMk restoration and the external raster fallback).
#![cfg(target_os = "macos")]

use std::sync::Arc;
use tessera_ffi::*;
use vector::{
    Alignment, Anchor, Fill, FillRule, Gradient, GradientKind, LineCap, Path, Point, Rect, Shape,
    ShapeModel, Stop, Stroke, Subpath, Vec2,
};

const W: u32 = 200;
const H: u32 = 160;
const RED: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
const BLUE: [f32; 4] = [0.0, 0.0, 1.0, 1.0];

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn doc(engine: &Arc<Engine>) -> Arc<DocumentSession> {
    engine
        .clone()
        .new_document(W, H, DocDepth::U8, None)
        .unwrap()
}

const IDENTITY: TransformMatrix = TransformMatrix {
    a: 1.0,
    b: 0.0,
    c: 0.0,
    d: 0.0,
    e: 1.0,
    f: 0.0,
};

fn translate(tx: f64, ty: f64) -> TransformMatrix {
    TransformMatrix {
        c: tx,
        f: ty,
        ..IDENTITY
    }
}

fn json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap()
}

fn rect_model(x0: f64, y0: f64, x1: f64, y1: f64, radii: [f64; 4]) -> ShapeModel {
    ShapeModel::from_shape(
        Shape::Rectangle {
            rect: Rect::new(x0, y0, x1, y1),
            radii,
        },
        Some(Fill::Solid(RED)),
        None,
    )
    .unwrap()
}

fn add(s: &DocumentSession, model: &ShapeModel, t: TransformMatrix) -> u64 {
    let u = s
        .add_shape_layer(String::new(), None, None, json(model), t)
        .unwrap();
    assert_eq!(u.created.len(), 1, "additions report the created id");
    u.created[0]
}

fn model(s: &DocumentSession, id: u64) -> ShapeModel {
    serde_json::from_str(&s.shape_layer(id).unwrap().model_json).unwrap()
}

fn history(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn head_label(s: &DocumentSession) -> String {
    let items = s.history_items().unwrap();
    items.iter().find(|h| h.is_current).unwrap().label.clone()
}

/// Composite RGBA at level 0.
fn px(s: &DocumentSession, x: u32, y: u32) -> [f32; 4] {
    let (w, _, v) = s.read_level(0).unwrap();
    let i = ((y * w + x) * 4) as usize;
    [v[i], v[i + 1], v[i + 2], v[i + 3]]
}

fn hit(s: &DocumentSession, x: f64, y: f64, stroke: bool) -> Option<(u64, ShapeHitPart)> {
    s.shape_hit_test(x, y, stroke, 0.0)
        .unwrap()
        .map(|h| (h.layer, h.part))
}

fn square(x0: f64, y0: f64, x1: f64, y1: f64) -> Subpath {
    Path::polyline(
        &[
            Point::new(x0, y0),
            Point::new(x1, y0),
            Point::new(x1, y1),
            Point::new(x0, y1),
        ],
        true,
    )
    .subpaths
    .remove(0)
}

fn mask_rect(x0: f64, y0: f64, x1: f64, y1: f64) -> VectorMaskRecord {
    VectorMaskRecord {
        path_json: json(&Path {
            subpaths: vec![square(x0, y0, x1, y1)],
            fill_rule: FillRule::NonZero,
        }),
        enabled: true,
        feather: 0.0,
        density: 1.0,
    }
}

#[test]
fn shape_rows_report_shape_kind_not_fill() {
    let (_d, e) = engine();
    let s = doc(&e);
    let before = history(&s);
    let id = add(&s, &rect_model(10., 10., 60., 40., [0.; 4]), IDENTITY);
    let row = s.layer(id).unwrap();
    assert_eq!(row.kind, DocLayerKind::Shape);
    assert_eq!(row.fill_json, None, "shapes are not fill layers");
    assert_eq!(row.name, "Rectangle 1");
    assert_eq!(history(&s), before + 1);
    assert_eq!(head_label(&s), "Rectangle Tool");
    // The top row is the new shape (rows are top-first, indexes bottom-first).
    let rows = s.layers().unwrap();
    assert_eq!(rows[0].id, id);
    assert_eq!(rows[0].index, 1);
    let rec = s.shape_layer(id).unwrap();
    assert_eq!(rec.live_kind.as_deref(), Some("rectangle"));
    assert_eq!(rec.transform, IDENTITY);
    assert_eq!(rec.vector_mask, None);
    assert!(rec.notes.is_empty(), "{:?}", rec.notes);
    // Other kinds are refused by the shape reads and conversion helper.
    let pixel = rows[1].id;
    assert!(s.shape_layer(pixel).is_err());
    assert!(s.convert_shape_to_pixels(pixel).is_err());
    // Rendered pixels (fill inside, transparent outside).
    s.wait_idle();
    assert_eq!(px(&s, 30, 20), RED);
    assert_eq!(px(&s, 100, 100)[3], 0.0);
    // Insertion index and a parent group are honoured.
    let g = s
        .add_layer(
            NewLayer::Group {
                mode: DocGroupMode::PassThrough,
            },
            String::new(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    let inner = s
        .add_shape_layer(
            "Inner".into(),
            Some(g),
            Some(0),
            json(&rect_model(0., 0., 5., 5., [0.; 4])),
            IDENTITY,
        )
        .unwrap()
        .created[0];
    let row = s.layer(inner).unwrap();
    assert_eq!(
        (row.parent, row.index, row.name.as_str()),
        (Some(g), 0, "Inner")
    );
}

#[test]
fn live_primitives_regenerate_paths_in_one_node() {
    let (_d, e) = engine();
    let s = doc(&e);
    let stroke = Some((Stroke::default(), Fill::Solid(BLUE)));
    let shapes = [
        (
            Shape::Rectangle {
                rect: Rect::new(10., 10., 90., 60.),
                radii: [0., 5., 10., 20.],
            },
            "rectangle",
        ),
        (
            Shape::Ellipse {
                center: Point::new(50., 50.),
                radii: Vec2::new(30., 20.),
            },
            "ellipse",
        ),
        (
            Shape::Polygon {
                center: Point::new(50., 50.),
                radius: 30.,
                sides: 5,
                rotation: 0.,
                inner_radius: Some(12.),
            },
            "polygon",
        ),
        (
            Shape::Line {
                start: Point::new(5., 5.),
                end: Point::new(80., 40.),
            },
            "line",
        ),
    ];
    for (shape, kind) in shapes {
        let m =
            ShapeModel::from_shape(shape.clone(), Some(Fill::Solid(RED)), stroke.clone()).unwrap();
        let id = add(&s, &m, IDENTITY);
        let rec = s.shape_layer(id).unwrap();
        assert_eq!(rec.live_kind.as_deref(), Some(kind));
        assert_eq!(model(&s, id).path, shape.path().unwrap());
        // Edit the construction parameters only (stale path in the JSON):
        // the engine regenerates the path.
        let edited = match shape {
            Shape::Rectangle { rect, .. } => Shape::Rectangle {
                rect,
                radii: [15., 0., 0., 3.],
            },
            Shape::Ellipse { center, .. } => Shape::Ellipse {
                center,
                radii: Vec2::new(10., 40.),
            },
            Shape::Polygon { center, .. } => Shape::Polygon {
                center,
                radius: 30.,
                sides: 8,
                rotation: 0.3,
                inner_radius: Some(20.),
            },
            Shape::Line { start, .. } => Shape::Line {
                start,
                end: Point::new(20., 100.),
            },
            Shape::Custom(_) => unreachable!(),
        };
        let mut stale = m.clone();
        stale.live_shape = Some(edited.clone());
        let before = history(&s);
        // An inspector drag: three previews, then one final value.
        for _ in 0..3 {
            s.set_shape_layer(id, json(&stale), IDENTITY, true).unwrap();
        }
        assert_eq!(history(&s), before, "previews record nothing");
        assert_eq!(
            model(&s, id).path,
            edited.path().unwrap(),
            "preview is live"
        );
        s.set_shape_layer(id, json(&stale), IDENTITY, false)
            .unwrap();
        assert_eq!(history(&s), before + 1);
        assert_eq!(head_label(&s), "Edit Shape");
        assert_eq!(model(&s, id).path, edited.path().unwrap());
        assert_eq!(model(&s, id).live_shape, Some(edited));
        // Setting the same source again records nothing.
        s.set_shape_layer(id, json(&stale), IDENTITY, false)
            .unwrap();
        assert_eq!(history(&s), before + 1);
    }
    // The public primitive helper matches the engine's regeneration.
    let star = Shape::Polygon {
        center: Point::new(0., 0.),
        radius: 10.,
        sides: 5,
        rotation: 0.,
        inner_radius: Some(4.),
    };
    let p: Path = serde_json::from_str(&shape_primitive_path(json(&star)).unwrap()).unwrap();
    assert_eq!(p, star.path().unwrap());
    assert_eq!(p.subpaths[0].anchors.len(), 10);
}

#[test]
fn custom_path_edits_clear_live_shape_and_persist_natively() {
    let (d, e) = engine();
    let s = doc(&e);
    let id = add(&s, &rect_model(10., 10., 50., 50., [0.; 4]), IDENTITY);
    // Square corners: coincident generated anchors merge to four corners.
    s.edit_shape_path(
        id,
        r#"{"op":"move_anchor","subpath":0,"anchor":2,"x":70,"y":80}"#.into(),
        false,
    )
    .unwrap();
    assert_eq!(head_label(&s), "Move Anchor Point");
    let m = model(&s, id);
    assert_eq!(m.live_shape, None, "custom edits drop the primitive");
    assert_eq!(s.shape_layer(id).unwrap().live_kind, None);
    assert_eq!(m.path.subpaths[0].anchors.len(), 4);
    assert_eq!(m.path.subpaths[0].anchors[2].point, Point::new(70., 80.));
    // A later model edit (fill colour) cannot regenerate over the custom path.
    let mut recolored = m.clone();
    recolored.fill = Some(Fill::Solid(BLUE));
    s.set_shape_layer(id, json(&recolored), IDENTITY, false)
        .unwrap();
    assert_eq!(model(&s, id).path, m.path);
    // Native save / reopen keeps the custom path and paint exactly.
    let path = d.path().join("custom.tessera-doc");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let again = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let row = again
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Shape)
        .unwrap();
    assert_eq!(model(&again, row.id), recolored);
}

#[test]
fn inverse_affine_hits_track_skew_and_translation() {
    let (_d, e) = engine();
    let s = doc(&e);
    let t = TransformMatrix {
        a: 1.5,
        b: 0.6,
        c: 40.0,
        d: -0.2,
        e: 1.2,
        f: 30.0,
    };
    let id = add(&s, &rect_model(0., 0., 40., 30., [0.; 4]), t);
    let map = |x: f64, y: f64| (t.a * x + t.b * y + t.c, t.d * x + t.e * y + t.f);
    // Inside local points map to hits; the local coordinates come back.
    for (lx, ly) in [(1., 1.), (20., 15.), (39., 29.)] {
        let (x, y) = map(lx, ly);
        let h = s.shape_hit_test(x, y, false, 0.0).unwrap().unwrap();
        assert_eq!((h.layer, h.part), (id, ShapeHitPart::Fill));
        assert!((h.local_x - lx).abs() < 1e-9 && (h.local_y - ly).abs() < 1e-9);
    }
    // Points inside the untransformed rectangle but outside the image miss.
    for (lx, ly) in [(-2., 15.), (42., 15.), (20., -2.), (20., 32.)] {
        let (x, y) = map(lx, ly);
        assert_eq!(hit(&s, x, y, true), None, "local ({lx},{ly})");
    }
    assert_eq!(hit(&s, 5., 5., true), None);
    assert_eq!(
        s.shape_layer_hit_test(id, map(20., 15.).0, map(20., 15.).1, false, 0.0)
            .unwrap(),
        Some(ShapeHitPart::Fill)
    );
    // Bounds are the transformed geometry.
    let b = s.shape_layer(id).unwrap().bounds.unwrap();
    let corners = [map(0., 0.), map(40., 0.), map(40., 30.), map(0., 30.)];
    let min_x = corners.iter().map(|c| c.0).fold(f64::MAX, f64::min);
    let max_y = corners.iter().map(|c| c.1).fold(f64::MIN, f64::max);
    assert!((b.x0 - min_x).abs() < 1e-6 && (b.y1 - max_y).abs() < 1e-6);
    // Hidden layers do not hit; non-finite input is an error.
    s.set_visible(id, false).unwrap();
    let (x, y) = map(20., 15.);
    assert_eq!(hit(&s, x, y, true), None);
    assert!(s.shape_hit_test(f64::NAN, 0., true, 0.).is_err());
}

#[test]
fn dash_only_stroke_hits_follow_the_dashes() {
    let (_d, e) = engine();
    let s = doc(&e);
    let stroke = Stroke {
        width: 6.0,
        dashes: vec![10.0, 10.0],
        cap: LineCap::Butt,
        ..Stroke::default()
    };
    let m = ShapeModel::from_shape(
        Shape::Line {
            start: Point::new(0., 0.),
            end: Point::new(100., 0.),
        },
        None,
        Some((stroke, Fill::Solid(BLUE))),
    )
    .unwrap();
    let id = add(&s, &m, translate(20., 50.));
    assert_eq!(head_label(&s), "Line Tool");
    // On a dash (local x 5), in a gap (local x 15), off the line.
    assert_eq!(hit(&s, 25., 50., true), Some((id, ShapeHitPart::Stroke)));
    assert_eq!(hit(&s, 25., 52., true), Some((id, ShapeHitPart::Stroke)));
    assert_eq!(hit(&s, 35., 50., true), None, "gaps are not the stroke");
    assert_eq!(hit(&s, 25., 60., true), None);
    // Without stroke hits a stroke-only line is not hit at all.
    assert_eq!(hit(&s, 25., 50., false), None);
    // Tolerance (document pixels) reaches a dash from just outside it.
    let near = s.shape_hit_test(25., 55., true, 3.0).unwrap().unwrap();
    assert_eq!(near.part, ShapeHitPart::Stroke);
    // Rendered: dash painted, gap clear.
    s.wait_idle();
    assert!(px(&s, 25, 50)[3] > 0.99);
    assert!(px(&s, 35, 50)[3] < 0.01);
}

#[test]
fn even_odd_holes_hit_and_render_as_holes() {
    let (_d, e) = engine();
    let s = doc(&e);
    let path = Path {
        subpaths: vec![square(10., 10., 90., 90.), square(30., 30., 70., 70.)],
        fill_rule: FillRule::EvenOdd,
    };
    let m =
        ShapeModel::from_shape(Shape::Custom(path.clone()), Some(Fill::Solid(RED)), None).unwrap();
    let id = add(&s, &m, IDENTITY);
    assert_eq!(head_label(&s), "Pen");
    assert_eq!(hit(&s, 20., 20., false), Some((id, ShapeHitPart::Fill)));
    assert_eq!(hit(&s, 50., 50., false), None, "even-odd hole");
    s.wait_idle();
    assert_eq!(px(&s, 20, 20), RED);
    assert_eq!(px(&s, 50, 50)[3], 0.0);
    // Nonzero with the same orientation fills the hole.
    s.edit_shape_path(
        id,
        r#"{"op":"set_fill_rule","rule":"NonZero"}"#.into(),
        false,
    )
    .unwrap();
    assert_eq!(hit(&s, 50., 50., false), Some((id, ShapeHitPart::Fill)));
    s.wait_idle();
    assert_eq!(px(&s, 50, 50), RED);
}

#[test]
fn anchor_splitting_handles_and_deletion() {
    let (_d, e) = engine();
    let s = doc(&e);
    let path = Path {
        subpaths: vec![Subpath {
            anchors: vec![
                Anchor {
                    point: Point::new(10., 50.),
                    incoming: Point::new(10., 50.),
                    outgoing: Point::new(30., 10.),
                },
                Anchor {
                    point: Point::new(90., 50.),
                    incoming: Point::new(70., 10.),
                    outgoing: Point::new(90., 50.),
                },
            ],
            closed: false,
        }],
        fill_rule: FillRule::NonZero,
    };
    let m = ShapeModel::from_shape(
        Shape::Custom(path.clone()),
        None,
        Some((Stroke::default(), Fill::Solid(BLUE))),
    )
    .unwrap();
    let id = add(&s, &m, IDENTITY);
    // Exact de Casteljau split at t = 0.5 keeps the curve.
    s.edit_shape_path(
        id,
        r#"{"op":"insert_anchor","subpath":0,"segment":0,"t":0.5}"#.into(),
        false,
    )
    .unwrap();
    assert_eq!(head_label(&s), "Add Anchor Point");
    let a = model(&s, id).path.subpaths[0].anchors.clone();
    assert_eq!(a.len(), 3);
    assert_eq!(a[1].point, Point::new(50., 20.));
    assert_eq!(a[0].outgoing, Point::new(20., 30.));
    assert_eq!(a[2].incoming, Point::new(80., 30.));
    // Mirrored handle drag: the opposite handle stays symmetric.
    s.edit_shape_path(
        id,
        r#"{"op":"set_handle","subpath":0,"anchor":1,"handle":"outgoing","x":60,"y":10,"mirror":true}"#
            .into(),
        false,
    )
    .unwrap();
    let a = model(&s, id).path.subpaths[0].anchors.clone();
    assert_eq!(a[1].outgoing, Point::new(60., 10.));
    assert_eq!(a[1].incoming, Point::new(40., 30.));
    // Independent handle drag (⌥): the other handle stays.
    s.edit_shape_path(
        id,
        r#"{"op":"set_handle","subpath":0,"anchor":1,"handle":"incoming","x":35,"y":20,"mirror":false}"#
            .into(),
        false,
    )
    .unwrap();
    let a = model(&s, id).path.subpaths[0].anchors.clone();
    assert_eq!(
        (a[1].incoming, a[1].outgoing),
        (Point::new(35., 20.), Point::new(60., 10.))
    );
    // An anchor drag moves its handles with it; commands batch.
    let before = history(&s);
    s.edit_shape_path(
        id,
        r#"[{"op":"move_anchor","subpath":0,"anchor":1,"x":55,"y":25},{"op":"set_closed","subpath":0,"closed":true}]"#
            .into(),
        false,
    )
    .unwrap();
    assert_eq!(history(&s), before + 1, "a command array is one node");
    let p = model(&s, id).path;
    assert!(p.subpaths[0].closed);
    assert_eq!(p.subpaths[0].anchors[1].incoming, Point::new(40., 25.));
    // Delete the middle anchor; the last anchor cannot be deleted.
    s.edit_shape_path(
        id,
        r#"{"op":"delete_anchor","subpath":0,"anchor":1}"#.into(),
        false,
    )
    .unwrap();
    assert_eq!(head_label(&s), "Delete Anchor Point");
    assert_eq!(model(&s, id).path.subpaths[0].anchors.len(), 2);
    s.edit_shape_path(
        id,
        r#"{"op":"delete_anchor","subpath":0,"anchor":0}"#.into(),
        false,
    )
    .unwrap();
    let before = history(&s);
    let err = s
        .edit_shape_path(
            id,
            r#"{"op":"delete_anchor","subpath":0,"anchor":0}"#.into(),
            false,
        )
        .unwrap_err();
    assert!(err.to_string().contains("last anchor"), "{err}");
    // Bad indexes / parameters fail without a node.
    for bad in [
        r#"{"op":"move_anchor","subpath":3,"anchor":0,"x":1,"y":1}"#,
        r#"{"op":"insert_anchor","subpath":0,"segment":0,"t":1.0}"#,
        r#"{"op":"spin","subpath":0}"#,
        r#"[]"#,
    ] {
        assert!(s.edit_shape_path(id, bad.into(), false).is_err(), "{bad}");
    }
    assert_eq!(history(&s), before);
}

#[test]
fn boolean_operations_make_holes_in_one_undo_step() {
    let (_d, e) = engine();
    let s = doc(&e);
    let cases = [
        (
            ShapePathOperation::Combine,
            "Combine Shapes",
            [true, true, true],
        ),
        (
            ShapePathOperation::Subtract,
            "Subtract Front Shape",
            [true, false, false],
        ),
        (
            ShapePathOperation::Intersect,
            "Intersect Shape Areas",
            [false, true, false],
        ),
        (
            ShapePathOperation::Exclude,
            "Exclude Overlapping Shapes",
            [true, false, true],
        ),
    ];
    for (op, label, expect) in cases {
        // Target 10…60 × 10…60; operand 40…90 (a translated local 0…50 square).
        let a = add(&s, &rect_model(10., 10., 60., 60., [0.; 4]), IDENTITY);
        let b = add(
            &s,
            &rect_model(0., 0., 50., 50., [0.; 4]),
            translate(40., 10.),
        );
        let layers = s.layers().unwrap().len();
        let before = history(&s);
        s.boolean_shape_paths(a, vec![b], op).unwrap();
        assert_eq!(history(&s), before + 1, "{label}: one node");
        assert_eq!(head_label(&s), label);
        assert_eq!(s.layers().unwrap().len(), layers - 1, "operand removed");
        assert!(s.layer(b).is_err());
        assert_eq!(model(&s, a).live_shape, None);
        // Only-a, overlap, only-b.
        for ((x, y), want) in [(20., 30.), (50., 30.), (80., 30.)].into_iter().zip(expect) {
            assert_eq!(hit(&s, x, y, false).is_some(), want, "{label} at {x}");
        }
        s.undo().unwrap();
        assert!(s.layer(b).is_ok(), "undo restores the operand");
        assert!(model(&s, a).live_kind_is_rect());
        s.redo().unwrap();
        s.remove_layer(a).unwrap();
    }
    // Operands must be distinct shape layers.
    let a = add(&s, &rect_model(0., 0., 5., 5., [0.; 4]), IDENTITY);
    let pixel = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Pixel)
        .unwrap()
        .id;
    assert!(
        s.boolean_shape_paths(a, vec![a], ShapePathOperation::Combine)
            .is_err()
    );
    assert!(
        s.boolean_shape_paths(a, vec![pixel], ShapePathOperation::Combine)
            .is_err()
    );
    assert!(
        s.boolean_shape_paths(a, vec![], ShapePathOperation::Combine)
            .is_err()
    );
    // A hole from Subtract with an inner operand.
    let outer = add(&s, &rect_model(100., 20., 180., 100., [0.; 4]), IDENTITY);
    let inner = add(&s, &rect_model(120., 40., 160., 80., [0.; 4]), IDENTITY);
    s.boolean_shape_paths(outer, vec![inner], ShapePathOperation::Subtract)
        .unwrap();
    assert!(hit(&s, 110., 30., false).is_some());
    assert_eq!(hit(&s, 140., 60., false), None, "subtracted hole");
    s.wait_idle();
    assert_eq!(px(&s, 140, 60)[3], 0.0);
    assert_eq!(px(&s, 110, 30), RED);
}

trait LiveRect {
    fn live_kind_is_rect(&self) -> bool;
}
impl LiveRect for ShapeModel {
    fn live_kind_is_rect(&self) -> bool {
        matches!(self.live_shape, Some(Shape::Rectangle { .. }))
    }
}

#[test]
fn bounds_and_thumbnail_revisions_follow_the_source() {
    let (_d, e) = engine();
    let s = doc(&e);
    let mut m = rect_model(10., 20., 50., 60., [0.; 4]);
    m.stroke = Some((
        Stroke {
            width: 4.0,
            ..Stroke::default()
        },
        Fill::Solid(BLUE),
    ));
    let id = add(&s, &m, translate(5., 0.));
    let rec = s.shape_layer(id).unwrap();
    let b = rec.bounds.unwrap();
    // Centre stroke adds half its width outside the path (miter corners).
    assert!(
        (b.x0 - 13.0).abs() < 0.05 && (b.y0 - 18.0).abs() < 0.05,
        "{b:?}"
    );
    assert!(
        (b.x1 - 57.0).abs() < 0.05 && (b.y1 - 62.0).abs() < 0.05,
        "{b:?}"
    );
    let rev = rec.revision;
    assert_eq!(s.layer(id).unwrap().revision, rev);
    // Thumbnails are cached by revision.
    s.layer_thumbnail(id, 64).unwrap();
    let renders = s.thumbnail_renders();
    s.layer_thumbnail(id, 64).unwrap();
    assert_eq!(s.thumbnail_renders(), renders, "cached");
    // Properties do not change the thumbnail revision; content does.
    s.set_opacity(id, 0.5, false).unwrap();
    assert_eq!(s.layer(id).unwrap().revision, rev);
    s.layer_thumbnail(id, 64).unwrap();
    assert_eq!(
        s.thumbnail_renders(),
        renders,
        "opacity keeps the thumbnail"
    );
    s.set_shape_layer(id, json(&m), translate(9., 0.), false)
        .unwrap();
    assert_eq!(head_label(&s), "Transform Shape");
    let rev2 = s.layer(id).unwrap().revision;
    assert!(rev2 > rev);
    s.set_vector_mask(id, Some(mask_rect(0., 0., 30., 30.)), false)
        .unwrap();
    let rev3 = s.layer(id).unwrap().revision;
    assert!(rev3 > rev2, "a vector mask changes the thumbnail");
    assert_eq!(s.shape_layer(id).unwrap().revision, rev3);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(
        s.layer(id).unwrap().revision,
        rev,
        "undo restores revisions"
    );
    s.redo().unwrap();
    s.layer_thumbnail(id, 64).unwrap();
    assert!(s.thumbnail_renders() > renders, "content edits re-render");
}

#[test]
fn vector_mask_coexists_with_raster_mask() {
    let (_d, e) = engine();
    let s = doc(&e);
    let id = add(
        &s,
        &rect_model(0., 0., W as f64, H as f64, [0.; 4]),
        IDENTITY,
    );
    s.add_mask(id, MaskInit::RevealAll).unwrap();
    let before = history(&s);
    s.set_vector_mask(id, Some(mask_rect(20., 20., 100., 100.)), false)
        .unwrap();
    assert_eq!(history(&s), before + 1);
    assert_eq!(head_label(&s), "Add Vector Mask");
    let row = s.layer(id).unwrap();
    assert!(row.has_mask, "the raster mask is still there");
    assert!(s.vector_mask(id).unwrap().is_some());
    s.wait_idle();
    assert_eq!(px(&s, 50, 50), RED);
    assert_eq!(px(&s, 150, 50)[3], 0.0, "outside the vector mask");
    // Both masks multiply: a hide-all raster mask hides the inside too.
    s.remove_mask(id).unwrap();
    s.add_mask(id, MaskInit::HideAll).unwrap();
    assert!(
        s.vector_mask(id).unwrap().is_some(),
        "raster edits keep the vector mask"
    );
    s.wait_idle();
    assert_eq!(px(&s, 50, 50)[3], 0.0);
    // Vector masks work on pixel layers too, and deletion is one node.
    let pixel = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Pixel)
        .unwrap()
        .id;
    s.set_vector_mask(pixel, Some(mask_rect(0., 0., 10., 10.)), false)
        .unwrap();
    s.set_vector_mask(pixel, None, false).unwrap();
    assert_eq!(head_label(&s), "Delete Vector Mask");
    assert_eq!(s.vector_mask(pixel).unwrap(), None);
}

#[test]
fn mask_density_feather_and_disabled_states() {
    let (_d, e) = engine();
    let s = doc(&e);
    let id = add(
        &s,
        &rect_model(0., 0., W as f64, H as f64, [0.; 4]),
        IDENTITY,
    );
    let mut m = mask_rect(50., 0., 150., H as f64);
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    s.wait_idle();
    assert_eq!(px(&s, 20, 50)[3], 0.0);
    // Density endpoints: 0 ignores the mask, 1 hides outside completely.
    let before = history(&s);
    for d in [0.2, 0.4, 0.0] {
        m.density = d;
        s.set_vector_mask(id, Some(m.clone()), true).unwrap();
    }
    assert_eq!(history(&s), before, "density drag previews only");
    s.wait_idle();
    assert!((px(&s, 20, 50)[3] - 1.0).abs() < 1e-3);
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    assert_eq!(history(&s), before + 1);
    assert_eq!(head_label(&s), "Vector Mask Density");
    m.density = 0.5;
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    s.wait_idle();
    assert!(
        (px(&s, 20, 50)[3] - 0.5).abs() < 0.01,
        "{:?}",
        px(&s, 20, 50)
    );
    assert_eq!(px(&s, 100, 50)[3], 1.0);
    // Feather softens the edge symmetrically.
    m.density = 1.0;
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    m.feather = 8.0;
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    assert_eq!(head_label(&s), "Vector Mask Feather");
    s.wait_idle();
    let edge = px(&s, 50, 50)[3];
    assert!((edge - 0.5).abs() < 0.06, "edge {edge}");
    assert!(px(&s, 44, 50)[3] > 0.05 && px(&s, 44, 50)[3] < 0.5);
    assert!(px(&s, 30, 50)[3] < 0.01);
    // Disabled: ignored.
    m.enabled = false;
    s.set_vector_mask(id, Some(m.clone()), false).unwrap();
    assert_eq!(head_label(&s), "Disable Vector Mask");
    s.wait_idle();
    assert_eq!(px(&s, 20, 50)[3], 1.0);
    // Invalid values fail with no node.
    let before = history(&s);
    for bad in [
        VectorMaskRecord {
            density: 1.5,
            ..m.clone()
        },
        VectorMaskRecord {
            feather: -1.0,
            ..m.clone()
        },
        VectorMaskRecord {
            feather: f32::NAN,
            ..m.clone()
        },
        VectorMaskRecord {
            path_json: "{\"subpaths\":7}".into(),
            ..m.clone()
        },
    ] {
        assert!(s.set_vector_mask(id, Some(bad), false).is_err());
    }
    assert_eq!(history(&s), before);
}

#[test]
fn masks_stay_fixed_on_move_but_move_with_the_linked_batch() {
    let (_d, e) = engine();
    let s = doc(&e);
    let m = rect_model(0., 0., 40., 40., [0.; 4]);
    let id = add(&s, &m, translate(10., 10.));
    let mask = mask_rect(0., 0., 60., 60.);
    s.set_vector_mask(id, Some(mask.clone()), false).unwrap();
    // Plain move: the document-space mask stays.
    s.set_shape_layer(id, json(&m), translate(40., 10.), false)
        .unwrap();
    assert_eq!(s.vector_mask(id).unwrap().unwrap(), mask);
    s.wait_idle();
    assert_eq!(px(&s, 50, 20), RED);
    assert_eq!(px(&s, 70, 20)[3], 0.0, "clipped by the fixed mask");
    // Linked gesture: a drag previews, then ONE node moves both.
    let before = history(&s);
    for x in [50., 60., 70.] {
        s.transform_shape_with_mask(id, translate(x, 10.), true)
            .unwrap();
    }
    assert_eq!(history(&s), before);
    s.transform_shape_with_mask(id, translate(70., 10.), false)
        .unwrap();
    assert_eq!(history(&s), before + 1);
    assert_eq!(head_label(&s), "Transform Shape and Vector Mask");
    let moved: Path = serde_json::from_str(&s.vector_mask(id).unwrap().unwrap().path_json).unwrap();
    let want = Path {
        subpaths: vec![square(30., 0., 90., 60.)],
        fill_rule: FillRule::NonZero,
    };
    assert_eq!(moved.bounds(), want.bounds());
    assert_eq!(s.shape_layer(id).unwrap().transform, translate(70., 10.));
    s.wait_idle();
    assert_eq!(px(&s, 85, 20), RED);
    // One undo restores both.
    s.undo().unwrap();
    assert_eq!(s.shape_layer(id).unwrap().transform, translate(40., 10.));
    assert_eq!(s.vector_mask(id).unwrap().unwrap(), mask);
}

#[test]
fn drafts_commit_once_and_cancel_leaves_history_unchanged() {
    let (_d, e) = engine();
    let s = doc(&e);
    let m = rect_model(0., 0., 40., 40., [0.; 4]);
    let id = add(&s, &m, IDENTITY);
    let head = s.info().unwrap().history_head;
    let n = history(&s);
    // Affine handle drag then Esc.
    for x in [5., 10., 15.] {
        s.set_shape_layer(id, json(&m), translate(x, 0.), true)
            .unwrap();
    }
    assert_eq!(s.shape_layer(id).unwrap().transform, translate(15., 0.));
    s.cancel_shape_preview().unwrap();
    assert_eq!(s.info().unwrap().history_head, head);
    assert_eq!(history(&s), n);
    assert_eq!(s.shape_layer(id).unwrap().transform, IDENTITY);
    // Direct-selection drag then Esc.
    s.edit_shape_path(
        id,
        r#"{"op":"move_anchor","subpath":0,"anchor":0,"x":-5,"y":-5}"#.into(),
        true,
    )
    .unwrap();
    assert_eq!(s.shape_layer(id).unwrap().live_kind, None);
    s.cancel_shape_preview().unwrap();
    assert_eq!(
        s.shape_layer(id).unwrap().live_kind.as_deref(),
        Some("rectangle")
    );
    assert_eq!(history(&s), n);
    // A draft equal to the base drops itself; the final call records once.
    s.set_shape_layer(id, json(&m), translate(3., 0.), true)
        .unwrap();
    s.set_shape_layer(id, json(&m), IDENTITY, true).unwrap();
    s.set_shape_layer(id, json(&m), IDENTITY, false).unwrap();
    assert_eq!(history(&s), n, "net no-op records nothing");
    // Cancel leaves a pending opacity drag of another layer alone.
    let pixel = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Pixel)
        .unwrap()
        .id;
    s.set_opacity(pixel, 0.25, true).unwrap();
    s.cancel_shape_preview().unwrap();
    assert_eq!(s.layer(pixel).unwrap().opacity, 0.25);
    s.commit("Opacity".into()).unwrap();
    assert_eq!(history(&s), n + 1);
    // A shape draft commits a pending drag of another control first.
    s.set_opacity(pixel, 0.5, true).unwrap();
    s.set_shape_layer(id, json(&m), translate(8., 0.), true)
        .unwrap();
    assert_eq!(history(&s), n + 2, "the opacity drag became its own node");
    // Undo flushes the shape draft as its own node first.
    s.undo().unwrap();
    assert_eq!(s.shape_layer(id).unwrap().transform, IDENTITY);
    s.redo().unwrap();
    assert_eq!(s.shape_layer(id).unwrap().transform, translate(8., 0.));
}

#[test]
fn invalid_inputs_and_locks_fail_without_history() {
    let (_d, e) = engine();
    let s = doc(&e);
    let m = rect_model(0., 0., 40., 40., [0.; 4]);
    let id = add(&s, &m, IDENTITY);
    let n = history(&s);
    let singular = TransformMatrix {
        a: 1.0,
        b: 2.0,
        c: 0.0,
        d: 2.0,
        e: 4.0,
        f: 0.0,
    };
    assert!(
        s.add_shape_layer(String::new(), None, None, json(&m), singular)
            .is_err()
    );
    assert!(s.set_shape_layer(id, json(&m), singular, false).is_err());
    assert!(
        s.set_shape_layer(id, "{\"path\":1}".into(), IDENTITY, false)
            .is_err()
    );
    let nan = TransformMatrix {
        c: f64::NAN,
        ..IDENTITY
    };
    assert!(s.set_shape_layer(id, json(&m), nan, false).is_err());
    // Inside / Outside strokes on an open path: a clear error.
    let line = ShapeModel {
        stroke: Some((
            Stroke {
                alignment: Alignment::Inside,
                ..Stroke::default()
            },
            Fill::Solid(BLUE),
        )),
        ..ShapeModel::from_shape(
            Shape::Line {
                start: Point::new(0., 0.),
                end: Point::new(10., 0.),
            },
            None,
            None,
        )
        .unwrap()
    };
    let err = s
        .add_shape_layer(String::new(), None, None, json(&line), IDENTITY)
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("closed path") && err.contains("Center"),
        "{err}"
    );
    // Bad paint.
    let mut bad = m.clone();
    bad.fill = Some(Fill::Solid([2.0, 0.0, 0.0, 1.0]));
    assert!(s.set_shape_layer(id, json(&bad), IDENTITY, false).is_err());
    assert_eq!(history(&s), n);
    // Pixel lock: content edits fail; the position lock only blocks moves.
    s.set_locks(
        id,
        LayerLocks {
            pixels: true,
            ..Default::default()
        },
    )
    .unwrap();
    let n = history(&s);
    let mut blue = m.clone();
    blue.fill = Some(Fill::Solid(BLUE));
    assert!(s.set_shape_layer(id, json(&blue), IDENTITY, false).is_err());
    assert!(
        s.edit_shape_path(
            id,
            r#"{"op":"move_anchor","subpath":0,"anchor":0,"x":1,"y":1}"#.into(),
            false
        )
        .is_err()
    );
    assert!(s.convert_shape_to_pixels(id).is_err());
    assert!(
        s.set_vector_mask(id, Some(mask_rect(0., 0., 5., 5.)), false)
            .is_err()
    );
    assert_eq!(history(&s), n);
    s.set_locks(
        id,
        LayerLocks {
            position: true,
            ..Default::default()
        },
    )
    .unwrap();
    let n = history(&s);
    let err = s
        .set_shape_layer(id, json(&m), translate(5., 5.), false)
        .unwrap_err();
    assert!(err.to_string().contains("position is locked"), "{err}");
    assert!(
        s.transform_shape_with_mask(id, translate(5., 5.), false)
            .is_err()
    );
    assert_eq!(history(&s), n);
    s.set_shape_layer(id, json(&blue), IDENTITY, false).unwrap();
    assert_eq!(
        history(&s),
        n + 1,
        "local source edits pass the position lock"
    );
    // A failing final call keeps the committed source.
    assert_eq!(model(&s, id).fill, Some(Fill::Solid(BLUE)));
}

#[test]
fn conversion_to_pixels_is_exact_and_undo_restores_the_source() {
    let (_d, e) = engine();
    let s = doc(&e);
    let mut m = rect_model(20., 20., 120., 100., [10.; 4]);
    m.stroke = Some((
        Stroke {
            width: 6.0,
            ..Stroke::default()
        },
        Fill::Solid(BLUE),
    ));
    let id = add(&s, &m, translate(10., 5.));
    s.add_mask(id, MaskInit::RevealAll).unwrap();
    s.set_vector_mask(id, Some(mask_rect(0., 0., 90., H as f64)), false)
        .unwrap();
    s.set_opacity(id, 0.8, false).unwrap();
    s.wait_idle();
    let before = s.read_level(0).unwrap().2;
    let source = model(&s, id);
    let n = history(&s);
    s.convert_shape_to_pixels(id).unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Rasterize Shape");
    let row = s.layer(id).unwrap();
    assert_eq!(row.kind, DocLayerKind::Pixel, "same id, now pixels");
    assert!(row.has_mask);
    assert!((row.opacity - 0.8).abs() < 1e-6);
    assert!(
        s.vector_mask(id).unwrap().is_some(),
        "the vector mask is kept"
    );
    s.wait_idle();
    let after = s.read_level(0).unwrap().2;
    let worst = before
        .iter()
        .zip(&after)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(
        worst <= 1.0 / 255.0 + 1e-6,
        "masks / opacity apply once: {worst}"
    );
    s.undo().unwrap();
    assert_eq!(s.layer(id).unwrap().kind, DocLayerKind::Shape);
    assert_eq!(model(&s, id), source);
    assert_eq!(s.shape_layer(id).unwrap().transform, translate(10., 5.));
}

fn gradient_model() -> ShapeModel {
    let g = Gradient::new(
        GradientKind::Linear,
        Point::new(0., 0.),
        Point::new(W as f64, 0.),
        vec![
            Stop {
                position: 0.,
                color: RED,
            },
            Stop {
                position: 1.,
                color: BLUE,
            },
        ],
        false,
    )
    .unwrap();
    ShapeModel::from_shape(
        Shape::Rectangle {
            rect: Rect::new(10., 10., 110., 70.),
            radii: [6., 0., 6., 0.],
        },
        Some(Fill::Gradient(g)),
        Some((
            Stroke {
                width: 4.0,
                dashes: vec![8.0, 4.0],
                ..Stroke::default()
            },
            Fill::Solid(BLUE),
        )),
    )
    .unwrap()
}

#[test]
fn native_and_psd_reopen_keep_editable_shapes_and_masks() {
    let (d, e) = engine();
    let s = doc(&e);
    let m = gradient_model();
    let t = translate(12., 8.);
    let id = add(&s, &m, t);
    let mask = mask_rect(0., 0., 100., H as f64);
    s.set_vector_mask(id, Some(mask.clone()), false).unwrap();
    // Paint stays document-anchored when the shape moves.
    s.wait_idle();
    let paint_before = px(&s, 60, 40);
    s.set_shape_layer(id, json(&m), translate(14., 8.), false)
        .unwrap();
    s.wait_idle();
    let paint_after = px(&s, 60, 40);
    assert!(
        paint_before
            .iter()
            .zip(paint_after)
            .all(|(a, b)| (a - b).abs() < 2.0 / 255.0),
        "gradient sampled in document space: {paint_before:?} {paint_after:?}"
    );
    let notes = s.shape_layer(id).unwrap().notes;
    assert!(notes.iter().any(|n| n.contains("tvMk")), "{notes:?}");
    s.wait_idle();
    let pixels = s.read_level(0).unwrap().2;
    for ext in ["tessera-doc", "psd"] {
        let path = d.path().join(format!("shapes.{ext}"));
        s.save_as(path.to_string_lossy().into_owned()).unwrap();
        let again = e
            .clone()
            .open_document(path.to_string_lossy().into_owned())
            .unwrap();
        // save_as keeps this session registered for the path: reopen from
        // a fresh engine to read the file itself.
        let (_d2, e2) = engine();
        let fresh = e2
            .clone()
            .open_document(path.to_string_lossy().into_owned())
            .unwrap();
        assert!(Arc::ptr_eq(&again, &s));
        let row = fresh
            .layers()
            .unwrap()
            .into_iter()
            .find(|r| r.kind == DocLayerKind::Shape)
            .unwrap_or_else(|| panic!("{ext}: shape row"));
        let rec = fresh.shape_layer(row.id).unwrap();
        assert_eq!(model(&fresh, row.id), m, "{ext}: exact model");
        assert_eq!(rec.live_kind.as_deref(), Some("rectangle"));
        assert_eq!(rec.transform, translate(14., 8.), "{ext}");
        assert_eq!(
            fresh.vector_mask(row.id).unwrap(),
            Some(mask.clone()),
            "{ext}"
        );
        assert!(!row.has_mask, "{ext}: the bridge restores no raster mask");
        fresh.wait_idle();
        let reopened = fresh.read_level(0).unwrap().2;
        let worst = pixels
            .iter()
            .zip(&reopened)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(worst <= 2.0 / 255.0, "{ext}: pixels {worst}");
        fresh.close();
    }
}

#[test]
fn psd_standard_tags_cached_pixels_and_raster_mask_fallback() {
    let (d, e) = engine();
    let s = doc(&e);
    let m = rect_model(20., 20., 120., 100., [0.; 4]);
    let id = add(&s, &m, IDENTITY);
    s.set_vector_mask(id, Some(mask_rect(0., 0., 70., H as f64)), false)
        .unwrap();
    let path = d.path().join("standard.psd");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    let psd = psd::PsdDocument::read(&bytes).unwrap();
    let rec = psd
        .layer_section
        .layers
        .iter()
        .find(|l| l.info(b"tvSh").is_some())
        .expect("shape record");
    // Standard Adobe shape records next to the private supplements.
    for key in [b"vmsk", b"SoCo", b"vogk", b"tvMk"] {
        assert!(rec.info(key).is_some(), "{}", String::from_utf8_lossy(key));
    }
    // The extra vector mask is a standard raster user mask (-2) for other apps.
    let user = rec.channels.iter().find(|c| c.id == -2).expect("user mask");
    assert!(!user.data.is_empty());
    // Cached pixels exist for apps that do not render shapes.
    assert!(rec.channels.iter().any(|c| c.id == 0 && !c.data.is_empty()));
    // External fallback 1: without tvMk the combined raster mask is what opens.
    let mut stripped = psd.clone();
    for l in &mut stripped.layer_section.layers {
        l.additional.retain(|b| b.key != *b"tvMk");
    }
    let fallback = d.path().join("fallback.psd");
    std::fs::write(&fallback, stripped.write().unwrap()).unwrap();
    let (_d2, e2) = engine();
    let f = e2
        .clone()
        .open_document(fallback.to_string_lossy().into_owned())
        .unwrap();
    let row = f
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Shape)
        .unwrap();
    assert!(row.has_mask, "combined raster mask");
    assert_eq!(f.vector_mask(row.id).unwrap(), None);
    f.wait_idle();
    assert!(px(&f, 40, 50)[3] > 0.99);
    assert!(
        px(&f, 100, 50)[3] < 0.01,
        "the combined raster mask still clips"
    );
    f.close();
    // External fallback 2: a raster-mask edit in another app (one changed
    // sample of the combined plane) defers to that edit: tvMk is stale.
    let mut edited = psd.clone();
    for l in &mut edited.layer_section.layers {
        if l.info(b"tvMk").is_some() {
            let c = l.channels.iter_mut().find(|c| c.id == -2).unwrap();
            c.data[0] ^= 0x40;
        }
    }
    let ext = d.path().join("external.psd");
    std::fs::write(&ext, edited.write().unwrap()).unwrap();
    let (_d3, e3) = engine();
    let x = e3
        .clone()
        .open_document(ext.to_string_lossy().into_owned())
        .unwrap();
    let row = x
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.kind == DocLayerKind::Shape)
        .unwrap();
    assert_eq!(x.vector_mask(row.id).unwrap(), None, "stale tvMk ignored");
    assert!(row.has_mask, "the externally edited raster mask is used");
    x.close();
    // Pattern fills: kept natively, explicitly flagged for PSD export.
    let pattern = vector::Pattern::new(2, 1, vec![RED, BLUE], vector::Affine::scale(4.0)).unwrap();
    let mut pm = m.clone();
    pm.fill = Some(Fill::Pattern(pattern));
    let p = add(&s, &pm, IDENTITY);
    let notes = s.shape_layer(p).unwrap().notes;
    assert!(
        notes
            .iter()
            .any(|n| n.contains("Pattern") && n.contains("does not support")),
        "{notes:?}"
    );
    assert_eq!(model(&s, p).fill, pm.fill, "pattern paint is preserved");
    // The stated limitation is real: PSD save refuses the pattern fill, native save keeps it.
    let err = s
        .save_as(d.path().join("pattern.psd").to_string_lossy().into_owned())
        .unwrap_err()
        .to_string();
    assert!(err.contains("pattern"), "{err}");
    s.save_as(
        d.path()
            .join("pattern.tessera-doc")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
}
