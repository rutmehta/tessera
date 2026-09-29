//! Auto-Align Layers, Auto-Blend Layers and Photomerge into layers over the
//! bridge (WP B5-19, ACCEPTANCE 440–459): registration to a known offset and
//! canvas growth, validation that changes nothing, panorama masks summing to
//! one in the overlap with one undo, focus stacking choosing the sharper
//! source, content-aware fill only on request, Photomerge as exactly one
//! history node (into the open document or a new Untitled one), and native
//! save / reopen and the layered PSD copy keeping the masks and composite.
#![cfg(target_os = "macos")]

use compositor::{Depth, DocOp, DocState, Document, Layer, LayerId, LayerKind, Raster, Rect};
use engine_api::tile::Extent;
use std::{path::Path, sync::Arc};
use tessera_ffi::*;

const W: u32 = 240;
const H: u32 = 180;
/// Horizontal offset of the second crop.
const DX: f64 = 140.0;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// Deterministic per-pixel noise in [-1, 1].
fn noise(x: i64, y: i64) -> f64 {
    let mut h = (x as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (y as u64).wrapping_mul(0xC2B2_AE3D_27D4_EB4F);
    h ^= h >> 29;
    h = h.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    h ^= h >> 32;
    (h % 10_000) as f64 / 5_000.0 - 1.0
}

/// A textured scene (the compositor panorama test's texture plus fine grain).
fn scene(x: f64, y: f64) -> f32 {
    (0.5 + 0.12 * (x * 0.17 + y * 0.09).sin()
        + 0.1 * (x * 0.07 - y * 0.21).cos()
        + 0.12 * ((x * 0.039).sin() * 7. + (y * 0.051).cos() * 9.).sin()
        + 0.04 * noise(x.round() as i64, y.round() as i64)) as f32
}

/// `W × H` crop of the scene starting at `offset`, rotated by `angle`.
fn crop(offset: f64, angle: f64) -> Vec<[f32; 3]> {
    let (s, c) = angle.sin_cos();
    (0..W * H)
        .map(|i| {
            let (u, v) = ((i % W) as f64, (i / W) as f64);
            let p = scene(c * u - s * v + offset, s * u + c * v);
            [p, p * 0.8, p * 0.6]
        })
        .collect()
}

/// `pixels` box-blurred (radius 4) where `blur(x)` holds.
fn blurred(pixels: &[[f32; 3]], blur: impl Fn(u32) -> bool) -> Vec<[f32; 3]> {
    let mut out = pixels.to_vec();
    for y in 0..H {
        for x in 0..W {
            if !blur(x) {
                continue;
            }
            let mut sum = [0.0f32; 3];
            let mut n = 0.0;
            for yy in y.saturating_sub(4)..=(y + 4).min(H - 1) {
                for xx in x.saturating_sub(4)..=(x + 4).min(W - 1) {
                    let p = pixels[(yy * W + xx) as usize];
                    for c in 0..3 {
                        sum[c] += p[c];
                    }
                    n += 1.0;
                }
            }
            out[(y * W + x) as usize] = sum.map(|v| v / n);
        }
    }
    out
}

fn raster(pixels: &[[f32; 3]]) -> Raster {
    let e = Extent::new(W, H);
    let mut r = Raster::new(e, 4, Depth::F32, 0.);
    r.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        let c = pixels[(y * W + x) as usize];
        *p = [c[0], c[1], c[2], 1.];
    })
    .unwrap();
    r
}

/// A document with root pixel layers `(name, pixels)`, bottom first; returns
/// the session and the layer ids in the same order.
fn doc(
    engine: &Arc<Engine>,
    layers: Vec<(&str, Vec<[f32; 3]>)>,
) -> (Arc<DocumentSession>, Vec<u64>) {
    let mut d = Document::new(DocState::new(Extent::new(W, H), Depth::F32));
    let mut ids = Vec::new();
    for (name, pixels) in layers {
        let id = d
            .apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: Layer::new(name, LayerKind::Pixel(raster(&pixels))),
            })
            .unwrap()
            .created[0];
        ids.push(id.0);
    }
    let s = engine.adopt_document(d, "Stack".into());
    (s, ids)
}

fn write_png(path: &Path, pixels: &[[f32; 3]]) {
    let buf: Vec<u16> = pixels
        .iter()
        .flat_map(|p| p.map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16))
        .collect();
    image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(W, H, buf)
        .unwrap()
        .save(path)
        .unwrap();
}

fn align(mode: StackAlignMode) -> StackAlignOptions {
    StackAlignOptions {
        mode,
        reference_index: 0,
        vignette_removal: false,
        geometric_distortion: false,
        lens_corrections: vec![],
        seed: 1,
    }
}

fn blend(mode: StackBlendMode, content_aware_fill: bool) -> StackBlendOptions {
    StackBlendOptions {
        mode,
        seamless_tones: true,
        content_aware_fill,
        seed: 7,
    }
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

fn mask_at(s: &DocumentSession, id: u64, x: u32, y: u32) -> f32 {
    let st = s.document_state().unwrap();
    st.find(LayerId(id))
        .unwrap()
        .mask
        .as_ref()
        .expect("mask")
        .raster
        .pixel(x, y)[0]
}

/// First column of row `y` of the composite whose alpha exceeds one half.
fn first_opaque_column(s: &DocumentSession, y: u32) -> Option<u32> {
    let (w, _, px) = s.read_level(0).unwrap();
    (0..w).find(|x| px[((y * w + x) * 4 + 3) as usize] > 0.5)
}

fn paths(dir: &Path, crops: &[(&str, Vec<[f32; 3]>)]) -> Vec<String> {
    crops
        .iter()
        .map(|(name, pixels)| {
            let p = dir.join(format!("{name}.png"));
            write_png(&p, pixels);
            p.to_string_lossy().into_owned()
        })
        .collect()
}

#[test]
fn auto_align_registers_known_offset_grows_canvas_and_undoes_once() {
    let (_d, e) = engine();
    let (s, ids) = doc(&e, vec![("left", crop(0., 0.)), ("right", crop(DX, 0.))]);
    let history = history_len(&s);
    let u = s
        .auto_align_layers(ids.clone(), align(StackAlignMode::Collage))
        .unwrap();
    assert_eq!(history_len(&s), history + 1);
    assert_eq!(labels(&s).last().unwrap(), "Auto-Align Layers");
    assert!(u.dirty_rect.is_some());
    let info = s.info().unwrap();
    assert!(
        (W + DX as u32 - 1..=W + DX as u32 + 1).contains(&info.width),
        "canvas grows to the union: {}",
        info.width
    );
    assert!((H - 1..=H + 1).contains(&info.height), "{}", info.height);
    // Both layers keep their names and become editable (source-retaining) layers.
    let rows = s.layers().unwrap();
    for id in &ids {
        let row = rows.iter().find(|r| r.id == *id).unwrap();
        assert_eq!(row.kind, DocLayerKind::SmartObject);
    }
    // Hide the reference: the moved layer starts at the known offset (±1 px).
    s.set_visible(ids[0], false).unwrap();
    let x = first_opaque_column(&s, H / 2).expect("right layer is visible");
    assert!(
        x.abs_diff(DX as u32) <= 1,
        "registered at {x}, expected {DX}"
    );
    s.undo().unwrap();
    s.undo().unwrap();
    assert_eq!(s.info().unwrap().width, W);
    assert_eq!(history_len(&s), history + 2); // undo keeps the branch
}

#[test]
fn invalid_stacks_error_and_change_nothing() {
    let (_d, e) = engine();
    let (s, ids) = doc(
        &e,
        vec![
            ("a", crop(0., 0.)),
            ("b", crop(DX, 0.)),
            ("c", crop(40., 0.)),
            ("locked", crop(20., 0.)),
        ],
    );
    // A fill layer, a group holding "c", a position-locked layer.
    let fill = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"solid","color":[0.2,0.2,0.2]}"#.into(),
            },
            "Fill".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.group_layers(vec![ids[2]], "Group".into()).unwrap();
    let mut locks = s.layer(ids[3]).unwrap().locks;
    locks.position = true;
    s.set_locks(ids[3], locks).unwrap();

    let head = s.info().unwrap().history_head;
    let n = history_len(&s);
    let width = s.info().unwrap().width;
    let mut cases: Vec<(Vec<u64>, StackAlignOptions, &str)> = vec![
        (vec![ids[0]], align(StackAlignMode::Auto), "one layer"),
        (
            vec![ids[0], ids[0]],
            align(StackAlignMode::Auto),
            "duplicate",
        ),
        (
            vec![ids[0], 999_999],
            align(StackAlignMode::Auto),
            "unknown",
        ),
        (
            vec![ids[0], ids[2]],
            align(StackAlignMode::Auto),
            "nested in a group",
        ),
        (vec![ids[0], ids[3]], align(StackAlignMode::Auto), "locked"),
        (
            vec![ids[0], ids[1]],
            StackAlignOptions {
                reference_index: 2,
                ..align(StackAlignMode::Auto)
            },
            "reference out of range",
        ),
        (
            vec![ids[0], ids[1]],
            StackAlignOptions {
                vignette_removal: true,
                ..align(StackAlignMode::Auto)
            },
            "vignette removal without calibration",
        ),
        (
            vec![ids[0], ids[1]],
            StackAlignOptions {
                geometric_distortion: true,
                ..align(StackAlignMode::Auto)
            },
            "distortion without calibration",
        ),
    ];
    cases.push((
        vec![ids[0], fill],
        align(StackAlignMode::Auto),
        "fill layer",
    ));
    for (sel, opts, why) in cases {
        let err = s.auto_align_layers(sel.clone(), opts);
        assert!(err.is_err(), "align accepted: {why}");
        assert_eq!(s.info().unwrap().history_head, head, "{why}");
        assert_eq!(history_len(&s), n, "{why}");
        assert_eq!(s.info().unwrap().width, width, "{why}");
    }
    for (sel, why) in [
        (vec![ids[0]], "one layer"),
        (vec![ids[0], ids[2]], "nested"),
        (vec![ids[0], ids[3]], "locked"),
    ] {
        assert!(
            s.auto_blend_layers(sel, blend(StackBlendMode::Panorama, false))
                .is_err(),
            "blend accepted: {why}"
        );
        assert_eq!(history_len(&s), n, "{why}");
    }
    // The pixel-layer requirement is reported in words the sheet can show.
    let msg = format!(
        "{:?}",
        s.auto_align_layers(vec![ids[0], ids[2]], align(StackAlignMode::Auto))
            .unwrap_err()
    );
    assert!(msg.to_lowercase().contains("top-level"), "{msg}");
}

#[test]
fn stack_eligibility_reports_root_pixel_layers() {
    let (_d, e) = engine();
    let (s, ids) = doc(&e, vec![("a", crop(0., 0.)), ("b", crop(DX, 0.))]);
    let el = s.stack_eligibility(ids.clone()).unwrap();
    assert!(el.can_align, "{:?}", el.reason);
    assert!(el.can_blend, "{:?}", el.reason);
    let el = s.stack_eligibility(vec![ids[0]]).unwrap();
    assert!(!el.can_align && !el.can_blend);
    assert!(el.reason.unwrap().contains("two"));
}

#[test]
fn auto_blend_panorama_masks_sum_to_one_in_overlap_and_undo_once() {
    let (_d, e) = engine();
    let (s, ids) = doc(&e, vec![("left", crop(0., 0.)), ("right", crop(DX, 0.))]);
    s.auto_align_layers(ids.clone(), align(StackAlignMode::Collage))
        .unwrap();
    let aligned_head = s.info().unwrap().history_head;
    let n = history_len(&s);
    s.auto_blend_layers(ids.clone(), blend(StackBlendMode::Panorama, false))
        .unwrap();
    assert_eq!(history_len(&s), n + 1);
    assert_eq!(labels(&s).last().unwrap(), "Auto-Blend Layers");
    for id in &ids {
        assert!(s.layer(*id).unwrap().has_mask);
    }
    let mut checked = 0;
    for y in (20..H - 20).step_by(9) {
        for x in (DX as u32 + 8..W - 8).step_by(7) {
            let sum = mask_at(&s, ids[0], x, y) + mask_at(&s, ids[1], x, y);
            assert!((sum - 1.0).abs() < 0.02, "masks sum to {sum} at ({x}, {y})");
            checked += 1;
        }
    }
    assert!(checked > 100);
    // Outside the overlap each layer owns its side.
    assert!(mask_at(&s, ids[0], 20, 90) > 0.98);
    assert!(mask_at(&s, ids[1], W + 100, 90) > 0.98);
    s.undo().unwrap();
    assert_eq!(s.info().unwrap().history_head, aligned_head);
    for id in &ids {
        assert!(!s.layer(*id).unwrap().has_mask);
    }
}

#[test]
fn stack_images_masks_pick_the_sharper_source() {
    let (_d, e) = engine();
    let sharp = crop(0., 0.);
    let (s, ids) = doc(
        &e,
        vec![
            ("near", blurred(&sharp, |x| x >= W / 2)),
            ("far", blurred(&sharp, |x| x < W / 2)),
        ],
    );
    s.auto_blend_layers(ids.clone(), blend(StackBlendMode::StackImages, false))
        .unwrap();
    for y in [40, 90, 140] {
        assert!(
            mask_at(&s, ids[0], 40, y) > 0.5,
            "near owns the left at y {y}"
        );
        assert!(mask_at(&s, ids[1], 40, y) < 0.5);
        assert!(
            mask_at(&s, ids[1], 200, y) > 0.5,
            "far owns the right at y {y}"
        );
        assert!(mask_at(&s, ids[0], 200, y) < 0.5);
    }
}

#[test]
fn content_aware_fill_fills_transparent_corners_only_when_requested() {
    let (_d, e) = engine();
    let (s, ids) = doc(&e, vec![("a", crop(0., 0.)), ("b", crop(DX, 0.03))]);
    s.auto_align_layers(ids.clone(), align(StackAlignMode::Collage))
        .unwrap();
    let u = s
        .auto_blend_layers(ids.clone(), blend(StackBlendMode::Panorama, false))
        .unwrap();
    assert!(u.created.is_empty(), "no fill layer unless requested");
    let (w, h, composite) = s.read_level(0).unwrap();
    let holes = composite.chunks(4).filter(|p| p[3] < 1e-4).count();
    assert!(holes > 0, "rotated panorama leaves transparent corners");
    s.undo().unwrap();
    let u = s
        .auto_blend_layers(ids.clone(), blend(StackBlendMode::Panorama, true))
        .unwrap();
    assert_eq!(u.created.len(), 1);
    let st = s.document_state().unwrap();
    let fill = st.find(LayerId(u.created[0])).unwrap();
    let r = fill.raster().expect("pixel fill layer");
    let (mut filled, mut untouched) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let covered = composite[((y * w + x) * 4 + 3) as usize];
            let a = r.pixel(x, y)[3];
            if covered > 0.999 {
                assert_eq!(a, 0.0, "covered pixel ({x}, {y}) is not filled");
                untouched += 1;
            } else if covered < 1e-4 {
                assert!(a > 0.999, "hole ({x}, {y}) is filled");
                filled += 1;
            }
        }
    }
    assert!(filled > 0 && untouched > 0);
    // One undo removes the masks and the fill layer together.
    s.undo().unwrap();
    assert!(s.layers().unwrap().iter().all(|l| l.id != u.created[0]));
}

#[test]
fn photomerge_into_layers_is_one_history_node_with_named_layers() {
    let (d, e) = engine();
    let files = paths(
        d.path(),
        &[("pano-left", crop(0., 0.)), ("pano-right", crop(DX, 0.))],
    );
    let s = e.clone().new_document(W, H, DocDepth::U16, None).unwrap();
    let n = history_len(&s);
    let u = s
        .photomerge_into_layers(
            files,
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
        )
        .unwrap();
    assert_eq!(history_len(&s), n + 1);
    assert_eq!(labels(&s).last().unwrap(), "Photomerge");
    assert_eq!(u.created.len(), 2);
    let rows = s.layers().unwrap();
    let names: Vec<_> = u
        .created
        .iter()
        .map(|id| rows.iter().find(|r| r.id == *id).unwrap().name.clone())
        .collect();
    assert_eq!(names, ["pano-left", "pano-right"]);
    for id in &u.created {
        assert!(rows.iter().find(|r| r.id == *id).unwrap().has_mask);
    }
    assert!(s.info().unwrap().width >= W + DX as u32 - 1);
    s.undo().unwrap();
    assert_eq!(s.layers().unwrap().len(), 1);
    assert_eq!(s.info().unwrap().width, W);
}

#[test]
fn photomerge_rejects_bad_sources_without_a_history_node() {
    let (d, e) = engine();
    let s = e.clone().new_document(W, H, DocDepth::U16, None).unwrap();
    let n = history_len(&s);
    let files = paths(d.path(), &[("only", crop(0., 0.))]);
    let missing = d.path().join("missing.png").to_string_lossy().into_owned();
    for sources in [
        vec![],
        files.clone(),
        vec![files[0].clone(), missing],
        vec![files[0].clone(), "0123456789abcdef0123456789abcdef".into()],
    ] {
        assert!(
            s.photomerge_into_layers(
                sources.clone(),
                align(StackAlignMode::Auto),
                blend(StackBlendMode::Panorama, false)
            )
            .is_err(),
            "{sources:?}"
        );
        assert_eq!(history_len(&s), n);
    }
}

#[test]
fn photomerge_document_is_untitled_with_n_layers_and_masks_survive_save_and_psd() {
    let (d, e) = engine();
    let files = paths(d.path(), &[("pm-a", crop(0., 0.)), ("pm-b", crop(DX, 0.))]);
    let s = e
        .clone()
        .photomerge_document(
            files,
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
        )
        .unwrap();
    let info = s.info().unwrap();
    assert_eq!(info.title, "Untitled");
    assert_eq!(info.path, None);
    assert!(info.dirty);
    assert_eq!(info.layer_count, 2);
    assert_eq!(labels(&s).last().unwrap(), "Photomerge");
    assert_eq!(
        history_len(&s),
        2,
        "the new document plus one Photomerge node"
    );
    assert!(e.document_ids().contains(&s.id()));
    let rows = s.layers().unwrap();
    assert!(rows.iter().all(|r| r.has_mask));
    let before = s.read_level(0).unwrap();

    // Native save and reopen keep sources, masks and the composite.
    let native = d.path().join("pano.tessera-doc");
    s.save_as(native.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = e
        .clone()
        .open_document(native.to_string_lossy().into_owned())
        .unwrap();
    let reopened = r.layers().unwrap();
    assert_eq!(reopened.len(), 2);
    assert!(reopened.iter().all(|l| l.has_mask));
    let after = r.read_level(0).unwrap();
    assert_eq!((before.0, before.1), (after.0, after.1));
    assert!(
        before
            .2
            .iter()
            .zip(&after.2)
            .all(|(a, b)| (a - b).abs() < 1e-3)
    );

    // The layered PSD copy rasterizes the transforms and keeps the masks.
    let psd = d.path().join("pano.psd");
    let op = r.prepare_rasterized_psd_copy().unwrap();
    assert_eq!(
        op.run(psd.to_string_lossy().into_owned()).unwrap(),
        RasterizedPsdCopyOutcome::Saved
    );
    let p = e
        .clone()
        .open_document(psd.to_string_lossy().into_owned())
        .unwrap();
    let layers = p.layers().unwrap();
    assert_eq!(layers.len(), 2);
    assert!(layers.iter().all(|l| l.has_mask), "{layers:#?}");
    let flat = p.read_level(0).unwrap();
    assert_eq!((flat.0, flat.1), (after.0, after.1));
    let worst = after
        .2
        .iter()
        .zip(&flat.2)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(worst < 0.02, "PSD composite differs by {worst}");
}

#[test]
fn stack_options_default_like_photoshop() {
    let a = default_stack_align_options();
    assert_eq!(a.mode, StackAlignMode::Auto);
    assert!(!a.vignette_removal && !a.geometric_distortion);
    let b = default_stack_blend_options();
    assert_eq!(b.mode, StackBlendMode::Panorama);
    assert!(b.seamless_tones && !b.content_aware_fill);
}
