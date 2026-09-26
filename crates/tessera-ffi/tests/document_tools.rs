//! Layered-editor tools over the bridge (WP B5-04): strokes (brush,
//! eraser, mask, clone, heal, symmetry, sampled tips), every selection call,
//! outlines, channels, Free Transform, fill / clear and the eyedropper. The
//! ignored bench measures interactive dab latency on a sample.dng-sized
//! 16-bit document with a presented viewport.
#![cfg(target_os = "macos")]

use compositor::{LayerId, LayerKind};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tessera_ffi::surface::testing::create_rgba8;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn doc(engine: &Arc<Engine>, w: u32, h: u32) -> (Arc<DocumentSession>, u64) {
    let s = engine
        .clone()
        .new_document(w, h, DocDepth::U8, None)
        .unwrap();
    let id = s.layers().unwrap()[0].id;
    (s, id)
}

fn brush(size: f32) -> PaintBrush {
    PaintBrush {
        size,
        hardness: 1.0,
        opacity: 1.0,
        flow: 1.0,
        spacing: 0.1,
        angle: 0.0,
        roundness: 1.0,
        blend_mode: "normal".into(),
        pressure_size: false,
        pressure_opacity: false,
        pressure_flow: false,
        smoothing: 0.0,
        symmetry: PaintSymmetry::None,
        symmetry_x: 0.0,
        symmetry_y: 0.0,
        symmetry_count: 6,
        tip_id: None,
        sample_all_layers: false,
    }
}

const RED: PaintColor = PaintColor {
    r: 1.0,
    g: 0.0,
    b: 0.0,
};
const BLUE: PaintColor = PaintColor {
    r: 0.0,
    g: 0.0,
    b: 1.0,
};
const BLACK: PaintColor = PaintColor {
    r: 0.0,
    g: 0.0,
    b: 0.0,
};

fn sample(x: f32, y: f32) -> StrokeSample {
    StrokeSample {
        x,
        y,
        pressure: 1.0,
        tilt_x: 0.0,
        tilt_y: 0.0,
        timestamp: 0.0,
    }
}

fn line(x0: f32, y0: f32, x1: f32, y1: f32, n: usize) -> Vec<StrokeSample> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            sample(x0 + (x1 - x0) * t, y0 + (y1 - y0) * t)
        })
        .collect()
}

/// Straight RGBA of a layer's pixel in the live state.
fn px(s: &DocumentSession, layer: u64, x: u32, y: u32) -> [f32; 4] {
    let st = s.document_state().unwrap();
    let l = st.find(LayerId(layer)).unwrap();
    match &l.kind {
        LayerKind::Pixel(r) => r.pixel(x, y),
        _ => panic!("not a pixel layer"),
    }
}

fn mask_px(s: &DocumentSession, layer: u64, x: u32, y: u32) -> f32 {
    let st = s.document_state().unwrap();
    st.find(LayerId(layer))
        .unwrap()
        .mask
        .as_ref()
        .expect("mask")
        .raster
        .pixel(x, y)[0]
}

fn sel_px(s: &DocumentSession, x: u32, y: u32) -> f32 {
    s.document_state()
        .unwrap()
        .selection
        .as_ref()
        .map_or(0.0, |r| r.pixel(x, y)[0])
}

fn bounds(s: &DocumentSession) -> Option<(i64, i64, i64, i64)> {
    s.info()
        .unwrap()
        .selection_bounds
        .map(|r| (r.x, r.y, r.width, r.height))
}

fn labels(s: &DocumentSession) -> Vec<String> {
    s.history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect()
}

fn close(a: f32, b: f32, tol: f32) -> bool {
    (a - b).abs() <= tol
}

/// Left half red, right half blue, opaque (two selection fills).
fn two_tone(s: &DocumentSession, layer: u64, w: u32, h: u32) {
    s.select_marquee(
        MarqueeShape::Rect,
        0.0,
        0.0,
        f64::from(w / 2),
        f64::from(h),
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    s.fill_selection(layer, SelectionFill::Color { color: RED }, 1.0)
        .unwrap();
    s.select_inverse().unwrap();
    s.fill_selection(layer, SelectionFill::Color { color: BLUE }, 1.0)
        .unwrap();
    s.select_none().unwrap();
}

#[test]
fn brush_stroke_is_live_then_one_undoable_node() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 300, 200);
    let nodes = s.history_items().unwrap().len();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(20.0),
        RED,
    )
    .unwrap();
    assert!(s.stroke_open());
    let f = s.stroke_points(line(50.0, 100.0, 150.0, 100.0, 8)).unwrap();
    let d = f.dirty_rect.expect("dirty");
    assert!(d.x <= 40 && d.x + d.width >= 160 && d.y <= 90 && d.y + d.height >= 110);
    assert!(f.dabs > 10 && f.total_dabs == f.dabs);
    // Live on the scratch, no history yet.
    assert_eq!(px(&s, layer, 100, 100), [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.history_items().unwrap().len(), nodes);
    let f2 = s
        .stroke_points(line(150.0, 100.0, 150.0, 150.0, 4))
        .unwrap();
    assert!(f2.total_dabs > f.total_dabs && f2.epoch > f.epoch);
    // A frame under smoothing may place nothing.
    let mut b = brush(20.0);
    b.smoothing = 30.0;
    let u = s.end_stroke().unwrap();
    assert!(!s.stroke_open());
    assert!(u.layers_changed.contains(&layer));
    assert_eq!(labels(&s).last().unwrap(), "Brush Tool");
    assert_eq!(s.history_items().unwrap().len(), nodes + 1);
    assert_eq!(px(&s, layer, 150, 140)[3], 1.0);
    assert_eq!(px(&s, layer, 10, 10)[3], 0.0);
    s.undo().unwrap();
    assert_eq!(px(&s, layer, 100, 100)[3], 0.0);
    s.redo().unwrap();
    assert_eq!(px(&s, layer, 100, 100)[0], 1.0);

    // Smoothing: a short wiggle inside the string places no dab until the end.
    s.begin_stroke(layer, StrokeTarget::Pixels, StrokeTool::Brush, b, BLUE)
        .unwrap();
    s.stroke_points(vec![sample(20.0, 20.0)]).unwrap();
    let f = s.stroke_points(line(20.0, 20.0, 30.0, 20.0, 3)).unwrap();
    assert_eq!(f.dabs, 0);
    assert!(f.dirty_rect.is_none());
    s.end_stroke().unwrap();
    // Cancel records nothing.
    let n = s.history_items().unwrap().len();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(9.0),
        BLUE,
    )
    .unwrap();
    s.stroke_points(line(200.0, 20.0, 260.0, 20.0, 5)).unwrap();
    s.cancel_stroke().unwrap();
    assert_eq!(s.history_items().unwrap().len(), n);
    assert_eq!(px(&s, layer, 230, 20)[3], 0.0);
    assert!(s.stroke_points(vec![sample(1.0, 1.0)]).is_err());
    assert!(s.end_stroke().is_err());
    // Bad input.
    let mut bad = brush(0.0);
    assert!(
        s.begin_stroke(
            layer,
            StrokeTarget::Pixels,
            StrokeTool::Brush,
            bad.clone(),
            RED
        )
        .is_err()
    );
    bad.size = 10.0;
    bad.blend_mode = "nope".into();
    assert!(
        s.begin_stroke(layer, StrokeTarget::Pixels, StrokeTool::Brush, bad, RED)
            .is_err()
    );
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(5.0),
        RED,
    )
    .unwrap();
    assert!(s.stroke_points(vec![sample(f32::NAN, 1.0)]).is_err());
    s.cancel_stroke().unwrap();
}

#[test]
fn eraser_locks_selection_limit_and_mask_strokes() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 256, 256);
    s.fill_selection(layer, SelectionFill::Color { color: RED }, 1.0)
        .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Fill");
    assert_eq!(px(&s, layer, 5, 5), [1.0, 0.0, 0.0, 1.0]);
    // Eraser to transparency.
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Eraser,
        brush(16.0),
        RED,
    )
    .unwrap();
    s.stroke_points(line(20.0, 50.0, 120.0, 50.0, 10)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Eraser");
    assert_eq!(px(&s, layer, 70, 50)[3], 0.0);
    assert_eq!(px(&s, layer, 70, 90)[3], 1.0);

    // The selection limits the paint.
    s.select_marquee(
        MarqueeShape::Rect,
        0.0,
        100.0,
        128.0,
        50.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(30.0),
        BLUE,
    )
    .unwrap();
    s.stroke_points(line(20.0, 125.0, 240.0, 125.0, 20))
        .unwrap();
    s.end_stroke().unwrap();
    assert_eq!(px(&s, layer, 60, 125)[2], 1.0);
    assert_eq!(px(&s, layer, 200, 125)[0], 1.0, "outside the selection");
    s.select_none().unwrap();

    // Transparency lock keeps alpha.
    let mut locks = s.layer(layer).unwrap().locks;
    locks.transparency = true;
    s.set_locks(layer, locks).unwrap();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(24.0),
        BLUE,
    )
    .unwrap();
    s.stroke_points(line(20.0, 50.0, 120.0, 50.0, 10)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(
        px(&s, layer, 70, 50)[3],
        0.0,
        "erased pixels stay transparent"
    );
    assert_eq!(px(&s, layer, 70, 58)[2], 1.0);
    // Pixel lock refuses.
    locks.pixels = true;
    s.set_locks(layer, locks).unwrap();
    assert!(
        s.begin_stroke(
            layer,
            StrokeTarget::Pixels,
            StrokeTool::Brush,
            brush(5.0),
            RED
        )
        .is_err()
    );
    locks = LayerLocks::default();
    s.set_locks(layer, locks).unwrap();

    // A mask stroke on a layer without a mask adds one (reveal all) in the same node.
    let n = s.history_items().unwrap().len();
    s.begin_stroke(
        layer,
        StrokeTarget::Mask,
        StrokeTool::Brush,
        brush(20.0),
        BLACK,
    )
    .unwrap();
    s.stroke_points(line(40.0, 200.0, 200.0, 200.0, 10))
        .unwrap();
    assert!(s.layer(layer).unwrap().has_mask, "live mask");
    s.end_stroke().unwrap();
    assert_eq!(s.history_items().unwrap().len(), n + 1);
    assert!(s.layer(layer).unwrap().has_mask);
    assert_eq!(mask_px(&s, layer, 100, 200), 0.0);
    assert_eq!(mask_px(&s, layer, 100, 20), 1.0);
    // Eraser on a mask hides too; brush with white reveals again.
    s.begin_stroke(
        layer,
        StrokeTarget::Mask,
        StrokeTool::Brush,
        brush(20.0),
        PaintColor {
            r: 1.0,
            g: 1.0,
            b: 1.0,
        },
    )
    .unwrap();
    s.stroke_points(line(90.0, 200.0, 110.0, 200.0, 4)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(mask_px(&s, layer, 100, 200), 1.0);
    s.undo().unwrap();
    assert_eq!(mask_px(&s, layer, 100, 200), 0.0);
    // Adjustment layers have no pixels.
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: r#"{"kind":"invert"}"#.into(),
            },
            String::new(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    assert!(
        s.begin_stroke(
            adj,
            StrokeTarget::Pixels,
            StrokeTool::Brush,
            brush(5.0),
            RED
        )
        .is_err()
    );
    s.begin_stroke(
        adj,
        StrokeTarget::Mask,
        StrokeTool::Eraser,
        brush(10.0),
        RED,
    )
    .unwrap();
    s.stroke_points(line(10.0, 10.0, 30.0, 10.0, 3)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(mask_px(&s, adj, 20, 10), 0.0);
}

#[test]
fn clone_heal_symmetry_pressure_and_sampled_tips() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 256, 128);
    two_tone(&s, layer, 256, 128);
    assert_eq!(px(&s, layer, 10, 10)[0], 1.0);
    assert_eq!(px(&s, layer, 200, 10)[2], 1.0);
    // Without a source, clone refuses.
    assert!(
        s.begin_stroke(
            layer,
            StrokeTarget::Pixels,
            StrokeTool::Clone,
            brush(10.0),
            RED
        )
        .is_err()
    );
    // Clone the red half into the blue half: sample at p − 128.
    s.set_clone_source(layer, -128.0, 0.0).unwrap();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Clone,
        brush(16.0),
        BLACK,
    )
    .unwrap();
    s.stroke_points(line(160.0, 60.0, 220.0, 60.0, 8)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Clone Stamp");
    let p = px(&s, layer, 190, 60);
    assert!(p[0] > 0.99 && p[2] < 0.01, "{p:?}");
    // Healing: runs and changes pixels near the source colour, blended.
    s.set_clone_source(layer, -128.0, 0.0).unwrap();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Heal,
        brush(12.0),
        BLACK,
    )
    .unwrap();
    s.stroke_points(line(170.0, 100.0, 190.0, 100.0, 4))
        .unwrap();
    s.end_stroke().unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Healing Brush");
    assert!(s.set_clone_source(layer, f32::NAN, 0.0).is_err());
    assert!(s.set_clone_source(9999, 0.0, 0.0).is_err());
    // Sample all layers clones from the composite.
    let top = s
        .add_layer(NewLayer::Pixel, String::new(), None, None)
        .unwrap()
        .created[0];
    s.set_clone_source(top, 128.0, 0.0).unwrap();
    let mut b = brush(10.0);
    b.sample_all_layers = true;
    s.begin_stroke(top, StrokeTarget::Pixels, StrokeTool::Clone, b, BLACK)
        .unwrap();
    s.stroke_points(line(20.0, 20.0, 40.0, 20.0, 4)).unwrap();
    s.end_stroke().unwrap();
    let p = px(&s, top, 30, 20);
    assert!(p[2] > 0.99 && p[3] > 0.99, "{p:?}");

    // Symmetry: a stroke left of x = 128 is mirrored right of it.
    let (s, layer) = doc(&e, 256, 128);
    let mut b = brush(10.0);
    b.symmetry = PaintSymmetry::Vertical;
    b.symmetry_x = 128.0;
    s.begin_stroke(layer, StrokeTarget::Pixels, StrokeTool::Brush, b, RED)
        .unwrap();
    s.stroke_points(line(20.0, 40.0, 60.0, 40.0, 4)).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(px(&s, layer, 40, 40)[3], 1.0);
    assert_eq!(px(&s, layer, 216, 40)[3], 1.0);
    assert_eq!(px(&s, layer, 128, 40)[3], 0.0);

    // Pressure controls size: a light stroke is thinner.
    let mut b = brush(40.0);
    b.pressure_size = true;
    s.begin_stroke(layer, StrokeTarget::Pixels, StrokeTool::Brush, b, BLUE)
        .unwrap();
    let light: Vec<StrokeSample> = line(20.0, 100.0, 100.0, 100.0, 8)
        .into_iter()
        .map(|mut p| {
            p.pressure = 0.25;
            p.tilt_x = 0.3;
            p
        })
        .collect();
    s.stroke_points(light).unwrap();
    s.end_stroke().unwrap();
    assert_eq!(px(&s, layer, 60, 100)[3], 1.0);
    assert_eq!(px(&s, layer, 60, 115)[3], 0.0, "a 10 px dab at 25 %");

    // Sampled tips: built-ins, an imported ABR, previews and a stroke.
    let tips = brush_tips();
    assert!(tips.iter().any(|t| t.id == "builtin:chalk"));
    let dir = tempfile::tempdir().unwrap();
    let tip = brush::SampledTip::new(
        "Dot",
        8,
        8,
        (0..64)
            .map(|i| if i % 9 == 0 { 1.0 } else { 0.5 })
            .collect(),
    )
    .unwrap();
    let path = dir.path().join("set.abr");
    std::fs::write(&path, brush::abr::write_v6(&[tip], 6, 2, true).unwrap()).unwrap();
    let added = import_abr(path.to_string_lossy().into_owned()).unwrap();
    assert_eq!(added.len(), 1);
    assert!(added[0].sampled && added[0].width == 8);
    assert!(brush_tips().iter().any(|t| t.id == added[0].id));
    let img = brush_tip_preview(added[0].id.clone(), 32).unwrap();
    assert_eq!((img.width, img.height), (32, 32));
    assert_eq!(img.pixels.len(), 32 * 32);
    let round = brush_tip_preview("round:0.5".into(), 16).unwrap();
    assert!(round.pixels[8 * 16 + 8] == 255 && round.pixels[0] == 0);
    assert!(brush_tip_preview("nope".into(), 16).is_err());
    assert!(
        import_abr(
            dir.path()
                .join("missing.abr")
                .to_string_lossy()
                .into_owned()
        )
        .is_err()
    );
    let mut b = brush(24.0);
    b.tip_id = Some(added[0].id.clone());
    s.begin_stroke(layer, StrokeTarget::Pixels, StrokeTool::Brush, b, RED)
        .unwrap();
    let f = s.stroke_points(line(150.0, 60.0, 230.0, 60.0, 8)).unwrap();
    assert!(f.dabs > 0);
    s.end_stroke().unwrap();
    assert!(px(&s, layer, 190, 60)[3] > 0.0);
}

#[test]
fn marquee_lasso_and_selection_ops() {
    let (_d, e) = engine();
    let (s, _) = doc(&e, 400, 300);
    let m = |shape, x, y, w, h, op| {
        s.select_marquee(shape, x, y, w, h, 0.0, true, op).unwrap();
    };
    m(
        MarqueeShape::Rect,
        10.0,
        20.0,
        100.0,
        50.0,
        SelectionOp::Replace,
    );
    assert_eq!(bounds(&s), Some((10, 20, 100, 50)));
    assert_eq!(labels(&s).last().unwrap(), "Rectangular Marquee");
    m(
        MarqueeShape::Rect,
        50.0,
        20.0,
        100.0,
        50.0,
        SelectionOp::Add,
    );
    assert_eq!(bounds(&s), Some((10, 20, 140, 50)));
    m(
        MarqueeShape::Rect,
        10.0,
        20.0,
        40.0,
        50.0,
        SelectionOp::Subtract,
    );
    assert_eq!(bounds(&s), Some((50, 20, 100, 50)));
    m(
        MarqueeShape::Rect,
        0.0,
        0.0,
        80.0,
        300.0,
        SelectionOp::Intersect,
    );
    assert_eq!(bounds(&s), Some((50, 20, 30, 50)));
    // Subtracting everything deselects.
    m(
        MarqueeShape::Rect,
        0.0,
        0.0,
        400.0,
        300.0,
        SelectionOp::Subtract,
    );
    assert_eq!(bounds(&s), None);
    // Without a selection, add = new shape; intersect = nothing.
    m(MarqueeShape::Rect, 5.0, 5.0, 10.0, 10.0, SelectionOp::Add);
    assert_eq!(bounds(&s), Some((5, 5, 10, 10)));
    s.select_none().unwrap();
    m(
        MarqueeShape::Rect,
        5.0,
        5.0,
        10.0,
        10.0,
        SelectionOp::Intersect,
    );
    assert_eq!(bounds(&s), None);

    m(
        MarqueeShape::Ellipse,
        100.0,
        100.0,
        200.0,
        100.0,
        SelectionOp::Replace,
    );
    assert_eq!(labels(&s).last().unwrap(), "Elliptical Marquee");
    let (x, y, w, h) = bounds(&s).unwrap();
    assert!(
        (99..=101).contains(&x) && (99..=101).contains(&y),
        "{x} {y}"
    );
    assert!(
        (198..=202).contains(&w) && (98..=102).contains(&h),
        "{w} {h}"
    );
    assert_eq!(sel_px(&s, 200, 150), 1.0);
    assert_eq!(sel_px(&s, 102, 102), 0.0, "outside the ellipse corner");
    // Feathered ellipse: soft edge.
    s.select_marquee(
        MarqueeShape::Ellipse,
        100.0,
        100.0,
        200.0,
        100.0,
        10.0,
        true,
        SelectionOp::Replace,
    )
    .unwrap();
    let edge = sel_px(&s, 200, 100);
    assert!(edge > 0.1 && edge < 0.9, "{edge}");
    m(MarqueeShape::Row, 0.0, 42.0, 0.0, 0.0, SelectionOp::Replace);
    assert_eq!(bounds(&s), Some((0, 42, 400, 1)));
    m(
        MarqueeShape::Column,
        17.0,
        0.0,
        0.0,
        0.0,
        SelectionOp::Replace,
    );
    assert_eq!(bounds(&s), Some((17, 0, 1, 300)));
    assert!(
        s.select_marquee(
            MarqueeShape::Rect,
            500.0,
            500.0,
            5.0,
            5.0,
            0.0,
            false,
            SelectionOp::Replace
        )
        .is_err()
    );
    assert!(
        s.select_marquee(
            MarqueeShape::Rect,
            f64::NAN,
            0.0,
            5.0,
            5.0,
            0.0,
            false,
            SelectionOp::Replace
        )
        .is_err()
    );

    // Lassos.
    let tri = vec![
        ToolPoint { x: 50.0, y: 50.0 },
        ToolPoint { x: 250.0, y: 50.0 },
        ToolPoint { x: 50.0, y: 250.0 },
    ];
    s.select_lasso(
        tri.clone(),
        LassoKind::Polygon,
        0.0,
        true,
        SelectionOp::Replace,
    )
    .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Polygonal Lasso");
    let (x, y, w, h) = bounds(&s).unwrap();
    assert!(
        (49..=51).contains(&x)
            && (49..=51).contains(&y)
            && (198..=202).contains(&w)
            && (198..=202).contains(&h)
    );
    assert_eq!(sel_px(&s, 80, 80), 1.0);
    assert_eq!(sel_px(&s, 200, 200), 0.0);
    s.select_lasso(
        tri.clone(),
        LassoKind::Free,
        4.0,
        true,
        SelectionOp::Replace,
    )
    .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Lasso");
    assert!(
        s.select_lasso(
            tri[..2].to_vec(),
            LassoKind::Free,
            0.0,
            true,
            SelectionOp::Replace
        )
        .is_err()
    );
    // Magnetic on a two-tone image snaps to the colour edge at x = 200.
    let layer = s.layers().unwrap()[0].id;
    two_tone(&s, layer, 400, 300);
    let anchors = vec![
        ToolPoint { x: 196.0, y: 20.0 },
        ToolPoint { x: 196.0, y: 280.0 },
        ToolPoint { x: 20.0, y: 280.0 },
        ToolPoint { x: 20.0, y: 20.0 },
    ];
    s.select_lasso(
        anchors,
        LassoKind::Magnetic,
        0.0,
        true,
        SelectionOp::Replace,
    )
    .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Magnetic Lasso");
    let (x, _, w, _) = bounds(&s).unwrap();
    assert!(
        (x + w - 200).abs() <= 3,
        "right edge snapped to 200: {}",
        x + w
    );
    let path = s
        .magnetic_path(
            ToolPoint { x: 196.0, y: 20.0 },
            ToolPoint { x: 197.0, y: 200.0 },
        )
        .unwrap();
    assert!(path.len() > 2);
    assert!(path.iter().all(|p| (p.x - 200.0).abs() < 6.0), "{path:?}");
}

#[test]
fn wand_quick_range_all_inverse_modify_and_channels() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 400, 300);
    two_tone(&s, layer, 400, 300);
    s.set_selected_layers(vec![layer]).unwrap();
    for sample_all in [false, true] {
        s.select_wand(
            50.0,
            50.0,
            32.0,
            true,
            sample_all,
            false,
            SelectionOp::Replace,
        )
        .unwrap();
        assert_eq!(
            bounds(&s),
            Some((0, 0, 200, 300)),
            "sample_all {sample_all}"
        );
    }
    assert_eq!(labels(&s).last().unwrap(), "Magic Wand");
    s.select_wand(350.0, 50.0, 32.0, false, true, true, SelectionOp::Add)
        .unwrap();
    assert_eq!(bounds(&s), Some((0, 0, 400, 300)));
    assert!(
        s.select_wand(-1.0, 0.0, 32.0, true, true, true, SelectionOp::Replace)
            .is_err()
    );

    s.select_quick(
        vec![
            ToolPoint { x: 300.0, y: 100.0 },
            ToolPoint { x: 320.0, y: 150.0 },
        ],
        10.0,
        true,
        SelectionOp::Replace,
    )
    .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Quick Selection");
    let (x, _, w, _) = bounds(&s).unwrap();
    assert!((x - 200).abs() <= 1 && (x + w - 400).abs() <= 1, "{x} {w}");
    assert!(
        s.select_quick(vec![], 10.0, true, SelectionOp::Add)
            .is_err()
    );

    s.select_color_range(RED, 40.0, SelectionOp::Replace)
        .unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Color Range");
    assert_eq!(bounds(&s), Some((0, 0, 200, 300)));

    s.select_inverse().unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Inverse");
    assert_eq!(bounds(&s), Some((200, 0, 200, 300)));
    s.select_all().unwrap();
    assert_eq!(bounds(&s), Some((0, 0, 400, 300)));
    assert_eq!(labels(&s).last().unwrap(), "Select All");
    s.select_inverse().unwrap();
    assert_eq!(bounds(&s), None);
    assert!(s.select_inverse().is_err());

    // Modify.
    let rect = |s: &DocumentSession| {
        s.select_marquee(
            MarqueeShape::Rect,
            100.0,
            100.0,
            100.0,
            50.0,
            0.0,
            false,
            SelectionOp::Replace,
        )
        .unwrap()
    };
    rect(&s);
    s.modify_selection(SelectionModify::Expand, 10.0).unwrap();
    assert_eq!(bounds(&s), Some((90, 90, 120, 70)));
    assert_eq!(labels(&s).last().unwrap(), "Expand");
    rect(&s);
    s.modify_selection(SelectionModify::Contract, 10.0).unwrap();
    assert_eq!(bounds(&s), Some((110, 110, 80, 30)));
    rect(&s);
    s.modify_selection(SelectionModify::Border, 6.0).unwrap();
    assert_eq!(sel_px(&s, 150, 125), 0.0, "border hollows the middle");
    assert!(sel_px(&s, 100, 125) > 0.5);
    rect(&s);
    s.modify_selection(SelectionModify::Feather, 8.0).unwrap();
    let v = sel_px(&s, 100, 125);
    assert!(v > 0.2 && v < 0.8, "{v}");
    rect(&s);
    s.modify_selection(SelectionModify::Smooth, 5.0).unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Smooth");
    assert!(s.modify_selection(SelectionModify::Expand, -1.0).is_err());
    s.select_none().unwrap();
    assert!(s.modify_selection(SelectionModify::Expand, 1.0).is_err());

    // Channels.
    rect(&s);
    s.save_selection("Alpha 1".into()).unwrap();
    s.select_none().unwrap();
    assert!(s.save_selection("empty".into()).is_err());
    assert_eq!(s.selection_channels().unwrap(), vec!["Alpha 1".to_string()]);
    s.load_selection("Alpha 1".into(), SelectionOp::Replace)
        .unwrap();
    assert_eq!(bounds(&s), Some((100, 100, 100, 50)));
    assert_eq!(labels(&s).last().unwrap(), "Load Selection");
    assert!(
        s.load_selection("nope".into(), SelectionOp::Replace)
            .is_err()
    );
    assert!(s.save_selection(" ".into()).is_err());
}

/// Selects the left half, as a stand-in for the models.
struct HalfSegmenter;

impl MaskSegmenter for HalfSegmenter {
    fn segment(
        &mut self,
        image: &image::RgbImage,
        request: &SegmentRequest,
    ) -> anyhow::Result<Vec<f32>> {
        let (w, h) = image.dimensions();
        Ok((0..w * h)
            .map(|i| {
                let left = (i % w) < w / 2;
                match request {
                    SegmentRequest::Sky => f32::from(u8::from(!left)),
                    _ => f32::from(u8::from(left)),
                }
            })
            .collect())
    }
}

#[test]
fn subject_sky_object_refine_and_outline() {
    let (_d, e) = engine();
    e.install_mask_segmenter(Box::new(HalfSegmenter));
    let (s, layer) = doc(&e, 400, 300);
    two_tone(&s, layer, 400, 300);
    s.select_subject(SelectionOp::Replace).unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Select Subject");
    let (x, _, w, _) = bounds(&s).unwrap();
    assert!(x == 0 && (w - 200).abs() <= 3, "{w}");
    s.select_sky(SelectionOp::Replace).unwrap();
    let (x, _, _, _) = bounds(&s).unwrap();
    assert!((x - 200).abs() <= 3);
    s.select_object(50.0, 50.0, SelectionOp::Add).unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Object Selection");
    assert!(bounds(&s).unwrap().2 >= 396);

    // Outline of a rectangle: one closed loop on its edges.
    s.select_marquee(
        MarqueeShape::Rect,
        100.0,
        50.0,
        120.0,
        80.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    for level in [0u8, 1] {
        let o = s.selection_outline(level).unwrap();
        assert_eq!(o.len(), 1, "level {level}");
        assert!(o[0].closed);
        let xs: Vec<f32> = o[0].points.iter().map(|p| p.x).collect();
        let ys: Vec<f32> = o[0].points.iter().map(|p| p.y).collect();
        let (x0, x1) = (
            xs.iter().cloned().fold(f32::MAX, f32::min),
            xs.iter().cloned().fold(f32::MIN, f32::max),
        );
        let (y0, y1) = (
            ys.iter().cloned().fold(f32::MAX, f32::min),
            ys.iter().cloned().fold(f32::MIN, f32::max),
        );
        let tol = if level == 0 { 0.6 } else { 1.6 };
        assert!(close(x0, 100.0, tol) && close(x1, 220.0, tol), "{x0} {x1}");
        assert!(close(y0, 50.0, tol) && close(y1, 130.0, tol), "{y0} {y1}");
        // Cached.
        assert_eq!(s.selection_outline(level).unwrap(), o);
    }
    // Two rectangles → two loops.
    s.select_marquee(
        MarqueeShape::Rect,
        300.0,
        200.0,
        50.0,
        50.0,
        0.0,
        false,
        SelectionOp::Add,
    )
    .unwrap();
    assert_eq!(s.selection_outline(0).unwrap().len(), 2);

    // Refine edge: interactive shows live, then one node; cancel restores.
    s.select_marquee(
        MarqueeShape::Rect,
        150.0,
        50.0,
        100.0,
        100.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    let n = s.history_items().unwrap().len();
    let p = RefineEdgeParams {
        radius: 8.0,
        smart_radius: false,
        smooth: 0.0,
        feather: 3.0,
        contrast: 0.0,
        shift_edge: 0.0,
    };
    s.refine_edge(p, true).unwrap();
    assert_eq!(s.history_items().unwrap().len(), n);
    let live = sel_px(&s, 150, 100);
    assert!(live > 0.0 && live < 1.0, "{live}");
    s.refine_edge(RefineEdgeParams { feather: 6.0, ..p }, true)
        .unwrap();
    s.cancel_refine_edge().unwrap();
    assert_eq!(sel_px(&s, 150, 100), 1.0, "restored");
    s.refine_edge(
        RefineEdgeParams {
            shift_edge: 5.0,
            feather: 0.0,
            ..p
        },
        true,
    )
    .unwrap();
    s.refine_edge(
        RefineEdgeParams {
            shift_edge: 5.0,
            feather: 0.0,
            ..p
        },
        false,
    )
    .unwrap();
    assert_eq!(s.history_items().unwrap().len(), n + 1);
    assert_eq!(labels(&s).last().unwrap(), "Refine Edge");
    let (x, _, _, _) = bounds(&s).unwrap();
    assert!(x < 150, "shifted out: {x}");
    s.select_none().unwrap();
    assert!(s.selection_outline(0).unwrap().is_empty());
    assert!(s.refine_edge(p, false).is_err());
}

#[test]
fn free_transform_preview_commit_cancel() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 300, 200);
    s.select_marquee(
        MarqueeShape::Rect,
        20.0,
        30.0,
        40.0,
        50.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    s.fill_selection(layer, SelectionFill::Color { color: RED }, 1.0)
        .unwrap();
    s.select_none().unwrap();
    let info = s.begin_transform(vec![layer]).unwrap();
    assert_eq!(
        info.bounds,
        Some(DocRect {
            x: 20,
            y: 30,
            width: 40,
            height: 50
        })
    );
    let n = s.history_items().unwrap().len();
    let shift = |dx: f64, dy: f64| TransformMatrix {
        a: 1.0,
        b: 0.0,
        c: dx,
        d: 0.0,
        e: 1.0,
        f: dy,
    };
    s.set_transform(shift(100.0, 10.0), TransformInterpolation::Bicubic)
        .unwrap();
    assert_eq!(
        s.history_items().unwrap().len(),
        n,
        "preview has no history"
    );
    assert_eq!(px(&s, layer, 140, 60)[3], 1.0);
    assert_eq!(px(&s, layer, 40, 60)[3], 0.0);
    s.cancel_transform().unwrap();
    assert_eq!(px(&s, layer, 40, 60)[3], 1.0);
    assert!(s.commit_transform().is_err());

    // Scale 2× about the rectangle's centre, then commit.
    s.begin_transform(vec![layer]).unwrap();
    let (cx, cy) = (40.0, 55.0);
    let m = TransformMatrix {
        a: 2.0,
        b: 0.0,
        c: cx - 2.0 * cx,
        d: 0.0,
        e: 2.0,
        f: cy - 2.0 * cy,
    };
    assert!(
        s.set_transform(
            TransformMatrix {
                a: 0.0,
                b: 0.0,
                c: 0.0,
                d: 0.0,
                e: 0.0,
                f: 0.0
            },
            TransformInterpolation::Nearest
        )
        .is_err()
    );
    s.set_transform(m, TransformInterpolation::Bilinear)
        .unwrap();
    let u = s.commit_transform().unwrap();
    assert!(u.layers_changed.contains(&layer));
    assert_eq!(labels(&s).last().unwrap(), "Free Transform");
    assert_eq!(s.history_items().unwrap().len(), n + 1);
    let b = s.layers().unwrap()[0].bounds;
    assert!(b.is_some());

    assert_eq!(px(&s, layer, 5, 10)[3], 1.0, "grew to x 0…80, y 5…105");
    assert_eq!(px(&s, layer, 78, 103)[3], 1.0);
    assert_eq!(px(&s, layer, 82, 60)[3], 0.0);
    s.undo().unwrap();
    assert_eq!(px(&s, layer, 5, 10)[3], 0.0);
    // Rotation 90° with the mask moving along; locked or non-pixel layers refuse.
    s.add_mask(layer, MaskInit::HideAll).unwrap();
    s.begin_transform(vec![layer]).unwrap();
    s.set_transform(
        TransformMatrix {
            a: 0.0,
            b: -1.0,
            c: 200.0,
            d: 1.0,
            e: 0.0,
            f: 0.0,
        },
        TransformInterpolation::Nearest,
    )
    .unwrap();
    s.commit_transform().unwrap();
    // (40, 55) → (145, 40).
    assert_eq!(px(&s, layer, 145, 40)[3], 1.0);
    assert_eq!(mask_px(&s, layer, 145, 40), 0.0, "hidden mask moved too");
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: r#"{"kind":"invert"}"#.into(),
            },
            String::new(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    assert!(s.begin_transform(vec![adj]).is_err());
    assert!(s.begin_transform(vec![]).is_err());
    let mut locks = s.layer(layer).unwrap().locks;
    locks.position = true;
    s.set_locks(layer, locks).unwrap();
    assert!(s.begin_transform(vec![layer]).is_err());
}

#[test]
fn fill_clear_content_aware_and_eyedropper() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 200, 200);
    s.fill_selection(layer, SelectionFill::Color { color: BLUE }, 0.5)
        .unwrap();
    let p = px(&s, layer, 10, 10);
    assert!(close(p[2], 1.0, 0.01) && close(p[3], 0.5, 0.01), "{p:?}");
    s.fill_selection(layer, SelectionFill::Color { color: BLUE }, 1.0)
        .unwrap();
    s.select_marquee(
        MarqueeShape::Rect,
        50.0,
        50.0,
        20.0,
        20.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    s.fill_selection(layer, SelectionFill::Color { color: RED }, 1.0)
        .unwrap();
    // Content-aware (placeholder) fills the red square from its blue surroundings.
    s.fill_selection(layer, SelectionFill::ContentAware, 1.0)
        .unwrap();
    let p = px(&s, layer, 60, 60);
    assert!(p[2] > 0.95 && p[0] < 0.05, "{p:?}");
    // Clear to transparency.
    s.delete_selection(layer, BLACK).unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Clear");
    assert_eq!(px(&s, layer, 60, 60)[3], 0.0);
    assert_eq!(px(&s, layer, 40, 40)[3], 1.0);
    s.select_none().unwrap();
    assert!(s.delete_selection(layer, BLACK).is_err());
    assert!(
        s.fill_selection(layer, SelectionFill::ContentAware, 1.0)
            .is_err()
    );
    // Background layers fill with the background colour.
    s.flatten().unwrap();
    let bg = s.layers().unwrap()[0].id;
    s.select_marquee(
        MarqueeShape::Rect,
        0.0,
        0.0,
        10.0,
        10.0,
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    s.delete_selection(bg, RED).unwrap();
    let p = px(&s, bg, 5, 5);
    assert!(p[0] > 0.99 && p[1] < 0.01, "{p:?}");

    // Eyedropper.
    let c = s.sample_color(5.0, 5.0, true, None, 0).unwrap();
    assert!(close(c.r, 1.0, 0.01) && close(c.b, 0.0, 0.01));
    let c = s.sample_color(100.0, 100.0, false, Some(bg), 2).unwrap();
    assert!(close(c.b, 1.0, 0.01));
    assert!(s.sample_color(500.0, 0.0, true, None, 0).is_err());
    let (s2, l2) = doc(&e, 64, 64);
    assert!(
        s2.sample_color(5.0, 5.0, false, Some(l2), 0).is_err(),
        "transparent"
    );
}

/// Frames reaching the listener.
#[derive(Default)]
struct Frames(Mutex<Vec<DocFrameInfo>>);

impl DocumentListener for Frames {
    fn on_frame(&self, frame: DocFrameInfo) {
        self.0.lock().unwrap().push(frame);
    }
    fn on_layers_changed(&self, _: Vec<u64>) {}
    fn on_history_changed(&self, _: u64) {}
    fn on_render_failed(&self, m: String) {
        panic!("render failed: {m}");
    }
}

#[test]
fn stroke_frames_reach_the_viewport() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e, 512, 384);
    let rec = Arc::new(Frames::default());
    s.set_listener(Some(rec.clone()));
    let plan = s.plan_surface(256, 192).unwrap();
    for _ in 0..2 {
        s.attach_surface(
            create_rgba8(plan.width, plan.height),
            plan.width,
            plan.height,
        )
        .unwrap();
    }
    s.wait_idle();
    s.begin_stroke(
        layer,
        StrokeTarget::Pixels,
        StrokeTool::Brush,
        brush(12.0),
        RED,
    )
    .unwrap();
    let mut last = 0;
    for i in 0..10 {
        let x = 40.0 + i as f32 * 40.0;
        let f = s.stroke_points(line(x, 100.0, x + 40.0, 100.0, 4)).unwrap();
        last = f.epoch;
        s.wait_idle();
    }
    let frames = rec.0.lock().unwrap().clone();
    assert_eq!(frames.last().unwrap().epoch, last);
    assert!(frames.len() >= 2);
    s.end_stroke().unwrap();
    s.close();
}

/// Interactive dab latency on a sample.dng-sized 16-bit layer (5212 × 3468)
/// with a presented level-1 viewport: `stroke_points` time plus the frame's
/// render time (change → pixels in the IOSurface).
#[test]
#[ignore]
fn bench_interactive_dabs_20mp() {
    let (_d, e) = engine();
    let (w, h) = (5212u32, 3468u32);
    let s = e.clone().new_document(w, h, DocDepth::U16, None).unwrap();
    let layer = s.layers().unwrap()[0].id;
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.6,
                g: 0.5,
                b: 0.4,
            },
        },
        1.0,
    )
    .unwrap();
    let rec = Arc::new(Frames::default());
    s.set_listener(Some(rec.clone()));
    // A 1440 × 900 @2x window at fit: level 1.
    let (vw, vh) = (2606u32, 1734u32);
    for _ in 0..3 {
        s.attach_surface(create_rgba8(vw, vh), vw, vh).unwrap();
    }
    s.set_viewport(1, 0, 0, vw, vh, 0.5).unwrap();
    s.wait_idle();
    for size in [30.0f32, 100.0, 300.0] {
        s.begin_stroke(
            layer,
            StrokeTarget::Pixels,
            StrokeTool::Brush,
            brush(size),
            RED,
        )
        .unwrap();
        let before = rec.0.lock().unwrap().len();
        let mut calls = Vec::new();
        let t0 = Instant::now();
        // 120 frames of pointer motion, ~6 samples per frame, a diagonal sweep.
        for f in 0..120 {
            let x0 = 400.0 + f as f32 * 30.0;
            let y0 = 600.0 + f as f32 * 15.0;
            let pts: Vec<StrokeSample> = (0..6)
                .map(|k| {
                    let mut p = sample(x0 + k as f32 * 5.0, y0 + k as f32 * 2.5);
                    p.timestamp = t0.elapsed().as_secs_f64();
                    p
                })
                .collect();
            let t = Instant::now();
            s.stroke_points(pts).unwrap();
            calls.push(t.elapsed().as_secs_f64() * 1000.0);
            s.wait_idle();
            std::thread::sleep(Duration::from_millis(1));
        }
        let frames: Vec<f64> = rec.0.lock().unwrap()[before..]
            .iter()
            .map(|f| f.render_ms)
            .collect();
        s.end_stroke().unwrap();
        let q = |v: &mut Vec<f64>, p: f64| {
            v.sort_by(|a, b| a.partial_cmp(b).unwrap());
            v[((v.len() - 1) as f64 * p).round() as usize]
        };
        let (mut c, mut r) = (calls.clone(), frames.clone());
        let mut total: Vec<f64> = calls
            .iter()
            .zip(frames.iter())
            .map(|(a, b)| a + b)
            .collect();
        eprintln!(
            "size {size:>5}: stroke_points median {:.2} ms p90 {:.2} max {:.2}; frame render median {:.2} ms p90 {:.2} max {:.2}; dab→surface median {:.2} p90 {:.2} ({} frames)",
            q(&mut c, 0.5),
            q(&mut c, 0.9),
            q(&mut c, 1.0),
            q(&mut r, 0.5),
            q(&mut r, 0.9),
            q(&mut r, 1.0),
            q(&mut total, 0.5),
            q(&mut total, 0.9),
            frames.len()
        );
    }
    s.close();
}
