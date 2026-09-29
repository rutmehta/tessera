//! Quick Mask with an active selection (WP B5-17d): entering turns the
//! selection into the Quick Mask channel and drops the selection in one
//! "Quick Mask" history node, so mask strokes are not clipped to the old
//! selection and painting white can grow it; exiting turns the mask back into
//! the selection and removes the channel in one node.
#![cfg(target_os = "macos")]

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

fn stroke(s: &DocumentSession, layer: u64, id: u64, color: PaintColor, x0: f32, x1: f32) {
    s.begin_stroke(
        layer,
        StrokeTarget::Channel { id },
        StrokeTool::Brush,
        brush(30.0),
        color,
    )
    .unwrap();
    let pts = (0..=16)
        .map(|i| StrokeSample {
            x: x0 + (x1 - x0) * i as f32 / 16.0,
            y: 150.0,
            pressure: 1.0,
            tilt_x: 0.0,
            tilt_y: 0.0,
            timestamp: 0.0,
        })
        .collect();
    s.stroke_points(pts).unwrap();
    s.end_stroke().unwrap();
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

fn has_selection(s: &DocumentSession) -> bool {
    s.document_state().unwrap().selection.is_some()
}

fn sel_px(s: &DocumentSession, x: u32, y: u32) -> f32 {
    s.document_state()
        .unwrap()
        .selection
        .as_ref()
        .map_or(1.0, |r| r.pixel(x, y)[0])
}

fn labels(s: &DocumentSession) -> Vec<String> {
    s.history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect()
}

/// The left half (x < 200) selected.
fn select_left(s: &DocumentSession) {
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
}

#[test]
fn entering_with_a_selection_moves_it_into_the_mask_in_one_node() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    select_left(&s);
    let n = labels(&s).len();
    let q = s.enter_quick_mask("Quick Mask".into()).unwrap().channel_id;
    let after = labels(&s);
    assert_eq!(after.len(), n + 1, "one node: {after:?}");
    assert_eq!(after.last().map(String::as_str), Some("Quick Mask"));
    assert!(!has_selection(&s), "the selection is dropped");
    assert_eq!(chan_px(&s, q, 100, 150), 1.0, "selected: white");
    assert_eq!(chan_px(&s, q, 300, 150), 0.0, "unselected: black");
    let rec = s.document_channels().unwrap();
    assert_eq!(rec.len(), 1);
    assert!(rec[0].visible, "the mask is shown");
    // Undo restores the selection and removes the channel in one step.
    s.undo().unwrap();
    assert!(s.document_channels().unwrap().is_empty());
    assert_eq!(sel_px(&s, 100, 150), 1.0);
    assert_eq!(sel_px(&s, 300, 150), 0.0);
}

#[test]
fn painting_white_outside_the_old_selection_grows_it_after_exit() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    select_left(&s);
    let q = s.enter_quick_mask("Quick Mask".into()).unwrap().channel_id;
    stroke(&s, layer, q, WHITE, 250.0, 350.0);
    assert_eq!(
        chan_px(&s, q, 300, 150),
        1.0,
        "not clipped to the old selection"
    );
    let n = labels(&s).len();
    s.exit_quick_mask(q).unwrap();
    let after = labels(&s);
    assert_eq!(after.len(), n + 1, "one node: {after:?}");
    assert_eq!(after.last().map(String::as_str), Some("Quick Mask"));
    assert!(
        s.document_channels().unwrap().is_empty(),
        "the channel is gone"
    );
    assert_eq!(sel_px(&s, 100, 150), 1.0, "the old selection is kept");
    assert_eq!(sel_px(&s, 300, 150), 1.0, "the painted area is added");
    assert_eq!(sel_px(&s, 300, 20), 0.0, "the rest stays unselected");
    // Undo brings the mask back and the selection stays dropped.
    s.undo().unwrap();
    assert_eq!(s.document_channels().unwrap().len(), 1);
    assert!(!has_selection(&s));
    assert_eq!(chan_px(&s, q, 300, 150), 1.0);
    s.redo().unwrap();
    assert!(s.document_channels().unwrap().is_empty());
    assert_eq!(sel_px(&s, 300, 150), 1.0);
}

#[test]
fn entering_without_a_selection_is_unchanged() {
    let (_d, e) = engine();
    let (s, layer) = doc(&e);
    let n = labels(&s).len();
    let q = s.enter_quick_mask("Quick Mask".into()).unwrap().channel_id;
    assert_eq!(labels(&s).len(), n + 1);
    assert!(!has_selection(&s));
    assert_eq!(chan_px(&s, q, 100, 150), 1.0, "all selected");
    assert_eq!(chan_px(&s, q, 300, 20), 1.0);
    stroke(&s, layer, q, BLACK, 50.0, 150.0);
    s.exit_quick_mask(q).unwrap();
    assert!(s.document_channels().unwrap().is_empty());
    assert_eq!(sel_px(&s, 100, 150), 0.0, "painted black: masked");
    assert_eq!(sel_px(&s, 300, 150), 1.0);
    assert_eq!(sel_px(&s, 100, 20), 1.0);
}

#[test]
fn exiting_an_unknown_channel_fails_without_a_node() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    select_left(&s);
    let n = labels(&s).len();
    assert!(s.exit_quick_mask(9_999).is_err());
    assert_eq!(labels(&s).len(), n);
    assert_eq!(sel_px(&s, 100, 150), 1.0);
}
