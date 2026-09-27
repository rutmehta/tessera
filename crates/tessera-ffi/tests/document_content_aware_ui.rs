//! Content-Aware Move / Extend for the app (WP B5-13): a destination
//! OUTSIDE the original selection keeps the moved subject (the generic
//! `apply_raster_filter` path clips it away), Move versus Extend, feather
//! applied once, stable seeds, mask mapping at any zoom, integer offsets,
//! cancel / stale / lock / missing-selection behaviour, one node per apply,
//! smart output with an explicit frozen mask, and B5-09 regressions.
#![cfg(target_os = "macos")]

use compositor::{geom::Rect, raster::Depth, raster::Raster};
use engine_api::tile::Extent;
use filters::caf::{ColourAdaptation, FillParams, MoveMode};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// Grassy texture with a red 8×8 subject at (10..18, 10..18).
fn scene(dir: &Path, name: &str, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        if (10..18).contains(&x) && (10..18).contains(&y) {
            return image::Rgba([220, 30, 30, 255]);
        }
        // Non-periodic texture, so fill settings matter.
        let v = ((x.wrapping_mul(2_654_435_761) ^ y.wrapping_mul(40_503)) >> 7) as u8 % 23;
        image::Rgba([60 + v, 140 + v * 2, 50 + v, 255])
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

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

const EPS8: f32 = 0.5 / 255.0 + 1e-5;
const FILL: &str = r#"{"seed":7,"iterations":3}"#;

fn red_at(px: &[f32], w: u32, x: u32, y: u32) -> bool {
    let i = ((y * w + x) * 4) as usize;
    px[i] > 0.7 && px[i + 1] < 0.3
}

/// The engine's `caf::move_or_extend` on the same pixels and mask.
fn reference(
    w: u32,
    h: u32,
    px: &[f32],
    mask: &[f32],
    offset: [i32; 2],
    mode: MoveMode,
    seam: ColourAdaptation,
) -> Vec<f32> {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
    r.edit_region(Rect::new(0, 0, i64::from(w), i64::from(h)), 1, |x, y, p| {
        let i = ((y * w + x) * 4) as usize;
        p.copy_from_slice(&px[i..i + 4]);
    })
    .unwrap();
    let fill: FillParams = serde_json::from_str(FILL).unwrap();
    let out =
        filters::caf::move_or_extend(&r, mask, offset, mode, &fill, seam, &AtomicBool::new(false))
            .unwrap()
            .composite;
    let mut v = Vec::new();
    for y in 0..h {
        for x in 0..w {
            v.extend_from_slice(&out.pixel(x, y));
        }
    }
    v
}

#[test]
fn destination_outside_the_selection_keeps_the_moved_subject_and_heals_the_source() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "a.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let outline = s.selection_outline(0).unwrap();
    let before = pixels(&s);
    let n = history(&s);
    let info = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap();
    assert_eq!(
        (
            info.selection_bounds.x,
            info.selection_bounds.y,
            info.selection_bounds.width,
            info.selection_bounds.height
        ),
        (8, 8, 12, 12)
    );
    let mask = s.content_aware_mask(info.token).unwrap();
    let p = s
        .preview_content_aware_move(info.token, 30, 6, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    let a = p.affected.unwrap();
    assert_eq!(
        (a.x, a.y, a.width, a.height),
        (8, 8, 42, 18),
        "source ∪ destination"
    );
    // The preview shows the result; the document is not changed yet.
    let shown = presented(&s);
    assert!(red_at(&shown, 64, 44, 20), "moved subject previewed");
    assert_eq!(pixels(&s), before);
    assert_eq!(history(&s), n);
    s.commit_content_aware_move(info.token).unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Content-Aware Move");
    let after = pixels(&s);
    // The whole subject landed at (40..48, 16..24), entirely outside the selection.
    for y in 16..24 {
        for x in 40..48 {
            assert!(red_at(&after, 64, x, y), "({x},{y}) moved subject");
        }
    }
    // The source was healed from its surroundings.
    let red_left = (10..18)
        .flat_map(|y| (10..18).map(move |x| (x, y)))
        .filter(|&(x, y)| red_at(&after, 64, x, y))
        .count();
    assert_eq!(red_left, 0, "source healed");
    let expected = reference(
        64,
        40,
        &before,
        &mask,
        [30, 6],
        MoveMode::Move,
        ColourAdaptation::None,
    );
    assert!(
        max_diff(&after, &expected) <= EPS8,
        "equals caf::move_or_extend"
    );
    assert_eq!(
        s.selection_outline(0).unwrap(),
        outline,
        "selection untouched"
    );

    // Regression guard: the generic adapter clips to the selection and loses
    // the destination, which is why this module does not use it.
    let g = open(&engine, &scene(dir.path(), "a2.png", 64, 40));
    let gl = g.layers().unwrap()[0].id;
    g.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let params = serde_json::json!({"mask": mask, "offset": [30, 6], "fill": serde_json::from_str::<serde_json::Value>(FILL).unwrap(), "seam": "none"});
    g.apply_raster_filter(
        gl,
        RasterFilterRequest {
            operation: RasterFilterOperation::ContentAwareMove,
            params_json: params.to_string(),
        },
    )
    .unwrap();
    assert!(
        !red_at(&pixels(&g), 64, 44, 20),
        "the generic path drops the destination"
    );
}

#[test]
fn extend_keeps_the_original_and_adds_the_copy() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "b.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let before = pixels(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Extend)
        .unwrap()
        .token;
    let mask = s.content_aware_mask(t).unwrap();
    s.preview_content_aware_move(t, 30, 6, FILL.into(), ContentAwareSeam::Default)
        .unwrap();
    s.commit_content_aware_move(t).unwrap();
    assert_eq!(head_label(&s), "Content-Aware Extend");
    let after = pixels(&s);
    for y in 10..18 {
        for x in 10..18 {
            assert_eq!(
                after[((y * 64 + x) * 4) as usize..][..4],
                before[((y * 64 + x) * 4) as usize..][..4],
                "original kept"
            );
        }
    }
    assert!(red_at(&after, 64, 44, 20), "copy added");
    let expected = reference(
        64,
        40,
        &before,
        &mask,
        [30, 6],
        MoveMode::Extend,
        ColourAdaptation::Default,
    );
    assert!(max_diff(&after, &expected) <= EPS8);
}

#[test]
fn fractional_feather_is_applied_once_not_double_clipped() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "c.png", 72, 48));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(6, 6, 16, 16, 3.0).unwrap();
    let before = pixels(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    let mask = s.content_aware_mask(t).unwrap();
    assert!(mask.iter().any(|&m| m > 0.05 && m < 0.95), "feathered");
    s.preview_content_aware_move(t, 34, 12, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    s.commit_content_aware_move(t).unwrap();
    let after = pixels(&s);
    // Everywhere — including the soft edge of the moved copy, outside the
    // selection — exactly the engine's single application of coverage.
    let expected = reference(
        72,
        48,
        &before,
        &mask,
        [34, 12],
        MoveMode::Move,
        ColourAdaptation::None,
    );
    assert!(max_diff(&after, &expected) <= EPS8);
}

#[test]
fn same_seed_repeats_and_settings_change_the_preview() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "d.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    let run = |fill: &str, seam| {
        s.preview_content_aware_move(t, 26, 4, fill.into(), seam)
            .unwrap();
        presented(&s)
    };
    let a = run(FILL, ContentAwareSeam::None);
    let b = run(FILL, ContentAwareSeam::None);
    assert_eq!(a, b, "same seed, same result");
    let c = run(FILL, ContentAwareSeam::VeryHigh);
    assert_ne!(a, c, "seam adaptation changes the preview");
    let d = run(
        r#"{"seed":7,"iterations":3,"patch_radius":6}"#,
        ContentAwareSeam::None,
    );
    assert_ne!(a, d, "fill settings change the preview");
    // Bad settings are rejected without touching the preview.
    assert!(
        s.preview_content_aware_move(
            t,
            26,
            4,
            r#"{"patch_radius":0}"#.into(),
            ContentAwareSeam::None
        )
        .is_err()
    );
    assert!(
        s.preview_content_aware_move(t, 26, 4, r#"{"typo":1}"#.into(), ContentAwareSeam::None)
            .is_err()
    );
    assert!(
        s.preview_content_aware_move(
            t,
            26,
            4,
            r#"{"output_new_layer":true}"#.into(),
            ContentAwareSeam::None
        )
        .is_err()
    );
    assert!(
        s.preview_content_aware_move(t, 64, 0, FILL.into(), ContentAwareSeam::None)
            .is_err(),
        "off canvas"
    );
    s.cancel_content_aware_move(t);
}

#[test]
fn mask_maps_one_to_one_at_any_zoom_and_offsets_are_integer_pixels() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "e.png", 96, 64));
    let layer = s.layers().unwrap()[0].id;
    s.set_viewport(2, 0, 0, 24, 16, 0.25).unwrap();
    s.set_selection_rect(9, 7, 13, 11, 0.0).unwrap();
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    let mask = s.content_aware_mask(t).unwrap();
    assert_eq!(
        mask.len(),
        96 * 64,
        "level-0 canvas mask, not the view level"
    );
    for y in 0..64 {
        for x in 0..96 {
            let inside = (9..22).contains(&x) && (7..18).contains(&y);
            assert_eq!(
                mask[y * 96 + x],
                if inside { 1.0 } else { 0.0 },
                "({x},{y})"
            );
        }
    }
    let p = s
        .preview_content_aware_move(t, 5, -3, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    let a = p.affected.unwrap();
    assert_eq!((p.dx, p.dy), (5, -3));
    assert_eq!((a.x, a.y, a.width, a.height), (9, 4, 18, 14));
    let p = s
        .preview_content_aware_move(t, -20, 0, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    let a = p.affected.unwrap();
    assert_eq!(
        (a.x, a.y, a.width, a.height),
        (0, 7, 22, 11),
        "clipped at the canvas edge"
    );
    s.cancel_content_aware_move(t);
}

#[test]
fn cancel_leaves_layer_selection_and_history_unchanged() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "f.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let outline = s.selection_outline(0).unwrap();
    let before = pixels(&s);
    let shown_before = presented(&s);
    let n = history(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    s.preview_content_aware_move(t, 30, 6, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    assert_ne!(presented(&s), shown_before);
    s.cancel_content_aware_move(t);
    assert_eq!(presented(&s), shown_before, "the preview ended");
    assert_eq!(pixels(&s), before);
    assert_eq!(history(&s), n);
    assert_eq!(s.selection_outline(0).unwrap(), outline);
    assert!(s.commit_content_aware_move(t).is_err());
    assert!(
        s.preview_content_aware_move(t, 1, 1, FILL.into(), ContentAwareSeam::None)
            .is_err()
    );
}

#[test]
fn apply_is_one_undoable_step() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "g.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let before = pixels(&s);
    let n = history(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    // Several previews (drags) before the apply add nothing to history.
    for dx in [10, 20, 30] {
        s.preview_content_aware_move(t, dx, 6, FILL.into(), ContentAwareSeam::None)
            .unwrap();
    }
    assert_eq!(history(&s), n);
    s.commit_content_aware_move(t).unwrap();
    assert_eq!(history(&s), n + 1);
    let after = pixels(&s);
    assert!(red_at(&after, 64, 44, 20), "the last preview was applied");
    s.undo().unwrap();
    assert_eq!(pixels(&s), before);
    s.redo().unwrap();
    assert_eq!(pixels(&s), after);
    assert!(
        s.commit_content_aware_move(t).is_err(),
        "closed after apply"
    );
}

#[test]
fn locks_stale_deleted_and_missing_selection_leave_no_partial_result() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "h.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    let e = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap_err();
    assert!(e.to_string().contains("selection"), "{e}");
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    // No preview yet.
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    assert!(s.commit_content_aware_move(t).is_err());
    // Stale: the layer changed after the move started.
    s.preview_content_aware_move(t, 30, 6, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.0,
                g: 0.0,
                b: 1.0,
            },
        },
        1.0,
    )
    .unwrap();
    let n = history(&s);
    let changed = pixels(&s);
    let e = s.commit_content_aware_move(t).unwrap_err();
    assert!(e.to_string().contains("changed"), "{e}");
    assert_eq!((history(&s), pixels(&s)), (n, changed));
    s.cancel_content_aware_move(t);
    // Deleted target.
    let extra = s
        .add_layer(NewLayer::Pixel, "extra".into(), None, None)
        .unwrap()
        .created[0];
    let t = s
        .begin_content_aware_move(extra, ContentAwareMode::Extend)
        .unwrap()
        .token;
    s.preview_content_aware_move(t, 3, 3, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    s.remove_layer(extra).unwrap();
    let n = history(&s);
    assert!(s.commit_content_aware_move(t).is_err());
    assert_eq!(history(&s), n);
    // Locks, non-raster layers, other documents.
    s.set_locks(
        layer,
        LayerLocks {
            transparency: false,
            pixels: false,
            position: false,
            all: true,
        },
    )
    .unwrap();
    assert!(
        s.begin_content_aware_move(layer, ContentAwareMode::Move)
            .is_err()
    );
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
    assert!(
        s.begin_content_aware_move(adj, ContentAwareMode::Move)
            .is_err()
    );
    let other = open(&engine, &scene(dir.path(), "h2.png", 64, 40));
    let ol = other.layers().unwrap()[0].id;
    other.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let ot = other
        .begin_content_aware_move(ol, ContentAwareMode::Move)
        .unwrap()
        .token;
    assert!(
        s.commit_content_aware_move(ot).is_err(),
        "another document's token"
    );
    other.cancel_content_aware_move(ot);
}

#[test]
fn transparency_lock_keeps_alpha() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "l.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_locks(
        layer,
        LayerLocks {
            transparency: true,
            pixels: false,
            position: false,
            all: false,
        },
    )
    .unwrap();
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let before = pixels(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    s.preview_content_aware_move(t, 30, 6, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    s.commit_content_aware_move(t).unwrap();
    let after = pixels(&s);
    assert!(
        before
            .chunks(4)
            .zip(after.chunks(4))
            .all(|(a, b)| a[3] == b[3])
    );
}

#[test]
fn smart_object_output_stores_an_explicit_frozen_mask_and_reopens() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "i.png", 64, 40));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    s.convert_for_smart_filters(layer).unwrap();
    s.set_selection_rect(8, 8, 12, 12, 0.0).unwrap();
    let info = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap();
    assert!(info.smart_object);
    let mask = s.content_aware_mask(info.token).unwrap();
    s.preview_content_aware_move(info.token, 30, 6, FILL.into(), ContentAwareSeam::None)
        .unwrap();
    let n = history(&s);
    s.commit_content_aware_move(info.token).unwrap();
    assert_eq!(history(&s), n + 1);
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].filter_id, "content_aware_move");
    assert!(
        !list[0].has_mask,
        "no shared filter mask from the selection"
    );
    let stored: serde_json::Value = serde_json::from_str(&list[0].filter_json).unwrap();
    let stored_mask: Vec<f32> = serde_json::from_value(stored["params"]["mask"].clone()).unwrap();
    assert_eq!(stored_mask, mask);
    assert_eq!(stored["params"]["offset"], serde_json::json!([30, 6]));
    let applied = presented(&s);
    let expected = reference(
        64,
        40,
        &before,
        &mask,
        [30, 6],
        MoveMode::Move,
        ColourAdaptation::None,
    );
    assert!(max_diff(&applied, &expected) <= 2.0 / 255.0);
    // Independent of the live selection.
    s.clear_selection().unwrap();
    assert!(max_diff(&presented(&s), &applied) <= 1e-6);
    let path = dir.path().join("cam.tessera-doc");
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
    assert!(max_diff(&presented(&r), &applied) <= 1e-3);
}

#[test]
fn cancelling_a_running_preview_discards_its_result() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "j.png", 900, 700));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(100, 100, 260, 220, 0.0).unwrap();
    let shown_before = presented(&s);
    let n = history(&s);
    let t = s
        .begin_content_aware_move(layer, ContentAwareMode::Move)
        .unwrap()
        .token;
    let worker = {
        let s = s.clone();
        std::thread::spawn(move || {
            let started = Instant::now();
            let r = s.preview_content_aware_move(
                t,
                400,
                200,
                r#"{"seed":1}"#.into(),
                ContentAwareSeam::Default,
            );
            (r, started.elapsed())
        })
    };
    std::thread::sleep(std::time::Duration::from_millis(30));
    let at = Instant::now();
    s.cancel_content_aware_move(t);
    let (r, took) = worker.join().unwrap();
    println!(
        "content-aware move preview cancel: {:?} after {:.0} ms total, {:.0} ms after cancel",
        r.as_ref().map(|_| "finished").map_err(|e| e.to_string()),
        took.as_secs_f64() * 1e3,
        at.elapsed().as_secs_f64() * 1e3
    );
    assert!(r.is_err(), "a cancelled preview never shows");
    assert_eq!(presented(&s), shown_before);
    assert_eq!(history(&s), n);
}

#[test]
fn b5_09_remove_content_aware_fill_and_neural_still_behave() {
    let (dir, engine) = engine();
    let s = open(&engine, &scene(dir.path(), "k.png", 48, 40));
    let layer = s.layers().unwrap()[0].id;
    s.set_selection_rect(10, 10, 8, 8, 0.0).unwrap();
    let n = history(&s);
    let r = s
        .remove_with_selection(layer, RemoveBackend::PatchMatch, r#"{"dilation":0}"#.into())
        .unwrap();
    assert_eq!(r.backend, "PatchMatch");
    assert_eq!((history(&s), head_label(&s)), (n + 1, "Remove".to_string()));
    let r = s.content_aware_fill_selection(layer, "{}".into()).unwrap();
    assert_eq!(r.backend, "Content-Aware Fill");
    assert_eq!(
        (history(&s), head_label(&s)),
        (n + 2, "Content-Aware Fill".to_string())
    );
    // A Remove stroke is still its own engine-side accumulator.
    s.begin_remove_stroke(layer, 5.0, RemoveBackend::PatchMatch)
        .unwrap();
    s.remove_stroke_points(vec![ToolPoint { x: 30.0, y: 30.0 }])
        .unwrap();
    s.end_remove_stroke("{}".into()).unwrap();
    assert_eq!(history(&s), n + 3);
    let before = pixels(&s);
    let e = s
        .neural_filter(
            layer,
            NeuralFilterKind::Colorize,
            "{}".into(),
            NeuralDestination::CurrentLayer,
        )
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("filters/ddcolor") && e.contains("not installed"),
        "{e}"
    );
    assert_eq!(history(&s), n + 3);
    assert_eq!(pixels(&s), before);
}
