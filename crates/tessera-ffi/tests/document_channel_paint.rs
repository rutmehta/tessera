//! Painting into saved alpha channels and the Quick Mask (WP B5-17c):
//! `StrokeTarget::Channel` strokes change only the channel (layer pixels,
//! masks and the RGB composite stay as they were), record exactly one
//! undoable history node, are clipped by the selection, ignore layer locks,
//! reject unknown channels without a node and survive `.tessera-doc` / PSD
//! round trips.
#![cfg(target_os = "macos")]

use compositor::{LayerId, LayerKind};
use std::sync::Arc;
use tessera_ffi::*;

const W: u32 = 400;
const H: u32 = 300;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn doc(engine: &Arc<Engine>) -> (Arc<DocumentSession>, u64) {
    let s = engine
        .clone()
        .new_document(W, H, DocDepth::U8, None)
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

const WHITE: PaintColor = PaintColor {
    r: 1.0,
    g: 1.0,
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

/// A horizontal line of samples at `y` from `x0` to `x1`.
fn line(x0: f32, x1: f32, y: f32) -> Vec<StrokeSample> {
    (0..=16)
        .map(|i| sample(x0 + (x1 - x0) * i as f32 / 16.0, y))
        .collect()
}

fn chan_px(s: &DocumentSession, id: u64, x: u32, y: u32) -> f32 {
    let st = s.document_state().unwrap();
    st.channels
        .iter()
        .find(|c| c.id.0 == id)
        .expect("channel")
        .raster
        .pixel(x, y)[0]
}

fn layer_px(s: &DocumentSession, layer: u64, x: u32, y: u32) -> [f32; 4] {
    let st = s.document_state().unwrap();
    match &st.find(LayerId(layer)).unwrap().kind {
        LayerKind::Pixel(r) => r.pixel(x, y),
        _ => panic!("not a pixel layer"),
    }
}

fn has_mask(s: &DocumentSession, layer: u64) -> bool {
    let st = s.document_state().unwrap();
    st.find(LayerId(layer)).unwrap().mask.is_some()
}

fn nodes(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn labels(s: &DocumentSession) -> Vec<String> {
    s.history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect()
}

fn sel_px(s: &DocumentSession, x: u32, y: u32) -> f32 {
    s.document_state()
        .unwrap()
        .selection
        .as_ref()
        .map_or(1.0, |r| r.pixel(x, y)[0])
}

/// The layer filled with an opaque colour, so the RGB composite is not blank.
fn fill(s: &DocumentSession, layer: u64) {
    s.select_all().unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.9,
                g: 0.2,
                b: 0.4,
            },
        },
        1.0,
    )
    .unwrap();
    s.select_none().unwrap();
}

fn export_png(s: &DocumentSession, path: &std::path::Path) -> Vec<u8> {
    s.export_flat(
        path.to_string_lossy().into_owned(),
        ExportFormat::Png,
        100,
        ExportColor::Document,
    )
    .unwrap();
    image::open(path).unwrap().to_rgba8().into_raw()
}

/// A stroke of `tool` in `color` along y = 150 from x0 to x1 into channel `id`.
fn stroke(
    s: &DocumentSession,
    layer: u64,
    id: u64,
    tool: StrokeTool,
    color: PaintColor,
    x0: f32,
    x1: f32,
) -> StrokeFrame {
    s.begin_stroke(
        layer,
        StrokeTarget::Channel { id },
        tool,
        brush(30.0),
        color,
    )
    .unwrap();
    let f = s.stroke_points(line(x0, x1, 150.0)).unwrap();
    s.end_stroke().unwrap();
    f
}

#[test]
fn channel_stroke_changes_only_the_channel_in_one_undoable_node() {
    let (d, e) = engine();
    let (s, layer) = doc(&e);
    fill(&s, layer);
    let a = s
        .new_alpha_channel("Alpha 1".into(), false)
        .unwrap()
        .channel_id;
    let composite = export_png(&s, &d.path().join("before.png"));
    let pixels = layer_px(&s, layer, 100, 150);
    let n = nodes(&s);

    s.begin_stroke(
        layer,
        StrokeTarget::Channel { id: a },
        StrokeTool::Brush,
        brush(30.0),
        WHITE,
    )
    .unwrap();
    let f = s.stroke_points(line(50.0, 150.0, 150.0)).unwrap();
    // The frame names the painted canvas area (the channel op itself has no
    // RGB damage) so the host can refresh the overlay.
    let r = f.dirty_rect.expect("dirty rect for a channel stroke");
    assert!(r.x <= 40 && r.x + r.width >= 160 && r.y <= 140 && r.y + r.height >= 160);
    // Live before the node exists.
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    assert_eq!(nodes(&s), n);
    s.end_stroke().unwrap();

    assert_eq!(nodes(&s), n + 1, "exactly one history node per stroke");
    assert_eq!(labels(&s).last().unwrap(), "Brush Tool");
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    assert_eq!(chan_px(&s, a, 300, 150), 0.0);
    assert_eq!(chan_px(&s, a, 100, 20), 0.0);
    // Layer pixels, mask and the RGB composite are untouched.
    assert_eq!(layer_px(&s, layer, 100, 150), pixels);
    assert!(!has_mask(&s, layer));
    assert!(
        export_png(&s, &d.path().join("after.png")) == composite,
        "a channel stroke changed the RGB composite"
    );

    s.undo().unwrap();
    assert_eq!(chan_px(&s, a, 100, 150), 0.0);
    s.redo().unwrap();
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    assert_eq!(nodes(&s), n + 1);
}

#[test]
fn brush_paints_luminance_and_eraser_paints_toward_zero() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let a = s
        .new_alpha_channel("Alpha 1".into(), true)
        .unwrap()
        .channel_id;
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    stroke(&s, layer, a, StrokeTool::Eraser, WHITE, 50.0, 150.0);
    assert_eq!(labels(&s).last().unwrap(), "Eraser");
    assert_eq!(chan_px(&s, a, 100, 150), 0.0);
    assert_eq!(chan_px(&s, a, 300, 150), 1.0);
    // Mid grey paints half.
    let grey = PaintColor {
        r: 0.5,
        g: 0.5,
        b: 0.5,
    };
    stroke(&s, layer, a, StrokeTool::Brush, grey, 50.0, 150.0);
    assert!((chan_px(&s, a, 100, 150) - 0.5).abs() < 0.01);
    stroke(&s, layer, a, StrokeTool::Brush, BLACK, 250.0, 350.0);
    assert_eq!(chan_px(&s, a, 300, 150), 0.0);
}

#[test]
fn selection_limits_channel_paint() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let a = s
        .new_alpha_channel("Alpha 1".into(), false)
        .unwrap()
        .channel_id;
    s.select_marquee(
        MarqueeShape::Rect,
        0.0,
        0.0,
        200.0,
        f64::from(H),
        0.0,
        false,
        SelectionOp::Replace,
    )
    .unwrap();
    let n = nodes(&s);
    stroke(&s, layer, a, StrokeTool::Brush, WHITE, 50.0, 350.0);
    assert_eq!(nodes(&s), n + 1);
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    assert_eq!(chan_px(&s, a, 300, 150), 0.0, "outside the selection");
}

#[test]
fn layer_locks_and_layer_kind_do_not_block_channel_paint() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let a = s
        .new_alpha_channel("Alpha 1".into(), false)
        .unwrap()
        .channel_id;
    s.set_locks(
        layer,
        LayerLocks {
            transparency: true,
            pixels: true,
            position: true,
            all: true,
        },
    )
    .unwrap();
    // The locked layer still refuses pixel paint.
    assert!(
        s.begin_stroke(
            layer,
            StrokeTarget::Pixels,
            StrokeTool::Brush,
            brush(30.0),
            WHITE
        )
        .is_err()
    );
    stroke(&s, layer, a, StrokeTool::Brush, WHITE, 50.0, 150.0);
    assert_eq!(chan_px(&s, a, 100, 150), 1.0);
    // The layer argument is ignored for channel targets.
    stroke(&s, u64::MAX, a, StrokeTool::Brush, WHITE, 250.0, 350.0);
    assert_eq!(chan_px(&s, a, 300, 150), 1.0);
}

#[test]
fn unknown_channel_fails_without_a_history_node() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let n = nodes(&s);
    let err = s.begin_stroke(
        layer,
        StrokeTarget::Channel { id: 9_999 },
        StrokeTool::Brush,
        brush(30.0),
        WHITE,
    );
    assert!(err.is_err());
    assert!(!s.stroke_open());
    assert_eq!(nodes(&s), n);
}

#[test]
fn clone_and_heal_are_refused_on_channels() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let a = s
        .new_alpha_channel("Alpha 1".into(), false)
        .unwrap()
        .channel_id;
    s.set_clone_source(layer, 10.0, 0.0).unwrap();
    let n = nodes(&s);
    for tool in [StrokeTool::Clone, StrokeTool::Heal] {
        let err = s
            .begin_stroke(
                layer,
                StrokeTarget::Channel { id: a },
                tool,
                brush(30.0),
                WHITE,
            )
            .unwrap_err();
        assert!(format!("{err}").contains("channel"), "{err}");
        assert!(!s.stroke_open());
    }
    assert_eq!(nodes(&s), n);
}

#[test]
fn quick_mask_enter_paint_exit_yields_the_painted_selection() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    // Enter without a selection: all selected (white); paint black to mask.
    let q = s
        .new_alpha_channel("Quick Mask".into(), true)
        .unwrap()
        .channel_id;
    stroke(&s, layer, q, StrokeTool::Brush, BLACK, 50.0, 150.0);
    // Exit: the mask becomes the selection and the channel goes away.
    s.load_selection_channel(q, SelectionOp::Replace, false)
        .unwrap();
    s.delete_document_channel(q).unwrap();
    assert!(s.document_channels().unwrap().is_empty());
    assert_eq!(sel_px(&s, 100, 150), 0.0, "painted black: masked");
    assert_eq!(sel_px(&s, 300, 150), 1.0);
    assert_eq!(sel_px(&s, 100, 20), 1.0);
}

fn round_trip(ext: &str) {
    let (d, e) = engine();
    let (s, layer) = doc(&e);
    fill(&s, layer);
    let a = s
        .new_alpha_channel("Painted".into(), false)
        .unwrap()
        .channel_id;
    stroke(&s, layer, a, StrokeTool::Brush, WHITE, 50.0, 150.0);
    let composite = export_png(&s, &d.path().join("before.png"));
    let path = d.path().join(format!("painted.{ext}"));
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    drop(s);

    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = r.document_channels().unwrap();
    assert_eq!(rows.len(), 1, "{ext}");
    assert_eq!(rows[0].name, "Painted", "{ext}");
    let id = rows[0].id;
    assert_eq!(chan_px(&r, id, 100, 150), 1.0, "{ext}");
    assert_eq!(chan_px(&r, id, 300, 150), 0.0, "{ext}");
    assert!(
        export_png(&r, &d.path().join("reopened.png")) == composite,
        "{ext}: composite changed"
    );
    // A reopened channel can be painted again.
    let layer = r.layers().unwrap()[0].id;
    stroke(&r, layer, id, StrokeTool::Brush, WHITE, 250.0, 350.0);
    assert_eq!(chan_px(&r, id, 300, 150), 1.0, "{ext}");
}

#[test]
fn painted_channel_round_trips_as_tessera_doc() {
    round_trip("tessera-doc");
}

#[test]
fn painted_channel_round_trips_as_psd() {
    round_trip("psd");
}
