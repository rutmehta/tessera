//! Warp, Perspective Warp, Puppet Warp and Content-Aware Scale over the
//! bridge (WP B5-12): pixel-to-smart consent and cancel, source / masks /
//! styles kept once, warp presets and splits, linked perspective quads and
//! rejected geometry, puppet meshes, pins, rotation and limits,
//! content-aware amount and channel protection on the evaluation grid,
//! child coordinates, one node per apply, stale tokens, re-edit keeping the
//! stack, locks, live text inside the wrapper, native / PSD save, all
//! kernels, draft proxies and real Metal versus CPU renders.
#![cfg(target_os = "macos")]

use compositor::{
    Affine, Compositor, Document, Mask, VectorMask,
    channels::{ChannelId, ChannelKind, DocumentChannel},
    document::{DocState, Layer, LayerId, LayerKind, SmartObject},
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use serde_json::{Value, json};
use std::sync::Arc;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// Opaque detail at every scale: ramps plus a checker.
fn pattern(x: u32, y: u32, w: u32, h: u32) -> [f32; 4] {
    let checker = if (x / 6 + y / 5).is_multiple_of(2) {
        0.8
    } else {
        0.15
    };
    [x as f32 / w as f32, checker, y as f32 / h as f32, 1.0]
}

fn raster(w: u32, h: u32, f: impl Fn(u32, u32) -> [f32; 4]) -> Raster {
    let e = Extent::new(w, h);
    let mut r = Raster::new(e, 4, Depth::U8, 0.0);
    r.edit_region(compositor::Rect::of_extent(e), 1, |x, y, p| *p = f(x, y))
        .unwrap();
    r
}

fn pixel_layer(name: &str, w: u32, h: u32) -> Layer {
    Layer::new(
        name,
        LayerKind::Pixel(raster(w, h, |x, y| pattern(x, y, w, h))),
    )
}

/// A document of `layers` (bottom first), ids assigned 1…n.
fn adopt(engine: &Arc<Engine>, w: u32, h: u32, layers: Vec<Layer>) -> Arc<DocumentSession> {
    let mut s = DocState::new(Extent::new(w, h), Depth::U8);
    for (i, mut l) in layers.into_iter().enumerate() {
        l.id = LayerId(i as u64 + 1);
        s.root.push(Arc::new(l));
    }
    s.next_id = s.root.len() as u64 + 1;
    engine.adopt_document(Document::new(s), "transform".into())
}

fn history_len(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn labels(s: &DocumentSession) -> Vec<String> {
    s.history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect()
}

fn op(operation: Value, kernel: &str) -> String {
    json!({ "version": 1, "operation": operation, "kernel": kernel }).to_string()
}

fn warp_op(w: u32, h: u32, preset: &str, bend: f64) -> String {
    let mesh: Value =
        serde_json::from_str(&warp_preset(w as f64, h as f64, preset.into(), bend).unwrap())
            .unwrap();
    op(json!({ "Warp": mesh }), "Bicubic")
}

fn quad(x0: f64, y0: f64, x1: f64, y1: f64) -> Value {
    json!([[x0, y0], [x1, y0], [x1, y1], [x0, y1]])
}

fn perspective_op(src: Vec<Value>, dst: Vec<Value>) -> String {
    op(
        json!({ "Perspective": { "source_quads": src, "destination_quads": dst } }),
        "Bilinear",
    )
}

fn cas_op(w: u32, h: u32, amount: f32) -> String {
    op(
        json!({ "ContentAwareScale": { "target_width": w, "target_height": h, "amount": amount, "protect": null } }),
        "Bilinear",
    )
}

/// The live document through the CPU compositor (transform stages native).
fn cpu(s: &DocumentSession, level: u8) -> Vec<f32> {
    let state = (*s.document_state().unwrap()).clone();
    Compositor::new(64 << 20)
        .render_level_rgba(&Document::new(state), level)
        .unwrap()
        .1
}

/// The presented frame (resident renderer on Metal, bakes applied).
fn shown(s: &DocumentSession, level: u8) -> Vec<f32> {
    s.read_presented_level(level).unwrap().2
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn smart(s: &DocumentSession, id: u64) -> SmartObject {
    let state = s.document_state().unwrap();
    match &state.find(LayerId(id)).unwrap().kind {
        LayerKind::SmartObject(so) => so.clone(),
        k => panic!("layer {id} is not a smart object: {k:?}"),
    }
}

fn kind(s: &DocumentSession, id: u64) -> DocLayerKind {
    s.layer(id).unwrap().kind
}

/// Commits a converted warp and returns the smart object's id.
fn warped(s: &DocumentSession, id: u64, w: u32, h: u32) {
    let t = s
        .begin_advanced_transform(id, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.4), false)
        .unwrap();
    s.commit_advanced_transform(t.token, true).unwrap();
}

#[test]
fn b512_pixel_to_smart_needs_consent_and_cancel_creates_nothing() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    let original = cpu(&s, 0);
    let nodes = history_len(&s);

    // Cancel: the layer stays Pixel and history is untouched.
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert!(t.needs_conversion);
    assert_eq!((t.child_width, t.child_height), (w, h));
    assert_eq!(t.child_to_document.a, 1.0);
    s.preview_advanced_transform(t.token, warp_op(w, h, "Bulge", 0.5), false)
        .unwrap();
    assert_eq!(kind(&s, 1), DocLayerKind::SmartObject, "the preview wraps");
    assert!(max_diff(&cpu(&s, 0), &original) > 0.05, "the preview warps");
    s.cancel_advanced_transform(t.token).unwrap();
    assert_eq!(kind(&s, 1), DocLayerKind::Pixel);
    assert_eq!(history_len(&s), nodes);
    assert_eq!(max_diff(&cpu(&s, 0), &original), 0.0);

    // Apply without consent fails, the session stays open; with consent: one node.
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Bulge", 0.5), false)
        .unwrap();
    let e = s.commit_advanced_transform(t.token, false).unwrap_err();
    assert!(e.to_string().contains("confirm the conversion"), "{e}");
    assert!(s.advanced_transform_open());
    assert_eq!(history_len(&s), nodes);
    s.commit_advanced_transform(t.token, true).unwrap();
    assert_eq!(history_len(&s), nodes + 1);
    assert_eq!(labels(&s).last().unwrap(), "Warp");
    assert_eq!(kind(&s, 1), DocLayerKind::SmartObject);
    let so = smart(&s, 1);
    assert_eq!(so.filters.len(), 1);
    assert_eq!(so.filters[0].name, "transform");
    assert!(so.transform.m == Affine::IDENTITY.m);

    // Undo restores the exact pixel layer; redo the stage.
    s.undo().unwrap();
    assert_eq!(kind(&s, 1), DocLayerKind::Pixel);
    assert_eq!(max_diff(&cpu(&s, 0), &original), 0.0);
    s.redo().unwrap();
    assert_eq!(smart(&s, 1).filters.len(), 1);
}

#[test]
fn b512_wrapper_keeps_source_masks_and_styles_exactly_once() {
    let (_d, engine) = engine();
    let (w, h) = (64, 48);
    let mut l = pixel_layer("masked", w, h);
    l.props.opacity = 0.7;
    l.props.styles.scale = 2.0;
    let mut mask = Mask::reveal_all(Extent::new(w, h), Depth::U8);
    mask.density = 0.6;
    l.mask = Some(mask);
    l.vector_mask = Some(VectorMask {
        enabled: false,
        density: 0.25,
        ..VectorMask::default()
    });
    let s = adopt(&engine, w, h, vec![l]);
    let source = match &s.document_state().unwrap().find(LayerId(1)).unwrap().kind {
        LayerKind::Pixel(r) => r.clone(),
        _ => unreachable!(),
    };
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Perspective)
        .unwrap();
    let q = quad(0.0, 0.0, w as f64, h as f64);
    let d = json!([[4.0, 2.0], [60.0, 0.0], [64.0, 48.0], [0.0, 44.0]]);
    s.preview_advanced_transform(t.token, perspective_op(vec![q], vec![d]), false)
        .unwrap();
    s.commit_advanced_transform(t.token, true).unwrap();

    let state = s.document_state().unwrap();
    let outer = state.find(LayerId(1)).unwrap();
    assert_eq!(outer.props.name, "masked");
    assert_eq!(outer.props.opacity, 0.7);
    assert_eq!(outer.props.styles.scale, 2.0);
    assert_eq!(outer.mask.as_ref().unwrap().density, 0.6);
    assert_eq!(outer.vector_mask.as_ref().unwrap().density, 0.25);
    let LayerKind::SmartObject(so) = &outer.kind else {
        panic!("not wrapped")
    };
    assert_eq!(so.state.root.len(), 1);
    let inner = &so.state.root[0];
    assert!(inner.mask.is_none() && inner.vector_mask.is_none());
    assert_eq!(inner.props.styles.scale, 1.0);
    assert_eq!(inner.props.opacity, 1.0);
    let LayerKind::Pixel(kept) = &inner.kind else {
        panic!("source not retained as pixels")
    };
    for (x, y) in [(0, 0), (13, 7), (63, 47), (30, 20)] {
        assert_eq!(kept.pixel(x, y), source.pixel(x, y), "source pixel {x},{y}");
    }
}

#[test]
fn b512_warp_presets_bend_zero_identity_and_splits_do_not_jump() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    let original = cpu(&s, 0);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    // Every preset at bend 0 is the identity mesh; the image does not move.
    for name in warp_preset_names() {
        let zero = warp_preset(w as f64, h as f64, name.clone(), 0.0).unwrap();
        assert_eq!(
            zero,
            warp_preset(w as f64, h as f64, "Arc".into(), 0.0).unwrap()
        );
    }
    s.preview_advanced_transform(t.token, warp_op(w, h, "Wave", 0.0), false)
        .unwrap();
    let d0 = max_diff(&cpu(&s, 0), &original);
    assert!(d0 < 2e-3, "bend 0 is identity: {d0}");
    assert!(warp_preset(10.0, 10.0, "Arc".into(), 1.5).is_err());
    assert!(warp_preset(10.0, 10.0, "Spiral".into(), 0.5).is_err());

    // A bent warp, then splits through a point: no jump in the render.
    let mesh = warp_preset(w as f64, h as f64, "Flag".into(), 0.6).unwrap();
    let full = op(
        json!({ "Warp": serde_json::from_str::<Value>(&mesh).unwrap() }),
        "Bicubic",
    );
    s.preview_advanced_transform(t.token, full, false).unwrap();
    let before = cpu(&s, 0);
    assert!(max_diff(&before, &original) > 0.05);
    let split = warp_split(mesh.clone(), 40.0, 30.0, true, true).unwrap();
    let parsed: Value = serde_json::from_str(&split).unwrap();
    assert_eq!(parsed["u_splits"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["v_splits"].as_array().unwrap().len(), 3);
    assert_eq!(parsed["control_points"].as_array().unwrap().len(), 7);
    s.preview_advanced_transform(t.token, op(json!({ "Warp": parsed }), "Bicubic"), false)
        .unwrap();
    let after = cpu(&s, 0);
    let jump = max_diff(&after, &before);
    assert!(jump < 0.03, "split changed the warp by {jump}");
    assert!(warp_split(mesh.clone(), -500.0, -500.0, true, false).is_err());
    let grid = warp_subdivide(mesh, 3, 3).unwrap();
    let g: Value = serde_json::from_str(&grid).unwrap();
    assert_eq!(g["control_points"].as_array().unwrap().len(), 10);
    s.cancel_advanced_transform(t.token).unwrap();
}

#[test]
fn b512_linked_perspective_quads_share_edges_and_invalid_quads_keep_preview() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Perspective)
        .unwrap();
    // Two planes sharing the x = 48 edge; the shared edge moves in both.
    let src = vec![quad(0.0, 0.0, 48.0, 64.0), quad(48.0, 0.0, 96.0, 64.0)];
    let dst = vec![
        json!([[0.0, 4.0], [52.0, 0.0], [50.0, 64.0], [0.0, 60.0]]),
        json!([[52.0, 0.0], [96.0, 6.0], [96.0, 58.0], [50.0, 64.0]]),
    ];
    s.preview_advanced_transform(t.token, perspective_op(src.clone(), dst), false)
        .unwrap();
    let good = cpu(&s, 0);
    // No crack along the shared edge: every row is covered at the seam.
    for y in 4..60u32 {
        let a = good[((y * w + 51) * 4 + 3) as usize];
        assert!(a > 0.99, "crack at row {y}: alpha {a}");
    }
    // A self-crossing quad and a degenerate quad fail; the preview stays.
    let crossing = vec![
        json!([[0.0, 0.0], [48.0, 64.0], [48.0, 0.0], [0.0, 64.0]]),
        quad(48.0, 0.0, 96.0, 64.0),
    ];
    assert!(
        s.preview_advanced_transform(t.token, perspective_op(src.clone(), crossing), false)
            .is_err()
    );
    let degenerate = vec![
        json!([[0.0, 0.0], [48.0, 0.0], [48.0, 0.0], [0.0, 64.0]]),
        quad(48.0, 0.0, 96.0, 64.0),
    ];
    assert!(
        s.preview_advanced_transform(t.token, perspective_op(src.clone(), degenerate), false)
            .is_err()
    );
    // A vertex that is shared in the source but split in the destination.
    let cracked = vec![
        json!([[0.0, 4.0], [52.0, 0.0], [50.0, 64.0], [0.0, 60.0]]),
        json!([[55.0, 0.0], [96.0, 6.0], [96.0, 58.0], [50.0, 64.0]]),
    ];
    assert!(
        s.preview_advanced_transform(t.token, perspective_op(src, cracked), false)
            .is_err()
    );
    assert_eq!(max_diff(&cpu(&s, 0), &good), 0.0, "previous preview intact");
    s.commit_advanced_transform(t.token, true).unwrap();
    assert_eq!(labels(&s).last().unwrap(), "Perspective Warp");
}

/// An opaque 40 × 24 bar in a transparent 96 × 64 layer.
fn bar_layer(w: u32, h: u32) -> Layer {
    Layer::new(
        "bar",
        LayerKind::Pixel(raster(w, h, |x, y| {
            if (28..68).contains(&x) && (20..44).contains(&y) {
                pattern(x, y, w, h)
            } else {
                [0.0; 4]
            }
        })),
    )
}

fn puppet_with_pins(mesh: &str, pins: Value) -> String {
    let mut m: Value = serde_json::from_str(mesh).unwrap();
    m["pins"] = pins;
    op(json!({ "Puppet": m }), "Bilinear")
}

fn nearest(mesh: &Value, x: f64, y: f64) -> usize {
    mesh["rest_vertices"]
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .min_by(|a, b| {
            let d = |v: &Value| (v[0].as_f64().unwrap() - x).hypot(v[1].as_f64().unwrap() - y);
            d(a.1).total_cmp(&d(b.1))
        })
        .unwrap()
        .0
}

#[test]
fn b512_puppet_mesh_pins_rotation_and_honest_limits() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![bar_layer(w, h)]);
    let m = s.puppet_mesh_from_layer(1, "Normal".into(), 2).unwrap();
    assert_eq!(m.level, 0);
    assert_eq!(m.cell_px, 4);
    assert!(m.vertex_count > 20 && m.triangle_count > 20);
    let mesh: Value = serde_json::from_str(&m.mesh_json).unwrap();
    for v in mesh["rest_vertices"].as_array().unwrap() {
        let (x, y) = (v[0].as_f64().unwrap(), v[1].as_f64().unwrap());
        assert!(
            (24.0..=72.0).contains(&x) && (16.0..=48.0).contains(&y),
            "{x},{y}"
        );
    }
    // Densities differ; invalid values are errors, not clamps.
    let sparse = s.puppet_mesh_from_layer(1, "Sparse".into(), 0).unwrap();
    let dense = s.puppet_mesh_from_layer(1, "Dense".into(), 0).unwrap();
    assert!(sparse.vertex_count < m.vertex_count && m.vertex_count < dense.vertex_count);
    assert!(s.puppet_mesh_from_layer(1, "Medium".into(), 0).is_err());
    let e = s
        .puppet_mesh_from_layer(1, "Normal".into(), 65)
        .unwrap_err();
    assert!(e.to_string().contains("0…64"), "{e}");

    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Puppet)
        .unwrap();
    let left = nearest(&mesh, 28.0, 32.0);
    let right = nearest(&mesh, 68.0, 32.0);
    // Add two pins, move one: the solved mesh follows.
    let p = s
        .preview_advanced_transform(
            t.token,
            puppet_with_pins(
                &m.mesh_json,
                json!([{ "vertex": left, "target": mesh["rest_vertices"][left], "rotation": null },
                       { "vertex": right, "target": [68.0, 12.0], "rotation": null }]),
            ),
            false,
        )
        .unwrap();
    let solved: Vec<[f64; 2]> = serde_json::from_str(&p.deformed_json.unwrap()).unwrap();
    assert!(
        (solved[right][1] - 12.0).abs() < 1e-6,
        "pin target honoured"
    );
    let moved = cpu(&s, 0);
    // Rotate the fixed pin: a different solution.
    let p2 = s
        .preview_advanced_transform(
            t.token,
            puppet_with_pins(
                &m.mesh_json,
                json!([{ "vertex": left, "target": mesh["rest_vertices"][left], "rotation": 0.6 },
                       { "vertex": right, "target": [68.0, 12.0], "rotation": null }]),
            ),
            false,
        )
        .unwrap();
    let rotated: Vec<[f64; 2]> = serde_json::from_str(&p2.deformed_json.unwrap()).unwrap();
    assert!(
        solved
            .iter()
            .zip(&rotated)
            .any(|(a, b)| (a[1] - b[1]).abs() > 0.05)
    );
    // Rigid mode, then invalid pins / iterations are rejected with the preview kept.
    let mut rigid: Value = serde_json::from_str(&m.mesh_json).unwrap();
    rigid["mode"] = json!("Rigid");
    rigid["pins"] = json!([{ "vertex": right, "target": [68.0, 12.0], "rotation": null }]);
    s.preview_advanced_transform(t.token, op(json!({ "Puppet": rigid }), "Bilinear"), false)
        .unwrap();
    let rigid_px = cpu(&s, 0);
    assert!(max_diff(&rigid_px, &moved) > 1e-3);
    let dup = puppet_with_pins(
        &m.mesh_json,
        json!([{ "vertex": right, "target": [1.0, 1.0], "rotation": null },
               { "vertex": right, "target": [2.0, 2.0], "rotation": null }]),
    );
    assert!(s.preview_advanced_transform(t.token, dup, false).is_err());
    let mut bad: Value = serde_json::from_str(&m.mesh_json).unwrap();
    bad["iterations"] = json!(101);
    assert!(
        s.preview_advanced_transform(t.token, op(json!({ "Puppet": bad }), "Bilinear"), false)
            .is_err()
    );
    // Deleting all pins restores the rest pose.
    s.preview_advanced_transform(t.token, puppet_with_pins(&m.mesh_json, json!([])), false)
        .unwrap();
    assert_eq!(max_diff(&cpu(&s, 0), &cpu_rest(&s)), 0.0);
    assert!(max_diff(&rigid_px, &cpu(&s, 0)) > 1e-3);
    s.cancel_advanced_transform(t.token).unwrap();

    // A large opaque source is meshed from a coarser level, and says so.
    let (bw, bh) = (1024, 1024);
    let big = adopt(&engine, bw, bh, vec![pixel_layer("big", bw, bh)]);
    let coarse = big.puppet_mesh_from_layer(1, "Dense".into(), 0).unwrap();
    assert!(coarse.level > 0 && coarse.note.is_some());
    assert!(coarse.vertex_count <= 16_384);
}

/// The live document with the preview removed (the committed pixels).
fn cpu_rest(s: &DocumentSession) -> Vec<f32> {
    let mut state = (*s.document_state().unwrap()).clone();
    if let LayerKind::SmartObject(so) = &mut Arc::make_mut(&mut state.root[0]).kind {
        so.filters.clear();
    }
    Compositor::new(64 << 20)
        .render_level_rgba(&Document::new(state), 0)
        .unwrap()
        .1
}

fn channel(w: u32, h: u32, f: impl Fn(u32, u32) -> f32) -> DocumentChannel {
    let e = Extent::new(w, h);
    let mut r = Raster::new(e, 1, Depth::U8, 0.0);
    r.edit_region(compositor::Rect::of_extent(e), 1, |x, y, p| p[0] = f(x, y))
        .unwrap();
    DocumentChannel {
        id: ChannelId(7),
        name: "Protect".into(),
        kind: ChannelKind::Alpha,
        raster: r,
    }
}

#[test]
fn b512_content_aware_amount_and_saved_channel_protection() {
    let (_d, engine) = engine();
    let (w, h) = (80, 48);
    let mut state = DocState::new(Extent::new(w, h), Depth::U8);
    let mut l = pixel_layer("photo", w, h);
    l.id = LayerId(1);
    state.root.push(Arc::new(l));
    state.next_id = 2;
    state
        .channels
        .push(channel(w, h, |x, _| if x < 20 { 1.0 } else { 0.0 }));
    let s = engine.adopt_document(Document::new(state), "cas".into());
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::ContentAwareScale)
        .unwrap();
    assert!(
        t.limitations
            .iter()
            .any(|l| l.contains("no automatic skin detection"))
    );
    s.preview_advanced_transform(t.token, cas_op(56, 48, 0.0), false)
        .unwrap();
    let resized = shown(&s, 0);
    s.preview_advanced_transform(t.token, cas_op(56, 48, 1.0), false)
        .unwrap();
    let carved = shown(&s, 0);
    assert!(max_diff(&resized, &carved) > 0.05, "amount 0 vs 1");
    // Output is padded at the origin inside the fixed canvas.
    assert_eq!(carved[((10 * w + 70) * 4 + 3) as usize], 0.0);
    // Protection from the saved channel, sampled onto the child grid.
    s.content_aware_scale_from_channel(t.token, 56, 48, 1.0, Some(7), false)
        .unwrap();
    let protected = shown(&s, 0);
    assert!(
        max_diff(&protected, &carved) > 1e-3,
        "protection changes the carve"
    );
    assert!(
        s.content_aware_scale_from_channel(t.token, 56, 48, 1.0, Some(99), false)
            .is_err()
    );
    s.commit_advanced_transform(t.token, true).unwrap();
    let st = s.transform_stage(1, 0).unwrap();
    assert_eq!(st.kind, AdvancedTransformKind::ContentAwareScale);
    assert!(st.has_protection);
    assert!(
        st.transform_json.contains("\"protect\":null"),
        "protect stripped"
    );
    let so = smart(&s, 1);
    let protect = so.filters[0].params["operation"]["ContentAwareScale"]["protect"]
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(protect.len(), (w * h) as usize);
    assert_eq!(protect[5].as_f64().unwrap(), 1.0);
    assert_eq!(protect[30].as_f64().unwrap(), 0.0);
    // The committed CAS stack is baked through the compositor: matches CPU.
    let d = max_diff(&shown(&s, 0), &cpu(&s, 0));
    assert!(d <= 2.0 / 255.0, "CAS bake vs CPU compositor {d}");
}

#[test]
fn b512_child_coordinates_follow_placement_and_draft_protect_matches_mip() {
    let (_d, engine) = engine();
    // A 2400 × 2000 child placed at (40, 30) and scaled by 0.5 in a 1200 × 900 document.
    let (cw, ch) = (2400u32, 2000u32);
    let mut child = DocState::new(Extent::new(cw, ch), Depth::U8);
    let mut inner = pixel_layer("inner", cw, ch);
    inner.id = LayerId(1);
    child.root.push(Arc::new(inner));
    child.next_id = 2;
    let placement = Affine {
        m: [0.5, 0.0, 40.0, 0.0, 0.5, 30.0],
    };
    let mut state = DocState::new(Extent::new(1200, 900), Depth::U8);
    let mut so = Layer::new(
        "placed",
        LayerKind::SmartObject(SmartObject::new(child, placement)),
    );
    so.id = LayerId(1);
    state.root.push(Arc::new(so));
    state.next_id = 2;
    // Protect the document's left half.
    state
        .channels
        .push(channel(1200, 900, |x, _| if x < 600 { 1.0 } else { 0.0 }));
    let s = engine.adopt_document(Document::new(state), "placed".into());
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::ContentAwareScale)
        .unwrap();
    assert!(!t.needs_conversion);
    assert_eq!((t.child_width, t.child_height), (cw, ch));
    let m = t.child_to_document;
    assert_eq!(
        (m.a, m.b, m.c, m.d, m.e, m.f),
        (0.5, 0.0, 40.0, 0.0, 0.5, 30.0)
    );
    assert!(t.draft_level >= 1, "a 4.8 MP child gets a draft proxy");
    // Draft: the channel is sampled on the proxy grid (child mip), through the placement.
    s.content_aware_scale_from_channel(t.token, 2200, 2000, 1.0, Some(7), true)
        .unwrap();
    let state = s.document_state().unwrap();
    let LayerKind::SmartObject(proxy) = &state.find(LayerId(1)).unwrap().kind else {
        panic!()
    };
    let e = proxy.state.canvas;
    let scale = 1u32 << t.draft_level;
    assert_eq!(e, Extent::new(cw, ch).at_level(t.draft_level));
    let p = proxy.filters[0].params["operation"]["ContentAwareScale"]["protect"]
        .as_array()
        .unwrap();
    assert_eq!(p.len(), (e.width * e.height) as usize);
    // Child x maps to document 40 + 0.5·x: protected where that is < 600.
    let boundary = ((600.0 - 40.0) / 0.5 / scale as f64) as u32;
    assert_eq!(p[(boundary - 2) as usize].as_f64().unwrap(), 1.0);
    assert_eq!(p[(boundary + 2) as usize].as_f64().unwrap(), 0.0);
    assert_eq!(
        proxy.transform.m,
        [0.5 * scale as f64, 0.0, 40.0, 0.0, 0.5 * scale as f64, 30.0]
    );
    // The full grid exceeds this build's inline protect limit: honest error at commit.
    let e = s.commit_advanced_transform(t.token, false).unwrap_err();
    assert!(e.to_string().contains("MP"), "{e}");
    s.cancel_advanced_transform(t.token).unwrap();
}

#[test]
fn b512_apply_is_one_node_with_exact_undo_redo() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    s.convert_for_smart_filters(1).unwrap();
    let base = cpu(&s, 0);
    let nodes = history_len(&s);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert!(!t.needs_conversion);
    for bend in [0.1, 0.2, 0.3, 0.4, 0.5] {
        s.preview_advanced_transform(t.token, warp_op(w, h, "Arch", bend), false)
            .unwrap();
    }
    assert_eq!(history_len(&s), nodes, "previews record nothing");
    s.commit_advanced_transform(t.token, false).unwrap();
    assert_eq!(history_len(&s), nodes + 1);
    let applied = cpu(&s, 0);
    assert!(max_diff(&applied, &base) > 0.05);
    s.undo().unwrap();
    assert_eq!(max_diff(&cpu(&s, 0), &base), 0.0);
    assert!(smart(&s, 1).filters.is_empty());
    s.redo().unwrap();
    assert_eq!(max_diff(&cpu(&s, 0), &applied), 0.0);
    // Nothing previewed: the session ends with no node.
    let t = s
        .begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
        .unwrap();
    s.commit_advanced_transform(t.token, false).unwrap();
    assert_eq!(history_len(&s), nodes + 1);
}

#[test]
fn b512_stale_tokens_changed_layers_and_switching_never_mutate() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    s.convert_for_smart_filters(1).unwrap();
    let nodes = history_len(&s);
    let a = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(a.token, warp_op(w, h, "Arc", 0.5), false)
        .unwrap();
    // A new session replaces the old one: its token is stale, its preview gone.
    let b = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert!(smart(&s, 1).filters.is_empty(), "old preview dropped");
    for r in [
        s.preview_advanced_transform(a.token, warp_op(w, h, "Arc", 0.2), false)
            .map(|_| ()),
        s.commit_advanced_transform(a.token, false).map(|_| ()),
        s.cancel_advanced_transform(a.token).map(|_| ()),
    ] {
        assert!(r.unwrap_err().to_string().contains("stale"));
    }
    assert_eq!(history_len(&s), nodes);
    // The layer changes underneath (undo of the conversion): rejected.
    s.preview_advanced_transform(b.token, warp_op(w, h, "Arc", 0.5), false)
        .unwrap();
    s.undo().unwrap();
    let e = s
        .preview_advanced_transform(b.token, warp_op(w, h, "Arc", 0.1), false)
        .unwrap_err();
    assert!(e.to_string().contains("changed"), "{e}");
    assert!(s.commit_advanced_transform(b.token, true).is_err());
    assert_eq!(kind(&s, 1), DocLayerKind::Pixel);
    s.cancel_advanced_transform(b.token).unwrap();
    // After commit or cancel the token is dead.
    assert!(s.cancel_advanced_transform(b.token).is_err());

    // Another document's session is independent; closing drops the preview.
    let other = adopt(&engine, w, h, vec![pixel_layer("other", w, h)]);
    let c = other
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    other
        .preview_advanced_transform(c.token, warp_op(w, h, "Arc", 0.5), false)
        .unwrap();
    let before = history_len(&s);
    assert!(
        s.preview_advanced_transform(c.token, warp_op(w, h, "Arc", 0.5), false)
            .is_err()
    );
    other.close();
    assert!(other.commit_advanced_transform(c.token, true).is_err());
    assert_eq!(history_len(&s), before);
}

#[test]
fn b512_reedit_keeps_neighbours_order_blending_and_enabled_state() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    s.convert_for_smart_filters(1).unwrap();
    s.apply_filter(1, r#"{"id":"gaussian_blur","params":{"radius":2}}"#.into())
        .unwrap();
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert_eq!(t.insert_index, 1);
    assert_eq!(t.other_stages, 1);
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.3), false)
        .unwrap();
    s.commit_advanced_transform(t.token, false).unwrap();
    s.apply_filter(1, r#"{"id":"add_noise","params":{"amount":5}}"#.into())
        .unwrap();
    s.set_smart_filter(
        1,
        1,
        SmartFilterEdit::Blending {
            mode: "multiply".into(),
            opacity: 0.75,
        },
    )
    .unwrap();
    let list = s.smart_filters(1).unwrap();
    assert_eq!(
        list.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["Gaussian Blur", "Warp", "Add Noise"]
    );
    let stage = smart(&s, 1).filters[1].clone();
    // Editing the noise filter leaves the transform stage verbatim.
    s.set_smart_filter(
        1,
        2,
        SmartFilterEdit::Params {
            filter_json: r#"{"id":"add_noise","params":{"amount":9}}"#.into(),
        },
    )
    .unwrap();
    assert_eq!(smart(&s, 1).filters[1], stage);
    // The transform row cannot be re-parsed as a menu filter.
    assert!(
        s.set_smart_filter(
            1,
            1,
            SmartFilterEdit::Params {
                filter_json: r#"{"id":"gaussian_blur","params":{"radius":1}}"#.into()
            }
        )
        .is_err()
    );
    // Disable, then re-edit: SetTransform keeps enabled/blend/order.
    s.set_smart_filter(1, 1, SmartFilterEdit::Enabled { enabled: false })
        .unwrap();
    let t = s
        .begin_advanced_transform(1, Some(1), AdvancedTransformKind::Warp)
        .unwrap();
    assert_eq!(t.editing_index, Some(1));
    let existing = t.existing.unwrap();
    assert_eq!(
        (
            existing.enabled,
            existing.blend_mode.as_str(),
            existing.opacity
        ),
        (false, "multiply", 0.75)
    );
    assert!(
        s.begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
            .is_err()
    );
    let t = s
        .begin_advanced_transform(1, Some(1), AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Twist", 0.3), false)
        .unwrap();
    s.commit_advanced_transform(t.token, false).unwrap();
    let filters = smart(&s, 1).filters;
    assert_eq!(
        filters.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["gaussian_blur", "transform", "add_noise"]
    );
    assert!(!filters[1].enabled);
    assert_eq!(filters[1].blend.opacity, 0.75);
    assert_ne!(filters[1].params, stage.params);
    let list = s.smart_filters(1).unwrap();
    assert_eq!(
        list[2].filter_json,
        r#"{"id":"add_noise","params":{"amount":9}}"#
    );
    // Mixed stacks bake through the compositor (menu filters bridged).
    s.set_smart_filter(1, 1, SmartFilterEdit::Enabled { enabled: true })
        .unwrap();
    let (_, _, px) = s.read_presented_level(0).unwrap();
    assert!(px.iter().any(|v| *v > 0.0));
    assert_eq!(labels(&s).last().unwrap(), "Enable Smart Filter");
}

#[test]
fn b512_locks_reject_geometry_while_color_filters_keep_their_rules() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    warped(&s, 1, w, h);
    let nodes = history_len(&s);
    let lock = |position: bool, all: bool, pixels: bool| LayerLocks {
        pixels,
        position,
        transparency: false,
        all,
    };
    s.set_locks(1, lock(true, false, false)).unwrap();
    let e = s
        .begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
        .unwrap_err();
    assert!(e.to_string().contains("locked"), "{e}");
    // A colour filter may still be added under a position lock …
    s.apply_filter(1, r#"{"id":"add_noise","params":{"amount":5}}"#.into())
        .unwrap();
    // … but removing or disabling the transform stage is geometry.
    assert!(s.remove_smart_filter(1, 0).is_err());
    assert!(
        s.set_smart_filter(1, 0, SmartFilterEdit::Enabled { enabled: false })
            .is_err()
    );
    s.remove_smart_filter(1, 1).unwrap();
    assert_eq!(history_len(&s), nodes + 3);
    s.set_locks(1, lock(false, true, false)).unwrap();
    assert!(
        s.begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
            .is_err()
    );
    // A pixel-locked pixel layer cannot be converted.
    let p = adopt(&engine, w, h, vec![pixel_layer("pixels", w, h)]);
    p.set_locks(1, lock(false, false, true)).unwrap();
    let e = p
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap_err();
    assert!(e.to_string().contains("locked pixels"), "{e}");
    // Adjustment layers have no pixels to transform.
    let a = engine
        .clone()
        .new_document(w, h, DocDepth::U8, None)
        .unwrap();
    a.add_layer(
        NewLayer::Adjustment {
            json: r#"{"kind":"exposure","exposure":1,"offset":0,"gamma":1}"#.into(),
        },
        "exposure".into(),
        None,
        None,
    )
    .unwrap();
    let id = a.layers().unwrap()[0].id;
    assert!(
        a.begin_advanced_transform(id, None, AdvancedTransformKind::Warp)
            .is_err()
    );
}

#[test]
fn b512_text_stays_live_inside_the_explicit_wrapper() {
    let (_d, engine) = engine();
    let (w, h) = (160, 80);
    let s = engine
        .clone()
        .new_document(w, h, DocDepth::U8, None)
        .unwrap();
    let m = typography::TextModel {
        runs: vec![typography::TextRun {
            text: "Warp".into(),
            family: "Helvetica".into(),
            size: 40.0,
            ..typography::TextRun::default()
        }],
        ..typography::TextModel::default()
    };
    s.add_text_layer(
        "title".into(),
        None,
        None,
        serde_json::to_string(&m).unwrap(),
        TransformMatrix {
            a: 1.0,
            b: 0.0,
            c: 10.0,
            d: 0.0,
            e: 1.0,
            f: 50.0,
        },
        false,
    )
    .unwrap();
    let id = s.layers().unwrap()[0].id;
    assert_eq!(kind(&s, id), DocLayerKind::Text);
    // A basic affine edit stays a live text edit (no conversion).
    let t = s
        .begin_advanced_transform(id, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert!(t.needs_conversion);
    assert!(t.limitations.iter().any(|l| l.contains("stays editable")));
    s.preview_advanced_transform(t.token, warp_op(w, h, "Flag", 0.4), false)
        .unwrap();
    assert!(s.commit_advanced_transform(t.token, false).is_err());
    assert_eq!(kind(&s, id), DocLayerKind::SmartObject, "still a preview");
    s.cancel_advanced_transform(t.token).unwrap();
    assert_eq!(kind(&s, id), DocLayerKind::Text, "never silently flattened");
    let t = s
        .begin_advanced_transform(id, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Flag", 0.4), false)
        .unwrap();
    s.commit_advanced_transform(t.token, true).unwrap();
    let so = smart(&s, id);
    assert!(matches!(so.state.root[0].kind, LayerKind::Text { .. }));
}

#[test]
fn b512_native_reopen_keeps_stage_masks_and_source_and_psd_is_explicit() {
    let (dir, engine) = engine();
    let (w, h) = (96, 64);
    let mut l = pixel_layer("photo", w, h);
    let mut mask = Mask::reveal_all(Extent::new(w, h), Depth::U8);
    mask.density = 0.5;
    l.mask = Some(mask);
    let s = adopt(&engine, w, h, vec![l]);
    warped(&s, 1, w, h);
    let rendered = cpu(&s, 0);
    let native = dir.path().join("warp.tessera-doc");
    s.save_as(native.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = engine
        .clone()
        .open_document(native.to_string_lossy().into_owned())
        .unwrap();
    let stages = r.transform_stages(1).unwrap();
    assert_eq!(stages.len(), 1);
    assert_eq!(stages[0].kind, AdvancedTransformKind::Warp);
    let state = r.document_state().unwrap();
    let l = state.find(LayerId(1)).unwrap();
    assert_eq!(l.mask.as_ref().unwrap().density, 0.5);
    let LayerKind::SmartObject(so) = &l.kind else {
        panic!()
    };
    assert!(matches!(so.state.root[0].kind, LayerKind::Pixel(_)));
    assert_eq!(max_diff(&cpu(&r, 0), &rendered), 0.0);
    // Re-edit after reopen.
    let t = r
        .begin_advanced_transform(1, Some(0), AdvancedTransformKind::Warp)
        .unwrap();
    r.preview_advanced_transform(t.token, warp_op(w, h, "Wave", 0.2), false)
        .unwrap();
    r.commit_advanced_transform(t.token, false).unwrap();

    // PSD refuses the nonlinear stage; the explicit rasterized copy works.
    let psd = dir.path().join("warp.psd");
    let e = r
        .save_as(psd.to_string_lossy().into_owned())
        .unwrap_err()
        .to_string();
    assert!(e.to_lowercase().contains("rasteriz"), "{e}");
    assert!(!psd.exists());
    let copy = dir.path().join("copy.psd");
    r.save_psd_rasterizing_transforms(copy.to_string_lossy().into_owned())
        .unwrap();
    assert!(
        r.save_psd_rasterizing_transforms(native.to_string_lossy().into_owned())
            .is_err()
    );
    assert!(
        r.info()
            .unwrap()
            .path
            .is_some_and(|p| p.ends_with("warp.tessera-doc")),
        "the session keeps its native path"
    );
    let reopened = engine
        .clone()
        .open_document(copy.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(
        kind(&reopened, reopened.layers().unwrap()[0].id),
        DocLayerKind::Pixel
    );
    // Bicubic lobes may overshoot [0, 1] live; stored pixels are clamped.
    let clamp = |v: Vec<f32>| v.into_iter().map(|x| x.clamp(0.0, 1.0)).collect::<Vec<_>>();
    let d = max_diff(&clamp(cpu(&reopened, 0)), &clamp(cpu(&r, 0)));
    assert!(d <= 2.0 / 255.0, "rasterized copy {d}");
}

#[test]
fn b512_all_interpolation_kernels_render_and_differ() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(&engine, w, h, vec![pixel_layer("photo", w, h)]);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Perspective)
        .unwrap();
    let src = vec![quad(0.0, 0.0, 96.0, 64.0)];
    let dst = vec![json!([[3.3, 1.7], [93.1, 4.2], [95.0, 62.4], [1.2, 60.9]])];
    let mut outputs = Vec::new();
    for k in ["Nearest", "Bilinear", "Bicubic", "Lanczos3", "Automatic"] {
        let json = op(
            json!({ "Perspective": { "source_quads": src, "destination_quads": dst } }),
            k,
        );
        s.preview_advanced_transform(t.token, json, false).unwrap();
        outputs.push((k, cpu(&s, 0)));
    }
    assert!(
        max_diff(&outputs[0].1, &outputs[1].1) > 1e-3,
        "nearest vs bilinear"
    );
    assert!(
        max_diff(&outputs[1].1, &outputs[2].1) > 1e-4,
        "bilinear vs bicubic"
    );
    assert!(
        max_diff(&outputs[2].1, &outputs[3].1) > 1e-5,
        "bicubic vs lanczos"
    );
    assert_eq!(
        max_diff(&outputs[2].1, &outputs[4].1),
        0.0,
        "automatic = bicubic here"
    );
    assert!(
        s.preview_advanced_transform(
            t.token,
            op(
                json!({ "Perspective": { "source_quads": src, "destination_quads": dst } }),
                "Mitchell"
            ),
            false
        )
        .is_err()
    );
    s.commit_advanced_transform(t.token, true).unwrap();
    let stage: Value =
        serde_json::from_str(&s.transform_stage(1, 0).unwrap().transform_json).unwrap();
    assert_eq!(stage["kernel"], "Automatic");
}

#[test]
fn b512_metal_matches_cpu_for_every_geometric_stage() {
    let (_d, engine) = engine();
    let (w, h) = (128, 96);
    let s = adopt(
        &engine,
        w,
        h,
        vec![bar_layer(w, h), pixel_layer("top", w, h)],
    );
    assert!(
        s.info().unwrap().backend.contains("Metal"),
        "this test needs the resident Metal renderer: {}",
        s.info().unwrap().backend
    );
    // Warp on the top layer, perspective and puppet on the bar underneath.
    let t = s
        .begin_advanced_transform(2, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Fisheye", 0.4), false)
        .unwrap();
    let d = max_diff(&shown(&s, 0), &cpu(&s, 0));
    assert!(d <= 1.0 / 255.0, "warp preview Metal vs CPU {d}");
    s.commit_advanced_transform(t.token, true).unwrap();
    s.set_opacity(2, 0.5, false).unwrap();
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Perspective)
        .unwrap();
    let json = perspective_op(
        vec![quad(0.0, 0.0, 128.0, 96.0)],
        vec![json!([
            [6.0, 3.0],
            [120.0, 0.0],
            [128.0, 96.0],
            [0.0, 90.0]
        ])],
    );
    s.preview_advanced_transform(t.token, json, false).unwrap();
    s.commit_advanced_transform(t.token, true).unwrap();
    let mesh = s.puppet_mesh_from_layer(1, "Normal".into(), 4).unwrap();
    let m: Value = serde_json::from_str(&mesh.mesh_json).unwrap();
    let a = nearest(&m, 30.0, 40.0);
    let b = nearest(&m, 90.0, 40.0);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Puppet)
        .unwrap();
    assert_eq!(t.insert_index, 1);
    s.preview_advanced_transform(
        t.token,
        puppet_with_pins(
            &mesh.mesh_json,
            json!([{ "vertex": a, "target": m["rest_vertices"][a], "rotation": null },
                   { "vertex": b, "target": [96.0, 60.0], "rotation": null }]),
        ),
        false,
    )
    .unwrap();
    s.commit_advanced_transform(t.token, false).unwrap();
    for level in [0u8, 1] {
        let d = max_diff(&shown(&s, level), &cpu(&s, level));
        assert!(d <= 1.0 / 255.0, "stack Metal vs CPU at level {level}: {d}");
    }
}

#[test]
fn b512_unrelated_edits_drop_a_preview_without_recording_it() {
    let (_d, engine) = engine();
    let (w, h) = (96, 64);
    let s = adopt(
        &engine,
        w,
        h,
        vec![pixel_layer("a", w, h), pixel_layer("b", w, h)],
    );
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.5), false)
        .unwrap();
    let nodes = history_len(&s);
    s.set_opacity(2, 0.4, false).unwrap();
    assert_eq!(history_len(&s), nodes + 1);
    assert_ne!(labels(&s).last().unwrap(), "Warp");
    assert_eq!(
        kind(&s, 1),
        DocLayerKind::Pixel,
        "the preview was not flushed into history"
    );
    // The session continues from the new base.
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.5), false)
        .unwrap();
    s.commit_advanced_transform(t.token, true).unwrap();
    assert_eq!(history_len(&s), nodes + 2);
    assert_eq!(s.layer(2).unwrap().opacity, 0.4);
}

#[test]
fn b512_draft_proxy_previews_then_exact_commit() {
    let (_d, engine) = engine();
    let (w, h) = (2400, 1600);
    let s = adopt(&engine, w, h, vec![pixel_layer("large", w, h)]);
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    assert_eq!(t.draft_level, 1);
    assert!(t.limitations.iter().any(|l| l.contains("1/2 resolution")));
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.3), true)
        .unwrap();
    let draft = smart(&s, 1);
    assert_eq!(draft.state.canvas, Extent::new(1200, 800));
    let mesh = &draft.filters[0].params["operation"]["Warp"];
    assert_eq!(mesh["width"].as_f64().unwrap(), 1200.0);
    let draft_px = cpu(&s, 2);
    s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", 0.3), false)
        .unwrap();
    let exact = smart(&s, 1);
    assert_eq!(exact.state.canvas, Extent::new(w, h));
    let exact_px = cpu(&s, 2);
    let mean = exact_px
        .iter()
        .zip(&draft_px)
        .map(|(a, b)| (a - b).abs() as f64)
        .sum::<f64>()
        / exact_px.len() as f64;
    assert!(
        mean < 0.02,
        "draft approximates the exact render at level 2: mean {mean}"
    );
    s.commit_advanced_transform(t.token, true).unwrap();
    let committed = smart(&s, 1);
    assert_eq!(committed.state.canvas, Extent::new(w, h));
    assert_eq!(
        committed.filters[0].params["operation"]["Warp"]["width"]
            .as_f64()
            .unwrap(),
        w as f64
    );
}

/// Drag latency on a 20 MP smart object: preview call → presented level
/// read back (draft proxy, then the final full-resolution preview).
#[test]
#[ignore]
fn bench_warp_drag_20mp() {
    let (_d, engine) = engine();
    let (w, h) = (5472u32, 3648u32);
    let s = adopt(&engine, w, h, vec![pixel_layer("20mp", w, h)]);
    s.convert_for_smart_filters(1).unwrap();
    let t0 = std::time::Instant::now();
    let t = s
        .begin_advanced_transform(1, None, AdvancedTransformKind::Warp)
        .unwrap();
    eprintln!(
        "begin (proxy level {}): {:.1} ms",
        t.draft_level,
        t0.elapsed().as_secs_f64() * 1e3
    );
    for (draft, level) in [(true, 2u8), (true, 3), (false, 2)] {
        let mut times = Vec::new();
        for i in 0..8 {
            let bend = 0.3 + 0.005 * i as f64;
            let start = std::time::Instant::now();
            s.preview_advanced_transform(t.token, warp_op(w, h, "Arc", bend), draft)
                .unwrap();
            let call = start.elapsed().as_secs_f64() * 1e3;
            let _ = s.read_presented_level(level).unwrap();
            times.push(start.elapsed().as_secs_f64() * 1e3);
            eprintln!(
                "  call {call:.1} ms, total {:.1} ms",
                times[times.len() - 1]
            );
        }
        times.sort_by(f64::total_cmp);
        eprintln!(
            "warp preview draft={draft} read level {level}: median {:.1} ms, max {:.1} ms ({times:?})",
            times[times.len() / 2],
            times[times.len() - 1]
        );
    }
    let start = std::time::Instant::now();
    s.commit_advanced_transform(t.token, false).unwrap();
    let _ = s.read_presented_level(2).unwrap();
    eprintln!(
        "commit + level-2 read: {:.1} ms",
        start.elapsed().as_secs_f64() * 1e3
    );
}

// Source-only candidate: UNRUN on Machine B under resource hold.
#[test]
fn rasterized_copy_handle_precancel_retry_and_close() {
    let (dir, engine) = engine();
    let session = adopt(&engine, 2, 2, vec![pixel_layer("tiny", 2, 2)]);
    let path = dir.path().join("copy.psd");
    std::fs::write(&path, b"sentinel").unwrap();
    let before = session.info().unwrap();
    let history = history_len(&session);
    let old = session.prepare_rasterized_psd_copy().unwrap();
    assert!(old.cancel());
    let fresh = session.prepare_rasterized_psd_copy().unwrap();
    let name = path.to_string_lossy().into_owned();
    assert_eq!(
        old.run(name.clone()).unwrap(),
        RasterizedPsdCopyOutcome::Cancelled
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"sentinel");
    assert!(old.run(name.clone()).is_err());
    assert_eq!(
        fresh.run(name.clone()).unwrap(),
        RasterizedPsdCopyOutcome::Saved
    );
    assert!(std::fs::read(&path).unwrap().starts_with(b"8BPS"));
    let after = session.info().unwrap();
    assert_eq!(after.path, before.path);
    assert_eq!(after.dirty, before.dirty);
    assert_eq!(after.history_head, before.history_head);
    assert_eq!(history_len(&session), history);
    let queued = session.prepare_rasterized_psd_copy().unwrap();
    session.close();
    let saved = std::fs::read(&path).unwrap();
    assert_eq!(
        queued.run(name).unwrap(),
        RasterizedPsdCopyOutcome::Cancelled
    );
    assert_eq!(std::fs::read(&path).unwrap(), saved);
    assert!(session.prepare_rasterized_psd_copy().is_err());
}
