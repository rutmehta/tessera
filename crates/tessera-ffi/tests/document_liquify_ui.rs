//! Liquify workspace for the app (WP B5-13): the session mesh against the
//! engine's `Mesh::apply_brush` / `Mesh::render`, inverse direction, the ten
//! tools, freeze, reconstruct/reset, before/after, one history node per
//! apply at every destination, smart-filter re-edit without duplicates,
//! selection clipping, full-resolution output from a fit-size proxy,
//! cancellation / stale targets / locks, and brush latency on 20 MP.
#![cfg(target_os = "macos")]

use compositor::{geom::Rect, raster::Depth, raster::Raster};
use engine_api::tile::Extent;
use filters::liquify::{Interpolation, Mesh};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// 8 px checker with a semi-transparent band (y 40..48) and an opaque red
/// vertical stripe at x 20..22.
fn checker(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let a = if (40..48).contains(&y) { 128 } else { 255 };
        if (20..22).contains(&x) {
            return image::Rgba([230, 20, 20, 255]);
        }
        if (x / 8 + y / 8) % 2 == 0 {
            image::Rgba([40, 60, 200, a])
        } else {
            image::Rgba([240, 230, 90, a])
        }
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

fn pixels(s: &DocumentSession) -> Vec<f32> {
    s.wait_idle();
    s.read_level(0).unwrap().2
}

fn presented(s: &DocumentSession) -> Vec<f32> {
    s.wait_idle();
    s.read_presented_level(0).unwrap().2
}

fn history(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn head_label(s: &DocumentSession) -> String {
    s.history_items()
        .unwrap()
        .into_iter()
        .find(|i| i.is_current)
        .unwrap()
        .label
}

fn brush(size: f32) -> LiquifyBrush {
    LiquifyBrush {
        size,
        density: 0.5,
        pressure: 1.0,
        rate: 1.0,
    }
}

fn path(from: [f32; 2], to: [f32; 2], n: usize) -> Vec<LiquifyPoint> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            LiquifyPoint {
                x: from[0] + (to[0] - from[0]) * t,
                y: from[1] + (to[1] - from[1]) * t,
                pressure: 1.0,
            }
        })
        .collect()
}

fn raster(w: u32, h: u32, px: &[f32]) -> Raster {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    r.edit_region(Rect::new(0, 0, i64::from(w), i64::from(h)), 1, |x, y, p| {
        let i = ((y * w + x) * 4) as usize;
        p.copy_from_slice(&px[i..i + 4]);
    })
    .unwrap();
    r
}

fn rgba(r: &Raster) -> Vec<f32> {
    let e = r.extent();
    let mut out = Vec::with_capacity(e.area() as usize * 4);
    for y in 0..e.height {
        for x in 0..e.width {
            out.extend_from_slice(&r.pixel(x, y));
        }
    }
    out
}

/// The engine's full-resolution render of `mesh` over `px`.
fn reference(mesh: &Mesh, w: u32, h: u32, px: &[f32]) -> Vec<f32> {
    let out = mesh
        .render(
            &raster(w, h, px),
            Interpolation::Bilinear,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    rgba(&out)
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// Quantization of an 8-bit document plus float slack.
const EPS8: f32 = 0.5 / 255.0 + 1e-5;

#[test]
fn forward_warp_follows_the_brush_and_matches_the_engine_render() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "a.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let info = s.begin_liquify(layer, None).unwrap();
    assert_eq!((info.width, info.height, info.preview_factor), (64, 64, 1));
    assert!(!info.smart_object && !info.edited && info.stage_index.is_none());
    // Drag right across the red stripe at y = 24.
    let r = s
        .liquify_brush_points(
            info.token,
            LiquifyTool::ForwardWarp,
            brush(24.0),
            path([21.0, 24.0], [31.0, 24.0], 10),
        )
        .unwrap();
    assert!(r.dabs >= 4, "{} dabs", r.dabs);
    s.liquify_end_stroke(info.token).unwrap();
    let mesh = s.liquify_engine_mesh(info.token).unwrap();
    // Inverse map: output samples to the LEFT of itself, so content moves right.
    assert!(mesh.displacement_at(26.0, 24.0)[0] < -3.0);
    let expected = reference(&mesh, 64, 64, &before);
    let u = s
        .commit_liquify(info.token, LiquifyDestination::CurrentLayer)
        .unwrap();
    assert!(u.layers_changed.contains(&layer));
    let after = pixels(&s);
    assert!(max_diff(&after, &expected) <= EPS8, "equals Mesh::render");
    // The red stripe's centre on row 24 moved right, rows far away did not.
    let red = |px: &[f32], y: usize| {
        (10..50usize)
            .max_by(|&a, &b| {
                let ra = px[(y * 64 + a) * 4] - px[(y * 64 + a) * 4 + 2];
                let rb = px[(y * 64 + b) * 4] - px[(y * 64 + b) * 4 + 2];
                ra.partial_cmp(&rb).unwrap()
            })
            .unwrap()
    };
    assert!(red(&after, 24) >= 25, "stripe at x={}", red(&after, 24));
    assert!((20..22).contains(&red(&after, 62)));
    assert!(s.liquify_mesh(info.token).is_err(), "the workspace closed");
}

#[test]
fn preview_matches_full_render_and_before_leaves_document_untouched() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "b.png", 48, 56));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let info = s.begin_liquify(layer, None).unwrap();
    s.liquify_brush_points(
        info.token,
        LiquifyTool::Bloat,
        brush(30.0),
        vec![
            LiquifyPoint {
                x: 24.0,
                y: 30.0,
                pressure: 1.0
            };
            6
        ],
    )
    .unwrap();
    let mesh = s.liquify_engine_mesh(info.token).unwrap();
    let p = s.preview_liquify(info.token, false).unwrap();
    assert_eq!((p.width, p.height, p.original), (48, 56, false));
    assert!(p.surface_id != 0);
    let shown = s.liquify_preview_pixels(info.token).unwrap();
    let expected = reference(&mesh, 48, 56, &before);
    let worst = shown
        .iter()
        .zip(&expected)
        .map(|(a, b)| (f32::from(*a) / 255.0 - b.clamp(0.0, 1.0)).abs())
        .fold(0.0, f32::max);
    assert!(
        worst <= 1.0 / 255.0 + 1e-5,
        "preview = Mesh::render ({worst})"
    );
    // Before: the untouched source; nothing in the document changes.
    let o = s.preview_liquify(info.token, true).unwrap();
    assert!(
        o.original && o.surface_id != p.surface_id,
        "double-buffered"
    );
    let original = s.liquify_preview_pixels(info.token).unwrap();
    let worst = original
        .iter()
        .zip(&before)
        .map(|(a, b)| (f32::from(*a) / 255.0 - b).abs())
        .fold(0.0, f32::max);
    assert!(worst <= 0.5 / 255.0 + 1e-5);
    assert_eq!(history(&s), n);
    assert_eq!(pixels(&s), before);
    assert_eq!(
        s.liquify_engine_mesh(info.token).unwrap(),
        mesh,
        "Before does not edit"
    );
    s.cancel_liquify(info.token);
}

#[test]
fn twirl_pucker_bloat_and_push_left_are_distinct() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "c.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let mut meshes = Vec::new();
    for tool in [
        LiquifyTool::TwirlClockwise,
        LiquifyTool::TwirlCounterClockwise,
        LiquifyTool::Pucker,
        LiquifyTool::Bloat,
        LiquifyTool::PushLeft,
    ] {
        let t = s.begin_liquify(layer, None).unwrap().token;
        let pts = if tool == LiquifyTool::PushLeft {
            path([26.0, 32.0], [38.0, 32.0], 12)
        } else {
            vec![LiquifyPoint {
                x: 32.0,
                y: 32.0,
                pressure: 1.0,
            }]
        };
        s.liquify_brush_points(t, tool, brush(40.0), pts).unwrap();
        meshes.push((tool, s.liquify_engine_mesh(t).unwrap()));
        s.cancel_liquify(t);
    }
    for i in 0..meshes.len() {
        for j in i + 1..meshes.len() {
            assert_ne!(
                meshes[i].1, meshes[j].1,
                "{:?} vs {:?}",
                meshes[i].0, meshes[j].0
            );
        }
    }
    let at = |k: usize, x: f32, y: f32| meshes[k].1.displacement_at(x, y);
    // Clockwise and counter-clockwise mirror each other (first dab).
    let (cw, ccw) = (at(0, 40.0, 32.0), at(1, 40.0, 32.0));
    assert!(cw[1] < 0.0 && ccw[1] > 0.0 && (cw[1] + ccw[1]).abs() < 1e-4);
    // Pucker samples outward (content shrinks), Bloat inward.
    assert!(at(2, 40.0, 32.0)[0] > 0.0 && at(3, 40.0, 32.0)[0] < 0.0);
    // Push Left: dragging right pushes content up (the drag's left).
    let d = at(4, 32.0, 32.0);
    assert!(d[1] > 0.5 && d[0].abs() < 1e-3, "{d:?}");
}

#[test]
fn freeze_blocks_deformation_and_thaw_restores_it() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "d.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let t = s.begin_liquify(layer, None).unwrap().token;
    s.liquify_brush_points(
        t,
        LiquifyTool::Freeze,
        brush(24.0),
        vec![LiquifyPoint {
            x: 32.0,
            y: 32.0,
            pressure: 1.0,
        }],
    )
    .unwrap();
    s.liquify_end_stroke(t).unwrap();
    let m = s.liquify_mesh(t).unwrap();
    let cols = m.columns as usize;
    let node = |x: u32, y: u32| (y / m.cell_size) as usize * cols + (x / m.cell_size) as usize;
    assert_eq!(m.freeze[node(32, 32)], 1.0);
    assert_eq!(m.freeze[node(4, 4)], 0.0);
    let drag = path([20.0, 32.0], [44.0, 32.0], 24);
    s.liquify_brush_points(t, LiquifyTool::ForwardWarp, brush(40.0), drag.clone())
        .unwrap();
    s.liquify_end_stroke(t).unwrap();
    let m = s.liquify_mesh(t).unwrap();
    assert_eq!(
        [
            m.displacement[node(32, 32) * 2],
            m.displacement[node(32, 32) * 2 + 1]
        ],
        [0.0, 0.0],
        "frozen node unchanged"
    );
    assert!(m.max_displacement > 1.0, "unfrozen nodes moved");
    // Thaw the centre; the same drag now deforms it.
    s.liquify_brush_points(
        t,
        LiquifyTool::Thaw,
        brush(24.0),
        vec![LiquifyPoint {
            x: 32.0,
            y: 32.0,
            pressure: 1.0,
        }],
    )
    .unwrap();
    s.liquify_end_stroke(t).unwrap();
    assert_eq!(s.liquify_mesh(t).unwrap().freeze[node(32, 32)], 0.0);
    s.liquify_brush_points(t, LiquifyTool::ForwardWarp, brush(40.0), drag)
        .unwrap();
    let m = s.liquify_mesh(t).unwrap();
    assert!(m.displacement[node(32, 32) * 2].abs() > 0.5);
    // Freeze all / thaw all.
    s.liquify_freeze_all(t, true).unwrap();
    assert!(s.liquify_mesh(t).unwrap().freeze.iter().all(|&f| f == 1.0));
    s.liquify_freeze_all(t, false).unwrap();
    assert!(s.liquify_mesh(t).unwrap().freeze.iter().all(|&f| f == 0.0));
    s.cancel_liquify(t);
}

#[test]
fn reconstruct_smooth_and_reset_work_against_the_original() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "e.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let t = s.begin_liquify(layer, None).unwrap().token;
    let warp = |s: &DocumentSession| {
        s.liquify_brush_points(
            t,
            LiquifyTool::ForwardWarp,
            brush(30.0),
            path([16.0, 32.0], [40.0, 32.0], 24),
        )
        .unwrap();
        s.liquify_end_stroke(t).unwrap();
    };
    warp(&s);
    let warped = s.liquify_engine_mesh(t).unwrap();
    let d0 = warped.displacement_at(32.0, 32.0);
    // The Reconstruct brush pulls back towards the original.
    s.liquify_brush_points(
        t,
        LiquifyTool::Reconstruct,
        LiquifyBrush {
            rate: 0.5,
            ..brush(30.0)
        },
        vec![LiquifyPoint {
            x: 32.0,
            y: 32.0,
            pressure: 1.0,
        }],
    )
    .unwrap();
    s.liquify_end_stroke(t).unwrap();
    let d1 = s
        .liquify_engine_mesh(t)
        .unwrap()
        .displacement_at(32.0, 32.0);
    assert!(d1[0].hypot(d1[1]) < d0[0].hypot(d0[1]) * 0.9);
    // Smooth changes the field without adding new motion.
    s.liquify_brush_points(
        t,
        LiquifyTool::Smooth,
        brush(30.0),
        vec![LiquifyPoint {
            x: 24.0,
            y: 32.0,
            pressure: 1.0,
        }],
    )
    .unwrap();
    s.liquify_end_stroke(t).unwrap();
    let smooth = s.liquify_mesh(t).unwrap();
    assert!(
        smooth.max_displacement
            <= warped
                .displacement
                .iter()
                .map(|d| d[0].hypot(d[1]))
                .fold(0.0, f32::max)
                + 1e-4
    );
    // Reconstruct (whole mesh) 100 % restores; Reset clears.
    s.liquify_reconstruct_all(t, 1.0).unwrap();
    assert_eq!(s.liquify_mesh(t).unwrap().max_displacement, 0.0);
    warp(&s);
    s.liquify_reconstruct_all(t, 0.5).unwrap();
    let half = s
        .liquify_engine_mesh(t)
        .unwrap()
        .displacement_at(32.0, 32.0);
    assert!(half[0].abs() > 0.0);
    s.liquify_reset(t, true).unwrap();
    assert_eq!(s.liquify_mesh(t).unwrap().max_displacement, 0.0);
    s.preview_liquify(t, false).unwrap();
    let after_reset = s.liquify_preview_pixels(t).unwrap();
    s.preview_liquify(t, true).unwrap();
    assert_eq!(s.liquify_preview_pixels(t).unwrap(), after_reset);
    assert!(s.liquify_reconstruct_all(t, 1.5).is_err());
    s.cancel_liquify(t);
}

#[test]
fn apply_is_one_undoable_node_with_exact_undo_and_redo() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "f.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let t = s.begin_liquify(layer, None).unwrap().token;
    for _ in 0..3 {
        s.liquify_brush_points(
            t,
            LiquifyTool::TwirlClockwise,
            brush(40.0),
            vec![
                LiquifyPoint {
                    x: 30.0,
                    y: 30.0,
                    pressure: 1.0
                };
                4
            ],
        )
        .unwrap();
        s.preview_liquify(t, false).unwrap();
    }
    assert_eq!(history(&s), n, "strokes and previews are workspace state");
    s.commit_liquify(t, LiquifyDestination::CurrentLayer)
        .unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Liquify");
    let after = pixels(&s);
    assert_ne!(after, before);
    s.undo().unwrap();
    assert_eq!(pixels(&s), before);
    s.redo().unwrap();
    assert_eq!(pixels(&s), after);
}

#[test]
fn new_layer_and_pixel_to_smart_filter_destinations_are_one_node() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "g.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let warp = |t: u64| {
        s.liquify_brush_points(
            t,
            LiquifyTool::ForwardWarp,
            brush(30.0),
            path([20.0, 20.0], [36.0, 30.0], 16),
        )
        .unwrap();
    };
    let t = s.begin_liquify(layer, None).unwrap().token;
    warp(t);
    let mesh = s.liquify_engine_mesh(t).unwrap();
    let u = s.commit_liquify(t, LiquifyDestination::NewLayer).unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(u.created.len(), 1);
    let rows = s.layers().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].name,
        format!("{} (Liquify)", rows[1].name),
        "added above"
    );
    // The copy alone (the source hidden) is the liquified layer.
    s.set_visible(layer, false).unwrap();
    assert!(max_diff(&pixels(&s), &reference(&mesh, 64, 64, &before)) <= EPS8);
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.layers().unwrap().len(), 1);

    // Pixel layer → smart object with the Liquify filter, same id, one node.
    let t = s.begin_liquify(layer, None).unwrap().token;
    warp(t);
    let n = history(&s);
    let u = s
        .commit_liquify(t, LiquifyDestination::SmartFilter)
        .unwrap();
    assert!(u.created.is_empty());
    assert_eq!(history(&s), n + 1);
    let row = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.id == layer)
        .unwrap();
    assert_eq!(row.kind, DocLayerKind::SmartObject);
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].filter_id, "liquify");
    assert!(max_diff(&presented(&s), &reference(&mesh, 64, 64, &before)) <= 2.0 / 255.0);
    // The smart filter preserves the source: disabling it shows the original.
    s.set_smart_filter(layer, 0, SmartFilterEdit::Enabled { enabled: false })
        .unwrap();
    assert!(max_diff(&presented(&s), &before) <= 1.0 / 255.0);
    s.undo().unwrap();
    s.undo().unwrap();
    let row = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|r| r.id == layer)
        .unwrap();
    assert_eq!(row.kind, DocLayerKind::Pixel);
    assert_eq!(pixels(&s), before);
}

#[test]
fn smart_filter_re_edit_replaces_in_place_and_round_trips() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "h.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    s.convert_for_smart_filters(layer).unwrap();
    let info = s.begin_liquify(layer, None).unwrap();
    assert!(info.smart_object);
    s.liquify_brush_points(
        info.token,
        LiquifyTool::Pucker,
        brush(36.0),
        vec![
            LiquifyPoint {
                x: 32.0,
                y: 32.0,
                pressure: 1.0
            };
            5
        ],
    )
    .unwrap();
    let first = s.liquify_engine_mesh(info.token).unwrap();
    assert!(
        s.commit_liquify(info.token, LiquifyDestination::NewLayer)
            .is_err(),
        "new layer output needs a pixel layer"
    );
    s.commit_liquify(info.token, LiquifyDestination::SmartFilter)
        .unwrap();
    let applied = presented(&s);
    assert!(max_diff(&applied, &reference(&first, 64, 64, &before)) <= 2.0 / 255.0);
    assert_eq!(s.smart_filters(layer).unwrap().len(), 1);

    // Re-edit stage 0: the stored mesh comes back, and applying replaces it.
    let again = s.begin_liquify(layer, Some(0)).unwrap();
    assert_eq!(again.stage_index, Some(0));
    assert!(again.edited);
    assert_eq!(s.liquify_engine_mesh(again.token).unwrap(), first);
    s.liquify_brush_points(
        again.token,
        LiquifyTool::ForwardWarp,
        brush(20.0),
        path([10.0, 10.0], [20.0, 12.0], 10),
    )
    .unwrap();
    let second = s.liquify_engine_mesh(again.token).unwrap();
    let n = history(&s);
    s.commit_liquify(again.token, LiquifyDestination::CurrentLayer)
        .unwrap();
    assert_eq!(history(&s), n + 1);
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1, "no duplicate appended");
    let stored: serde_json::Value = serde_json::from_str(&list[0].filter_json).unwrap();
    let stored: Mesh = serde_json::from_value(stored["params"]["mesh"].clone()).unwrap();
    assert_eq!(stored, second);
    let edited = presented(&s);
    // A non-Liquify stage cannot be opened as Liquify.
    s.apply_filter(
        layer,
        r#"{"id":"gaussian_blur","params":{"radius":1}}"#.into(),
    )
    .unwrap();
    assert!(s.begin_liquify(layer, Some(1)).is_err());
    assert!(s.begin_liquify(layer, Some(7)).is_err());
    s.undo().unwrap();

    // Native save / reopen keeps the same appearance and stage.
    let path = dir.path().join("liquify.tessera-doc");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = open(&engine, &path);
    let id = r
        .layers()
        .unwrap()
        .into_iter()
        .find(|n| n.kind == DocLayerKind::SmartObject)
        .unwrap()
        .id;
    assert_eq!(r.smart_filters(id).unwrap(), list);
    assert!(max_diff(&presented(&r), &edited) <= 1e-3);
    let reopened = r.begin_liquify(id, Some(0)).unwrap();
    assert_eq!(r.liquify_engine_mesh(reopened.token).unwrap(), second);
    r.cancel_liquify(reopened.token);
}

#[test]
fn selection_freezes_outside_and_clips_the_write() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "i.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(0, 0, 32, 64, 0.0).unwrap();
    let before = pixels(&s);
    let info = s.begin_liquify(layer, None).unwrap();
    assert!(info.selection_frozen);
    let m = s.liquify_mesh(info.token).unwrap();
    let cols = m.columns as usize;
    assert_eq!(m.freeze[4 * cols + 1], 0.0, "inside the selection: thawed");
    assert_eq!(m.freeze[4 * cols + 12], 1.0, "outside: frozen");
    s.liquify_brush_points(
        info.token,
        LiquifyTool::ForwardWarp,
        brush(30.0),
        path([20.0, 32.0], [44.0, 32.0], 24),
    )
    .unwrap();
    assert!(
        s.commit_liquify(info.token, LiquifyDestination::SmartFilter)
            .is_err(),
        "smart output refuses a selection"
    );
    s.commit_liquify(info.token, LiquifyDestination::CurrentLayer)
        .unwrap();
    let after = pixels(&s);
    let mut changed_inside = false;
    for y in 0..64usize {
        for x in 0..64usize {
            let i = (y * 64 + x) * 4;
            if x >= 32 {
                assert_eq!(after[i..i + 4], before[i..i + 4], "({x},{y}) outside");
            } else if after[i..i + 4] != before[i..i + 4] {
                changed_inside = true;
            }
        }
    }
    assert!(changed_inside);
}

#[test]
fn full_resolution_output_from_a_fit_size_proxy() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "wide.png", 4200, 160));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let info = s.begin_liquify(layer, None).unwrap();
    assert_eq!(info.preview_factor, 3);
    assert_eq!((info.preview_width, info.preview_height), (1400, 54));
    // A drag given in full-resolution source pixels (the app maps its fit view).
    s.liquify_brush_points(
        info.token,
        LiquifyTool::ForwardWarp,
        brush(120.0),
        path([2000.0, 80.0], [2060.0, 80.0], 30),
    )
    .unwrap();
    let p = s.preview_liquify(info.token, false).unwrap();
    assert_eq!((p.width, p.height), (1400, 54));
    let mesh = s.liquify_engine_mesh(info.token).unwrap();
    s.commit_liquify(info.token, LiquifyDestination::CurrentLayer)
        .unwrap();
    let (w, h, after) = s.read_level(0).unwrap();
    assert_eq!((w, h), (4200, 160), "full-resolution result");
    assert!(max_diff(&after, &reference(&mesh, 4200, 160, &before)) <= EPS8);
}

#[test]
fn cancel_stale_deleted_and_locked_targets_never_commit() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "j.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let warp = |t: u64| {
        s.liquify_brush_points(
            t,
            LiquifyTool::ForwardWarp,
            brush(30.0),
            path([20.0, 20.0], [36.0, 30.0], 16),
        )
        .unwrap();
    };
    // Cancel: the token is gone, nothing changes.
    let before = pixels(&s);
    let n = history(&s);
    let t = s.begin_liquify(layer, None).unwrap().token;
    warp(t);
    s.cancel_liquify(t);
    assert!(
        s.commit_liquify(t, LiquifyDestination::CurrentLayer)
            .is_err()
    );
    assert!(s.preview_liquify(t, false).is_err());
    assert_eq!((history(&s), pixels(&s)), (n, before.clone()));
    // Stale: the layer changed after the workspace opened.
    let t = s.begin_liquify(layer, None).unwrap().token;
    warp(t);
    s.set_selection_rect(0, 0, 8, 8, 0.0).unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.0,
                g: 1.0,
                b: 0.0,
            },
        },
        1.0,
    )
    .unwrap();
    let n = history(&s);
    let changed = pixels(&s);
    let e = s
        .commit_liquify(t, LiquifyDestination::CurrentLayer)
        .unwrap_err();
    assert!(e.to_string().contains("changed"), "{e}");
    assert_eq!((history(&s), pixels(&s)), (n, changed));
    s.cancel_liquify(t);
    // A second workspace replaces the first.
    let a = s.begin_liquify(layer, None).unwrap().token;
    let b = s.begin_liquify(layer, None).unwrap().token;
    assert!(s.liquify_mesh(a).is_err() && s.liquify_mesh(b).is_ok());
    // Deleted target.
    s.clear_selection().unwrap();
    let extra = s
        .add_layer(NewLayer::Pixel, "extra".into(), None, None)
        .unwrap()
        .created[0];
    let t = s.begin_liquify(extra, None).unwrap().token;
    s.remove_layer(extra).unwrap();
    let n = history(&s);
    assert!(
        s.commit_liquify(t, LiquifyDestination::CurrentLayer)
            .is_err()
    );
    assert_eq!(history(&s), n);
    // Locks and non-raster targets.
    s.set_locks(
        layer,
        LayerLocks {
            transparency: false,
            pixels: true,
            position: false,
            all: false,
        },
    )
    .unwrap();
    assert!(s.begin_liquify(layer, None).is_err());
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: r#"{"kind":"exposure","exposure":1,"offset":0,"gamma":1}"#.into(),
            },
            "adj".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    assert!(s.begin_liquify(adj, None).is_err());
    // Another document's token is refused.
    let other = open(&engine, &checker(dir.path(), "k.png", 16, 16));
    assert!(other.liquify_mesh(b).is_err());
    // Invalid brush controls.
    assert!(
        s.liquify_brush_points(
            b,
            LiquifyTool::Bloat,
            LiquifyBrush {
                density: 2.0,
                ..brush(10.0)
            },
            path([1.0, 1.0], [2.0, 2.0], 1)
        )
        .is_err()
    );
}

/// Installs an apply checkpoint hook on `s` that records every site and, the
/// first time `site` is reached, runs `cancel` (on the committing thread, at
/// exactly that point). Returns the recorded sites.
fn cancel_at(
    s: &Arc<DocumentSession>,
    site: &'static str,
    cancel: impl Fn(&DocumentSession) + Send + Sync + 'static,
) -> Arc<Mutex<Vec<String>>> {
    let seen = Arc::new(Mutex::new(Vec::<String>::new()));
    let (log, weak, fired) = (seen.clone(), Arc::downgrade(s), AtomicBool::new(false));
    s.set_apply_checkpoint_hook(Some(Arc::new(move |at: &str| {
        log.lock().unwrap().push(at.to_owned());
        if at == site
            && !fired.swap(true, Ordering::SeqCst)
            && let Some(s) = weak.upgrade()
        {
            cancel(&s);
        }
    })));
    seen
}

fn deform(s: &DocumentSession, t: u64) {
    s.liquify_brush_points(
        t,
        LiquifyTool::ForwardWarp,
        brush(30.0),
        path([20.0, 32.0], [40.0, 34.0], 12),
    )
    .unwrap();
    s.liquify_end_stroke(t).unwrap();
}

/// A cancel that lands after every earlier check, at the last moment before
/// the history write (under the document lock), still commits nothing, at
/// every destination of a pixel layer. Without a cancel the same apply
/// reaches the same checkpoint and commits one node.
#[test]
fn a_cancel_just_before_the_write_never_reaches_history_on_a_pixel_layer() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "cw.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let rows = s.layers().unwrap();
    for dest in [
        LiquifyDestination::CurrentLayer,
        LiquifyDestination::NewLayer,
        LiquifyDestination::SmartFilter,
    ] {
        let n = history(&s);
        let t = s.begin_liquify(layer, None).unwrap().token;
        deform(&s, t);
        let seen = cancel_at(&s, "write", move |s| s.cancel_liquify(t));
        let e = s
            .commit_liquify(t, dest)
            .expect_err("a cancelled apply must not commit");
        s.set_apply_checkpoint_hook(None);
        assert!(e.to_string().contains("cancelled"), "{dest:?}: {e}");
        assert!(
            seen.lock().unwrap().iter().any(|x| x == "write"),
            "{dest:?}: the cancel landed at the write checkpoint"
        );
        assert_eq!(history(&s), n, "{dest:?}: no history node");
        assert_eq!(s.layers().unwrap(), rows, "{dest:?}: layers unchanged");
        assert_eq!(pixels(&s), before, "{dest:?}: pixels unchanged");
        assert!(s.liquify_mesh(t).is_err(), "{dest:?}: workspace closed");

        // Control: the same apply without the cancel commits one node.
        let t = s.begin_liquify(layer, None).unwrap().token;
        deform(&s, t);
        let seen = cancel_at(&s, "never", |_| {});
        s.commit_liquify(t, dest).unwrap();
        s.set_apply_checkpoint_hook(None);
        assert!(seen.lock().unwrap().iter().any(|x| x == "write"));
        assert_eq!(history(&s), n + 1, "{dest:?}");
        s.undo().unwrap();
        assert_eq!(pixels(&s), before);
    }
}

/// Smart objects commit through the smart-filter path, which validates the
/// whole stack by rendering it under the document lock: a cancel there (a
/// new Liquify filter or a re-edit) must also commit nothing.
#[test]
fn a_cancel_just_before_the_write_never_reaches_history_on_a_smart_object() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "cs.png", 64, 64));
    let layer = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(layer).unwrap();
    let shown = presented(&s);
    for stage in [None, Some(0)] {
        if stage.is_some() {
            // Something to re-edit.
            let t = s.begin_liquify(layer, None).unwrap().token;
            deform(&s, t);
            s.commit_liquify(t, LiquifyDestination::SmartFilter)
                .unwrap();
        }
        let n = history(&s);
        let list = s.smart_filters(layer).unwrap();
        let shown_now = presented(&s);
        let t = s.begin_liquify(layer, stage).unwrap().token;
        deform(&s, t);
        let seen = cancel_at(&s, "write", move |s| s.cancel_liquify(t));
        let e = s
            .commit_liquify(t, LiquifyDestination::SmartFilter)
            .expect_err("a cancelled apply must not commit");
        s.set_apply_checkpoint_hook(None);
        assert!(e.to_string().contains("cancelled"), "{stage:?}: {e}");
        assert!(seen.lock().unwrap().iter().any(|x| x == "write"));
        assert_eq!(history(&s), n, "{stage:?}: no history node");
        assert_eq!(s.smart_filters(layer).unwrap(), list, "{stage:?}");
        assert_eq!(presented(&s), shown_now, "{stage:?}");
        assert!(s.liquify_mesh(t).is_err());
    }
    s.undo().unwrap();
    assert!(max_diff(&presented(&s), &shown) <= 1e-6);
}

/// A cancel during the full-resolution render (deterministically: issued at
/// the render checkpoint) stops the render and commits nothing.
#[test]
fn cancel_during_a_full_resolution_apply_leaves_no_late_result() {
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(4000, 3000, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    s.select_all().unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.3,
                g: 0.5,
                b: 0.7,
            },
        },
        1.0,
    )
    .unwrap();
    s.clear_selection().unwrap();
    let t = s.begin_liquify(layer, None).unwrap().token;
    // Deform the whole canvas so the render has real work.
    for y in (100..3000).step_by(400) {
        s.liquify_brush_points(
            t,
            LiquifyTool::ForwardWarp,
            brush(800.0),
            path([200.0, y as f32], [3800.0, y as f32], 60),
        )
        .unwrap();
        s.liquify_end_stroke(t).unwrap();
    }
    let n = history(&s);
    let at = Arc::new(Mutex::new(None::<Instant>));
    let mark = at.clone();
    let seen = cancel_at(&s, "liquify:render", move |s| {
        *mark.lock().unwrap() = Some(Instant::now());
        s.cancel_liquify(t);
    });
    let r = s.commit_liquify(t, LiquifyDestination::CurrentLayer);
    s.set_apply_checkpoint_hook(None);
    let stop = at
        .lock()
        .unwrap()
        .expect("cancelled at the render")
        .elapsed();
    println!(
        "liquify apply cancel: returned {:.1} ms after the cancel",
        stop.as_secs_f64() * 1e3
    );
    assert!(r.is_err(), "a cancelled render never commits");
    assert!(
        !seen.lock().unwrap().iter().any(|x| x == "write"),
        "never reached the write"
    );
    assert_eq!(history(&s), n);
    assert!(s.liquify_mesh(t).is_err());
}

/// Measured brush latency (points → mesh → preview surface) on a 20 MP layer.
#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "release-only latency bound: skipped in debug builds"
)]
fn brush_latency_on_a_20_megapixel_layer() {
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(5472, 3648, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    s.select_all().unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.6,
                g: 0.4,
                b: 0.3,
            },
        },
        1.0,
    )
    .unwrap();
    s.clear_selection().unwrap();
    let started = Instant::now();
    let info = s.begin_liquify(layer, None).unwrap();
    let open_ms = started.elapsed().as_secs_f64() * 1e3;
    let mut events = Vec::new();
    let mut brush_only = Vec::new();
    let mut preview_only = Vec::new();
    // 120 pointer events of 4 samples each (a ~60 Hz drag with coalescing).
    for i in 0..120 {
        let x = 1000.0 + i as f32 * 25.0;
        let t0 = Instant::now();
        s.liquify_brush_points(
            info.token,
            LiquifyTool::ForwardWarp,
            brush(300.0),
            path([x, 1800.0], [x + 25.0, 1810.0], 4),
        )
        .unwrap();
        let t1 = Instant::now();
        s.preview_liquify(info.token, false).unwrap();
        let t2 = Instant::now();
        brush_only.push((t1 - t0).as_secs_f64() * 1e3);
        preview_only.push((t2 - t1).as_secs_f64() * 1e3);
        events.push((t2 - t0).as_secs_f64() * 1e3);
    }
    let stat = |v: &mut Vec<f64>| {
        v.sort_by(|a, b| a.partial_cmp(b).unwrap());
        (v[v.len() / 2], v[v.len() * 95 / 100], v[v.len() - 1])
    };
    let (m, p95, max) = stat(&mut events);
    let (bm, _, _) = stat(&mut brush_only);
    let (pm, _, _) = stat(&mut preview_only);
    let started = Instant::now();
    s.commit_liquify(info.token, LiquifyDestination::CurrentLayer)
        .unwrap();
    let commit_ms = started.elapsed().as_secs_f64() * 1e3;
    println!(
        "LIQUIFY 20MP ({}x{}, cell {}, proxy {}x{} /{}): open {open_ms:.0} ms; \
         brush+preview per event median {m:.1} ms, p95 {p95:.1} ms, max {max:.1} ms \
         (brush {bm:.1} ms, preview {pm:.1} ms median); full-res apply {commit_ms:.0} ms",
        info.width,
        info.height,
        info.cell_size,
        info.preview_width,
        info.preview_height,
        info.preview_factor
    );
    assert!(p95 < 250.0, "brush latency p95 {p95:.1} ms");
}
