//! Filters, Image ▸ Adjustments and smart filters on `DocumentSession`
//! (WP B5-05): the menu catalogue, previews (no history, viewport level),
//! destructive apply (one node, selection, undo), direct adjustments,
//! smart filters (list edits, rendering, save/load round trip, export).
#![cfg(target_os = "macos")]

use compositor::{Compositor, Document, raster::Depth};
use engine_api::tile::Extent;
use filters::{Filter, registry};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// An opaque PNG with detail at every scale (edges, a checker, ramps).
fn opaque_png(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let checker = if (x / 7 + y / 5) % 2 == 0 { 200 } else { 40 };
        image::Rgba([
            (x * 255 / w) as u8,
            checker,
            if x > w / 2 && y > h / 3 {
                230
            } else {
                (y * 255 / h) as u8
            },
            255,
        ])
    });
    let path = dir.join(name);
    img.save(&path).unwrap();
    path
}

fn open(engine: &Arc<Engine>, path: &Path) -> Arc<DocumentSession> {
    engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap()
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

fn gaussian(radius: f32) -> String {
    format!(r#"{{"id":"gaussian_blur","params":{{"radius":{radius}}}}}"#)
}

/// Smart filters removed, recursively: the session bakes them itself, so
/// the unfiltered layer contents are the reference input.
fn strip_smart_filters(layers: &mut [Arc<compositor::document::Layer>]) {
    use compositor::document::LayerKind;
    for l in layers {
        let l = Arc::make_mut(l);
        match &mut l.kind {
            LayerKind::SmartObject(so) => {
                so.filters.clear();
                so.filter_mask = None;
            }
            LayerKind::Group { children, .. } => strip_smart_filters(children),
            _ => {}
        }
    }
}

/// The document's live composite (smart filters not applied) at `level`
/// through the CPU compositor.
fn live_level(s: &DocumentSession, level: u8) -> (Extent, Vec<f32>) {
    let mut state = (*s.document_state().unwrap()).clone();
    strip_smart_filters(&mut state.root);
    Compositor::new(64 << 20)
        .render_level_rgba(&Document::new(state), level)
        .unwrap()
}

/// `filter_json` applied by the filters crate to an interleaved image.
fn reference(filter_json: &str, extent: Extent, rgba: &[f32], scale: f32) -> Vec<f32> {
    let v: serde_json::Value = serde_json::from_str(filter_json).unwrap();
    let mut values = std::collections::BTreeMap::new();
    for (k, p) in v["params"].as_object().unwrap() {
        values.insert(k.clone(), registry::ParamValue::Number(p.as_f64().unwrap()));
    }
    let (effect, params) = registry::build(v["id"].as_str().unwrap(), &values, scale).unwrap();
    let mut r = compositor::Raster::new(extent, 4, Depth::F32, 0.0);
    r.edit_region(compositor::Rect::of_extent(extent), 1, |x, y, p| {
        let i = ((y * extent.width + x) * 4) as usize;
        *p = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
    })
    .unwrap();
    let out = effect.apply(&r, &params, &AtomicBool::new(false)).unwrap();
    let mut o = Vec::with_capacity(rgba.len());
    for y in 0..extent.height {
        for x in 0..extent.width {
            o.extend_from_slice(&out.pixel(x, y));
        }
    }
    o
}

#[test]
fn list_filters_is_the_grouped_catalogue_with_json_schemas() {
    let list = list_filters();
    assert!(list.len() >= 20);
    let groups = [
        "Blur", "Sharpen", "Noise", "Distort", "Stylize", "Render", "Other",
    ];
    let mut last_group = 0;
    let mut ids = std::collections::BTreeSet::new();
    for f in &list {
        let g = groups
            .iter()
            .position(|g| *g == f.group)
            .expect("known group");
        assert!(g >= last_group, "grouped in menu order: {}", f.id);
        last_group = g;
        assert!(ids.insert(f.id.clone()), "unique id {}", f.id);
        let schema: serde_json::Value = serde_json::from_str(&f.params_schema_json).unwrap();
        for p in schema["params"].as_array().unwrap() {
            assert!(p["key"].is_string() && p["label"].is_string(), "{}", f.id);
            assert!(
                ["slider", "angle", "choice", "point", "toggle"]
                    .contains(&p["kind"].as_str().unwrap()),
                "{}",
                f.id
            );
        }
    }
    let g = list.iter().find(|f| f.id == "gaussian_blur").unwrap();
    assert_eq!(
        (g.group.as_str(), g.name.as_str()),
        ("Blur", "Gaussian Blur")
    );
    let schema: serde_json::Value = serde_json::from_str(&g.params_schema_json).unwrap();
    assert_eq!(schema["params"][0]["key"], "radius");
    assert_eq!(schema["params"][0]["unit"], "px");
    assert_eq!(schema["params"][0]["spatial"], true);
}

#[test]
fn preview_leaves_history_unchanged_and_renders_on_the_viewport_level() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "p.png", 384, 256));
    let id = s.layers().unwrap()[0].id;
    let before = (s.history_items().unwrap(), s.info().unwrap());
    // The viewport shows level 1: the preview renders there.
    s.set_viewport(1, 0, 0, 192, 128, 0.5).unwrap();
    let (_, _, plain) = s.read_presented_level(1).unwrap();
    s.preview_filter(id, gaussian(6.0), None).unwrap();
    s.wait_filters_idle();
    assert_eq!(s.filter_error(), None);
    let (w, h, shown) = s.read_presented_level(1).unwrap();
    assert_eq!((w, h), (192, 128));
    let (e, src) = live_level(&s, 1);
    let expected = reference(&gaussian(6.0), e, &src, 2.0);
    let d = max_diff(&shown, &expected);
    assert!(d <= 3.0 / 255.0, "preview vs level-1 reference {d}");
    assert!(
        max_diff(&shown, &plain) > 0.05,
        "the preview changes pixels"
    );
    // No history node, nothing dirty, the layer untouched.
    let after = (s.history_items().unwrap(), s.info().unwrap());
    assert_eq!(before.0, after.0);
    assert_eq!(before.1.history_head, after.1.history_head);
    assert_eq!(before.1.dirty, after.1.dirty);
    assert!(max_diff(&live_level(&s, 1).1, &src) == 0.0);
    // Latest wins: a burst of previews shows the last one.
    for r in [1.0, 2.0, 3.0, 12.0] {
        s.preview_filter(id, gaussian(r), None).unwrap();
    }
    s.wait_filters_idle();
    let (_, _, last) = s.read_presented_level(1).unwrap();
    let d = max_diff(&last, &reference(&gaussian(12.0), e, &src, 2.0));
    assert!(d <= 3.0 / 255.0, "latest preview {d}");
    // A region preview covers the region.
    s.preview_filter(
        id,
        gaussian(6.0),
        Some(DocRect {
            x: 0,
            y: 0,
            width: 200,
            height: 256,
        }),
    )
    .unwrap();
    s.wait_filters_idle();
    let (_, _, part) = s.read_presented_level(1).unwrap();
    for y in 0..128usize {
        for x in 0..100usize {
            let i = (y * 192 + x) * 4;
            assert!(
                max_diff(&part[i..i + 4], &expected[i..i + 4]) <= 3.0 / 255.0,
                "{x},{y}"
            );
        }
    }
    s.clear_preview().unwrap();
    let (_, _, cleared) = s.read_presented_level(1).unwrap();
    assert_eq!(max_diff(&cleared, &plain), 0.0);
    // Bad JSON and non-pixel layers are errors.
    assert!(
        s.preview_filter(id, r#"{"id":"nope"}"#.into(), None)
            .is_err()
    );
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
    assert!(s.preview_filter(adj, gaussian(2.0), None).is_err());
    assert!(s.apply_filter(adj, gaussian(2.0)).is_err());
}

#[test]
fn apply_filter_is_one_node_inside_the_selection_and_undoes() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "a.png", 300, 200));
    let id = s.layers().unwrap()[0].id;
    let (e, src) = live_level(&s, 0);
    let nodes = s.history_items().unwrap().len();
    s.preview_filter(id, gaussian(3.0), None).unwrap();
    let u = s.apply_filter(id, gaussian(3.0)).unwrap();
    assert!(u.dirty && u.created.is_empty() && u.layers_changed.contains(&id));
    let items = s.history_items().unwrap();
    assert_eq!(items.len(), nodes + 1, "one history node");
    assert_eq!(items.last().unwrap().label, "Gaussian Blur");
    let (_, out) = live_level(&s, 0);
    let expected = reference(&gaussian(3.0), e, &src, 1.0);
    let d = max_diff(&out, &expected);
    assert!(
        d <= 1.0 / 255.0 + 1e-4,
        "full-resolution apply vs crate {d}"
    );
    // The applied filter ends the preview.
    let (_, _, presented) = s.read_presented_level(0).unwrap();
    assert!(max_diff(&presented, &out) <= 2e-3);
    s.undo().unwrap();
    assert_eq!(max_diff(&live_level(&s, 0).1, &src), 0.0, "undo restores");

    // Inside a selection only.
    s.set_selection_rect(40, 30, 100, 80, 0.0).unwrap();
    s.apply_filter(id, r#"{"id":"find_edges"}"#.into()).unwrap();
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Find Edges"
    );
    let (_, sel) = live_level(&s, 0);
    let px = |v: &[f32], x: u32, y: u32| {
        v[((y * 300 + x) * 4) as usize..((y * 300 + x) * 4 + 4) as usize].to_vec()
    };
    assert_eq!(px(&sel, 10, 10), px(&src, 10, 10), "outside the selection");
    assert_eq!(
        px(&sel, 200, 150),
        px(&src, 200, 150),
        "outside the selection"
    );
    assert!(
        max_diff(&px(&sel, 80, 60), &px(&src, 80, 60)) > 0.0
            || max_diff(&px(&sel, 44, 37), &px(&src, 44, 37)) > 0.0
    );

    // Locked pixels refuse.
    s.clear_selection().unwrap();
    s.set_locks(
        id,
        LayerLocks {
            transparency: false,
            pixels: true,
            position: false,
            all: false,
        },
    )
    .unwrap();
    assert!(s.apply_filter(id, gaussian(2.0)).is_err());
}

#[test]
fn apply_adjustment_uses_the_adjustment_layer_maths() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "j.png", 200, 150));
    let id = s.layers().unwrap()[0].id;
    let (_, src) = live_level(&s, 0);
    // Preview: the adjustment shows, no node.
    let nodes = s.history_items().unwrap().len();
    s.preview_adjustment(id, r#"{"kind":"invert"}"#.into())
        .unwrap();
    let (_, _, shown) = s.read_presented_level(0).unwrap();
    assert!((shown[0] - (1.0 - src[0])).abs() <= 2.0 / 255.0);
    assert_eq!(s.history_items().unwrap().len(), nodes);
    s.apply_adjustment(id, r#"{"kind":"invert"}"#.into())
        .unwrap();
    let items = s.history_items().unwrap();
    assert_eq!(items.len(), nodes + 1);
    assert_eq!(items.last().unwrap().label, "Invert");
    let (_, out) = live_level(&s, 0);
    for (i, (a, b)) in src.iter().zip(&out).enumerate() {
        let want = if i % 4 == 3 { *a } else { 1.0 - a };
        assert!((b - want).abs() <= 1.0 / 255.0 + 1e-4, "{i}: {a} → {b}");
    }
    // Levels equals a Levels adjustment layer over the original.
    s.undo().unwrap();
    let levels = r#"{"kind":"levels","master":{"in_black":0.1,"in_white":0.8,"gamma":1.4,"out_black":0,"out_white":1},"rgb":[{},{},{}]}"#;
    let layer = s
        .add_layer(
            NewLayer::Adjustment {
                json: levels.into(),
            },
            String::new(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    let (_, with_layer) = live_level(&s, 0);
    s.remove_layer(layer).unwrap();
    s.apply_adjustment(id, levels.into()).unwrap();
    assert_eq!(s.history_items().unwrap().last().unwrap().label, "Levels");
    let d = max_diff(&live_level(&s, 0).1, &with_layer);
    assert!(
        d <= 1.0 / 255.0 + 1e-4,
        "Image ▸ Adjustments ▸ Levels vs layer {d}"
    );
    assert!(s.apply_adjustment(id, "{}".into()).is_err());
}

#[test]
fn smart_filters_render_edit_and_round_trip_through_save() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "so.png", 256, 192));
    let id = s.layers().unwrap()[0].id;
    let (e, src) = live_level(&s, 0);
    let nodes = s.history_items().unwrap().len();
    s.convert_for_smart_filters(id).unwrap();
    let row = s.layer(id).unwrap();
    assert_eq!(row.kind, DocLayerKind::SmartObject);
    assert_eq!(row.name, "so");
    assert_eq!(s.history_items().unwrap().len(), nodes + 1);
    assert!(s.smart_filters(id).unwrap().is_empty());
    // The smart object looks like the pixels it holds.
    let (_, _, raw) = s.read_presented_level(0).unwrap();
    assert!(max_diff(&raw, &src) <= 2e-3);

    // Applying a filter appends a smart filter: one node, pixels kept.
    s.apply_filter(id, gaussian(4.0)).unwrap();
    assert_eq!(s.history_items().unwrap().len(), nodes + 2);
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Gaussian Blur"
    );
    let list = s.smart_filters(id).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(
        (
            list[0].filter_id.as_str(),
            list[0].name.as_str(),
            list[0].enabled,
            list[0].opacity,
            list[0].blend_mode.as_str(),
            list[0].has_mask
        ),
        ("gaussian_blur", "Gaussian Blur", true, 1.0, "normal", false)
    );
    let blurred = reference(&gaussian(4.0), e, &src, 1.0);
    let (_, _, shown) = s.read_presented_level(0).unwrap();
    let d = max_diff(&shown, &blurred);
    assert!(d <= 3.0 / 255.0, "smart filter at level 0 vs crate {d}");
    // At a coarser view level the bake follows the level.
    let (_, _, coarse) = s.read_presented_level(1).unwrap();
    let (e1, src1) = live_level(&s, 1);
    let d1 = max_diff(&coarse, &reference(&gaussian(4.0), e1, &src1, 2.0));
    assert!(d1 <= 3.0 / 255.0, "smart filter at level 1 {d1}");

    // Toggle off: the raw pixels; on again: blurred. One node each.
    s.set_smart_filter(id, 0, SmartFilterEdit::Enabled { enabled: false })
        .unwrap();
    assert!(!s.smart_filters(id).unwrap()[0].enabled);
    let (_, _, off) = s.read_presented_level(0).unwrap();
    assert!(max_diff(&off, &src) <= 2e-3, "disabled smart filter");
    s.set_smart_filter(id, 0, SmartFilterEdit::Enabled { enabled: true })
        .unwrap();
    let (_, _, on) = s.read_presented_level(0).unwrap();
    assert!(max_diff(&on, &blurred) <= 3.0 / 255.0, "enabled again");
    let labels: Vec<String> = s
        .history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect();
    assert!(labels.ends_with(&["Disable Smart Filter".into(), "Enable Smart Filter".into()]));

    // Blending options: half opacity is halfway.
    s.set_smart_filter(
        id,
        0,
        SmartFilterEdit::Blending {
            mode: "normal".into(),
            opacity: 0.5,
        },
    )
    .unwrap();
    let (_, _, half) = s.read_presented_level(0).unwrap();
    let mid: Vec<f32> = src
        .iter()
        .zip(&blurred)
        .map(|(a, b)| (a + b) / 2.0)
        .collect();
    assert!(max_diff(&half, &mid) <= 3.0 / 255.0);
    // Parameters re-edit (and preview of a re-edit).
    s.preview_smart_filter(id, 0, gaussian(1.0), None).unwrap();
    s.wait_filters_idle();
    assert_eq!(s.filter_error(), None);
    s.clear_preview().unwrap();
    s.set_smart_filter(
        id,
        0,
        SmartFilterEdit::Params {
            filter_json: gaussian(2.0),
        },
    )
    .unwrap();
    assert!(
        s.smart_filters(id).unwrap()[0]
            .filter_json
            .contains("\"radius\":2")
    );
    assert!(
        s.set_smart_filter(
            id,
            0,
            SmartFilterEdit::Params {
                filter_json: r#"{"id":"median"}"#.into()
            }
        )
        .is_err(),
        "a smart filter keeps its filter"
    );
    // A second filter, masked by the selection.
    s.set_selection_rect(0, 0, 128, 192, 0.0).unwrap();
    s.apply_filter(id, r#"{"id":"add_noise","params":{"amount":20}}"#.into())
        .unwrap();
    s.clear_selection().unwrap();
    let list = s.smart_filters(id).unwrap();
    assert_eq!(list.len(), 2);
    assert!(list[1].has_mask && !list[0].has_mask);
    assert!(s.smart_filter_mask_thumbnail(id, 1, 64).unwrap() != 0);
    let (_, _, masked) = s.read_presented_level(0).unwrap();
    // Right half (mask hidden): no noise, only the first filter.
    let (_, blur2) = {
        let r = reference(&gaussian(2.0), e, &src, 1.0);
        let m: Vec<f32> = src.iter().zip(&r).map(|(a, b)| (a + b) / 2.0).collect();
        (0, m)
    };
    let i = ((100 * 256 + 200) * 4) as usize;
    assert!(
        max_diff(&masked[i..i + 4], &blur2[i..i + 4]) <= 3.0 / 255.0,
        "mask hides the noise"
    );

    // Save, close, reopen: the list round-trips.
    let before = s.smart_filters(id).unwrap();
    let path = dir.path().join("smart.tessera-doc");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let again = open(&engine, &path);
    let row = again
        .layers()
        .unwrap()
        .into_iter()
        .find(|n| n.kind == DocLayerKind::SmartObject)
        .unwrap();
    assert_eq!(again.smart_filters(row.id).unwrap(), before);
    let (_, _, reopened) = again.read_presented_level(0).unwrap();
    assert!(
        max_diff(&reopened, &masked) <= 2e-3,
        "same pixels after reopening"
    );

    // Export bakes the smart filters at full resolution.
    let out = dir.path().join("flat.png");
    again
        .export_flat(
            out.to_string_lossy().into_owned(),
            ExportFormat::Png,
            90,
            ExportColor::Document,
        )
        .unwrap();
    let decoded = image::open(&out).unwrap().to_rgba8();
    let worst = decoded
        .as_raw()
        .iter()
        .zip(&reopened)
        .map(|(p, r)| p.abs_diff((r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8))
        .max()
        .unwrap();
    assert!(worst <= 2, "export vs presented {worst}");

    // Delete a smart filter.
    again.remove_smart_filter(row.id, 1).unwrap();
    assert_eq!(again.smart_filters(row.id).unwrap().len(), 1);
    assert_eq!(
        again.history_items().unwrap().last().unwrap().label,
        "Delete Smart Filter"
    );
    assert!(again.remove_smart_filter(row.id, 5).is_err());
}

#[test]
fn filter_detail_is_a_one_to_one_crop() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "d.png", 320, 240));
    let id = s.layers().unwrap()[0].id;
    let (e, src) = live_level(&s, 0);
    let d = s.filter_detail(id, gaussian(3.0), 100, 80, 64, 48).unwrap();
    assert_eq!((d.width, d.height, d.level), (64, 48, 0));
    let surface = tessera_ffi::surface::Surface::lookup(d.surface_id, 64, 48).unwrap();
    let expected = reference(&gaussian(3.0), e, &src, 1.0);
    let mut worst = 0u8;
    surface
        .with_pixels(|px, stride| {
            for y in 0..48usize {
                for x in 0..64usize {
                    for c in 0..4 {
                        let want = expected[(((80 + y) * 320 + 100 + x) * 4) + c];
                        let got = px[y * stride + x * 4 + c];
                        worst = worst.max(got.abs_diff((want.clamp(0.0, 1.0) * 255.0 + 0.5) as u8));
                    }
                }
            }
        })
        .unwrap();
    assert!(worst <= 2, "detail vs crate {worst}");
    assert_eq!(s.history_items().unwrap().len(), 1, "no history");
}

/// Preview latency for Gaussian blur on a 20 MP layer at the fit level.
#[test]
#[ignore]
fn bench_gaussian_preview_20mp() {
    let (dir, engine) = engine();
    let s = open(&engine, &opaque_png(dir.path(), "big.png", 5472, 3648));
    let id = s.layers().unwrap()[0].id;
    for level in [2u8, 1] {
        let e = Extent::new(5472, 3648).at_level(level);
        s.set_viewport(
            level,
            0,
            0,
            e.width,
            e.height,
            1.0 / f64::from(1u32 << level),
        )
        .unwrap();
        let mut times = Vec::new();
        for (i, r) in [4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0].iter().enumerate() {
            let t = std::time::Instant::now();
            s.preview_filter(id, gaussian(*r), None).unwrap();
            s.wait_filters_idle();
            let ms = t.elapsed().as_secs_f64() * 1000.0;
            println!(
                "L{level} {}x{} gaussian r={r}: {ms:.1} ms{}",
                e.width,
                e.height,
                if i == 0 { " (cold source)" } else { "" }
            );
            if i > 0 {
                times.push(ms);
            }
        }
        times.sort_by(f64::total_cmp);
        println!(
            "L{level} median warm preview: {:.1} ms",
            times[times.len() / 2]
        );
    }
    let t = std::time::Instant::now();
    s.apply_filter(id, gaussian(8.0)).unwrap();
    println!(
        "apply gaussian r=8 at full resolution: {:.0} ms",
        t.elapsed().as_secs_f64() * 1000.0
    );
}
