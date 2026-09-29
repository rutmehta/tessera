//! Persistent alpha and spot channels over the bridge (WP B5-08): Save /
//! Load Selection with every `SelectionOp`, duplicate names, stale ids,
//! undo / redo of each channel op, spot metadata validation, `.tessera-doc`
//! and PSD round trips, thumbnails and the RGB composite staying unchanged by
//! spot channels.
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

fn marquee(s: &DocumentSession, x: f64, w: f64, op: SelectionOp) {
    s.select_marquee(MarqueeShape::Rect, x, 0.0, w, H as f64, 0.0, false, op)
        .unwrap();
}

/// Selection value at (x, 10); no selection reads as 1 (select all).
fn sel(s: &DocumentSession, x: u32) -> f32 {
    s.document_state()
        .unwrap()
        .selection
        .as_ref()
        .map_or(1.0, |r| r.pixel(x, 10)[0])
}

fn sel_row(s: &DocumentSession) -> [f32; 4] {
    [sel(s, 50), sel(s, 150), sel(s, 250), sel(s, 350)]
}

fn chan_px(s: &DocumentSession, id: u64, x: u32) -> f32 {
    let st = s.document_state().unwrap();
    st.channels
        .iter()
        .find(|c| c.id.0 == id)
        .expect("channel")
        .raster
        .pixel(x, 10)[0]
}

fn chan_row(s: &DocumentSession, id: u64) -> [f32; 4] {
    [
        chan_px(s, id, 50),
        chan_px(s, id, 150),
        chan_px(s, id, 250),
        chan_px(s, id, 350),
    ]
}

fn labels(s: &DocumentSession) -> Vec<String> {
    s.history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect()
}

/// Channel rows without the session-only `visible` flag.
fn snapshot(s: &DocumentSession) -> Vec<(u64, DocChannelKind, String, [u32; 4], u64)> {
    s.document_channels()
        .unwrap()
        .into_iter()
        .map(|c| {
            (
                c.id,
                c.kind,
                c.name,
                [
                    c.color.r.to_bits(),
                    c.color.g.to_bits(),
                    c.color.b.to_bits(),
                    c.opacity.to_bits(),
                ],
                c.revision,
            )
        })
        .collect()
}

const INK: PaintColor = PaintColor {
    r: 0.0,
    g: 0.6,
    b: 1.0,
};

/// Channel "A" = left half (x < 200); the selection is then x in 100..300.
fn setup_a(s: &DocumentSession) -> u64 {
    marquee(s, 0.0, 200.0, SelectionOp::Replace);
    let a = s
        .save_selection_channel("A".into(), None, SelectionOp::Replace)
        .unwrap();
    assert_eq!(labels(s).last().unwrap(), "Save Selection");
    marquee(s, 100.0, 200.0, SelectionOp::Replace);
    a.channel_id
}

#[test]
fn load_selection_with_every_op_and_invert() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let cases = [
        (SelectionOp::Replace, false, [1.0, 1.0, 0.0, 0.0]),
        (SelectionOp::Add, false, [1.0, 1.0, 1.0, 0.0]),
        (SelectionOp::Subtract, false, [0.0, 0.0, 1.0, 0.0]),
        (SelectionOp::Intersect, false, [0.0, 1.0, 0.0, 0.0]),
        (SelectionOp::Replace, true, [0.0, 0.0, 1.0, 1.0]),
        (SelectionOp::Add, true, [0.0, 1.0, 1.0, 1.0]),
        (SelectionOp::Subtract, true, [0.0, 1.0, 0.0, 0.0]),
        (SelectionOp::Intersect, true, [0.0, 0.0, 1.0, 0.0]),
    ];
    for (op, invert, want) in cases {
        marquee(&s, 100.0, 200.0, SelectionOp::Replace);
        let n = labels(&s).len();
        s.load_selection_channel(a, op, invert).unwrap();
        assert_eq!(sel_row(&s), want, "{op:?} invert {invert}");
        assert_eq!(labels(&s).len(), n + 1, "one history node");
        assert_eq!(labels(&s).last().unwrap(), "Load Selection");
    }
}

#[test]
fn save_into_existing_channel_with_every_op() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let cases = [
        (SelectionOp::Replace, [0.0, 1.0, 1.0, 0.0]),
        (SelectionOp::Add, [1.0, 1.0, 1.0, 0.0]),
        (SelectionOp::Subtract, [1.0, 0.0, 0.0, 0.0]),
        (SelectionOp::Intersect, [0.0, 1.0, 0.0, 0.0]),
    ];
    for (op, want) in cases {
        let a = setup_a(&s);
        let r = s
            .save_selection_channel("ignored".into(), Some(a), op)
            .unwrap();
        assert_eq!(r.channel_id, a);
        assert_eq!(chan_row(&s, a), want, "{op:?}");
        let c = s
            .document_channels()
            .unwrap()
            .into_iter()
            .find(|c| c.id == a)
            .unwrap();
        assert_eq!(c.name, "A", "combining keeps the name");
        s.delete_document_channel(a).unwrap();
    }
    s.select_none().unwrap();
    assert!(
        s.save_selection_channel("x".into(), None, SelectionOp::Replace)
            .is_err(),
        "no selection to save"
    );
    marquee(&s, 0.0, 10.0, SelectionOp::Replace);
    assert!(
        s.save_selection_channel("  ".into(), None, SelectionOp::Replace)
            .is_err()
    );
}

#[test]
fn duplicate_names_stay_distinct_by_id() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    marquee(&s, 0.0, 100.0, SelectionOp::Replace);
    let first = s
        .save_selection_channel("Alpha 1".into(), None, SelectionOp::Replace)
        .unwrap()
        .channel_id;
    marquee(&s, 300.0, 100.0, SelectionOp::Replace);
    let second = s
        .save_selection_channel("Alpha 1".into(), None, SelectionOp::Replace)
        .unwrap()
        .channel_id;
    assert_ne!(first, second);
    let rows = s.document_channels().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.iter().all(|c| c.name == "Alpha 1"));
    assert_eq!(rows.iter().map(|c| c.index).collect::<Vec<_>>(), vec![0, 1]);
    s.load_selection_channel(first, SelectionOp::Replace, false)
        .unwrap();
    assert_eq!(sel_row(&s), [1.0, 0.0, 0.0, 0.0]);
    s.load_selection_channel(second, SelectionOp::Replace, false)
        .unwrap();
    assert_eq!(sel_row(&s), [0.0, 0.0, 0.0, 1.0]);

    // The legacy name-based calls see the persistent channels.
    assert_eq!(
        s.selection_channels().unwrap(),
        vec!["Alpha 1".to_string(), "Alpha 1".to_string()]
    );
    marquee(&s, 100.0, 100.0, SelectionOp::Replace);
    s.save_selection("Alpha 1".into()).unwrap();
    assert_eq!(
        s.document_channels().unwrap().len(),
        2,
        "replaced, not added"
    );
    assert_eq!(chan_row(&s, first), [0.0, 1.0, 0.0, 0.0]);
    assert_eq!(chan_row(&s, second), [0.0, 0.0, 0.0, 1.0]);
    s.save_selection("Beta".into()).unwrap();
    assert_eq!(s.selection_channels().unwrap().len(), 3);
    s.select_none().unwrap();
    s.load_selection("Alpha 1".into(), SelectionOp::Replace)
        .unwrap();
    assert_eq!(sel_row(&s), [0.0, 1.0, 0.0, 0.0]);
    assert!(
        s.load_selection("missing".into(), SelectionOp::Replace)
            .is_err()
    );
}

#[test]
fn stale_ids_error_cleanly() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    s.delete_document_channel(a).unwrap();
    for id in [a, 0, 9_999] {
        assert!(s.rename_document_channel(id, "x".into()).is_err());
        assert!(s.delete_document_channel(id).is_err());
        assert!(s.duplicate_document_channel(id).is_err());
        assert!(
            s.load_selection_channel(id, SelectionOp::Replace, false)
                .is_err()
        );
        assert!(s.set_spot_channel(id, INK, 0.5).is_err());
        assert!(s.set_channel_visible(id, true).is_err());
        assert!(s.channel_thumbnail(id, 64).is_err());
        assert!(
            s.save_selection_channel("x".into(), Some(id), SelectionOp::Add)
                .is_err()
        );
    }
    assert!(s.document_channels().unwrap().is_empty());
    // Undo brings the channel back under its old id.
    s.undo().unwrap();
    assert_eq!(s.document_channels().unwrap()[0].id, a);
    s.rename_document_channel(a, "back".into()).unwrap();
    assert!(s.rename_document_channel(a, " ".into()).is_err());
}

#[test]
fn every_channel_op_undoes_and_redoes() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    type Op = Box<dyn Fn(&DocumentSession, u64)>;
    let ops: Vec<(&str, Op)> = vec![
        (
            "Save Selection",
            Box::new(|s, _| {
                s.save_selection_channel("B".into(), None, SelectionOp::Replace)
                    .unwrap();
            }),
        ),
        (
            "Save Selection",
            Box::new(|s, a| {
                s.save_selection_channel("B".into(), Some(a), SelectionOp::Add)
                    .unwrap();
            }),
        ),
        (
            "Rename Channel",
            Box::new(|s, a| {
                s.rename_document_channel(a, "Renamed".into()).unwrap();
            }),
        ),
        (
            "Duplicate Channel",
            Box::new(|s, a| {
                s.duplicate_document_channel(a).unwrap();
            }),
        ),
        (
            "Channel Options",
            Box::new(|s, a| {
                s.set_spot_channel(a, INK, 0.4).unwrap();
            }),
        ),
        (
            "New Spot Channel",
            Box::new(|s, _| {
                s.new_spot_channel("Spot".into(), INK, 1.0, true).unwrap();
            }),
        ),
        (
            "New Channel",
            Box::new(|s, _| {
                s.new_alpha_channel("Alpha 2".into(), false).unwrap();
            }),
        ),
        (
            "Delete Channel",
            Box::new(|s, a| {
                s.delete_document_channel(a).unwrap();
            }),
        ),
    ];
    for (label, op) in ops {
        let before = snapshot(&s);
        let n = labels(&s).len();
        op(&s, a);
        let after = snapshot(&s);
        assert_ne!(before, after, "{label} changed the channels");
        assert_eq!(labels(&s).len(), n + 1, "{label}: one history node");
        assert_eq!(labels(&s).last().unwrap(), label);
        s.undo().unwrap();
        assert_eq!(snapshot(&s), before, "{label} undone");
        s.redo().unwrap();
        assert_eq!(snapshot(&s), after, "{label} redone");
        s.undo().unwrap();
    }
    // Load Selection is undoable too (the selection, not the channels).
    let row = sel_row(&s);
    s.load_selection_channel(a, SelectionOp::Replace, true)
        .unwrap();
    assert_ne!(sel_row(&s), row);
    s.undo().unwrap();
    assert_eq!(sel_row(&s), row);
    s.redo().unwrap();
    assert_eq!(sel_row(&s), [0.0, 0.0, 1.0, 1.0]);
}

#[test]
fn invalid_spot_metadata_is_rejected() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let n = labels(&s).len();
    let bad = [
        (INK, 1.5),
        (INK, -0.1),
        (INK, f32::NAN),
        (
            PaintColor {
                r: f32::INFINITY,
                g: 0.0,
                b: 0.0,
            },
            0.5,
        ),
        (
            PaintColor {
                r: 0.0,
                g: 1.2,
                b: 0.0,
            },
            0.5,
        ),
    ];
    for (color, solidity) in bad {
        assert!(s.set_spot_channel(a, color, solidity).is_err());
        assert!(
            s.new_spot_channel("S".into(), color, solidity, false)
                .is_err()
        );
    }
    assert_eq!(labels(&s).len(), n, "rejected edits record nothing");
    assert_eq!(s.document_channels().unwrap().len(), 1);
    assert!(s.new_spot_channel(" ".into(), INK, 0.5, false).is_err());
    s.select_none().unwrap();
    assert!(
        s.new_spot_channel("S".into(), INK, 0.5, true).is_err(),
        "from_selection needs a selection"
    );
    let r = s.new_spot_channel("S".into(), INK, 0.5, false).unwrap();
    let row = s
        .document_channels()
        .unwrap()
        .into_iter()
        .find(|c| c.id == r.channel_id)
        .unwrap();
    assert_eq!(row.kind, DocChannelKind::Spot);
    assert_eq!(row.opacity, 0.5);
    assert_eq!(chan_row(&s, r.channel_id), [0.0; 4]);
}

/// Two channels (alpha "Mask" = left half, spot "Varnish" = x ≥ 300), saved
/// to `path` and reopened.
fn round_trip(ext: &str) {
    let (d, e) = engine();
    let (s, layer) = doc(&e);
    marquee(&s, 0.0, 200.0, SelectionOp::Replace);
    s.save_selection_channel("Mask".into(), None, SelectionOp::Replace)
        .unwrap();
    marquee(&s, 300.0, 100.0, SelectionOp::Replace);
    let ink = PaintColor {
        r: 1.0,
        g: 0.8,
        b: 0.0,
    };
    s.new_spot_channel("Varnish".into(), ink, 0.4, true)
        .unwrap();
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.2,
                g: 0.4,
                b: 0.6,
            },
        },
        1.0,
    )
    .unwrap();
    let path = d.path().join(format!("channels.{ext}"));
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    drop(s);

    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = r.document_channels().unwrap();
    assert_eq!(
        rows.iter()
            .map(|c| (c.name.as_str(), c.kind))
            .collect::<Vec<_>>(),
        vec![
            ("Mask", DocChannelKind::Alpha),
            ("Varnish", DocChannelKind::Spot)
        ],
        "{ext}"
    );
    let spot = &rows[1];
    assert!((spot.color.r - 1.0).abs() < 1e-3, "{ext} {:?}", spot.color);
    assert!((spot.color.g - 0.8).abs() < 1e-3, "{ext} {:?}", spot.color);
    assert!(spot.color.b.abs() < 1e-3, "{ext} {:?}", spot.color);
    assert!((spot.opacity - 0.4).abs() < 1e-3, "{ext} {}", spot.opacity);
    assert_eq!(chan_row(&r, rows[0].id), [1.0, 1.0, 0.0, 0.0], "{ext}");
    assert_eq!(chan_row(&r, rows[1].id), [0.0, 0.0, 0.0, 1.0], "{ext}");
    // Loading a reopened channel works (PSD planes arrive at 8 bits).
    r.load_selection_channel(rows[0].id, SelectionOp::Replace, false)
        .unwrap();
    assert_eq!(sel_row(&r), [1.0, 1.0, 0.0, 0.0], "{ext}");
    // New channels after reopening get fresh ids.
    let next = r.new_alpha_channel("After".into(), true).unwrap();
    assert!(rows.iter().all(|c| c.id != next.channel_id), "{ext}");
    assert_eq!(chan_row(&r, next.channel_id), [1.0; 4]);
}

#[test]
fn tessera_doc_round_trip_keeps_channels() {
    round_trip("tessera-doc");
}

#[test]
fn psd_round_trip_keeps_channels() {
    round_trip("psd");
}

fn export_png(s: &DocumentSession, path: &std::path::Path) -> Vec<u8> {
    s.export_flat(
        path.to_string_lossy().into_owned(),
        ExportFormat::Png,
        100,
        ExportColor::Document,
    )
    .unwrap();
    image::open(path).unwrap().to_rgba8().into_raw()
}

#[test]
fn spot_channels_do_not_change_the_rgb_composite() {
    let (d, e) = engine();
    let (s, layer) = doc(&e);
    marquee(&s, 50.0, 200.0, SelectionOp::Replace);
    s.fill_selection(
        layer,
        SelectionFill::Color {
            color: PaintColor {
                r: 0.9,
                g: 0.1,
                b: 0.3,
            },
        },
        1.0,
    )
    .unwrap();
    let before = export_png(&s, &d.path().join("before.png"));
    let spot = s
        .new_spot_channel("Ink".into(), INK, 1.0, true)
        .unwrap()
        .channel_id;
    s.set_channel_visible(spot, true).unwrap();
    s.save_selection_channel("Alpha".into(), None, SelectionOp::Replace)
        .unwrap();
    s.set_spot_channel(
        spot,
        PaintColor {
            r: 0.0,
            g: 0.0,
            b: 0.0,
        },
        1.0,
    )
    .unwrap();
    let after = export_png(&s, &d.path().join("after.png"));
    assert_eq!(before.len(), after.len());
    assert!(
        before == after,
        "spot and alpha channels changed the RGB export"
    );
    // PSD composite too: save, reopen and export.
    let psd = d.path().join("spot.psd");
    s.save_as(psd.to_string_lossy().into_owned()).unwrap();
    s.close();
    drop(s);
    let r = e
        .clone()
        .open_document(psd.to_string_lossy().into_owned())
        .unwrap();
    assert_eq!(r.document_channels().unwrap().len(), 2);
    assert!(export_png(&r, &d.path().join("reopened.png")) == before);
}

#[test]
fn thumbnails_visibility_and_revisions() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let t1 = s.channel_thumbnail(a, 64).unwrap();
    assert_ne!(t1, 0);
    assert_eq!(s.channel_thumbnail(a, 64).unwrap(), t1, "cached");
    let rev = s.document_channels().unwrap()[0].revision;
    assert_eq!(s.document_channels().unwrap()[0].revision, rev, "stable");
    assert!(s.channel_thumbnail(a, 0).is_err());
    assert!(s.channel_thumbnail(a, 5000).is_err());

    // Renaming keeps the samples: same revision, same thumbnail.
    s.rename_document_channel(a, "A2".into()).unwrap();
    assert_eq!(s.document_channels().unwrap()[0].revision, rev);
    assert_eq!(s.channel_thumbnail(a, 64).unwrap(), t1);

    // New samples: new revision and a new surface.
    s.save_selection_channel(String::new(), Some(a), SelectionOp::Add)
        .unwrap();
    assert_ne!(s.document_channels().unwrap()[0].revision, rev);
    assert_ne!(s.channel_thumbnail(a, 64).unwrap(), t1);

    // Visibility is session state, not a history node.
    let n = labels(&s).len();
    assert!(!s.document_channels().unwrap()[0].visible);
    s.set_channel_visible(a, true).unwrap();
    assert!(s.document_channels().unwrap()[0].visible);
    assert_eq!(labels(&s).len(), n);
    s.set_channel_visible(a, false).unwrap();
    assert!(!s.document_channels().unwrap()[0].visible);
}

// B5-10b begin: Channels thumbnails after a PSD reopen (seen black once in
// B5-10's evidence). Mask = left half (x < 200), Varnish = x ≥ 300.

/// Grey level of thumbnail `sid` (`w × h`) at thumbnail pixel `(x, y)`.
fn thumb_grey(sid: u32, w: u32, h: u32, x: u32, y: u32) -> u8 {
    let surface = tessera_ffi::surface::Surface::lookup(sid, w, h).unwrap();
    surface
        .with_pixels(|px, stride| {
            let o = y as usize * stride + x as usize * 4;
            assert_eq!(px[o], px[o + 1]);
            assert_eq!(px[o + 3], 255);
            px[o]
        })
        .unwrap()
}

/// `[x=50, 150, 250, 350]` of channel thumbnail `id` at 64 px (step 7).
fn thumb_row(s: &DocumentSession, id: u64) -> [u8; 4] {
    let sid = s.channel_thumbnail(id, 64).unwrap();
    let (w, h) = (W.div_ceil(7), H.div_ceil(7));
    [50, 150, 250, 350].map(|x| thumb_grey(sid, w, h, x / 7, 10 / 7))
}

fn thumbnails_after_reopen(ext: &str, depth: DocDepth) {
    let (d, e) = engine();
    let s = e.clone().new_document(W, H, depth, None).unwrap();
    marquee(&s, 0.0, 200.0, SelectionOp::Replace);
    let mask = s
        .save_selection_channel("Mask".into(), None, SelectionOp::Replace)
        .unwrap()
        .channel_id;
    marquee(&s, 300.0, 100.0, SelectionOp::Replace);
    let ink = PaintColor {
        r: 1.0,
        g: 0.8,
        b: 0.0,
    };
    let spot = s
        .new_spot_channel("Varnish".into(), ink, 0.4, true)
        .unwrap()
        .channel_id;
    assert_eq!(thumb_row(&s, mask), [255, 255, 0, 0], "{ext} before save");
    assert_eq!(thumb_row(&s, spot), [0, 0, 0, 255], "{ext} before save");
    let path = d.path().join(format!("thumbs.{ext}"));
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    drop(s);
    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = r.document_channels().unwrap();
    assert_eq!(rows.len(), 2, "{ext}");
    assert_eq!(
        chan_row(&r, rows[0].id),
        [1.0, 1.0, 0.0, 0.0],
        "{ext} samples"
    );
    assert_eq!(
        thumb_row(&r, rows[0].id),
        [255, 255, 0, 0],
        "{ext} {depth:?} Mask"
    );
    assert_eq!(
        thumb_row(&r, rows[1].id),
        [0, 0, 0, 255],
        "{ext} {depth:?} Varnish"
    );
}

#[test]
fn channel_thumbnails_survive_psd_reopen() {
    for depth in [DocDepth::U8, DocDepth::U16, DocDepth::F32] {
        thumbnails_after_reopen("psd", depth);
        thumbnails_after_reopen("tessera-doc", depth);
    }
}
// B5-10b end

// ─────────────────────── B5-17b: persisted alpha display ───────────────────────

const GREEN: PaintColor = PaintColor {
    r: 0.0,
    g: 1.0,
    b: 0.25,
};

const RED: PaintColor = PaintColor {
    r: 1.0,
    g: 0.0,
    b: 0.0,
};

fn row(s: &DocumentSession, id: u64) -> ChannelRecord {
    s.document_channels()
        .unwrap()
        .into_iter()
        .find(|c| c.id == id)
        .expect("channel row")
}

/// (kind, colour, opacity, selected_areas) of channel `id`.
fn display(s: &DocumentSession, id: u64) -> (DocChannelKind, [f32; 3], f32, bool) {
    let r = row(s, id);
    (
        r.kind,
        [r.color.r, r.color.g, r.color.b],
        r.opacity,
        r.selected_areas,
    )
}

fn engine_kind(s: &DocumentSession, id: u64) -> compositor::channels::ChannelKind {
    s.document_state()
        .unwrap()
        .channels
        .iter()
        .find(|c| c.id.0 == id)
        .expect("channel")
        .kind
        .clone()
}

#[test]
fn alpha_display_is_one_undoable_node() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    assert_eq!(
        display(&s, a),
        (DocChannelKind::Alpha, [1.0, 0.0, 0.0], 0.5, false),
        "legacy default: red, 50 %, masked areas"
    );
    let samples = chan_row(&s, a);
    let n = labels(&s).len();
    s.set_alpha_channel_display(a, GREEN, 0.3, true).unwrap();
    assert_eq!(labels(&s).len(), n + 1, "one history node");
    assert_eq!(labels(&s).last().unwrap(), "Channel Options");
    assert_eq!(
        display(&s, a),
        (DocChannelKind::Alpha, [0.0, 1.0, 0.25], 0.3, true)
    );
    assert_eq!(
        chan_row(&s, a),
        samples,
        "display metadata never changes samples"
    );
    s.undo().unwrap();
    assert_eq!(
        display(&s, a),
        (DocChannelKind::Alpha, [1.0, 0.0, 0.0], 0.5, false)
    );
    s.redo().unwrap();
    assert_eq!(
        display(&s, a),
        (DocChannelKind::Alpha, [0.0, 1.0, 0.25], 0.3, true)
    );
}

#[test]
fn legacy_default_display_stays_plain_alpha() {
    use compositor::channels::ChannelKind;
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    assert!(matches!(engine_kind(&s, a), ChannelKind::Alpha));
    s.set_alpha_channel_display(a, GREEN, 0.3, false).unwrap();
    assert!(matches!(
        engine_kind(&s, a),
        ChannelKind::AlphaDisplay {
            selected: false,
            ..
        }
    ));
    // Back to red / 50 % / masked: the legacy identity, not an explicit copy of it.
    s.set_alpha_channel_display(a, RED, 0.5, false).unwrap();
    assert!(matches!(engine_kind(&s, a), ChannelKind::Alpha));
    // Only the indicator differs from the default: explicit.
    s.set_alpha_channel_display(a, RED, 0.5, true).unwrap();
    assert!(matches!(
        engine_kind(&s, a),
        ChannelKind::AlphaDisplay { selected: true, .. }
    ));
}

#[test]
fn invalid_alpha_display_is_rejected_without_a_node() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let n = labels(&s).len();
    let before = display(&s, a);
    let bad = [
        (GREEN, 1.5),
        (GREEN, -0.01),
        (GREEN, f32::NAN),
        (
            PaintColor {
                r: f32::INFINITY,
                g: 0.0,
                b: 0.0,
            },
            0.5,
        ),
        (
            PaintColor {
                r: 0.0,
                g: -0.2,
                b: 0.0,
            },
            0.5,
        ),
    ];
    for (color, opacity) in bad {
        assert!(
            s.set_alpha_channel_display(a, color, opacity, true)
                .is_err()
        );
    }
    assert!(
        s.set_alpha_channel_display(999, GREEN, 0.5, false).is_err(),
        "unknown channel"
    );
    assert_eq!(labels(&s).len(), n, "rejected edits record nothing");
    assert_eq!(display(&s, a), before);
}

#[test]
fn alpha_to_spot_to_alpha_round_trip() {
    let (_d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let samples = chan_row(&s, a);
    s.set_alpha_channel_display(a, GREEN, 0.3, true).unwrap();
    s.set_spot_channel(a, INK, 0.8).unwrap();
    assert_eq!(row(&s, a).kind, DocChannelKind::Spot);
    assert!(!row(&s, a).selected_areas, "spot ink has no indicator");
    let n = labels(&s).len();
    // Channel Options ▸ Color Indicates: Selected Areas turns the spot back into an alpha channel.
    s.set_alpha_channel_display(a, GREEN, 0.3, true).unwrap();
    assert_eq!(labels(&s).len(), n + 1);
    assert_eq!(
        display(&s, a),
        (DocChannelKind::Alpha, [0.0, 1.0, 0.25], 0.3, true)
    );
    assert_eq!(chan_row(&s, a), samples);
    s.undo().unwrap();
    assert_eq!(row(&s, a).kind, DocChannelKind::Spot);
}

fn display_round_trip(ext: &str) {
    let (d, e) = engine();
    let (s, _) = doc(&e);
    let a = setup_a(&s);
    let legacy = s
        .new_alpha_channel("Legacy".into(), false)
        .unwrap()
        .channel_id;
    let masked = s
        .new_alpha_channel("Masked".into(), false)
        .unwrap()
        .channel_id;
    s.set_alpha_channel_display(a, GREEN, 0.3, true).unwrap();
    s.set_alpha_channel_display(
        masked,
        PaintColor {
            r: 0.0,
            g: 0.0,
            b: 1.0,
        },
        0.75,
        false,
    )
    .unwrap();
    let _ = legacy;
    let path = d.path().join(format!("display.{ext}"));
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    drop(s);

    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = r.document_channels().unwrap();
    assert_eq!(
        rows.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(),
        vec!["A", "Legacy", "Masked"],
        "{ext}"
    );
    let close = |c: &ChannelRecord, rgb: [f32; 3], op: f32, sel: bool| {
        assert_eq!(c.kind, DocChannelKind::Alpha, "{ext} {}", c.name);
        assert_eq!(c.selected_areas, sel, "{ext} {}", c.name);
        for (got, want) in [c.color.r, c.color.g, c.color.b].into_iter().zip(rgb) {
            assert!((got - want).abs() < 1e-3, "{ext} {} {:?}", c.name, c.color);
        }
        // PSD stores opacity in whole percent.
        assert!(
            (c.opacity - op).abs() < 6e-3,
            "{ext} {} {}",
            c.name,
            c.opacity
        );
    };
    close(&rows[0], [0.0, 1.0, 0.25], 0.3, true);
    close(&rows[1], [1.0, 0.0, 0.0], 0.5, false);
    close(&rows[2], [0.0, 0.0, 1.0], 0.75, false);
    assert_eq!(chan_row(&r, rows[0].id), [1.0, 1.0, 0.0, 0.0], "{ext}");
    // Reopened channels still take new display settings as one node.
    let n = labels(&r).len();
    r.set_alpha_channel_display(rows[0].id, RED, 0.5, false)
        .unwrap();
    assert_eq!(labels(&r).len(), n + 1, "{ext}");
    assert!(!row(&r, rows[0].id).selected_areas, "{ext}");
}

#[test]
fn tessera_doc_round_trip_keeps_alpha_display() {
    display_round_trip("tessera-doc");
}

#[test]
fn psd_round_trip_keeps_alpha_display() {
    display_round_trip("psd");
}
