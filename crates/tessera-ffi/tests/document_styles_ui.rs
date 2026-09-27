//! Layer styles and Global Light over the bridge (WP B5-07): the JSON API
//! the Layer Style inspector drives, its history behaviour, lock checks,
//! render invalidation by the Global Light and persistence through
//! `.tessera-doc` and PSD.
#![cfg(target_os = "macos")]

use compositor::{
    DocOp, DocState, Document, Layer, LayerId, LayerKind, Raster, geom::Rect, raster::Depth,
    render::styles::LayerStyles,
};
use engine_api::tile::Extent;
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tessera_ffi::surface::{Surface, testing::create_rgba8};
use tessera_ffi::*;

const W: u32 = 96;
const H: u32 = 64;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// A pixel layer with an opaque orange rectangle over transparency.
fn square(name: &str, rect: Rect) -> Layer {
    let canvas = Extent::new(W, H);
    let mut r = Raster::new(canvas, 4, Depth::U8, 0.0);
    r.edit_region(rect, 1, |_, _, p| *p = [1.0, 0.5, 0.2, 1.0])
        .unwrap();
    Layer::new(name, LayerKind::Pixel(r))
}

/// A document with two rectangles (left, right) over nothing; returns the
/// session and the layer ids bottom first.
fn two_layers(engine: &Arc<Engine>) -> (Arc<DocumentSession>, u64, u64) {
    let mut d = Document::new(DocState::new(Extent::new(W, H), Depth::U8));
    let mut ids = Vec::new();
    for (name, rect) in [
        ("left", Rect::new(12, 16, 36, 40)),
        ("right", Rect::new(60, 16, 84, 40)),
    ] {
        ids.push(
            d.apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: square(name, rect),
            })
            .unwrap()
            .created[0]
                .0,
        );
    }
    let s = engine.adopt_document(d, "styles".into());
    (s, ids[0], ids[1])
}

/// Copy Layer Style is application-wide: tests that copy take this lock.
static CLIPBOARD: Mutex<()> = Mutex::new(());

/// The session's composite of its live state, straight RGBA at level 0
/// (`read_level`: the resident renderer, whose styled documents fall back
/// to the CPU compositor).
fn render(s: &DocumentSession) -> Vec<f32> {
    s.read_level(0).unwrap().2
}

/// The CPU compositor's composite of the session's live state.
fn cpu_reference(s: &DocumentSession, level: u8) -> Vec<f32> {
    let state = (*s.document_state().unwrap()).clone();
    compositor::Compositor::new(64 << 20)
        .render_level_rgba(&Document::new(state), level)
        .unwrap()
        .1
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn schema() -> Value {
    serde_json::from_str(&style_effects_schema_json()).unwrap()
}

fn styles_value(s: &DocumentSession, id: u64) -> Value {
    serde_json::from_str(&s.layer_styles_json(id).unwrap()).unwrap()
}

fn history_len(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

/// Recursive JSON equality with a numeric tolerance (PSD stores 8-bit colours,
/// percent doubles and 16.16 fixed values).
fn approx(a: &Value, b: &Value, tol: f64, path: &str) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            assert!((x - y).abs() <= tol, "{path}: {x} vs {y}");
        }
        (Value::Array(x), Value::Array(y)) => {
            assert_eq!(x.len(), y.len(), "{path}: length");
            for (i, (x, y)) in x.iter().zip(y).enumerate() {
                approx(x, y, tol, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            assert_eq!(
                x.keys().collect::<Vec<_>>(),
                y.keys().collect::<Vec<_>>(),
                "{path}: keys"
            );
            for (k, v) in x {
                approx(v, &y[k], tol, &format!("{path}.{k}"));
            }
        }
        _ => assert_eq!(a, b, "{path}"),
    }
}

/// Non-default settings for each effect kind, so a round trip cannot pass
/// by falling back to defaults.
fn edited(kind: &str, defaults: &Value) -> Value {
    let mut v = defaults.clone();
    let o = v.as_object_mut().unwrap();
    if o.contains_key("opacity") {
        o.insert("opacity".into(), json!(0.4));
    }
    if o.contains_key("size") {
        o.insert("size".into(), json!(7.5));
    }
    if o.contains_key("mode") {
        o.insert("mode".into(), json!("linear_burn"));
    }
    if o.contains_key("shape") {
        // Contour and jitter: metadata the renderer keeps but does not use.
        o.insert(
            "shape".into(),
            json!({"contour": [[0.0, 0.0], [0.5, 0.8], [1.0, 1.0]], "jitter": 0.25}),
        );
    }
    match kind {
        "drop_shadow" | "inner_shadow" => {
            o.insert("use_global_light".into(), json!(false));
            o.insert("angle".into(), json!(33.0));
            o.insert("distance".into(), json!(9.0));
            o.insert("color".into(), json!([0.2, 0.1, 0.6, 1.0]));
        }
        "outer_glow" | "inner_glow" => {
            o.insert("center".into(), json!(true));
            o.insert("spread".into(), json!(2.0));
        }
        "bevel" => {
            o.insert("kind".into(), json!("pillow"));
            o.insert("down".into(), json!(true));
            o.insert("depth".into(), json!(2.5));
            o.insert("elevation".into(), json!(55.0));
            o.insert("highlight_mode".into(), json!("overlay"));
        }
        "satin" => {
            o.insert("invert".into(), json!(true));
            o.insert("angle".into(), json!(-45.0));
        }
        "stroke" => {
            o.insert("position".into(), json!("center"));
            o.insert(
                "fill".into(),
                json!({"kind": "solid", "color": [0.0, 0.5, 1.0]}),
            );
        }
        "color_overlay" | "overlay" => {
            o.insert(
                "fill".into(),
                json!({"kind": "solid", "color": [0.1, 0.9, 0.3]}),
            );
        }
        _ => {}
    }
    v
}

#[test]
fn every_effect_kind_round_trips_through_the_json_api() {
    let (_d, engine) = engine();
    let (s, a, _) = two_layers(&engine);
    let schema = schema();
    let kinds: Vec<&Value> = schema["effects"].as_array().unwrap().iter().collect();
    assert_eq!(kinds.len(), 11, "every StyleEffect variant is described");
    for e in &kinds {
        let kind = e["kind"].as_str().unwrap();
        let styles = json!({"effects": [{"kind": kind, "settings": edited(kind, &e["defaults"])}], "scale": 1.5});
        s.set_layer_styles_json(a, styles.to_string(), false)
            .unwrap_or_else(|err| panic!("{kind}: {err}"));
        let back = styles_value(&s, a);
        approx(&back, &styles, 1e-6, kind);
        // The typed model agrees (no field was dropped on the way in).
        let typed: LayerStyles = serde_json::from_value(back).unwrap();
        assert_eq!(
            typed,
            serde_json::from_value::<LayerStyles>(styles.clone()).unwrap()
        );
        let summary = s.layer_style_summaries().unwrap();
        assert_eq!(summary.len(), 1);
        assert_eq!(summary[0].effects[0].kind, kind);
    }
    // All kinds on one layer, repeated ones too: summaries list them top
    // first in the engine's stacking order, later repeats above earlier ones.
    let mut all: Vec<Value> = kinds
        .iter()
        .rev()
        .map(|e| json!({"kind": e["kind"], "settings": e["defaults"]}))
        .collect();
    all.push(json!({"kind": "drop_shadow", "settings": {"distance": 20.0}}));
    all.push(json!({"kind": "stroke", "settings": {"size": 1.0}}));
    let styles = json!({"effects": all, "scale": 1.0});
    s.set_layer_styles_json(a, styles.to_string(), false)
        .unwrap();
    let summary = s.layer_style_summaries().unwrap().remove(0);
    let order: Vec<&str> = summary.effects.iter().map(|e| e.kind.as_str()).collect();
    assert_eq!(
        order,
        [
            "bevel",
            "stroke",
            "stroke",
            "inner_shadow",
            "inner_glow",
            "satin",
            // Tied ranks: the later vector entry (color_overlay) is above.
            "color_overlay",
            "overlay",
            "gradient_overlay",
            "pattern_overlay",
            "outer_glow",
            "drop_shadow",
            "drop_shadow",
        ]
    );
    // The second stroke (index 12, added last) is listed above the first.
    assert_eq!(summary.effects[1].index, 12);
    assert_eq!(summary.effects[11].index, 11);
    // Rendering all of them works (the CPU style path).
    let px = render(&s);
    assert_eq!(px.len(), (W * H * 4) as usize);
    assert!(px.iter().all(|v| v.is_finite()));
    // Invalid JSON and out-of-range values are rejected without an edit.
    let n = history_len(&s);
    assert!(s.set_layer_styles_json(a, "{".into(), false).is_err());
    let bad = json!({"effects": [{"kind": "drop_shadow", "settings": {"opacity": 2.0}}]});
    assert!(s.set_layer_styles_json(a, bad.to_string(), false).is_err());
    assert_eq!(history_len(&s), n);
}

#[test]
fn unrelated_properties_are_preserved() {
    let (_d, engine) = engine();
    let (s, a, _) = two_layers(&engine);
    s.set_opacity(a, 0.6, false).unwrap();
    s.set_fill_opacity(a, 0.0, false).unwrap();
    s.set_blend_mode(a, "multiply".into()).unwrap();
    s.add_mask(a, MaskInit::HideAll).unwrap();
    s.set_locks(
        a,
        LayerLocks {
            position: true,
            transparency: true,
            ..Default::default()
        },
    )
    .unwrap();
    let before = s.document_state().unwrap();
    let before_layer = before.find(LayerId(a)).unwrap().clone();
    let styles = json!({"effects": [{"kind": "drop_shadow", "settings": {}},
                                    {"kind": "stroke", "settings": {"size": 4.0}}]});
    s.set_layer_styles_json(a, styles.to_string(), false)
        .unwrap();
    // Dragging also keeps them (the scratch path).
    let drag = json!({"effects": [{"kind": "drop_shadow", "settings": {"distance": 12.0}},
                                  {"kind": "stroke", "settings": {"size": 4.0}}]});
    s.set_layer_styles_json(a, drag.to_string(), true).unwrap();
    s.commit("Layer Style".into()).unwrap();
    let after = s.document_state().unwrap();
    let l = after.find(LayerId(a)).unwrap();
    assert_eq!(l.props.styles.effects.len(), 2);
    let mut props = l.props.clone();
    props.styles = before_layer.props.styles.clone();
    assert_eq!(
        props, before_layer.props,
        "opacity, fill, blend, locks, knockout, Blend If kept"
    );
    let (m0, m1) = (
        l.mask.as_ref().unwrap(),
        before_layer.mask.as_ref().unwrap(),
    );
    assert!(
        m0.raster.shares_all_tiles_with(&m1.raster)
            && m0.density == m1.density
            && m0.enabled == m1.enabled,
        "mask kept"
    );
    let node = s.layer(a).unwrap();
    assert_eq!(node.blend_mode, "multiply");
    assert!((node.opacity - 0.6).abs() < 1e-6 && node.fill_opacity == 0.0);
    assert!(node.has_mask && node.locks.position && node.locks.transparency);
    // Fill 0 %: the interior disappears but the stroke is still drawn.
    let px = render(&s);
    let alpha = |x: u32, y: u32| px[((y * W + x) * 4 + 3) as usize];
    // The mask hides everything, so reveal it to look at the effects.
    s.remove_mask(a).unwrap();
    let px2 = render(&s);
    let alpha2 = |x: u32, y: u32| px2[((y * W + x) * 4 + 3) as usize];
    assert_eq!(alpha(24, 28), 0.0, "hide-all mask hides the styled layer");
    // Top-left of the interior, away from the shadow cast down-right.
    assert!(alpha2(15, 19) < 0.05, "fill 0 %: interior transparent");
    assert!(alpha2(10, 28) > 0.3, "stroke outside the left edge visible");
}

#[test]
fn locked_and_unstyleable_layers_refuse_edits() {
    let _clip = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, engine) = engine();
    let (s, a, b) = two_layers(&engine);
    let styles = json!({"effects": [{"kind": "drop_shadow", "settings": {}}]}).to_string();
    s.set_layer_styles_json(b, styles.clone(), false).unwrap();
    s.copy_layer_styles(b).unwrap();
    s.set_locks(
        a,
        LayerLocks {
            all: true,
            ..Default::default()
        },
    )
    .unwrap();
    let n = history_len(&s);
    let err = s
        .set_layer_styles_json(a, styles.clone(), false)
        .unwrap_err();
    assert!(err.to_string().contains("locked"), "{err}");
    assert!(s.set_layer_styles_json(a, styles.clone(), true).is_err());
    assert!(s.paste_layer_styles(vec![a]).is_err());
    assert!(s.paste_layer_styles(vec![b, a]).is_err(), "all or nothing");
    s.set_locks(
        b,
        LayerLocks {
            all: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.clear_layer_styles(b).is_err());
    assert_eq!(history_len(&s), n + 1, "only the lock change was recorded");
    assert!(
        s.document_state()
            .unwrap()
            .find(LayerId(a))
            .unwrap()
            .props
            .styles
            .effects
            .is_empty()
    );
    // Copy still works on a locked layer (it reads only).
    s.copy_layer_styles(b).unwrap();
    // Adjustment layers and pass-through groups cannot take styles.
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
    assert!(s.set_layer_styles_json(adj, styles.clone(), false).is_err());
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
    assert!(s.set_layer_styles_json(g, styles.clone(), false).is_err());
    s.set_group_mode(g, DocGroupMode::Isolated).unwrap();
    s.set_layer_styles_json(g, styles, false).unwrap();
}

#[test]
fn interactive_drag_records_one_node_on_commit() {
    let (_d, engine) = engine();
    let (s, a, _) = two_layers(&engine);
    let n = history_len(&s);
    let head = s.info().unwrap().history_head;
    for d in 1..=6 {
        let styles =
            json!({"effects": [{"kind": "drop_shadow", "settings": {"distance": d as f32 * 2.0}}]});
        let u = s
            .set_layer_styles_json(a, styles.to_string(), true)
            .unwrap();
        assert_eq!(u.history_head, head, "no node while dragging");
        assert!(u.dirty_rect.is_some());
    }
    assert_eq!(history_len(&s), n);
    assert_eq!(
        styles_value(&s, a)["effects"][0]["settings"]["distance"],
        json!(12.0)
    );
    let u = s.commit("Layer Style".into()).unwrap();
    assert_ne!(u.history_head, head);
    assert_eq!(history_len(&s), n + 1);
    let items = s.history_items().unwrap();
    assert_eq!(items.last().unwrap().label, "Layer Style");
    // A global-light drag is one node too.
    for angle in [100.0, 80.0, 60.0] {
        s.set_global_light(angle, 40.0, true).unwrap();
    }
    assert_eq!(history_len(&s), n + 1);
    assert_eq!(
        s.global_light().unwrap(),
        GlobalLightRecord {
            angle: 60.0,
            altitude: 40.0
        }
    );
    s.commit("Global Light".into()).unwrap();
    assert_eq!(history_len(&s), n + 2);
    // Undo restores the light, then the styles.
    s.undo().unwrap();
    assert_eq!(s.global_light().unwrap().angle, 120.0);
    s.undo().unwrap();
    assert!(
        styles_value(&s, a)["effects"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        s.set_global_light(0.0, 91.0, false).is_err(),
        "altitude above 90"
    );
    assert!(s.set_global_light(f32::NAN, 30.0, false).is_err());
}

#[test]
fn copy_paste_and_clear_are_one_node_each() {
    let _clip = CLIPBOARD.lock().unwrap_or_else(|e| e.into_inner());
    let (_d, engine) = engine();
    let (s, a, b) = two_layers(&engine);
    let styles = json!({"effects": [{"kind": "drop_shadow", "settings": {}},
                                    {"kind": "stroke", "settings": {}}], "scale": 2.0});
    s.set_layer_styles_json(a, styles.to_string(), false)
        .unwrap();
    let n = history_len(&s);
    s.copy_layer_styles(a).unwrap();
    assert!(s.can_paste_layer_styles());
    assert_eq!(history_len(&s), n, "copy records nothing");
    let c = s
        .add_layer(NewLayer::Pixel, String::new(), None, None)
        .unwrap()
        .created[0];
    let n = history_len(&s);
    s.paste_layer_styles(vec![b, c]).unwrap();
    assert_eq!(history_len(&s), n + 1);
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Paste Layer Style"
    );
    assert_eq!(styles_value(&s, b), styles_value(&s, a));
    assert_eq!(styles_value(&s, c), styles_value(&s, a));
    s.clear_layer_styles(a).unwrap();
    assert_eq!(history_len(&s), n + 2);
    assert_eq!(
        styles_value(&s, a),
        serde_json::to_value(LayerStyles::default()).unwrap()
    );
    let ids: Vec<u64> = s
        .layer_style_summaries()
        .unwrap()
        .iter()
        .map(|x| x.layer)
        .collect();
    assert_eq!(ids, vec![c, b], "layers() order: top first");
    s.undo().unwrap();
    assert_eq!(s.layer_style_summaries().unwrap().len(), 3);
}

#[test]
fn global_light_change_invalidates_both_layers_that_use_it() {
    let (_d, engine) = engine();
    let (s, a, b) = two_layers(&engine);
    // Both shadows follow the global light; a third, local-angle shadow does not.
    let global = json!({"effects": [{"kind": "drop_shadow",
        "settings": {"use_global_light": true, "distance": 8.0, "size": 0.0, "opacity": 1.0}}]});
    for id in [a, b] {
        s.set_layer_styles_json(id, global.to_string(), false)
            .unwrap();
    }
    s.set_global_light(180.0, 30.0, false).unwrap(); // light from the left: shadows to the right
    let before = render(&s);
    let alpha = |px: &[f32], x: u32, y: u32| px[((y * W + x) * 4 + 3) as usize];
    // Right of each rectangle: shadow; left: nothing.
    assert!(alpha(&before, 40, 28) > 0.5 && alpha(&before, 88, 28) > 0.5);
    assert!(alpha(&before, 8, 28) < 0.01 && alpha(&before, 56, 28) < 0.01);
    let head = s.info().unwrap().history_head;
    let u = s.set_global_light(0.0, 30.0, false).unwrap(); // light from the right
    assert_ne!(u.history_head, head);
    let full = u.dirty_rect.unwrap();
    assert_eq!(
        (full.width, full.height),
        (W as i64, H as i64),
        "whole canvas"
    );
    let after = render(&s);
    // Both layers' shadows moved to the left side.
    assert!(alpha(&after, 40, 28) < 0.01 && alpha(&after, 88, 28) < 0.01);
    assert!(alpha(&after, 8, 28) > 0.5 && alpha(&after, 56, 28) > 0.5);
    // The session's renderer agrees with the CPU compositor.
    let diff = max_diff(&after, &cpu_reference(&s, 0));
    assert!(diff < 2.0 / 255.0, "{diff}");
    // A local-angle shadow ignores the light.
    let local = json!({"effects": [{"kind": "drop_shadow",
        "settings": {"use_global_light": false, "angle": 180.0, "distance": 8.0, "size": 0.0, "opacity": 1.0}}]});
    s.set_layer_styles_json(b, local.to_string(), false)
        .unwrap();
    let one = render(&s);
    s.set_global_light(90.0, 30.0, false).unwrap();
    let two = render(&s);
    assert_eq!(alpha(&one, 88, 28), alpha(&two, 88, 28));
    assert!(alpha(&two, 88, 28) > 0.5);
}

#[test]
fn tessera_doc_and_psd_round_trips_keep_styles() {
    let (dir, engine) = engine();
    let (s, a, b) = two_layers(&engine);
    // Every kind for the native format; PSD takes the kinds it can encode.
    let schema = schema();
    let all: Vec<Value> = schema["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| json!({"kind": e["kind"], "settings": edited(e["kind"].as_str().unwrap(), &e["defaults"])}))
        .collect();
    let native = json!({"effects": all, "scale": 0.5});
    s.set_layer_styles_json(a, native.to_string(), false)
        .unwrap();
    let psd_kinds = json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 6.0, "size": 3.0, "opacity": 0.5, "angle": 45.0, "use_global_light": false}},
        {"kind": "stroke", "settings": {"size": 2.0, "position": "inside", "fill": {"kind": "solid", "color": [0.0, 0.0, 1.0]}}},
        {"kind": "outer_glow", "settings": {"size": 4.0}},
        {"kind": "inner_shadow", "settings": {}},
        {"kind": "inner_glow", "settings": {"center": true}},
        {"kind": "color_overlay", "settings": {"opacity": 0.25, "fill": {"kind": "solid", "color": [1.0, 0.0, 0.0]}}}
    ], "scale": 1.0});
    s.set_layer_styles_json(b, psd_kinds.to_string(), false)
        .unwrap();
    s.set_global_light(75.0, 20.0, false).unwrap();

    let doc = dir.path().join("styled.tessera-doc");
    s.save_as(doc.to_string_lossy().into_owned()).unwrap();
    let (sa, sb) = (styles_value(&s, a), styles_value(&s, b));
    s.close();
    let r = engine
        .clone()
        .open_document(doc.to_string_lossy().into_owned())
        .unwrap();
    let ids: Vec<u64> = r.layers().unwrap().iter().map(|n| n.id).collect();
    assert_eq!(ids, vec![b, a]);
    assert_eq!(styles_value(&r, a), sa);
    assert_eq!(styles_value(&r, b), sb);
    assert_eq!(
        r.global_light().unwrap(),
        GlobalLightRecord {
            angle: 75.0,
            altitude: 20.0
        }
    );

    // PSD: the PSD-encodable layer keeps its effects; clear the other first.
    r.clear_layer_styles(a).unwrap();
    let psd = dir.path().join("styled.psd");
    r.save_as(psd.to_string_lossy().into_owned()).unwrap();
    r.close();
    let p = engine
        .clone()
        .open_document(psd.to_string_lossy().into_owned())
        .unwrap();
    let nodes = p.layers().unwrap();
    let pb = nodes.iter().find(|n| n.name == "right").unwrap().id;
    let back = styles_value(&p, pb);
    let back_kinds: Vec<&str> = back["effects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["kind"].as_str().unwrap())
        .collect();
    assert_eq!(back_kinds.len(), 6, "{back}");
    for e in psd_kinds["effects"].as_array().unwrap() {
        let k = e["kind"].as_str().unwrap();
        let want: LayerStyles = serde_json::from_value(json!({"effects": [e]})).unwrap();
        let want = serde_json::to_value(&want.effects[0]).unwrap();
        let got = back["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["kind"] == k)
            .unwrap_or_else(|| panic!("{k} missing after PSD"));
        approx(got, &want, 1.0 / 255.0 + 1e-4, k);
    }
    let light = p.global_light().unwrap();
    assert!((light.angle - 75.0).abs() < 1e-3 && (light.altitude - 20.0).abs() < 1e-3);
    // A second styled layer saved with effects PSD cannot encode is refused
    // rather than silently dropped.
    let bevel = json!({"effects": [{"kind": "bevel", "settings": {}}]});
    p.set_layer_styles_json(pb, bevel.to_string(), false)
        .unwrap();
    assert!(
        p.save_as(dir.path().join("bevel.psd").to_string_lossy().into_owned())
            .is_err()
    );
}

#[derive(Default)]
struct Recorder {
    frames: Mutex<Vec<DocFrameInfo>>,
    failures: Mutex<Vec<String>>,
}

impl DocumentListener for Recorder {
    fn on_frame(&self, frame: DocFrameInfo) {
        self.frames.lock().unwrap().push(frame);
    }
    fn on_layers_changed(&self, _layer_ids: Vec<u64>) {}
    fn on_history_changed(&self, _head: u64) {}
    fn on_render_failed(&self, message: String) {
        self.failures.lock().unwrap().push(message);
    }
}

/// Attaches one surface fitting the whole canvas and waits for a frame.
fn present(s: &DocumentSession, rec: &Recorder, w: u32, h: u32) -> (DocSurfacePlan, DocFrameInfo) {
    let plan = s.plan_surface(w, h).unwrap();
    let id = create_rgba8(plan.width, plan.height);
    s.attach_surface(id, plan.width, plan.height).unwrap();
    s.wait_idle();
    let frame = rec.frames.lock().unwrap().last().cloned().expect("a frame");
    (plan, frame)
}

#[test]
fn styled_documents_present_frames_on_the_default_backend() {
    let (_d, engine) = engine();
    let (s, a, b) = two_layers(&engine);
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let styles = json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 6.0, "size": 2.0, "opacity": 1.0}},
        {"kind": "stroke", "settings": {"size": 3.0, "fill": {"kind": "solid", "color": [0.0, 0.0, 1.0]}}}]});
    for id in [a, b] {
        s.set_layer_styles_json(id, styles.to_string(), false)
            .unwrap();
    }
    s.set_fill_opacity(a, 0.0, false).unwrap();
    let (plan, frame) = present(&s, &rec, W, H);
    assert_eq!(frame.level, 0);
    assert!(
        rec.failures.lock().unwrap().is_empty(),
        "{:?}",
        rec.failures.lock().unwrap()
    );
    let reference = cpu_reference(&s, 0);
    let surface = Surface::lookup(frame.surface_id, plan.width, plan.height).unwrap();
    surface
        .with_pixels(|px, stride| {
            let mut worst = 0u8;
            for y in 0..H as usize {
                for x in 0..W as usize {
                    for c in 0..4 {
                        let want = (reference[(y * W as usize + x) * 4 + c].clamp(0.0, 1.0) * 255.0
                            + 0.5) as u8;
                        worst = worst.max(px[y * stride + x * 4 + c].abs_diff(want));
                    }
                }
            }
            assert!(
                worst <= 1,
                "frame differs from the CPU composite by {worst}"
            );
            // The stroke around the fill-0 % layer is on screen.
            assert!(px[28 * stride + 10 * 4 + 3] > 200);
        })
        .unwrap();
    // Removing the styles returns to the resident renderer; frames keep coming.
    let before = rec.frames.lock().unwrap().len();
    s.clear_layer_styles(a).unwrap();
    s.clear_layer_styles(b).unwrap();
    s.wait_idle();
    assert!(rec.frames.lock().unwrap().len() > before);
    assert!(rec.failures.lock().unwrap().is_empty());
    let diff = max_diff(&render(&s), &cpu_reference(&s, 0));
    assert!(diff < 2.0 / 255.0, "{diff}");
    s.detach_surfaces();
    s.close();
}

/// Frame time of a styled large document at the viewport level (1368×912)
/// on the default backend, for IMPLEMENTATION-STATUS.md: 20 MP (5472×3648,
/// over the CPU style path's pixel limit), 3 MP and 0.8 MP. (16 MP took
/// about 108 s per styled frame on an M4 Max, so it is not run here.)
/// `cargo test -p tessera-ffi --release --test document_styles_ui -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_styled_large_viewport_frame() {
    for (w, h) in [(5472u32, 3648u32), (2048, 1536), (1024, 768)] {
        bench_styled(w, h);
    }
}

fn bench_styled(w: u32, h: u32) {
    let (_d, engine) = engine();
    let e = Extent::new(w, h);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut r = Raster::new(e, 4, Depth::U8, 0.0);
    let (mx, my) = (w as i64 / 6, h as i64 / 6);
    r.edit_region(
        Rect::new(mx, my, w as i64 - mx, h as i64 - my),
        1,
        |x, y, p| *p = [(x % 256) as f32 / 255.0, (y % 256) as f32 / 255.0, 0.4, 1.0],
    )
    .unwrap();
    let id = d
        .apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: Layer::new("photo", LayerKind::Pixel(r)),
        })
        .unwrap()
        .created[0]
        .0;
    let s = engine.adopt_document(d, "bench".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let started = Instant::now();
    let (_, plain) = present(&s, &rec, 1368, 912);
    eprintln!(
        "{w}x{h} unstyled: first frame L{} {}x{} in {:?} (render_ms {:.1}), backend {}",
        plain.level,
        plain.width,
        plain.height,
        started.elapsed(),
        plain.render_ms,
        s.info().unwrap().backend
    );
    let styles = json!({"effects": [
        {"kind": "drop_shadow", "settings": {"distance": 20.0, "size": 20.0}},
        {"kind": "stroke", "settings": {"size": 8.0}}]});
    for i in 0..3 {
        let mut v = styles.clone();
        v["effects"][0]["settings"]["distance"] = json!(20.0 + i as f32);
        let t = Instant::now();
        s.set_layer_styles_json(id, v.to_string(), true).unwrap();
        s.wait_idle();
        let f = rec.frames.lock().unwrap().last().cloned().unwrap();
        let failed = rec.failures.lock().unwrap().pop();
        eprintln!(
            "{w}x{h} styled drag step: L{} wall {:.1} ms, render_ms {:.1}{}",
            f.level,
            t.elapsed().as_secs_f64() * 1000.0,
            f.render_ms,
            failed.map(|m| format!(", FAILED: {m}")).unwrap_or_default()
        );
    }
    s.commit("Layer Style".into()).unwrap();
    s.detach_surfaces();
    s.close();
}
