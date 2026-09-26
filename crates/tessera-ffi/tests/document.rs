//! Layered documents over the bridge (`DocumentSession`): layer tree,
//! history, persistence (`.tessera-doc`, PSD), library images, flat export,
//! thumbnails, interactive edits and presentation. Headless tests read
//! pixels back with `read_level`; presentation tests use in-process
//! IOSurfaces.
#![cfg(target_os = "macos")]

use compositor::{
    Compositor, DocOp, DocState, Document, Layer, LayerId,
    edit::{PaintTarget, TileDelta},
    geom::Rect,
    raster::Depth,
};
use engine_api::tile::{Extent, Tile, TileCoord};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tessera_ffi::surface::{Surface, testing::create_rgba8};
use tessera_ffi::*;

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

/// A PNG with a smooth colour pattern and an alpha ramp in the right half.
fn pattern_png(dir: &Path, w: u32, h: u32) -> PathBuf {
    let img = image::RgbaImage::from_fn(w, h, |x, y| {
        let a = if x < w / 2 {
            255
        } else {
            (255 * (w - x) / (w / 2)) as u8
        };
        image::Rgba([
            (x * 255 / w) as u8,
            (y * 255 / h) as u8,
            ((x + y) % 256) as u8,
            a,
        ])
    });
    let path = dir.join("pattern.png");
    img.save(&path).unwrap();
    path
}

fn exposure_json(stops: f32) -> String {
    format!(r#"{{"kind":"exposure","exposure":{stops},"offset":0,"gamma":1}}"#)
}

fn names(s: &DocumentSession) -> Vec<(String, u32, Option<u64>)> {
    s.layers()
        .unwrap()
        .into_iter()
        .map(|n| (n.name, n.depth, n.parent))
        .collect()
}

fn max_diff(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f32::max)
}

/// The CPU reference composite of a session's live state (straight RGBA).
fn cpu_reference(s: &DocumentSession, level: u8) -> Vec<f32> {
    let state = (*s.document_state().unwrap()).clone();
    Compositor::new(64 << 20)
        .render_level_rgba(&Document::new(state), level)
        .unwrap()
        .1
}

#[test]
fn new_document_layers_are_flat_preorder_top_first() {
    let (_d, engine) = engine();
    let s = engine
        .clone()
        .new_document(300, 200, DocDepth::U8, None)
        .unwrap();
    let info = s.info().unwrap();
    assert_eq!(
        (info.width, info.height, info.depth),
        (300, 200, DocDepth::U8)
    );
    assert_eq!(info.profile_name.as_deref(), Some("sRGB IEC61966-2.1"));
    assert!(info.id.starts_with("doc#"));
    assert!(info.dirty, "new documents are unsaved");
    let base = s.layers().unwrap()[0].id;

    let a = s
        .add_layer(NewLayer::Pixel, "A".into(), None, None)
        .unwrap();
    assert_eq!(a.created.len(), 1);
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: exposure_json(0.5),
            },
            String::new(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    let g = s
        .add_layer(
            NewLayer::Group {
                mode: DocGroupMode::Isolated,
            },
            "G".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.add_layer(NewLayer::Pixel, "in G".into(), Some(g), None)
        .unwrap();
    s.add_layer(
        NewLayer::Fill {
            json: r#"{"kind":"solid","color":[1,0,0]}"#.into(),
        },
        "fill".into(),
        Some(g),
        Some(0),
    )
    .unwrap();

    let rows = s.layers().unwrap();
    let got = names(&s);
    assert_eq!(
        got,
        vec![
            ("G".into(), 0, None),
            ("in G".into(), 1, Some(g)),
            ("fill".into(), 1, Some(g)),
            (rows[3].name.clone(), 0, None),
            ("A".into(), 0, None),
            ("Layer 1".into(), 0, None),
        ]
    );
    assert!(rows[3].name.starts_with("Exposure"), "{}", rows[3].name);
    assert_eq!(rows[3].id, adj);
    assert_eq!(rows[3].kind, DocLayerKind::Adjustment);
    assert!(
        rows[3]
            .adjustment_json
            .as_deref()
            .unwrap()
            .contains("exposure")
    );
    assert_eq!(rows[0].kind, DocLayerKind::Group);
    assert_eq!(rows[0].group_mode, Some(DocGroupMode::Isolated));
    assert_eq!(rows[0].blend_mode, "normal");
    assert_eq!(
        rows[2].fill_json.as_deref(),
        Some(r#"{"kind":"solid","color":[1.0,0.0,0.0]}"#)
    );
    // Compositor indices: 0 = bottom.
    assert_eq!(
        rows.iter().map(|r| r.index).collect::<Vec<_>>(),
        vec![3, 1, 0, 2, 1, 0]
    );
    assert_eq!(rows[5].id, base);
    assert_eq!(s.info().unwrap().layer_count, 6);

    // Pass-through through the blend-mode name; group and ungroup.
    s.set_blend_mode(g, "pass_through".into()).unwrap();
    let row = s.layer(g).unwrap();
    assert_eq!(
        (row.blend_mode.as_str(), row.group_mode),
        ("pass_through", Some(DocGroupMode::PassThrough))
    );
    s.set_blend_mode(g, "multiply".into()).unwrap();
    let row = s.layer(g).unwrap();
    assert_eq!(
        (row.blend_mode.as_str(), row.group_mode),
        ("multiply", Some(DocGroupMode::Isolated))
    );
    assert!(
        s.set_blend_mode(a.created[0], "pass_through".into())
            .is_err()
    );
    assert!(s.set_blend_mode(a.created[0], "bogus".into()).is_err());
    let grouped = s
        .group_layers(vec![base, a.created[0]], "pair".into())
        .unwrap();
    let pair = grouped.created[0];
    let rows = names(&s);
    assert_eq!(rows[rows.len() - 3], ("pair".into(), 0, None));
    assert_eq!(rows[rows.len() - 2], ("A".into(), 1, Some(pair)));
    assert_eq!(rows[rows.len() - 1], ("Layer 1".into(), 1, Some(pair)));
    s.ungroup_layer(pair).unwrap();
    assert_eq!(names(&s)[3..], got[3..]);
    assert_eq!(blend_mode_names().len(), 27);
    assert_eq!(blend_mode_names()[4], "color_burn");
}

#[test]
fn undo_redo_and_checkout_restore_the_tree() {
    let (_d, engine) = engine();
    let s = engine
        .clone()
        .new_document(128, 128, DocDepth::U16, Some("Display P3".into()))
        .unwrap();
    assert_eq!(
        s.info().unwrap().profile_name.as_deref(),
        Some("Display P3")
    );
    let mut states = vec![(s.info().unwrap().history_head, s.layers().unwrap())];
    let a = s
        .add_layer(NewLayer::Pixel, "A".into(), None, None)
        .unwrap()
        .created[0];
    states.push((s.info().unwrap().history_head, s.layers().unwrap()));
    s.set_opacity(a, 0.25, false).unwrap();
    states.push((s.info().unwrap().history_head, s.layers().unwrap()));
    s.duplicate_layer(a).unwrap();
    states.push((s.info().unwrap().history_head, s.layers().unwrap()));
    s.move_layer(a, None, 0).unwrap();
    states.push((s.info().unwrap().history_head, s.layers().unwrap()));
    assert_eq!(s.layers().unwrap().last().unwrap().id, a);

    for i in (0..states.len() - 1).rev() {
        let u = s.undo().unwrap();
        assert_eq!(u.history_head, states[i].0);
        assert_eq!(s.layers().unwrap(), states[i].1, "undo to {i}");
    }
    assert!(!s.info().unwrap().can_undo);
    for (i, (head, rows)) in states.iter().enumerate().skip(1) {
        s.redo().unwrap();
        assert_eq!(s.info().unwrap().history_head, *head);
        assert_eq!(&s.layers().unwrap(), rows, "redo to {i}");
    }
    s.checkout_history(states[2].0).unwrap();
    assert_eq!(s.layers().unwrap(), states[2].1);
    s.snapshot("two".into()).unwrap();
    s.checkout_history(states[4].0).unwrap();
    s.restore_snapshot("two".into()).unwrap();
    assert_eq!(s.layers().unwrap(), states[2].1);
    assert_eq!(s.snapshots().unwrap(), vec!["two".to_string()]);

    // An edit after a checkout branches; every state stays listed.
    s.remove_layer(a).unwrap();
    let items = s.history_items().unwrap();
    assert_eq!(items.len(), states.len() + 1);
    assert_eq!(items[0].label, "New Document");
    let current: Vec<_> = items.iter().filter(|i| i.is_current).collect();
    assert_eq!(current.len(), 1);
    assert_eq!(current[0].parent, Some(states[2].0));
    assert_eq!(current[0].label, "Delete Layer");
    assert!(s.history_memory_bytes().unwrap() < 1 << 20);
    s.set_max_states(3).unwrap();
    assert!(s.history_items().unwrap().len() <= 4);
}

#[test]
fn tessera_doc_round_trip_and_same_path_same_session() {
    let (dir, engine) = engine();
    let png = pattern_png(dir.path(), 300, 270);
    let s = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    let info = s.info().unwrap();
    assert_eq!(
        (info.width, info.height, info.title.as_str()),
        (300, 270, "pattern.png")
    );
    assert!(info.path.is_none() && !info.dirty);
    let rows = s.layers().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].name, "pattern");
    assert_eq!(rows[0].kind, DocLayerKind::Pixel);
    let again = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(again.id(), s.id(), "same file, same session");

    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: exposure_json(-0.5),
            },
            "dark".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.set_selection_rect(10, 20, 100, 50, 0.0).unwrap();
    assert_eq!(
        s.info().unwrap().selection_bounds,
        Some(DocRect {
            x: 0,
            y: 0,
            width: 256,
            height: 256
        }),
        "tile-granular bounds"
    );
    s.add_mask(adj, MaskInit::FromSelection).unwrap();
    s.set_blend_mode(adj, "soft_light".into()).unwrap();
    s.add_layer(
        NewLayer::Fill {
            json: r#"{"kind":"gradient","gradient":"linear","start":[0,0],"end":[300,0],"stops":[{"position":0,"color":[0,0,1,0.5]},{"position":1,"color":[1,1,0,0.2]}]}"#.into(),
        },
        "grad".into(),
        None,
        None,
    )
    .unwrap();
    assert!(s.info().unwrap().dirty);
    assert!(s.save().is_err(), "flat images need save_as");
    let saved = dir.path().join("doc.tessera-doc");
    s.save_as(saved.to_string_lossy().into_owned()).unwrap();
    let info = s.info().unwrap();
    assert!(!info.dirty);
    assert_eq!(info.title, "doc.tessera-doc");
    assert!(
        s.save_as(dir.path().join("x.gif").to_string_lossy().into_owned())
            .is_err()
    );
    let before = s.read_level(0).unwrap();
    let rows = s.layers().unwrap();
    let same = engine
        .clone()
        .open_document(saved.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(same.id(), s.id(), "the saved path finds the open session");
    s.close();
    assert!(s.info().is_ok(), "reads keep working after close");
    assert!(
        s.set_visible(adj, false).is_err(),
        "closed sessions reject edits"
    );

    let reopened = engine
        .clone()
        .open_document(saved.to_string_lossy().into_owned())
        .unwrap();
    assert_ne!(reopened.id(), s.id());
    let back = reopened.layers().unwrap();
    let strip = |v: &[LayerNode]| {
        v.iter()
            .map(|n| {
                (
                    n.id,
                    n.name.clone(),
                    n.blend_mode.clone(),
                    n.opacity,
                    n.has_mask,
                    n.kind,
                    n.adjustment_json.clone(),
                    n.fill_json.clone(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(strip(&back), strip(&rows));
    let after = reopened.read_level(0).unwrap();
    assert_eq!((after.0, after.1), (300, 270));
    assert_eq!(
        max_diff(&before.2, &after.2),
        0.0,
        "bit-identical after reload"
    );
    assert!(reopened.info().unwrap().selection_bounds.is_some());
}

fn psd_fixture() -> psd::PsdDocument {
    use psd::{
        AdditionalInfo, Channel, ColorMode, Compression, LayerSection, PsdDocument, Rect, Version,
    };
    let (w, h) = (4u32, 3u32);
    let plane = |v: u8| vec![v; (w * h) as usize];
    let layer =
        |name: &str, mode: &[u8; 4], opacity: u8, rgb: [u8; 3], extra: Vec<AdditionalInfo>| {
            psd::Layer {
                name: name.as_bytes().to_vec(),
                bounds: Rect {
                    top: 0,
                    left: 0,
                    bottom: h as i32,
                    right: w as i32,
                },
                channels: [(0, rgb[0]), (1, rgb[1]), (2, rgb[2]), (-1, 255)]
                    .into_iter()
                    .map(|(id, v)| Channel {
                        id,
                        compression: Compression::Raw,
                        data: plane(v),
                    })
                    .collect(),
                blend_mode: *mode,
                opacity,
                additional: extra,
                ..psd::Layer::default()
            }
        };
    PsdDocument {
        version: Version::Psd,
        width: w,
        height: h,
        depth: 8,
        channels: 3,
        color_mode: ColorMode::Rgb,
        color_data: vec![],
        resources: vec![],
        layer_section: LayerSection {
            // File order: top first.
            layers: vec![
                layer(
                    "top",
                    b"mul ",
                    128,
                    [200, 100, 50],
                    vec![AdditionalInfo {
                        signature: *b"8BIM",
                        key: *b"zzUK",
                        data: vec![1, 2, 3, 4],
                    }],
                ),
                layer("bottom", b"norm", 255, [20, 180, 240], vec![]),
            ],
            ..LayerSection::default()
        },
        composite: vec![0; (w * h * 3) as usize],
        compression: Compression::Raw,
    }
}

#[test]
fn psd_opens_with_names_and_modes_and_saves_back_unknown_keys() {
    let (dir, engine) = engine();
    let path = dir.path().join("layers.psd");
    std::fs::write(&path, psd_fixture().write().unwrap()).unwrap();
    let s = engine
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = s.layers().unwrap();
    let got: Vec<_> = rows
        .iter()
        .map(|n| {
            (
                n.name.as_str(),
                n.blend_mode.as_str(),
                (n.opacity * 255.0).round() as u32,
            )
        })
        .collect();
    assert_eq!(
        got,
        vec![("top", "multiply", 128), ("bottom", "normal", 255)]
    );
    let info = s.info().unwrap();
    assert!(info.path.is_some() && !info.dirty);

    let bottom = rows[1].id;
    s.set_opacity(bottom, 0.5, false).unwrap();
    s.rename_layer(bottom, "base".into()).unwrap();
    s.save().unwrap();
    let back = psd::PsdDocument::read(&std::fs::read(&path).unwrap()).unwrap();
    let layers = &back.layer_section.layers;
    assert_eq!(layers.len(), 2);
    assert_eq!(layers[0].name, b"top");
    assert_eq!(&layers[0].blend_mode, b"mul ");
    assert!(
        layers[0]
            .additional
            .iter()
            .any(|a| &a.key == b"zzUK" && a.data == [1, 2, 3, 4]),
        "unknown tagged block kept"
    );
    assert_eq!(layers[1].opacity, 128);
    // The flattened composite is written for readers without layers.
    assert!(back.composite.iter().any(|&b| b != 0));
    // A PSB save of the same document.
    let psb = dir.path().join("layers.psb");
    s.save_as(psb.to_string_lossy().into_owned()).unwrap();
    let wide = psd::PsdDocument::read(&std::fs::read(&psb).unwrap()).unwrap();
    assert_eq!(wide.version, psd::Version::Psb);
    assert_eq!(wide.layer_section.layers.len(), 2);
}

#[test]
fn export_flat_png_matches_the_cpu_composite() {
    let (dir, engine) = engine();
    let png = pattern_png(dir.path(), 520, 300);
    let s = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    let fill = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"gradient","gradient":"radial","start":[260,150],"end":[500,150],"stops":[{"position":0,"color":[1,0.2,0,1]},{"position":1,"color":[0,0.4,1,1]}]}"#.into(),
            },
            "glow".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.set_blend_mode(fill, "overlay".into()).unwrap();
    s.set_opacity(fill, 0.6, false).unwrap();
    s.add_layer(
        NewLayer::Adjustment {
            json: r#"{"kind":"hue_saturation","hue":20,"saturation":-30,"lightness":5,"colorize":false}"#.into(),
        },
        String::new(),
        None,
        None,
    )
    .unwrap();
    let out = dir.path().join("flat.png");
    s.export_flat(
        out.to_string_lossy().into_owned(),
        ExportFormat::Png,
        90,
        ExportColor::Srgb,
    )
    .unwrap();
    let decoded = image::open(&out).unwrap().to_rgba8();
    assert_eq!(decoded.dimensions(), (520, 300));
    let reference = cpu_reference(&s, 0);
    let mut worst = 0u8;
    for (p, r) in decoded.as_raw().iter().zip(&reference) {
        let q = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        worst = worst.max(p.abs_diff(q));
    }
    assert!(
        worst <= 1,
        "PNG differs from the CPU composite by {worst} codes"
    );
    // The GPU viewport readback agrees with the CPU reference (docs/11 §1.3).
    let (w, h, gpu) = s.read_level(0).unwrap();
    assert_eq!((w, h), (520, 300));
    let d = max_diff(&gpu, &reference);
    assert!(d <= 2e-3, "resident vs CPU {d}");
    // JPEG and TIFF in other spaces.
    for (name, format, color) in [
        ("flat.jpg", ExportFormat::Jpeg, ExportColor::DisplayP3),
        ("flat.tif", ExportFormat::Tiff, ExportColor::Rec2020),
    ] {
        let path = dir.path().join(name);
        s.export_flat(path.to_string_lossy().into_owned(), format, 85, color)
            .unwrap();
        assert!(std::fs::metadata(&path).unwrap().len() > 1000, "{name}");
    }
    let jpeg = image::open(dir.path().join("flat.jpg")).unwrap();
    assert_eq!((jpeg.width(), jpeg.height()), (520, 300));
    assert!(
        s.export_flat(
            dir.path().join("q.jpg").to_string_lossy().into_owned(),
            ExportFormat::Jpeg,
            0,
            ExportColor::Srgb
        )
        .is_err()
    );
}

#[test]
fn thumbnails_are_cached_per_revision() {
    let (dir, engine) = engine();
    let png = pattern_png(dir.path(), 600, 400);
    let s = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    let id = s.layers().unwrap()[0].id;
    let other = s
        .add_layer(NewLayer::Pixel, "other".into(), None, None)
        .unwrap()
        .created[0];
    let n0 = s.thumbnail_renders();
    let t1 = s.layer_thumbnail(id, 128).unwrap();
    assert_eq!(s.thumbnail_renders(), n0 + 1);
    // 600×400 → level 3 is 75×50 (the finest level within 128 px).
    let surface = Surface::lookup(t1, 75, 50).expect("thumbnail is the level extent");
    surface
        .with_pixels(|px, stride| {
            // Left half is opaque; the right edge fades out (straight alpha).
            assert_eq!(px[10 * stride + 5 * 4 + 3], 255);
            assert!(px[10 * stride + 73 * 4 + 3] < 40);
        })
        .unwrap();
    assert_eq!(s.layer_thumbnail(id, 128).unwrap(), t1, "cached");
    assert_eq!(s.thumbnail_renders(), n0 + 1);
    s.set_visible(other, false).unwrap();
    assert_eq!(
        s.layer_thumbnail(id, 128).unwrap(),
        t1,
        "other layers do not invalidate"
    );
    let rev = s.layer(id).unwrap().revision;
    s.set_opacity(id, 0.3, false).unwrap();
    assert!(s.layer(id).unwrap().revision > rev);
    let t2 = s.layer_thumbnail(id, 128).unwrap();
    assert_ne!(t2, t1);
    assert_eq!(s.thumbnail_renders(), n0 + 2);
    let c1 = s.composite_thumbnail(64).unwrap();
    assert_eq!(s.composite_thumbnail(64).unwrap(), c1);
    assert_eq!(s.thumbnail_renders(), n0 + 3);
    s.add_mask(id, MaskInit::HideAll).unwrap();
    let m = s.mask_thumbnail(id, 64).unwrap();
    let mask = Surface::lookup(m, 38, 25).unwrap();
    mask.with_pixels(|px, _| assert_eq!(&px[..4], &[0, 0, 0, 255]))
        .unwrap();
    assert_ne!(
        s.composite_thumbnail(64).unwrap(),
        c1,
        "a new state re-renders"
    );
    assert!(s.mask_thumbnail(other, 64).is_err());
}

#[test]
fn interactive_opacity_records_history_only_on_commit() {
    let (dir, engine) = engine();
    let png = pattern_png(dir.path(), 256, 128);
    let s = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    let id = s.layers().unwrap()[0].id;
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: exposure_json(0.0),
            },
            "exp".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    let nodes = s.history_items().unwrap().len();
    let head = s.info().unwrap().history_head;
    let full = s.read_level(1).unwrap().2;
    for v in [0.9f32, 0.7, 0.5, 0.3] {
        let u = s.set_opacity(id, v, true).unwrap();
        assert_eq!(u.history_head, head);
        assert!(u.dirty);
        s.set_adjustment_json(adj, exposure_json(v), true).unwrap();
    }
    assert_eq!(
        s.history_items().unwrap().len(),
        nodes,
        "no node while dragging"
    );
    assert_eq!(
        s.layer(id).unwrap().opacity,
        0.3,
        "rows show the live value"
    );
    let live = s.read_level(1).unwrap().2;
    assert!(max_diff(&live, &full) > 0.05, "the viewport shows the drag");
    assert!(max_diff(&live, &cpu_reference(&s, 1)) <= 2e-3);
    let info = s.info().unwrap();
    assert!(info.can_undo && !info.can_redo);

    let u = s.commit("Opacity".into()).unwrap();
    assert_ne!(u.history_head, head);
    let items = s.history_items().unwrap();
    assert_eq!(items.len(), nodes + 1, "one node for the whole drag");
    assert_eq!(items.last().unwrap().label, "Opacity");
    assert_eq!(s.layer(id).unwrap().opacity, 0.3);
    assert!(
        s.layer(adj)
            .unwrap()
            .adjustment_json
            .unwrap()
            .contains("0.3")
    );
    let committed = s.commit(String::new()).unwrap();
    assert_eq!(committed.history_head, u.history_head, "nothing pending");

    // A final non-interactive value folds into the drag: still one node.
    s.set_opacity(id, 0.8, true).unwrap();
    s.set_opacity(id, 0.6, false).unwrap();
    assert_eq!(s.history_items().unwrap().len(), nodes + 2);
    assert_eq!(s.layer(id).unwrap().opacity, 0.6);

    // Another control's value does not fold into a drag: two nodes.
    s.set_adjustment_json(adj, exposure_json(0.7), true)
        .unwrap();
    s.set_visible(id, true).unwrap();
    assert_eq!(s.history_items().unwrap().len(), nodes + 4);

    // Undo commits a pending drag first, then steps back over it.
    s.set_opacity(id, 0.1, true).unwrap();
    s.undo().unwrap();
    assert_eq!(s.layer(id).unwrap().opacity, 0.6);
    s.checkout_history(head).unwrap();
    assert_eq!(s.layer(id).unwrap().opacity, 1.0);
    assert_eq!(max_diff(&s.read_level(1).unwrap().2, &full), 0.0);
}

#[test]
fn merge_down_and_flatten_keep_the_composite() {
    let (dir, engine) = engine();
    let png = pattern_png(dir.path(), 300, 200);
    let s = engine
        .clone()
        .open_document(png.to_string_lossy().into_owned())
        .unwrap();
    let base = s.layers().unwrap()[0].id;
    let top = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"solid","color":[0.2,0.5,0.9]}"#.into(),
            },
            "blue".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.set_blend_mode(top, "multiply".into()).unwrap();
    s.set_opacity(top, 0.5, false).unwrap();
    let before = cpu_reference(&s, 0);
    assert!(
        s.merge_down(base).is_err(),
        "nothing below the bottom layer"
    );
    let u = s.merge_down(top).unwrap();
    let rows = s.layers().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!((rows[0].id, rows[0].name.as_str()), (base, "pattern"));
    assert_eq!(rows[0].kind, DocLayerKind::Pixel);
    assert!(u.layers_changed.contains(&top) && u.layers_changed.contains(&base));
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Merge Down"
    );
    // 8-bit storage of the merged pixels: within one code value.
    assert!(max_diff(&cpu_reference(&s, 0), &before) <= 1.0 / 255.0 + 1e-6);

    s.add_layer(NewLayer::Pixel, "empty".into(), None, None)
        .unwrap();
    s.flatten().unwrap();
    let rows = s.layers().unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].background);
    assert_eq!(rows[0].name, "Background");
    let flat = cpu_reference(&s, 0);
    assert!(flat.chunks(4).all(|p| p[3] == 1.0), "flattened over white");
    s.undo().unwrap();
    assert_eq!(s.layers().unwrap().len(), 2);
}

#[derive(Default)]
struct Recorder {
    frames: Mutex<Vec<DocFrameInfo>>,
    layers: Mutex<Vec<Vec<u64>>>,
    history: Mutex<Vec<u64>>,
    failures: Mutex<Vec<String>>,
}

impl DocumentListener for Recorder {
    fn on_frame(&self, frame: DocFrameInfo) {
        self.frames.lock().unwrap().push(frame);
    }
    fn on_layers_changed(&self, layer_ids: Vec<u64>) {
        self.layers.lock().unwrap().push(layer_ids);
    }
    fn on_history_changed(&self, head: u64) {
        self.history.lock().unwrap().push(head);
    }
    fn on_render_failed(&self, message: String) {
        self.failures.lock().unwrap().push(message);
    }
}

#[test]
fn frames_are_coalesced_and_straight_alpha() {
    let (_d, engine) = engine();
    let s = engine
        .clone()
        .new_document(512, 384, DocDepth::U8, None)
        .unwrap();
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let fill = s
        .add_layer(
            NewLayer::Fill {
                json: r#"{"kind":"solid","color":[1,0,0]}"#.into(),
            },
            "red".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.set_opacity(fill, 0.5, false).unwrap();
    let plan = s.plan_surface(200, 150).unwrap();
    assert_eq!((plan.level, plan.width, plan.height), (1, 256, 192));
    let ids: Vec<u32> = (0..2)
        .map(|_| create_rgba8(plan.width, plan.height))
        .collect();
    for id in &ids {
        s.attach_surface(*id, plan.width, plan.height).unwrap();
    }
    s.wait_idle();
    let frame = rec
        .frames
        .lock()
        .unwrap()
        .last()
        .cloned()
        .expect("first frame");
    assert_eq!((frame.level, frame.width, frame.height), (1, 256, 192));
    assert_eq!(
        frame.canvas_rect,
        DocRect {
            x: 0,
            y: 0,
            width: 512,
            height: 384
        }
    );
    let surface = Surface::lookup(frame.surface_id, plan.width, plan.height).unwrap();
    surface
        .with_pixels(|px, _| {
            // Red at 50 % over transparency, straight alpha.
            assert_eq!(px[0], 255);
            assert_eq!(&px[1..3], &[0, 0]);
            assert!(px[3].abs_diff(128) <= 1, "alpha {}", px[3]);
        })
        .unwrap();

    // A burst of edits yields far fewer frames, the last one current.
    let before = rec.frames.lock().unwrap().len();
    for i in 0..40 {
        s.set_opacity(fill, (i as f32) / 40.0, true).unwrap();
    }
    s.commit("Opacity".into()).unwrap();
    s.wait_idle();
    let frames = rec.frames.lock().unwrap().clone();
    let burst = frames.len() - before;
    assert!((1..=41).contains(&burst));
    eprintln!("41 edits → {burst} frames");
    assert_eq!(frames.last().unwrap().epoch, s.info().unwrap().epoch);
    assert!(rec.failures.lock().unwrap().is_empty());
    assert!(!rec.history.lock().unwrap().is_empty());
    assert!(
        rec.layers
            .lock()
            .unwrap()
            .iter()
            .flatten()
            .any(|l| *l == fill)
    );
    // Frames alternate over the ring.
    let used: std::collections::BTreeSet<u32> = frames.iter().map(|f| f.surface_id).collect();
    assert!(used.len() == 2 || burst == 1);

    // A zoomed viewport: level 0, a 100×80 region at (300, 200).
    s.set_viewport(0, 300, 200, 100, 80, 1.0).unwrap();
    s.wait_idle();
    let f = rec.frames.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        (f.level, f.x, f.y, f.width, f.height),
        (0, 300, 200, 100, 80)
    );
    assert_eq!(
        f.canvas_rect,
        DocRect {
            x: 300,
            y: 200,
            width: 100,
            height: 80
        }
    );
    assert_eq!((f.level_width, f.level_height), (512, 384));
    // Clipped at the canvas edge.
    s.set_viewport(0, 480, 360, 100, 80, 1.0).unwrap();
    s.wait_idle();
    let f = rec.frames.lock().unwrap().last().cloned().unwrap();
    assert_eq!((f.width, f.height), (32, 24));
    s.detach_surfaces();
    s.close();
}

#[test]
fn open_document_from_image_is_the_developed_raw() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
    if !root.exists() {
        eprintln!("skipping: no fixtures/raw/sample.dng");
        return;
    }
    let (dir, engine) = engine();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    std::fs::copy(&root, photos.join("sample.dng")).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let image_id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let develop = engine
        .clone()
        .open_develop_session(image_id.clone())
        .unwrap()
        .info();
    let (w, h) = if develop.orientation >= 5 {
        (develop.height, develop.width)
    } else {
        (develop.width, develop.height)
    };
    let started = Instant::now();
    let s = engine
        .clone()
        .open_document_from_image(image_id.clone(), true)
        .unwrap();
    eprintln!("open_document_from_image: {:?}", started.elapsed());
    let info = s.info().unwrap();
    assert_eq!((info.width, info.height), (w, h));
    assert_eq!(info.depth, DocDepth::U16);
    assert_eq!(info.source_image_id.as_deref(), Some(image_id.as_str()));
    let rows = s.layers().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].kind, DocLayerKind::Pixel);
    assert_eq!(rows[0].name, "sample");
    assert_eq!(rows[0].bounds.unwrap().width, i64::from(w));
    let same = engine
        .clone()
        .open_document_from_image(image_id, true)
        .unwrap();
    assert_eq!(same.id(), s.id());
    let (lw, lh, px) = s.read_level(4).unwrap();
    assert_eq!((lw, lh), (w.div_ceil(16), h.div_ceil(16)));
    let mean: f32 = px.chunks(4).map(|p| p[1]).sum::<f32>() / (lw * lh) as f32;
    assert!(
        mean > 0.02 && mean < 0.98,
        "developed image is not blank: {mean}"
    );
    assert!(px.chunks(4).all(|p| p[3] == 1.0));
}

// ─────────────────────────────── bench ───────────────────────────────

fn content(seed: u32, coord: TileCoord, layout: engine_api::tile::TileLayout) -> Tile {
    let n = layout.plane_len();
    let mut v = vec![0u8; 4 * n];
    let mut x = seed.wrapping_mul(2654435761) ^ (coord.x << 16) ^ coord.y;
    for i in 0..n {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        v[i] = x as u8;
        v[n + i] = (x >> 8) as u8;
        v[2 * n + i] = (x >> 16) as u8;
        v[3 * n + i] = 96 + ((x >> 24) as u8 >> 1);
    }
    Tile::from_samples(coord, layout, v).unwrap()
}

/// COMPOSITOR.md §10/§12.4: 100 semi-transparent 8-bit layers at 20 MP (10
/// distinct + 90 copy-on-write duplicates with one repainted tile), all 27
/// modes, a pass-through and an isolated group.
fn bench_document() -> (Document, Vec<LayerId>) {
    use compositor::{BlendMode, GroupMode};
    let e = Extent::new(5472, 3648);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut ids = Vec::new();
    for s in 0..10u32 {
        let mut l = Layer::pixel(format!("base {s}"), e, Depth::U8);
        l.props.background = s == 0;
        let r = l.raster_mut().unwrap();
        let (cols, rows) = r.grid();
        for ty in 0..rows {
            for tx in 0..cols {
                let mut t = content(s, TileCoord::new(0, tx, ty), r.layout(tx, ty));
                if s == 0 {
                    let n = t.layout().plane_len();
                    t.samples_mut::<u8>().unwrap()[3 * n..].fill(255);
                }
                r.set_slot(tx, ty, Some(t), 1).unwrap();
            }
        }
        ids.push(
            d.apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: l,
            })
            .unwrap()
            .created[0],
        );
    }
    let (cols, rows) = e.tile_grid(256);
    for j in 10..100u32 {
        let src = ids[1 + (j as usize % 9)];
        let id = d.apply(DocOp::DuplicateLayer { id: src }).unwrap().created[0];
        let raster = d.state().find(id).unwrap().raster().unwrap().clone();
        let (tx, ty) = (j % cols, (j / cols) % rows);
        d.apply(DocOp::PaintTiles {
            id,
            target: PaintTarget::Content,
            tiles: vec![TileDelta {
                tx,
                ty,
                tile: Some(content(j, TileCoord::new(0, tx, ty), raster.layout(tx, ty))),
            }],
            dirty: Rect::of_extent(e),
        })
        .unwrap();
        let mut props = d.state().find(id).unwrap().props.clone();
        props.blend_mode = BlendMode::ALL[j as usize % 27];
        d.apply(DocOp::SetProps { id, props }).unwrap();
        ids.push(id);
    }
    for (mode, range) in [
        (GroupMode::PassThrough, 20..30usize),
        (GroupMode::Isolated, 40..50),
    ] {
        let g = d
            .apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: Layer::group("group", mode),
            })
            .unwrap()
            .created[0];
        for id in &ids[range] {
            d.apply(DocOp::MoveLayer {
                id: *id,
                parent: Some(g),
                index: usize::MAX,
            })
            .unwrap();
        }
    }
    (d, ids)
}

/// The interactive path on the §12.4 bench document: opacity and adjustment
/// drags recomposite the level-2 viewport (1368×912) and present it.
/// `cargo test -p tessera-ffi --release --test document -- --ignored --nocapture`
#[test]
#[ignore]
fn bench_interactive_recomposite_100_layers_20mp() {
    let (_d, engine) = engine();
    let started = Instant::now();
    let (doc, ids) = bench_document();
    eprintln!("bench document built in {:?}", started.elapsed());
    let s = engine.adopt_document(doc, "bench".into());
    let rec = Arc::new(Recorder::default());
    s.set_listener(Some(rec.clone()));
    let plan = s.plan_surface(1368, 912).unwrap();
    assert_eq!((plan.level, plan.width, plan.height), (2, 1368, 912));
    for _ in 0..3 {
        s.attach_surface(
            create_rgba8(plan.width, plan.height),
            plan.width,
            plan.height,
        )
        .unwrap();
    }
    let started = Instant::now();
    s.wait_idle();
    eprintln!(
        "first frame (cold): {:?} backend {}",
        started.elapsed(),
        s.info().unwrap().backend
    );
    let adj = s
        .add_layer(
            NewLayer::Adjustment {
                json: exposure_json(0.0),
            },
            "exp".into(),
            None,
            None,
        )
        .unwrap()
        .created[0];
    s.wait_idle();
    let measure = |label: &str, edit: &dyn Fn(usize)| {
        let mut times = Vec::new();
        for i in 0..40 {
            let before = rec.frames.lock().unwrap().len();
            let t = Instant::now();
            edit(i);
            s.wait_idle();
            let wall = t.elapsed().as_secs_f64() * 1000.0;
            let frames = rec.frames.lock().unwrap();
            assert_eq!(frames.len(), before + 1);
            let f = frames.last().unwrap();
            if i >= 3 {
                times.push((f.render_ms, wall, f.full_recomposite, f.blocks));
            }
        }
        let mut ms: Vec<f64> = times.iter().map(|t| t.0).collect();
        ms.sort_by(f64::total_cmp);
        let mut wall: Vec<f64> = times.iter().map(|t| t.1).collect();
        wall.sort_by(f64::total_cmp);
        eprintln!(
            "{label}: render_ms median {:.2} p90 {:.2} max {:.2}; call→frame median {:.2} ms; blocks {}",
            ms[ms.len() / 2],
            ms[ms.len() * 9 / 10],
            ms[ms.len() - 1],
            wall[wall.len() / 2],
            times[0].3
        );
        ms[ms.len() / 2]
    };
    let layer = ids[55].0;
    let opacity = measure("opacity drag (L2, 100 layers)", &|i| {
        s.set_opacity(layer, 0.3 + (i % 10) as f32 * 0.05, true)
            .unwrap();
    });
    s.commit("Opacity".into()).unwrap();
    s.wait_idle();
    let adjust = measure("exposure drag (L2, 100 layers + adjustment)", &|i| {
        s.set_adjustment_json(adj, exposure_json((i % 10) as f32 * 0.1), true)
            .unwrap();
    });
    s.commit("Exposure".into()).unwrap();
    eprintln!("RESULT opacity {opacity:.2} ms, exposure {adjust:.2} ms (target < 16 ms)");
    let _ = Duration::ZERO;
}
