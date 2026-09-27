//! Editable text layers over the bridge (WP B5-10): insertion ids and
//! order, drafts and the one-node typing group, run splices versus whole-model
//! edits, invalid run ranges, engine layout for the host caret (UTF-8
//! clusters, graphemes, non-BMP, ligatures, bidi), cancellation, locks and
//! affine validation, Convert to Pixels (masks, styles, undo), CPU/resident
//! rendering with the shared font snapshot, and native/PSD reopen.
//!
//! Deterministic layout and rendering use the bundled OFL Noto Sans fixture
//! (crates/typography/tests/fonts), added to the shared snapshot before any
//! session exists. Conversion creates a system-font renderer, so those cases
//! use Helvetica, which macOS installs.
#![cfg(target_os = "macos")]

use std::sync::{Arc, Once};
use tessera_ffi::*;
use typography::{Alignment, TextBox, TextModel, TextRun};

const W: u32 = 320;
const H: u32 = 160;
const NOTO: &str = "Noto Sans";
const SYSTEM: &str = "Helvetica";

fn fixture_fonts() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        load_text_fonts_for_tests(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../typography/tests/fonts"
        ))
    });
}

fn engine() -> (tempfile::TempDir, Arc<Engine>) {
    fixture_fonts();
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    (dir, engine)
}

fn doc(engine: &Arc<Engine>) -> Arc<DocumentSession> {
    engine
        .clone()
        .new_document(W, H, DocDepth::U8, None)
        .unwrap()
}

fn run(text: &str, family: &str, size: f32) -> TextRun {
    TextRun {
        text: text.into(),
        family: family.into(),
        size,
        ..TextRun::default()
    }
}

fn point(runs: Vec<TextRun>) -> TextModel {
    TextModel {
        runs,
        ..TextModel::default()
    }
}

fn json(m: &TextModel) -> String {
    serde_json::to_string(m).unwrap()
}

fn model_of(s: &DocumentSession, id: u64) -> TextModel {
    serde_json::from_str(&s.text_layer(id).unwrap().model_json).unwrap()
}

fn at(x: f64, y: f64) -> TransformMatrix {
    TransformMatrix {
        a: 1.0,
        b: 0.0,
        c: x,
        d: 0.0,
        e: 1.0,
        f: y,
    }
}

fn history_len(s: &DocumentSession) -> usize {
    s.history_items().unwrap().len()
}

fn head(s: &DocumentSession) -> u64 {
    s.info().unwrap().history_head
}

/// Adds a committed text layer and returns its id.
fn add(s: &DocumentSession, m: &TextModel, t: TransformMatrix) -> u64 {
    let u = s
        .add_text_layer(String::new(), None, None, json(m), t, false)
        .unwrap();
    assert_eq!(u.created.len(), 1, "one created layer");
    u.created[0]
}

fn ids_bottom_first(s: &DocumentSession) -> Vec<u64> {
    let mut rows: Vec<_> = s
        .layers()
        .unwrap()
        .into_iter()
        .filter(|n| n.parent.is_none())
        .collect();
    rows.sort_by_key(|n| n.index);
    rows.into_iter().map(|n| n.id).collect()
}

fn alpha_sum(rgba: &[f32]) -> f32 {
    rgba.chunks(4).map(|p| p[3]).sum()
}

#[test]
fn add_text_layer_uses_created_ids_and_bottom_first_indexes() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = s.layers().unwrap()[0].id;
    let m = point(vec![run("Top", NOTO, 24.0)]);
    let top = add(&s, &m, at(10.0, 40.0));
    // Index 0 is the bottom of the root (below "Layer 1").
    let u = s
        .add_text_layer(
            "Bottom".into(),
            None,
            Some(0),
            json(&point(vec![run("Bottom", NOTO, 24.0)])),
            at(10.0, 90.0),
            false,
        )
        .unwrap();
    let bottom = u.created[0];
    assert_ne!(bottom, top);
    assert_eq!(ids_bottom_first(&s), vec![bottom, base, top]);
    let rows = s.layers().unwrap();
    let named = |id: u64| rows.iter().find(|n| n.id == id).unwrap().clone();
    assert_eq!(
        named(top).name,
        "Top",
        "unnamed layers take their first line"
    );
    assert_eq!(named(bottom).name, "Bottom");
    assert_eq!(named(top).kind, DocLayerKind::Text);
    // Into a group, at its bottom.
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
    let child = s
        .add_text_layer(
            String::new(),
            Some(g),
            Some(0),
            json(&m),
            at(0.0, 30.0),
            false,
        )
        .unwrap()
        .created[0];
    assert_eq!(s.layer(child).unwrap().parent, Some(g));
    let labels: Vec<_> = s
        .history_items()
        .unwrap()
        .into_iter()
        .map(|h| h.label)
        .collect();
    assert_eq!(labels.iter().filter(|l| *l == "Add Text").count(), 3);
}

#[test]
fn interactive_add_is_provisional_until_the_final_call() {
    let (_d, e) = engine();
    let s = doc(&e);
    let (n0, h0) = (history_len(&s), head(&s));
    let mut m = point(vec![run("H", NOTO, 30.0)]);
    let u = s
        .add_text_layer(String::new(), None, None, json(&m), at(20.0, 50.0), true)
        .unwrap();
    assert!(u.created.is_empty(), "a draft layer is not a created layer");
    assert_eq!(u.history_head, h0);
    assert_eq!(s.layers().unwrap().len(), 2, "the draft layer shows");
    m.runs[0].text = "Hello".into();
    s.add_text_layer(String::new(), None, None, json(&m), at(20.0, 50.0), true)
        .unwrap();
    assert_eq!(s.layers().unwrap().len(), 2, "rebuilt, not added twice");
    assert_eq!(history_len(&s), n0);
    let u = s
        .add_text_layer(String::new(), None, None, json(&m), at(20.0, 50.0), false)
        .unwrap();
    assert_eq!(u.created.len(), 1);
    assert_eq!(history_len(&s), n0 + 1);
    assert_eq!(model_of(&s, u.created[0]), m);
    // Draft then cancel: nothing added, no node.
    s.add_text_layer(String::new(), None, None, json(&m), at(20.0, 90.0), true)
        .unwrap();
    assert_eq!(s.layers().unwrap().len(), 3);
    s.cancel_source_preview().unwrap();
    assert_eq!(s.layers().unwrap().len(), 2);
    assert_eq!(history_len(&s), n0 + 1);
}

#[test]
fn run_only_drafts_commit_one_splice_and_keep_untouched_runs() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![
        run("Red ", NOTO, 24.0),
        TextRun {
            color: [0, 0, 255, 255],
            ..run("blue ", NOTO, 30.0)
        },
        run("tail", NOTO, 24.0),
    ]);
    let id = add(&s, &base, at(5.0, 40.0));
    let rev = s.text_layer(id).unwrap().revision;
    let mut next = base.clone();
    next.runs[1].text = "bluer ".into();
    // The splice replaces only run 1.
    let splice = text_run_splice(&base.runs, &next.runs).unwrap();
    assert_eq!((splice.start, splice.end), (1, 2));
    assert_eq!(splice.runs, vec![next.runs[1].clone()]);
    // Inserting a run between 0 and 1 is an empty-range insertion.
    let mut ins = base.clone();
    ins.runs.insert(1, run("new ", NOTO, 12.0));
    let sp = text_run_splice(&base.runs, &ins.runs).unwrap();
    assert_eq!((sp.start, sp.end, sp.runs.len()), (1, 1, 1));
    assert!(text_run_splice(&base.runs, &base.runs).is_none());
    let n0 = history_len(&s);
    s.set_text_layer(id, json(&next), at(5.0, 40.0), false, Some(rev))
        .unwrap();
    assert_eq!(history_len(&s), n0 + 1);
    let now = model_of(&s, id);
    assert_eq!(now, next);
    assert_eq!(now.runs[0], base.runs[0]);
    assert_eq!(now.runs[2], base.runs[2]);
    assert_eq!(now.paragraph, base.paragraph);
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Edit Text"
    );
}

#[test]
fn paragraph_and_box_changes_commit_the_whole_model() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("one two three four five", NOTO, 20.0)]);
    let id = add(&s, &base, at(5.0, 30.0));
    let mut area = base.clone();
    area.text_box = TextBox::Paragraph {
        width: 120.0,
        height: 100.0,
    };
    area.paragraph.alignment = Alignment::Center;
    area.paragraph.left_indent = 4.0;
    let n0 = history_len(&s);
    s.set_text_layer(id, json(&area), at(5.0, 30.0), false, None)
        .unwrap();
    assert_eq!(history_len(&s), n0 + 1);
    let now = model_of(&s, id);
    assert_eq!(now, area, "box and paragraph replaced, runs kept");
    let lay = layout_text(json(&now)).unwrap();
    assert!(lay.lines.len() > 1, "the area wraps");
    assert!(
        lay.lines.iter().all(|l| l.available_width.is_some()),
        "area lines are bounded"
    );
    // Resize the box narrower: more lines, same runs (no bitmap scaling).
    let mut narrow = now.clone();
    narrow.text_box = TextBox::Paragraph {
        width: 70.0,
        height: 30.0,
    };
    s.set_text_layer(id, json(&narrow), at(5.0, 30.0), false, None)
        .unwrap();
    let lay2 = layout_text(json(&narrow)).unwrap();
    assert!(lay2.overflow, "a short box reports overflow");
    assert_eq!(model_of(&s, id).runs, base.runs);
    s.undo().unwrap();
    assert_eq!(model_of(&s, id), area);
}

#[test]
fn insertion_and_deletion_across_mixed_style_runs() {
    let (_d, e) = engine();
    let s = doc(&e);
    let bold = TextRun {
        weight: 700,
        ..run("bold", NOTO, 24.0)
    };
    let base = point(vec![
        run("plain ", NOTO, 24.0),
        bold.clone(),
        run(" end", NOTO, 18.0),
    ]);
    let id = add(&s, &base, at(5.0, 40.0));
    // Delete "n " + "bo" across runs 0/1 (UTF-8 4..8), then insert "X" in run 1.
    let mut next = base.clone();
    next.runs[0].text = "plai".into();
    next.runs[1].text = "Xld".into();
    let rev = s.text_layer(id).unwrap().revision;
    s.set_text_layer(id, json(&next), at(5.0, 40.0), true, Some(rev))
        .unwrap();
    assert!(s.text_layer(id).unwrap().draft_pending);
    assert_eq!(
        s.text_layer(id).unwrap().revision,
        rev,
        "the base is unchanged"
    );
    s.commit("Typing".into()).unwrap();
    let now = model_of(&s, id);
    let text: String = now.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(text, "plaiXld end");
    assert_eq!(now.runs[1].weight, 700, "the bold run keeps its style");
    assert_eq!(now.runs[2], base.runs[2], "untouched run unchanged");
    assert_eq!(s.history_items().unwrap().last().unwrap().label, "Typing");
    // Deleting a whole run removes it through the same splice path.
    let mut gone = now.clone();
    gone.runs.remove(1);
    s.set_text_layer(id, json(&gone), at(5.0, 40.0), false, None)
        .unwrap();
    assert_eq!(model_of(&s, id).runs.len(), 2);
}

#[test]
fn edit_text_runs_rejects_invalid_ranges_stale_revisions_and_drafts() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("a", NOTO, 20.0), run("b", NOTO, 20.0)]);
    let id = add(&s, &base, at(5.0, 30.0));
    let rev = s.text_layer(id).unwrap().revision;
    let runs = serde_json::to_string(&vec![run("c", NOTO, 20.0)]).unwrap();
    let (n0, h0) = (history_len(&s), head(&s));
    assert!(
        s.edit_text_runs(id, 2, 1, runs.clone(), None).is_err(),
        "reversed"
    );
    assert!(
        s.edit_text_runs(id, 1, 3, runs.clone(), None).is_err(),
        "past the end"
    );
    assert!(
        s.edit_text_runs(id, 0, 1, runs.clone(), Some(rev + 99))
            .is_err(),
        "stale revision"
    );
    assert!(
        s.edit_text_runs(id, 0, 1, "[{\"bogus\":1}]".into(), None)
            .is_err()
    );
    assert!(
        s.edit_text_runs(
            s.layers()
                .unwrap()
                .iter()
                .find(|n| n.kind == DocLayerKind::Pixel)
                .unwrap()
                .id,
            0,
            0,
            runs.clone(),
            None
        )
        .is_err(),
        "not a text layer"
    );
    assert_eq!((history_len(&s), head(&s)), (n0, h0));
    assert_eq!(model_of(&s, id), base);
    // A pending draft of the layer blocks run edits (indexes would be stale).
    let mut draft = base.clone();
    draft.runs[0].text = "aa".into();
    s.set_text_layer(id, json(&draft), at(5.0, 30.0), true, None)
        .unwrap();
    assert!(s.edit_text_runs(id, 0, 0, runs.clone(), None).is_err());
    s.cancel_source_preview().unwrap();
    // Valid: an empty range inserts before run 1.
    s.edit_text_runs(id, 1, 1, runs, Some(rev)).unwrap();
    let texts: Vec<_> = model_of(&s, id).runs.into_iter().map(|r| r.text).collect();
    assert_eq!(texts, vec!["a", "c", "b"]);
    assert_eq!(history_len(&s), n0 + 1);
}

#[test]
fn layout_clusters_are_utf8_offsets_on_grapheme_boundaries() {
    fixture_fonts();
    // "e" + combining acute, a non-BMP letter (U+1D400), and CJK.
    let text = "ae\u{301}b\u{1D400}c";
    let m = point(vec![run(text, NOTO, 24.0)]);
    let lay = layout_text(json(&m)).unwrap();
    assert_eq!(lay.text_len as usize, text.len());
    for g in &lay.glyphs {
        assert!(
            text.is_char_boundary(g.cluster as usize),
            "cluster on a char boundary"
        );
    }
    let clusters: std::collections::BTreeSet<u32> = lay.glyphs.iter().map(|g| g.cluster).collect();
    // The combining mark shares the base letter's cluster (offset 1); the
    // mark's own offset (2) is never a cluster start.
    assert!(clusters.contains(&1));
    assert!(!clusters.contains(&2));
    // "b" is at 4; the 4-byte scalar is one cluster at 5, nothing inside it.
    assert!(clusters.contains(&4) && clusters.contains(&5) && clusters.contains(&9));
    assert!(!(6..9).any(|o| clusters.contains(&o)));
    assert_eq!(lay.lines.len(), 1);
    assert_eq!(
        (lay.lines[0].source_start, lay.lines[0].source_end),
        (0, text.len() as u32)
    );
    // Two runs: glyphs report their run index.
    let two = point(vec![run("ab", NOTO, 24.0), run("cd", NOTO, 12.0)]);
    let lay = layout_text(json(&two)).unwrap();
    let runs: Vec<_> = lay.glyphs.iter().map(|g| (g.run, g.cluster)).collect();
    assert_eq!(runs, vec![(0, 0), (0, 1), (1, 2), (1, 3)]);
    // Mandatory breaks: one line per paragraph, source ranges cover the text.
    let multi = point(vec![run("ab\ncd", NOTO, 24.0)]);
    let lay = layout_text(json(&multi)).unwrap();
    assert_eq!(lay.lines.len(), 2);
    assert_eq!(lay.lines[1].source_start, 3);
    assert!(lay.lines[1].baseline > lay.lines[0].baseline);
}

#[test]
fn layout_keeps_ligatures_as_one_cluster() {
    fixture_fonts();
    let m = point(vec![run("office", NOTO, 24.0)]);
    let lay = layout_text(json(&m)).unwrap();
    assert_eq!(lay.glyphs.len(), 4, "o + ffi ligature + c + e");
    let clusters: Vec<_> = lay.glyphs.iter().map(|g| g.cluster).collect();
    assert_eq!(
        clusters,
        vec![0, 1, 4, 5],
        "the ligature is one cluster at 1"
    );
    let mut plain = m.clone();
    plain.runs[0].features.insert("liga".into(), 0);
    let lay = layout_text(json(&plain)).unwrap();
    assert_eq!(lay.glyphs.len(), 6);
    assert!(lay.glyphs.iter().all(|g| !g.rtl));
}

#[test]
fn layout_bidi_reports_visual_rtl_runs_with_source_offsets() {
    fixture_fonts();
    let text = "a \u{5D0}\u{5D1}\u{5D2} z";
    let m = point(vec![run(text, NOTO, 20.0)]);
    let lay = layout_text(json(&m)).unwrap();
    let hebrew: Vec<_> = lay
        .glyphs
        .iter()
        .filter(|g| (2..8).contains(&g.cluster))
        .map(|g| (g.cluster, g.rtl))
        .collect();
    assert_eq!(
        hebrew,
        vec![(6, true), (4, true), (2, true)],
        "visual order, RTL"
    );
    assert!(!lay.glyphs[0].rtl && !lay.glyphs.last().unwrap().rtl);
    assert!(lay.glyphs.windows(2).all(|g| g[1].x >= g[0].x));
    // A single RTL letter between LTR text is still RTL (strong class).
    let one = point(vec![run("ab \u{5D0} cd", NOTO, 20.0)]);
    let lay = layout_text(json(&one)).unwrap();
    assert!(lay.glyphs.iter().find(|g| g.cluster == 3).unwrap().rtl);
}

#[test]
fn typing_group_is_one_node_with_exact_undo_and_redo() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![
        run("Hello", NOTO, 28.0),
        TextRun {
            italic: false,
            tracking: 1.5,
            ..run(" world", NOTO, 20.0)
        },
    ]);
    let id = add(&s, &base, at(10.0, 60.0));
    let (n0, h0) = (history_len(&s), head(&s));
    let mut draft = base.clone();
    for ch in ", dear".chars() {
        draft.runs[0].text.push(ch);
        let u = s
            .set_text_layer(id, json(&draft), at(10.0, 60.0), true, None)
            .unwrap();
        assert_eq!(u.history_head, h0, "keystrokes record nothing");
    }
    assert_eq!(history_len(&s), n0);
    assert!(s.info().unwrap().can_undo);
    let u = s
        .set_text_layer(id, json(&draft), at(10.0, 60.0), false, None)
        .unwrap();
    assert_ne!(u.history_head, h0);
    assert_eq!(history_len(&s), n0 + 1, "one typing group");
    assert_eq!(model_of(&s, id), draft);
    s.undo().unwrap();
    assert_eq!(model_of(&s, id), base, "exact text and style restored");
    assert_eq!(head(&s), h0);
    s.redo().unwrap();
    assert_eq!(model_of(&s, id), draft);
}

#[test]
fn cancel_and_no_op_drafts_record_nothing_and_spare_other_drafts() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("Keep", NOTO, 24.0)]);
    let id = add(&s, &base, at(10.0, 40.0));
    let (n0, h0) = (history_len(&s), head(&s));
    // A final call equal to the base records nothing.
    s.set_text_layer(id, json(&base), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!((history_len(&s), head(&s)), (n0, h0));
    // A draft, then cancel: the model is back, nothing recorded.
    let mut draft = base.clone();
    draft.runs[0].text = "Keep typing".into();
    s.set_text_layer(id, json(&draft), at(10.0, 40.0), true, None)
        .unwrap();
    assert_eq!(model_of(&s, id), draft);
    let u = s.cancel_source_preview().unwrap();
    assert_eq!(u.history_head, h0);
    assert_eq!(model_of(&s, id), base);
    assert!(!s.text_layer(id).unwrap().draft_pending);
    assert!(!s.info().unwrap().dirty || history_len(&s) == n0);
    // A draft reverted to the base drops its pending entry.
    s.set_text_layer(id, json(&draft), at(10.0, 40.0), true, None)
        .unwrap();
    s.set_text_layer(id, json(&base), at(10.0, 40.0), true, None)
        .unwrap();
    assert!(!s.text_layer(id).unwrap().draft_pending);
    s.commit(String::new()).unwrap();
    assert_eq!((history_len(&s), head(&s)), (n0, h0));
    // Cancel spares another control's drag (opacity) and does nothing without a source draft.
    let pixel = s
        .layers()
        .unwrap()
        .into_iter()
        .find(|n| n.kind == DocLayerKind::Pixel)
        .unwrap()
        .id;
    s.set_opacity(pixel, 0.5, true).unwrap();
    let u = s.cancel_source_preview().unwrap();
    assert_eq!(u.dirty_rect, None);
    assert!(
        (s.layer(pixel).unwrap().opacity - 0.5).abs() < 1e-6,
        "drag kept"
    );
    s.commit("Opacity".into()).unwrap();
    assert_eq!(history_len(&s), n0 + 1);
}

#[test]
fn unrelated_edits_undo_and_save_flush_a_pending_text_draft() {
    let (dir, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("Base", NOTO, 24.0)]);
    let id = add(&s, &base, at(10.0, 40.0));
    let n0 = history_len(&s);
    let mut draft = base.clone();
    draft.runs[0].text = "Base+".into();
    s.set_text_layer(id, json(&draft), at(10.0, 40.0), true, None)
        .unwrap();
    // An unrelated edit commits the draft as its own node first.
    s.rename_layer(id, "Renamed".into()).unwrap();
    assert_eq!(history_len(&s), n0 + 2);
    assert_eq!(model_of(&s, id), draft);
    // Undo flushes a draft before moving.
    let mut d2 = draft.clone();
    d2.runs[0].text = "Base++".into();
    s.set_text_layer(id, json(&d2), at(10.0, 40.0), true, None)
        .unwrap();
    s.undo().unwrap();
    assert_eq!(model_of(&s, id), draft, "undo reverts the flushed draft");
    // Save commits a draft too.
    s.set_text_layer(id, json(&d2), at(10.0, 40.0), true, None)
        .unwrap();
    s.save_as(
        dir.path()
            .join("flush.tessera-doc")
            .to_string_lossy()
            .into_owned(),
    )
    .unwrap();
    assert!(!s.text_layer(id).unwrap().draft_pending);
    assert_eq!(model_of(&s, id), d2);
}

#[test]
fn locks_and_affine_validation_fail_without_partial_change() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("Lock", NOTO, 24.0)]);
    let id = add(&s, &base, at(10.0, 40.0));
    let mut edited = base.clone();
    edited.runs[0].text = "Locked".into();
    let singular = TransformMatrix {
        a: 1.0,
        b: 2.0,
        c: 0.0,
        d: 2.0,
        e: 4.0,
        f: 0.0,
    };
    let nan = TransformMatrix {
        a: f64::NAN,
        ..at(0.0, 0.0)
    };
    let (n0, h0) = (history_len(&s), head(&s));
    for t in [singular, nan] {
        assert!(s.set_text_layer(id, json(&edited), t, false, None).is_err());
        assert!(s.set_text_layer(id, json(&edited), t, true, None).is_err());
        assert!(
            s.add_text_layer(String::new(), None, None, json(&base), t, false)
                .is_err()
        );
    }
    let mut bad = base.clone();
    bad.runs[0].size = -3.0;
    assert!(
        s.set_text_layer(id, json(&bad), at(10.0, 40.0), false, None)
            .is_err()
    );
    assert!(
        s.set_text_layer(
            id,
            "{\"runs\":[],\"nope\":1}".into(),
            at(10.0, 40.0),
            false,
            None
        )
        .is_err()
    );
    assert_eq!((history_len(&s), head(&s)), (n0, h0));
    let locks = |pixels, position, all| LayerLocks {
        transparency: false,
        pixels,
        position,
        all,
    };
    // Pixel lock: no content edit, no conversion.
    s.set_locks(id, locks(true, false, false)).unwrap();
    let n1 = history_len(&s);
    assert!(
        s.set_text_layer(id, json(&edited), at(10.0, 40.0), false, None)
            .is_err()
    );
    assert!(
        s.set_text_layer(id, json(&edited), at(10.0, 40.0), true, None)
            .is_err()
    );
    assert!(s.convert_to_pixels(id).is_err());
    assert!(s.edit_text_runs(id, 0, 1, "[]".into(), None).is_err());
    // All lock: likewise.
    s.set_locks(id, locks(false, false, true)).unwrap();
    assert!(
        s.set_text_layer(id, json(&edited), at(10.0, 40.0), false, None)
            .is_err()
    );
    // Position lock: content edits pass, affine moves fail.
    s.set_locks(id, locks(false, true, false)).unwrap();
    let n2 = history_len(&s);
    assert_eq!(n2, n1 + 2, "two lock changes");
    assert!(
        s.set_text_layer(id, json(&base), at(30.0, 40.0), false, None)
            .is_err()
    );
    assert_eq!(history_len(&s), n2);
    s.set_text_layer(id, json(&edited), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!(model_of(&s, id), edited);
    let t = s.text_layer(id).unwrap().transform;
    assert_eq!((t.c, t.f), (10.0, 40.0));
    // Unlocked: translation and skew keep the row-major layout.
    s.set_locks(id, locks(false, false, false)).unwrap();
    let skew = TransformMatrix {
        a: 1.0,
        b: 0.25,
        c: 12.0,
        d: 0.0,
        e: 1.0,
        f: 44.0,
    };
    s.set_text_layer(id, json(&edited), skew, false, None)
        .unwrap();
    assert_eq!(s.text_layer(id).unwrap().transform, skew);
    let st = s.document_state().unwrap();
    match &st.find(compositor::LayerId(id)).unwrap().kind {
        compositor::LayerKind::Text { transform, .. } => {
            assert_eq!(transform.m, [1.0, 0.25, 12.0, 0.0, 1.0, 44.0]);
            assert_eq!(transform.apply(4.0, 8.0), (4.0 + 2.0 + 12.0, 8.0 + 44.0));
        }
        _ => panic!("text layer"),
    }
}

#[test]
fn convert_to_pixels_keeps_id_masks_styles_and_undo_restores_source() {
    use compositor::render::styles::{Overlay, StyleEffect};
    use compositor::{Affine, DocState, Document, Layer, LayerKind, Mask, VectorMask};
    use engine_api::tile::Extent;
    fixture_fonts();
    let extent = Extent::new(W, H);
    let mut state = DocState::new(extent, compositor::Depth::U8);
    let model = point(vec![
        run("Pix", SYSTEM, 60.0),
        TextRun {
            weight: 700,
            color: [0, 128, 255, 255],
            ..run("el", SYSTEM, 60.0)
        },
    ]);
    let mut layer = Layer::new(
        "Styled",
        LayerKind::Text {
            model: model.clone(),
            transform: Affine {
                m: [1.0, 0.0, 20.0, 0.0, 1.0, 90.0],
            },
        },
    );
    layer.id = compositor::LayerId(7);
    layer.props.opacity = 0.8;
    layer
        .props
        .styles
        .effects
        .push(StyleEffect::ColorOverlay(Overlay {
            opacity: 0.5,
            ..Overlay::default()
        }));
    // Raster mask hiding the right half; vector mask cutting the top rows.
    let mut mask = Mask::reveal_all(extent, compositor::Depth::U8);
    mask.raster
        .edit_region(
            compositor::Rect::new(i64::from(W / 2), 0, i64::from(W), i64::from(H)),
            1,
            |_, _, p| p[0] = 0.0,
        )
        .unwrap();
    layer.mask = Some(mask);
    layer.vector_mask = Some(
        serde_json::from_value::<VectorMask>(serde_json::json!({
            "enabled": true,
            "path": [[0.0, 50.0], [W as f64, 50.0], [W as f64, H as f64], [0.0, H as f64]],
            "feather": 0.0,
            "density": 1.0
        }))
        .unwrap(),
    );
    state.next_id = 8;
    state.root.push(Arc::new(layer));
    let (_d, e) = engine();
    let s = e.adopt_document(Document::new(state), "convert".into());
    // Layer styles need the CPU compositor (the resident program rejects
    // them; B5-07 owns that fallback), so compare CPU composites.
    let composite = |s: &DocumentSession| {
        compositor::Compositor::new(64 << 20)
            .render_level_rgba(&Document::new((*s.document_state().unwrap()).clone()), 0)
            .unwrap()
            .1
    };
    let before = composite(&s);
    assert!(alpha_sum(&before) > 50.0, "the text renders");
    let n0 = history_len(&s);
    s.convert_to_pixels(7).unwrap();
    assert_eq!(history_len(&s), n0 + 1);
    assert_eq!(
        s.history_items().unwrap().last().unwrap().label,
        "Convert to Pixels"
    );
    let st = s.document_state().unwrap();
    let l = st.find(compositor::LayerId(7)).unwrap();
    assert!(matches!(l.kind, LayerKind::Pixel(_)), "same id, now pixels");
    assert_eq!(l.props.name, "Styled");
    assert!((l.props.opacity - 0.8).abs() < 1e-6);
    assert_eq!(l.props.styles.effects.len(), 1, "styles kept, applied once");
    assert!(
        l.mask.is_some() && l.vector_mask.is_some(),
        "both masks kept"
    );
    let after = composite(&s);
    // Straight alpha: compare premultiplied colour (hue is undefined where
    // U8 quantization rounds a faint edge's alpha to zero).
    let premul = |p: &[f32]| [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]];
    let diff = before
        .chunks(4)
        .zip(after.chunks(4))
        .flat_map(|(a, b)| {
            let (a, b) = (premul(a), premul(b));
            (0..4).map(move |c| (a[c] - b[c]).abs())
        })
        .fold(0.0f32, f32::max);
    assert!(
        diff < 0.03,
        "appearance matches after conversion (max {diff})"
    );
    // Other kinds are rejected.
    let pixel_err = s.convert_to_pixels(7);
    assert!(pixel_err.is_err());
    s.undo().unwrap();
    let st = s.document_state().unwrap();
    match &st.find(compositor::LayerId(7)).unwrap().kind {
        LayerKind::Text { model: m, .. } => assert_eq!(*m, model, "exact source"),
        _ => panic!("undo restores text"),
    }
}

#[test]
fn cpu_and_resident_render_text_with_the_shared_font_snapshot() {
    let (_d, e) = engine();
    let s = doc(&e);
    let m = point(vec![TextRun {
        color: [200, 30, 30, 255],
        ..run("Tessera", NOTO, 48.0)
    }]);
    add(&s, &m, at(20.0, 90.0));
    s.wait_idle();
    // The session renderer (resident on Metal, else CPU) has Noto Sans only
    // through the shared snapshot: without injection this is a missing font.
    let (w, h, session) = s.read_level(0).unwrap();
    assert_eq!((w, h), (W, H));
    assert!(alpha_sum(&session) > 100.0);
    // Reference: a CPU compositor over a renderer with the same fixture font.
    let mut fonts = typography::TextRenderer::new();
    fonts.load_font_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../typography/tests/fonts"
    ));
    let cpu = compositor::Compositor::new(64 << 20);
    cpu.set_text_renderer(fonts);
    let doc_state = s.document_state().unwrap();
    let (_, reference) = cpu
        .render_level_rgba(&compositor::Document::new((*doc_state).clone()), 0)
        .unwrap();
    let diff = session
        .iter()
        .zip(&reference)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(diff < 0.02, "session and CPU text agree (max {diff})");
    // A thumbnail renders with the same fonts (no missing-font failure).
    let id = s.layers().unwrap()[0].id;
    s.layer_thumbnail(id, 64).unwrap();
}

#[test]
fn native_reopen_keeps_text_editable() {
    let (dir, e) = engine();
    let s = doc(&e);
    let m = point(vec![
        run("Native ", NOTO, 24.0),
        TextRun {
            italic: true,
            baseline_shift: 3.0,
            ..run("text", NOTO, 24.0)
        },
    ]);
    let id = add(&s, &m, at(15.0, 50.0));
    let path = dir.path().join("text.tessera-doc");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rec = r.text_layer(id).unwrap();
    let back: TextModel = serde_json::from_str(&rec.model_json).unwrap();
    assert_eq!(back, m);
    assert_eq!((rec.transform.c, rec.transform.f), (15.0, 50.0));
    assert!(rec.caret_editable);
    let mut next = back.clone();
    next.runs[0].text = "Reopened ".into();
    r.set_text_layer(id, json(&next), rec.transform, false, Some(rec.revision))
        .unwrap();
    assert_eq!(model_of(&r, id), next);
}

#[test]
fn psd_reopen_keeps_mixed_runs_and_converted_layers_drop_tysh() {
    let (dir, e) = engine();
    let s = doc(&e);
    let m = point(vec![
        run("Mixed ", SYSTEM, 30.0),
        TextRun {
            weight: 700,
            color: [255, 0, 0, 255],
            ..run("styles", SYSTEM, 36.0)
        },
    ]);
    let keep = add(&s, &m, at(10.0, 50.0));
    let gone = add(
        &s,
        &point(vec![run("Raster me", SYSTEM, 30.0)]),
        at(10.0, 120.0),
    );
    s.convert_to_pixels(gone).unwrap();
    let path = dir.path().join("text.psd");
    s.save_as(path.to_string_lossy().into_owned()).unwrap();
    s.close();
    let r = e
        .clone()
        .open_document(path.to_string_lossy().into_owned())
        .unwrap();
    let rows = r.layers().unwrap();
    let texts: Vec<_> = rows
        .iter()
        .filter(|n| n.kind == DocLayerKind::Text)
        .collect();
    assert_eq!(
        texts.len(),
        1,
        "the converted layer does not resurrect TySh"
    );
    let back = model_of(&r, texts[0].id);
    let words: Vec<_> = back.runs.iter().map(|r| r.text.as_str()).collect();
    assert_eq!(words.concat(), "Mixed styles");
    assert!(back.runs.len() >= 2, "runs stay separate");
    assert!(
        back.runs
            .iter()
            .any(|r| r.weight == 700 && r.color == [255, 0, 0, 255])
    );
    assert!(
        rows.iter()
            .any(|n| n.kind == DocLayerKind::Pixel && n.name == "Raster me"),
        "converted layer stays pixels"
    );
    let _ = keep;
    // Still editable after reopen.
    let rec = r.text_layer(texts[0].id).unwrap();
    let mut next = back.clone();
    next.runs[0].text = "Edited ".into();
    r.set_text_layer(texts[0].id, json(&next), rec.transform, false, None)
        .unwrap();
    assert_eq!(model_of(&r, texts[0].id).runs[0].text, "Edited ");
}

#[test]
fn text_layer_reports_caret_and_colour_limitations() {
    let (_d, e) = engine();
    let s = doc(&e);
    let plain = point(vec![run("Plain", NOTO, 24.0)]);
    let id = add(&s, &plain, at(10.0, 40.0));
    let rec = s.text_layer(id).unwrap();
    assert!(rec.caret_editable);
    assert!(rec.limitations.is_empty(), "{:?}", rec.limitations);
    let mut warped = plain.clone();
    warped.warp.amount = 0.3;
    s.set_text_layer(id, json(&warped), at(10.0, 40.0), false, None)
        .unwrap();
    let rec = s.text_layer(id).unwrap();
    assert!(!rec.caret_editable);
    assert!(rec.limitations.iter().any(|l| l.starts_with("Warped text")));
    let mut vertical = plain.clone();
    vertical.vertical = true;
    s.set_text_layer(id, json(&vertical), at(10.0, 40.0), false, None)
        .unwrap();
    let rec = s.text_layer(id).unwrap();
    assert!(!rec.caret_editable);
    assert!(rec.limitations.iter().any(|l| l.contains("Vertical")));
    assert!(
        layout_text(json(&vertical)).is_err(),
        "vertical layout is unsupported"
    );
    let missing = point(vec![run("?", "No Such Family 12345", 24.0)]);
    let id2 = add(&s, &missing, at(10.0, 90.0));
    let rec = s.text_layer(id2).unwrap();
    assert!(!rec.caret_editable);
    assert!(rec.limitations.iter().any(|l| l.contains("Missing font")));
    assert!(layout_text(json(&missing)).is_err());
    // 32-bit documents: colour interpretation is reported, not guessed.
    let f = e.clone().new_document(64, 64, DocDepth::F32, None).unwrap();
    let id3 = f
        .add_text_layer(
            String::new(),
            None,
            None,
            json(&plain),
            at(0.0, 30.0),
            false,
        )
        .unwrap()
        .created[0];
    assert!(
        f.text_layer(id3)
            .unwrap()
            .limitations
            .iter()
            .any(|l| l.starts_with("Colour"))
    );
    let fonts = available_text_fonts();
    let noto = fonts
        .iter()
        .find(|f| f.family == NOTO)
        .expect("fixture family");
    assert!(noto.faces.iter().any(|f| f.weight == 400 && !f.italic));
    assert!(fonts.iter().all(|f| !f.family.starts_with('.')));
    assert!(fonts.windows(2).all(|w| w[0].family < w[1].family));
}

/// Typing preview cost on a 20 MP document (5472 × 3648): one keystroke = a full draft preview plus a
/// frame. Ignored by default (timing); run with `--ignored --nocapture`.
#[test]
#[ignore]
fn typing_preview_latency_20mp() {
    let (_d, e) = engine();
    let s = e
        .clone()
        .new_document(5472, 3648, DocDepth::U8, None)
        .unwrap();
    let mut m = point(vec![run("", SYSTEM, 274.0)]);
    let id = add(&s, &point(vec![run("H", SYSTEM, 274.0)]), at(500.0, 500.0));
    s.set_viewport(2, 0, 0, 1368, 912, 0.25).unwrap();
    s.wait_idle();
    let mut times = Vec::new();
    for (i, ch) in "Hello typing latency".chars().enumerate() {
        m.runs[0].text.push(ch);
        let t0 = std::time::Instant::now();
        s.set_text_layer(id, json(&m), at(500.0, 500.0), true, None)
            .unwrap();
        let t1 = t0.elapsed();
        let (_, _, px) = s.read_level(2).unwrap();
        let t2 = t0.elapsed();
        assert!(!px.is_empty());
        times.push((t1.as_secs_f64() * 1000.0, t2.as_secs_f64() * 1000.0));
        if i == 0 {
            println!(
                "first key: preview {:.1} ms, preview+level-2 render {:.1} ms",
                times[0].0, times[0].1
            );
        }
    }
    let mut total: Vec<f64> = times.iter().map(|t| t.1).collect();
    total.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!(
        "20 MP typing: preview median {:.1} ms; preview + level-2 frame median {:.1} ms, max {:.1} ms ({} keys)",
        {
            let mut p: Vec<f64> = times.iter().map(|t| t.0).collect();
            p.sort_by(|a, b| a.partial_cmp(b).unwrap());
            p[p.len() / 2]
        },
        total[total.len() / 2],
        total[total.len() - 1],
        total.len()
    );
}

fn name_of(s: &DocumentSession, id: u64) -> String {
    s.layers()
        .unwrap()
        .into_iter()
        .find(|n| n.id == id)
        .unwrap()
        .name
}

/// B5-11b item 13: an auto-named text layer's name follows its first line
/// (in the same history node as the text edit) until the user renames it.
#[test]
fn auto_named_text_layer_follows_its_first_line_until_renamed() {
    let (_d, e) = engine();
    let s = doc(&e);
    let base = point(vec![run("Hi", NOTO, 24.0)]);
    let id = add(&s, &base, at(10.0, 40.0));
    assert_eq!(name_of(&s, id), "Hi");
    // A typing group (draft, then commit) renames with the edit: one node.
    let n0 = history_len(&s);
    let mut typed = base.clone();
    typed.runs[0].text = "HiZQ\nsecond line".into();
    s.set_text_layer(id, json(&typed), at(10.0, 40.0), true, None)
        .unwrap();
    s.commit("Edit Text".into()).unwrap();
    assert_eq!(history_len(&s), n0 + 1, "text and name are one node");
    assert_eq!(name_of(&s, id), "HiZQ", "the name follows the first line");
    s.undo().unwrap();
    assert_eq!(name_of(&s, id), "Hi", "undo restores the old name");
    s.redo().unwrap();
    assert_eq!(name_of(&s, id), "HiZQ");
    // A final (non-interactive) edit and a run-range edit follow too.
    let mut more = typed.clone();
    more.runs[0].text = "Hello".into();
    s.set_text_layer(id, json(&more), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!(name_of(&s, id), "Hello");
    let runs = serde_json::to_string(&vec![run("Howdy", NOTO, 24.0)]).unwrap();
    s.edit_text_runs(id, 0, 1, runs, None).unwrap();
    assert_eq!(name_of(&s, id), "Howdy");
    // Emptied text falls back to "Text" and still follows afterwards.
    let mut empty = more.clone();
    empty.runs[0].text = String::new();
    s.set_text_layer(id, json(&empty), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!(name_of(&s, id), "Text");
    s.set_text_layer(id, json(&more), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!(name_of(&s, id), "Hello");
    // A user rename sticks.
    s.rename_layer(id, "Title".into()).unwrap();
    let mut after = more.clone();
    after.runs[0].text = "Changed".into();
    s.set_text_layer(id, json(&after), at(10.0, 40.0), false, None)
        .unwrap();
    assert_eq!(name_of(&s, id), "Title", "a user rename sticks");
    // A layer added with an explicit name keeps it.
    let named = s
        .add_text_layer(
            "Caption".into(),
            None,
            None,
            json(&point(vec![run("Body", NOTO, 24.0)])),
            at(10.0, 90.0),
            false,
        )
        .unwrap()
        .created[0];
    let mut body = point(vec![run("Body text", NOTO, 24.0)]);
    body.runs[0].text = "Other".into();
    s.set_text_layer(named, json(&body), at(10.0, 90.0), false, None)
        .unwrap();
    assert_eq!(name_of(&s, named), "Caption");
}
