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
            CancelFlag::new(),
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
        vec![
            files[0].clone(),
            d.path().join("x.psd").to_string_lossy().into(),
        ],
    ] {
        assert!(
            s.photomerge_into_layers(
                sources.clone(),
                align(StackAlignMode::Auto),
                blend(StackBlendMode::Panorama, false),
                CancelFlag::new(),
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
            CancelFlag::new(),
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

    // Photomerge layers are smart objects (the alignment transform stays
    // editable). The rasterized PSD copy rasterizes smart-filter stacks only,
    // so the layers come back as PSD smart objects, each with its mask.
    assert!(reopened.iter().all(|l| l.kind == DocLayerKind::SmartObject));
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
    assert!(
        layers
            .iter()
            .all(|l| l.has_mask && l.kind == DocLayerKind::SmartObject),
        "{layers:#?}"
    );
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

// ─────────────── A's B5-19 review: budget, cancel, profiles, Reposition ───────────────

/// CRC-32 (PNG chunks).
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
    }
    !c
}

/// A PNG whose header claims `w × h` but holds one pixel: only its header
/// can be read, so a decode attempt would fail differently (or allocate).
fn huge_png(path: &Path, w: u32, h: u32) {
    image::RgbImage::new(1, 1).save(path).unwrap();
    let mut b = std::fs::read(path).unwrap();
    assert_eq!(&b[12..16], b"IHDR");
    b[16..20].copy_from_slice(&w.to_be_bytes());
    b[20..24].copy_from_slice(&h.to_be_bytes());
    let crc = crc32(&b[12..29]);
    b[29..33].copy_from_slice(&crc.to_be_bytes());
    std::fs::write(path, b).unwrap();
}

#[test]
fn photomerge_refuses_over_the_pixel_budget_from_headers_before_decoding() {
    let (d, e) = engine();
    let s = e.clone().new_document(W, H, DocDepth::U16, None).unwrap();
    let n = history_len(&s);
    let docs = e.document_ids().len();
    // 3 × 10000 × 8000 = 240 MP > 200 MP; each file is really 1 × 1.
    let files: Vec<String> = (0..3)
        .map(|i| {
            let p = d.path().join(format!("huge-{i}.png"));
            huge_png(&p, 10_000, 8_000);
            p.to_string_lossy().into_owned()
        })
        .collect();
    // The budget scales with this machine's memory, never above 200 MP.
    let limit = stack_max_megapixels();
    assert!((1..=MAX_STACK_MEGAPIXELS).contains(&limit), "{limit}");
    for err in [
        s.photomerge_into_layers(
            files.clone(),
            align(StackAlignMode::Auto),
            blend(StackBlendMode::Panorama, false),
            CancelFlag::new(),
        )
        .unwrap_err(),
        e.clone()
            .photomerge_document(
                files.clone(),
                align(StackAlignMode::Auto),
                blend(StackBlendMode::Panorama, false),
                CancelFlag::new(),
            )
            .err()
            .unwrap(),
    ] {
        let msg = err.to_string();
        assert!(
            msg.contains(&format!("limited to {limit} megapixels")),
            "{msg}"
        );
        assert!(msg.contains("240 megapixels"), "{msg}");
    }
    assert_eq!(history_len(&s), n);
    assert_eq!(e.document_ids().len(), docs);
}

#[test]
fn stack_eligibility_refuses_layers_over_the_pixel_budget() {
    let (_d, e) = engine();
    let mut d = Document::new(DocState::new(Extent::new(W, H), Depth::F32));
    let mut ids = Vec::new();
    for name in ["a", "b"] {
        // Sparse (untouched) rasters: 2 × 12000 × 9000 = 216 MP, no memory.
        let r = Raster::new(Extent::new(12_000, 9_000), 4, Depth::F32, 0.);
        let id = d
            .apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: Layer::new(name, LayerKind::Pixel(r)),
            })
            .unwrap()
            .created[0];
        ids.push(id.0);
    }
    let s = e.adopt_document(d, "Huge".into());
    let el = s.stack_eligibility(ids.clone()).unwrap();
    assert!(!el.can_align && !el.can_blend);
    let limit = stack_max_megapixels();
    assert!(
        el.reason
            .unwrap()
            .contains(&format!("limited to {limit} megapixels"))
    );
    let n = history_len(&s);
    let err = s
        .auto_align_layers(ids.clone(), align(StackAlignMode::Auto))
        .unwrap_err();
    assert!(err.to_string().contains("216 megapixels"), "{err}");
    assert!(
        s.auto_blend_layers(ids, blend(StackBlendMode::Panorama, false))
            .is_err()
    );
    assert_eq!(history_len(&s), n);
}

#[test]
fn cancelled_photomerge_changes_nothing() {
    let (d, e) = engine();
    let files = paths(d.path(), &[("c-a", crop(0., 0.)), ("c-b", crop(DX, 0.))]);
    let s = e.clone().new_document(W, H, DocDepth::U16, None).unwrap();
    let n = history_len(&s);
    let docs = e.document_ids().len();
    let cancel = CancelFlag::new();
    cancel.cancel();
    let err = s
        .photomerge_into_layers(
            files.clone(),
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
            cancel.clone(),
        )
        .unwrap_err();
    assert_eq!(err.to_string(), "Photomerge was cancelled");
    let err = e
        .clone()
        .photomerge_document(
            files,
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
            cancel,
        )
        .err()
        .unwrap();
    assert_eq!(err.to_string(), "Photomerge was cancelled");
    assert_eq!(history_len(&s), n);
    assert_eq!(s.layers().unwrap().len(), 1);
    assert_eq!(e.document_ids().len(), docs);
}

fn p3_icc() -> Vec<u8> {
    color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::DisplayP3)
        .unwrap()
        .icc_bytes()
        .to_vec()
}

/// A 16-bit PNG of `pixels` with an embedded Display P3 profile.
fn write_p3_png(path: &Path, pixels: &[[f32; 3]]) {
    use image::ImageEncoder;
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|p| p.map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16))
        .flat_map(u16::to_ne_bytes)
        .collect();
    let mut enc = image::codecs::png::PngEncoder::new(std::fs::File::create(path).unwrap());
    enc.set_icc_profile(p3_icc()).unwrap();
    enc.write_image(&bytes, W, H, image::ExtendedColorType::Rgb16)
        .unwrap();
}

fn srgb_icc() -> Vec<u8> {
    color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::Srgb)
        .unwrap()
        .icc_bytes()
        .to_vec()
}

/// Converts RGB between two ICC profiles as the bridge does.
fn transform(from: &[u8], to: &[u8], px: &mut [[f32; 3]]) {
    let t: lcms2::Transform<[f32; 3], [f32; 3]> = lcms2::Transform::new_flags(
        &lcms2::Profile::new_icc(from).unwrap(),
        lcms2::PixelFormat::RGB_FLT,
        &lcms2::Profile::new_icc(to).unwrap(),
        lcms2::PixelFormat::RGB_FLT,
        lcms2::Intent::RelativeColorimetric,
        lcms2::Flags::BLACKPOINT_COMPENSATION,
    )
    .unwrap();
    t.transform_in_place(px);
}

fn q16(v: f32) -> f32 {
    ((v.clamp(0., 1.) * 65535. + 0.5) as u16) as f32 / 65535.
}

/// Composite RGB at (x, y).
fn rgb_at(s: &DocumentSession, x: u32, y: u32) -> [f32; 3] {
    let (w, _, px) = s.read_level(0).unwrap();
    let i = ((y * w + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2]]
}

/// Pixel (20, 90) lies only in the left (reference) crop.
const PROBE: (u32, u32) = (20, 90);

#[test]
fn photomerge_document_takes_the_first_photos_profile_and_converts_the_rest() {
    let (d, e) = engine();
    let left = crop(0., 0.);
    let (pa, pb) = (
        d.path().join("p3-left.png"),
        d.path().join("srgb-right.png"),
    );
    write_p3_png(&pa, &left);
    // The same scene, stored as untagged (sRGB) values.
    let mut right = crop(DX, 0.);
    transform(&p3_icc(), &srgb_icc(), &mut right);
    write_png(&pb, &right);
    let s = e
        .clone()
        .photomerge_document(
            vec![
                pa.to_string_lossy().into_owned(),
                pb.to_string_lossy().into_owned(),
            ],
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
            CancelFlag::new(),
        )
        .unwrap();
    let st = s.document_state().unwrap();
    let icc = st
        .profile
        .as_ref()
        .and_then(|p| p.icc.clone())
        .expect("an ICC profile");
    // The new document is in the first photo's Display P3, not sRGB: P3
    // red maps to itself (in sRGB it would fall outside [0, 1]).
    let mut red = [[1.0f32, 0., 0.]];
    transform(&icc, &p3_icc(), &mut red);
    assert!(
        (red[0][0] - 1.).abs() < 0.01 && red[0][1].abs() < 0.01,
        "{red:?}"
    );
    // The P3 reference comes through unconverted.
    let got = rgb_at(&s, PROBE.0, PROBE.1);
    let want = left[(PROBE.1 * W + PROBE.0) as usize].map(q16);
    for c in 0..3 {
        assert!((got[c] - want[c]).abs() < 0.01, "{got:?} vs {want:?}");
    }
}

#[test]
fn photomerge_into_an_srgb_document_converts_display_p3_photos() {
    let (d, e) = engine();
    let left = crop(0., 0.);
    let (pa, pb) = (d.path().join("p3-a.png"), d.path().join("p3-b.png"));
    write_p3_png(&pa, &left);
    write_p3_png(&pb, &crop(DX, 0.));
    let s = e.clone().new_document(W, H, DocDepth::U16, None).unwrap();
    s.set_visible(s.layers().unwrap()[0].id, false).unwrap();
    s.photomerge_into_layers(
        vec![
            pa.to_string_lossy().into_owned(),
            pb.to_string_lossy().into_owned(),
        ],
        align(StackAlignMode::Collage),
        blend(StackBlendMode::Panorama, false),
        CancelFlag::new(),
    )
    .unwrap();
    let raw = left[(PROBE.1 * W + PROBE.0) as usize].map(q16);
    let mut want = [raw];
    transform(&p3_icc(), &srgb_icc(), &mut want);
    let got = rgb_at(&s, PROBE.0, PROBE.1);
    for c in 0..3 {
        assert!(
            (got[c] - want[0][c]).abs() < 0.01,
            "{got:?} vs converted {:?} (raw {raw:?})",
            want[0]
        );
    }
    assert!(
        (got[0] - raw[0]).abs() > 0.012,
        "P3 values were ingested unconverted: {got:?} vs {raw:?}"
    );
}

/// Pins the engine bug that keeps Reposition out of `StackAlignMode`:
/// `merge::layers` registers two crops 140 px apart at about 1 px with
/// Reposition (Collage gets it right). When this fails, the engine is fixed:
/// restore `StackAlignMode::Reposition` (FFI, Swift layout list) and delete
/// this test.
#[test]
fn reposition_is_withheld_while_the_engine_misregisters_it() {
    use merge::layers::{AlignMode, AlignOptions, align_layers};
    let image = |pixels: Vec<[f32; 3]>| merge::LinearImage {
        width: W as usize,
        height: H as usize,
        pixels,
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    };
    let images = [image(crop(0., 0.)), image(crop(DX, 0.))];
    let width = |mode| {
        align_layers(
            &images,
            &AlignOptions {
                mode,
                reference: 0,
                seed: 1,
                ..AlignOptions::default()
            },
        )
        .unwrap()
        .width
    };
    let expected = (W as f64 + DX) as usize;
    assert!(width(AlignMode::Collage).abs_diff(expected) <= 2);
    let reposition = width(AlignMode::Reposition);
    assert!(
        reposition + 40 < expected,
        "Reposition now spans {reposition} px (expected {expected}): the engine is fixed, re-enable it"
    );
}

// ───────────────────────────── B5-19b follow-ups ─────────────────────────────

const GIB: u64 = 1 << 30;

#[test]
fn stack_budget_scales_with_physical_memory_up_to_200_megapixels() {
    // Half of RAM at about 50 bytes per source pixel, capped at 200 MP.
    assert_eq!(stack_megapixels_for_memory(None), 200);
    assert_eq!(stack_megapixels_for_memory(Some(64 * GIB)), 200);
    assert_eq!(stack_megapixels_for_memory(Some(20 * GIB)), 200);
    assert_eq!(stack_megapixels_for_memory(Some(16 * GIB)), 171);
    assert_eq!(stack_megapixels_for_memory(Some(8 * GIB)), 85);
    assert_eq!(stack_megapixels_for_memory(Some(1 << 20)), 1);
    assert_eq!(stack_megapixels_for_memory(Some(0)), 1);
}

#[cfg(target_os = "macos")]
#[test]
fn stack_budget_uses_this_macs_physical_memory() {
    let out = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .unwrap();
    let bytes: u64 = String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        stack_max_megapixels(),
        stack_megapixels_for_memory(Some(bytes))
    );
}

/// A 16-bit PNG of `pixels` with `icc` embedded.
fn write_tagged_png(path: &Path, pixels: &[[f32; 3]], icc: &[u8]) {
    use image::ImageEncoder;
    let bytes: Vec<u8> = pixels
        .iter()
        .flat_map(|p| p.map(|v| (v.clamp(0., 1.) * 65535. + 0.5) as u16))
        .flat_map(u16::to_ne_bytes)
        .collect();
    let mut enc = image::codecs::png::PngEncoder::new(std::fs::File::create(path).unwrap());
    enc.set_icc_profile(icc.to_vec()).unwrap();
    enc.write_image(&bytes, W, H, image::ExtendedColorType::Rgb16)
        .unwrap();
}

#[test]
fn photomerge_skips_conversion_when_the_unembedded_target_is_the_same_profile() {
    let (d, e) = engine();
    // One copy of the bytes: built-in profiles carry their creation time.
    let p3 = p3_icc();
    let files: Vec<String> = [("h-a", crop(0., 0.)), ("h-b", crop(DX, 0.))]
        .iter()
        .map(|(name, px)| {
            let p = d.path().join(format!("{name}.png"));
            write_tagged_png(&p, px, &p3);
            p.to_string_lossy().into_owned()
        })
        .collect();
    // A document tagged Display P3 by handle only (no embedded bytes): the
    // photos are in that very profile, so nothing needs converting.
    let unembedded = |icc: Vec<u8>, name: &str| {
        let p = compositor::ColorProfile::from_icc(name, icc);
        compositor::ColorProfile { icc: None, ..p }
    };
    let mut state = DocState::new(Extent::new(W, H), Depth::F32);
    state.profile = Some(unembedded(p3.clone(), "Display P3"));
    let s = e.adopt_document(Document::new(state), "Handle only".into());
    let n = history_len(&s);
    s.photomerge_into_layers(
        files.clone(),
        align(StackAlignMode::Collage),
        blend(StackBlendMode::Panorama, false),
        CancelFlag::new(),
    )
    .unwrap();
    assert_eq!(history_len(&s), n + 1);
    // Unconverted: the P3 values come through as stored.
    let got = rgb_at(&s, PROBE.0, PROBE.1);
    let want = crop(0., 0.)[(PROBE.1 * W + PROBE.0) as usize].map(q16);
    for c in 0..3 {
        assert!((got[c] - want[c]).abs() < 0.01, "{got:?} vs {want:?}");
    }
    // A different profile without bytes still cannot be converted to.
    let mut state = DocState::new(Extent::new(W, H), Depth::F32);
    state.profile = Some(unembedded(srgb_icc(), "sRGB IEC61966-2.1"));
    let s = e.adopt_document(Document::new(state), "sRGB handle only".into());
    let n = history_len(&s);
    let err = s
        .photomerge_into_layers(
            files,
            align(StackAlignMode::Collage),
            blend(StackBlendMode::Panorama, false),
            CancelFlag::new(),
        )
        .unwrap_err();
    assert!(err.to_string().contains("is not embedded"), "{err}");
    assert_eq!(history_len(&s), n);
}
