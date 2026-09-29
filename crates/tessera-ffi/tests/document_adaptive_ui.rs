//! Adaptive Wide Angle for the app (WP B5-20): the `adaptive_wide_angle`
//! smart filter through the session. Previews never touch history; apply is
//! one node (pixel layer destructive, smart object a smart filter) matching
//! the engine's stage render, with exact undo; re-edit replaces in place and
//! survives native save/reopen with params kept exactly; PSD stays
//! native-only with a matching rasterized copy; conflicting / degenerate
//! constraints and layers over 100 MP error with history unchanged; real-size
//! layers (over the dense lattice) apply through the coarse lattice; unknown
//! ids degrade as any unknown smart filter.
#![cfg(target_os = "macos")]

use compositor::{
    document::SmartFilter,
    geom::Rect,
    raster::{Depth, Raster},
    render::smart_filters::{FilterContext, SmartFilterEvaluator},
};
use engine_api::tile::Extent;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tessera_ffi::*;
use transform::adaptive::{Adaptive, CameraModel, LineConstraint, LineOrientation, Projection};

const W: u32 = 96;
const H: u32 = 72;
/// Quantization of an 8-bit document plus float slack.
const EPS8: f32 = 0.5 / 255.0 + 1e-5;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// 6 px checker, opaque.
fn checker(dir: &Path, name: &str) -> PathBuf {
    let img = image::RgbaImage::from_fn(W, H, |x, y| {
        if (x / 6 + y / 6) % 2 == 0 {
            image::Rgba([40, 60, 200, 255])
        } else {
            image::Rgba([240, 230, 90, 255])
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

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// Equidistant fisheye with one traced vertical edge.
fn fisheye() -> Adaptive {
    let (cx, cy, f) = (48.0, 36.0, 52.0);
    let observed = |p: [f64; 2]| {
        let (x, y) = (p[0] - cx, p[1] - cy);
        let r = x.hypot(y);
        let k = if r < 1e-12 {
            1.
        } else {
            f * (r / f).atan() / r
        };
        [cx + k * x, cy + k * y]
    };
    let mut a = Adaptive::new(
        W as usize,
        H as usize,
        CameraModel::Manual {
            focal_px: 60.,
            center: [cx, cy],
            projection: Projection::Equidistant,
        },
    );
    a.lines.push(LineConstraint {
        points: (0..=8)
            .map(|i| observed([26.0, 14.0 + 5.5 * i as f64]))
            .collect(),
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    a.scale = 0.9;
    a
}

fn json(a: &Adaptive) -> String {
    serde_json::to_string(a).unwrap()
}

/// The engine's stage render of `a` over straight RGBA `px`, clamped to the
/// 8-bit document's range (the bicubic kernel overshoots).
fn reference(a: &Adaptive, px: &[f32]) -> Vec<f32> {
    let mut r = Raster::new(Extent::new(W, H), 4, Depth::F32, 0.0);
    r.edit_region(Rect::new(0, 0, i64::from(W), i64::from(H)), 1, |x, y, p| {
        let i = ((y * W + x) * 4) as usize;
        p.copy_from_slice(&px[i..i + 4]);
    })
    .unwrap();
    let out = filters::CompositorFilters
        .evaluate(
            &r,
            &SmartFilter {
                name: "adaptive_wide_angle".into(),
                enabled: true,
                params: serde_json::to_value(a).unwrap(),
                ..Default::default()
            },
            &FilterContext {
                profile: None,
                level: 0,
                canvas: Extent::new(W, H),
            },
        )
        .unwrap();
    let mut v = Vec::new();
    for y in 0..H {
        for x in 0..W {
            v.extend(out.pixel(x, y).map(|c| c.clamp(0.0, 1.0)));
        }
    }
    v
}

#[test]
fn begin_returns_a_default_recipe_and_previews_record_no_history() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "a.png"));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    assert!(!info.smart_object);
    assert_eq!((info.width, info.height), (W, H));
    assert_eq!(info.preview_factor, 1);
    assert_eq!(info.exif_focal_35mm, None, "a file outside the library");
    let recipe: Adaptive = serde_json::from_str(&info.recipe_json).unwrap();
    assert_eq!([recipe.source_width, recipe.source_height], [96, 72]);
    assert!(matches!(
        recipe.camera,
        CameraModel::Manual {
            projection: Projection::Rectilinear,
            ..
        }
    ));
    assert!(recipe.lines.is_empty());

    let original = s.preview_adaptive_wide_angle(info.token, None).unwrap();
    assert!(original.original);
    let src = s.adaptive_wide_angle_preview_pixels(info.token).unwrap();
    s.preview_adaptive_wide_angle(info.token, Some(info.recipe_json.clone()))
        .unwrap();
    let identity = s.adaptive_wide_angle_preview_pixels(info.token).unwrap();
    let off = src
        .iter()
        .zip(&identity)
        .map(|(a, b)| a.abs_diff(*b))
        .max()
        .unwrap();
    assert!(off <= 1, "default rectilinear recipe is ~identity ({off})");
    let p = s
        .preview_adaptive_wide_angle(info.token, Some(json(&fisheye())))
        .unwrap();
    assert!(!p.original && (p.width, p.height) == (W, H));
    let corrected = s.adaptive_wide_angle_preview_pixels(info.token).unwrap();
    assert_ne!(corrected, src);
    // At factor 1 the preview is the full render (8-bit).
    let want = reference(&fisheye(), &before);
    let worst = corrected
        .iter()
        .zip(&want)
        .map(|(a, b)| (f32::from(*a) / 255.0 - b.clamp(0.0, 1.0)).abs())
        .fold(0.0f32, f32::max);
    assert!(worst <= 1.0 / 255.0, "preview vs full render {worst}");
    assert_eq!((history(&s), pixels(&s)), (n, before));
    s.cancel_adaptive_wide_angle(info.token);
    assert!(s.preview_adaptive_wide_angle(info.token, None).is_err());
}

#[test]
fn pixel_apply_is_one_node_matching_the_engine_with_exact_undo() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "b.png"));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let t = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    s.commit_adaptive_wide_angle(t, json(&fisheye())).unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Adaptive Wide Angle");
    let after = pixels(&s);
    assert!(max_diff(&after, &reference(&fisheye(), &before)) <= EPS8);
    assert!(s.smart_filters(layer).unwrap().is_empty(), "destructive");
    assert!(
        s.commit_adaptive_wide_angle(t, json(&fisheye())).is_err(),
        "the workspace closed on success"
    );
    s.undo().unwrap();
    assert_eq!(pixels(&s), before);
    s.redo().unwrap();
    assert_eq!(pixels(&s), after);
}

#[test]
fn smart_filter_apply_re_edit_and_native_reopen_keep_params_exactly() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "c.png"));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    s.convert_for_smart_filters(layer).unwrap();
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    assert!(info.smart_object && info.stage_index.is_none());
    let n = history(&s);
    let first = fisheye();
    s.commit_adaptive_wide_angle(info.token, json(&first))
        .unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Adaptive Wide Angle");
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].filter_id, "adaptive_wide_angle");
    let stored: serde_json::Value = serde_json::from_str(&list[0].filter_json).unwrap();
    assert_eq!(stored["params"], serde_json::to_value(&first).unwrap());
    assert!(max_diff(&presented(&s), &reference(&first, &before)) <= 2.0 / 255.0);

    // Re-edit: the stored recipe comes back; OK replaces it in place.
    let again = s.begin_adaptive_wide_angle(layer, Some(0)).unwrap();
    assert_eq!(again.stage_index, Some(0));
    let back: Adaptive = serde_json::from_str(&again.recipe_json).unwrap();
    assert_eq!(back, first);
    let mut second = first.clone();
    second.scale = 0.8;
    second.lines[0].orientation = LineOrientation::Straight;
    let n = history(&s);
    s.commit_adaptive_wide_angle(again.token, json(&second))
        .unwrap();
    assert_eq!(history(&s), n + 1);
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1, "no duplicate appended");
    let edited = presented(&s);
    assert!(max_diff(&edited, &reference(&second, &before)) <= 2.0 / 255.0);
    // Undo restores the first recipe exactly.
    s.undo().unwrap();
    let undone: serde_json::Value =
        serde_json::from_str(&s.smart_filters(layer).unwrap()[0].filter_json).unwrap();
    assert_eq!(undone["params"], serde_json::to_value(&first).unwrap());
    s.redo().unwrap();

    // Another filter's row cannot be opened as Adaptive Wide Angle.
    s.apply_filter(
        layer,
        r#"{"id":"gaussian_blur","params":{"radius":1}}"#.into(),
    )
    .unwrap();
    assert!(s.begin_adaptive_wide_angle(layer, Some(1)).is_err());
    assert!(s.begin_adaptive_wide_angle(layer, Some(7)).is_err());
    s.undo().unwrap();

    // Native save / reopen: same stage, params, appearance; still editable.
    let path = dir.path().join("awa.tessera-doc");
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
    let reopened = r.begin_adaptive_wide_angle(id, Some(0)).unwrap();
    let back: Adaptive = serde_json::from_str(&reopened.recipe_json).unwrap();
    assert_eq!(back, second, "f64 params survive save/reopen exactly");
    let mut third = second.clone();
    third.scale = 1.0;
    let n = history(&r);
    r.commit_adaptive_wide_angle(reopened.token, json(&third))
        .unwrap();
    assert_eq!(history(&r), n + 1);
    assert_eq!(r.smart_filters(id).unwrap().len(), 1);
    assert!(max_diff(&presented(&r), &reference(&third, &before)) <= 2.0 / 255.0);
}

#[test]
fn psd_save_is_native_only_and_the_rasterized_copy_matches() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "d.png"));
    let layer = s.layers().unwrap()[0].id;
    s.convert_for_smart_filters(layer).unwrap();
    let t = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    s.commit_adaptive_wide_angle(t, json(&fisheye())).unwrap();
    let native = presented(&s);
    let psd = dir.path().join("awa.psd");
    let e = s
        .save_as(psd.to_string_lossy().into_owned())
        .unwrap_err()
        .to_string();
    assert!(e.contains("native-only"), "{e}");
    let op = s.prepare_rasterized_psd_copy().unwrap();
    assert_eq!(
        op.run(psd.to_string_lossy().into_owned()).unwrap(),
        RasterizedPsdCopyOutcome::Saved
    );
    let p = open(&engine, &psd);
    let flat = pixels(&p);
    let worst = max_diff(&flat, &native);
    assert!(worst <= 2.0 / 255.0, "PSD copy differs by {worst}");
    // The session itself still holds the editable filter.
    assert_eq!(s.smart_filters(layer).unwrap().len(), 1);
}

#[test]
fn conflicting_and_degenerate_constraints_error_with_history_unchanged() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "e.png"));
    let layer = s.layers().unwrap()[0].id;
    let mut conflict = Adaptive::new(
        W as usize,
        H as usize,
        CameraModel::Manual {
            focal_px: 100.,
            center: [48., 36.],
            projection: Projection::Rectilinear,
        },
    );
    conflict.lines.push(LineConstraint {
        points: vec![[48., 10.], [48., 62.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    let mut degenerate = fisheye();
    degenerate.lines[0].points = vec![[10., 10.], [10., 10.]];
    let mut wrong_size = fisheye();
    wrong_size.source_width = 95;
    let before = pixels(&s);
    let n = history(&s);
    for smart in [false, true] {
        if smart {
            s.convert_for_smart_filters(layer).unwrap();
        }
        let n = if smart { n + 1 } else { n };
        let t = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
        for bad in [&conflict, &degenerate, &wrong_size] {
            assert!(s.preview_adaptive_wide_angle(t, Some(json(bad))).is_err());
            assert!(s.commit_adaptive_wide_angle(t, json(bad)).is_err());
            assert_eq!(history(&s), n);
        }
        assert!(
            s.commit_adaptive_wide_angle(t, "{\"not\":\"a recipe\"}".into())
                .is_err()
        );
        let mut typo = serde_json::to_value(fisheye()).unwrap();
        typo["lines"][0]["colour"] = "red".into();
        let e = s
            .commit_adaptive_wide_angle(t, typo.to_string())
            .unwrap_err();
        assert!(e.to_string().contains("colour"), "{e}");
        assert_eq!(history(&s), n);
        assert!(s.smart_filters(layer).unwrap().is_empty());
        // The workspace stays open after an error.
        s.preview_adaptive_wide_angle(t, None).unwrap();
        s.cancel_adaptive_wide_angle(t);
    }
    s.undo().unwrap();
    assert_eq!(pixels(&s), before);
}

/// A fisheye recipe for a `w × h` layer from `begin`'s default, with one
/// two-click vertical traced along the camera model's curve.
fn real_size_recipe(recipe_json: &str, w: u32, h: u32) -> Adaptive {
    let mut a: Adaptive = serde_json::from_str(recipe_json).unwrap();
    let (wf, hf) = (f64::from(w), f64::from(h));
    a.camera = CameraModel::Manual {
        focal_px: 0.4 * wf,
        center: [wf / 2., hf / 2.],
        projection: Projection::Equidistant,
    };
    a.output_focal_px = 0.4 * wf;
    let c = adaptive_wide_angle_curve(
        json(&a),
        vec![0.25 * wf, 0.2 * hf],
        vec![0.26 * wf, 0.8 * hf],
    )
    .unwrap();
    a.lines.push(LineConstraint {
        points: c.chunks(2).map(|p| [p[0], p[1]]).collect(),
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    a
}

/// B5-20b: a sample.dng-sized layer (5212 × 3468, over the dense lattice)
/// opens, previews on the proxy and applies through the coarse lattice, on a
/// pixel layer and as a smart filter (re-editable with the recipe unchanged).
#[test]
fn real_size_layers_preview_and_apply_through_the_coarse_lattice() {
    let (w, h) = (5212u32, 3468u32);
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(w, h, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    // Content: a dark vertical bar on the background.
    s.set_selection_rect(1200, 0, 400, i64::from(h), 0.0).unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.1,
                g: 0.2,
                b: 0.3,
            },
        },
        1.0,
    )
    .unwrap();
    s.select_none().unwrap();
    s.wait_idle();
    let before = s.read_level(3).unwrap();

    let n = history(&s);
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    assert_eq!((info.width, info.height), (w, h));
    assert!(info.preview_width.max(info.preview_height) <= 768);
    let a = real_size_recipe(&info.recipe_json, w, h);
    let p = s
        .preview_adaptive_wide_angle(info.token, Some(json(&a)))
        .unwrap();
    assert_eq!((p.width, p.height), (info.preview_width, info.preview_height));
    assert!(!p.original);
    assert_eq!(history(&s), n, "previews record no history");
    s.commit_adaptive_wide_angle(info.token, json(&a)).unwrap();
    assert_eq!(history(&s), n + 1);
    assert_eq!(head_label(&s), "Adaptive Wide Angle");
    s.wait_idle();
    let after = s.read_level(3).unwrap();
    assert_eq!((after.0, after.1), (before.0, before.1));
    assert!(max_diff(&after.2, &before.2) > 0.1, "the bar moved");
    s.undo().unwrap();
    s.wait_idle();
    assert_eq!(s.read_level(3).unwrap().2, before.2, "exact undo");

    // Smart filter: apply, then re-edit returns the stored recipe verbatim.
    s.convert_for_smart_filters(layer).unwrap();
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    let n = history(&s);
    s.commit_adaptive_wide_angle(info.token, json(&a)).unwrap();
    assert_eq!(history(&s), n + 1);
    let list = s.smart_filters(layer).unwrap();
    assert_eq!(list.len(), 1);
    let stored: serde_json::Value = serde_json::from_str(&list[0].filter_json).unwrap();
    assert_eq!(stored["params"], serde_json::to_value(&a).unwrap());
    s.wait_idle();
    assert!(max_diff(&s.read_presented_level(3).unwrap().2, &before.2) > 0.1);
    let again = s.begin_adaptive_wide_angle(layer, Some(0)).unwrap();
    let back: Adaptive = serde_json::from_str(&again.recipe_json).unwrap();
    assert_eq!(back, a);
    s.cancel_adaptive_wide_angle(again.token);
}

#[test]
fn layers_over_the_absolute_limit_are_refused_with_a_clear_error() {
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(12000, 9000, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    let n = history(&s);
    let e = s
        .begin_adaptive_wide_angle(layer, None)
        .unwrap_err()
        .to_string();
    assert!(
        e.contains("100 megapixels") && e.contains("12000 × 9000"),
        "{e}"
    );
    assert_eq!(history(&s), n);
}

#[test]
fn cancel_stale_locked_and_other_targets_never_commit() {
    let (dir, engine) = engine();
    let s = open(&engine, &checker(dir.path(), "f.png"));
    let layer = s.layers().unwrap()[0].id;
    let before = pixels(&s);
    let n = history(&s);
    let t = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    s.cancel_adaptive_wide_angle(t);
    assert!(s.commit_adaptive_wide_angle(t, json(&fisheye())).is_err());
    assert_eq!((history(&s), pixels(&s)), (n, before.clone()));
    // Stale: the layer changed after the workspace opened.
    let t = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    s.apply_filter(
        layer,
        r#"{"id":"gaussian_blur","params":{"radius":2}}"#.into(),
    )
    .unwrap();
    let n = history(&s);
    let changed = pixels(&s);
    let e = s
        .commit_adaptive_wide_angle(t, json(&fisheye()))
        .unwrap_err();
    assert!(e.to_string().contains("changed"), "{e}");
    assert_eq!((history(&s), pixels(&s)), (n, changed));
    // A second workspace replaces the first; other documents are refused.
    let a = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    let b = s.begin_adaptive_wide_angle(layer, None).unwrap().token;
    assert!(s.preview_adaptive_wide_angle(a, None).is_err());
    assert!(s.preview_adaptive_wide_angle(b, None).is_ok());
    let other = open(&engine, &checker(dir.path(), "g.png"));
    assert!(other.preview_adaptive_wide_angle(b, None).is_err());
    // Pixel layers have no stage to re-edit; locks and adjustment layers.
    assert!(s.begin_adaptive_wide_angle(layer, Some(0)).is_err());
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
    assert!(s.begin_adaptive_wide_angle(adj, None).is_err());
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
    assert!(s.begin_adaptive_wide_angle(layer, None).is_err());
}

/// A native document whose smart object carries `stage`.
fn doc_with_stage(dir: &Path, name: &str, stage: SmartFilter) -> PathBuf {
    use compositor::{Affine, DocState, Document, Layer, LayerKind, SmartObject};
    let e = Extent::new(W, H);
    let mut px = Raster::new(e, 4, Depth::U8, 0.0);
    px.edit_region(Rect::of_extent(e), 1, |x, _, p| {
        *p = [x as f32 / W as f32, 0.5, 0.2, 1.0]
    })
    .unwrap();
    let mut child = DocState::new(e, Depth::U8);
    let mut l = Layer::new("photo", LayerKind::Pixel(px));
    l.id = compositor::LayerId(1);
    child.next_id = 2;
    child.root.push(Arc::new(l));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(stage);
    let mut state = DocState::new(e, Depth::U8);
    let mut l = Layer::new("smart", LayerKind::SmartObject(so));
    l.id = compositor::LayerId(1);
    state.next_id = 2;
    state.root.push(Arc::new(l));
    let path = dir.join(name);
    compositor::format::save(&Document::new(state), &path).unwrap();
    path
}

#[test]
fn unknown_ids_degrade_like_any_unknown_smart_filter() {
    // What an older build sees: an id it does not know. The document opens;
    // listing its smart filters fails exactly as for any unknown id.
    let (dir, engine) = engine();
    let params = serde_json::to_value(fisheye()).unwrap();
    let stage = |name: &str| SmartFilter {
        name: name.into(),
        enabled: true,
        params: params.clone(),
        ..Default::default()
    };
    let unknown = open(
        &engine,
        &doc_with_stage(dir.path(), "future.tessera-doc", stage("future_filter")),
    );
    let id = unknown.layers().unwrap()[0].id;
    assert!(unknown.smart_filters(id).is_err());
    let known = open(
        &engine,
        &doc_with_stage(dir.path(), "awa.tessera-doc", stage("adaptive_wide_angle")),
    );
    let id = known.layers().unwrap()[0].id;
    let list = known.smart_filters(id).unwrap();
    assert_eq!(list[0].filter_id, "adaptive_wide_angle");
    let stored: serde_json::Value = serde_json::from_str(&list[0].filter_json).unwrap();
    assert_eq!(stored["params"], params);
}

/// Timing on a 4000 × 3000 layer (proxy preview and full apply). Run with
/// `cargo test --release -p tessera-ffi --test document_adaptive_ui -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_on_a_12_megapixel_layer() {
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(4000, 3000, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    let mut a: Adaptive = serde_json::from_str(&info.recipe_json).unwrap();
    a.camera = CameraModel::Manual {
        focal_px: 1800.,
        center: [2000., 1500.],
        projection: Projection::Equidistant,
    };
    a.output_focal_px = 1800.;
    // A two-click line, bent by the camera model (as the sheet draws it).
    let c = adaptive_wide_angle_curve(json(&a), vec![1000., 600.], vec![1000., 2400.]).unwrap();
    a.lines.push(LineConstraint {
        points: c.chunks(2).map(|p| [p[0], p[1]]).collect(),
        orientation: LineOrientation::Vertical,
        weight: 1.,
    });
    for _ in 0..2 {
        let p = s
            .preview_adaptive_wide_angle(info.token, Some(json(&a)))
            .unwrap();
        eprintln!(
            "preview {}×{} (factor {}): {:.0} ms",
            p.width, p.height, info.preview_factor, p.millis
        );
    }
    let t = std::time::Instant::now();
    s.commit_adaptive_wide_angle(info.token, json(&a)).unwrap();
    eprintln!("apply 4000×3000: {:.2} s", t.elapsed().as_secs_f64());
}

/// B5-20b timing on a 6000 × 4000 layer (coarse lattice). Run with
/// `cargo test --release -p tessera-ffi --test document_adaptive_ui -- --ignored --nocapture`.
#[test]
#[ignore]
fn timing_on_a_24_megapixel_layer() {
    let (w, h) = (6000u32, 4000u32);
    let (_dir, engine) = engine();
    let s = engine
        .clone()
        .new_document(w, h, DocDepth::U8, None)
        .unwrap();
    let layer = s.layers().unwrap()[0].id;
    let t = std::time::Instant::now();
    let info = s.begin_adaptive_wide_angle(layer, None).unwrap();
    eprintln!("begin 6000×4000: {:.2} s", t.elapsed().as_secs_f64());
    let a = real_size_recipe(&info.recipe_json, w, h);
    for _ in 0..2 {
        let p = s
            .preview_adaptive_wide_angle(info.token, Some(json(&a)))
            .unwrap();
        eprintln!(
            "preview {}×{} (factor {}): {:.0} ms",
            p.width, p.height, info.preview_factor, p.millis
        );
    }
    let t = std::time::Instant::now();
    s.commit_adaptive_wide_angle(info.token, json(&a)).unwrap();
    eprintln!("apply 6000×4000: {:.2} s", t.elapsed().as_secs_f64());
}
