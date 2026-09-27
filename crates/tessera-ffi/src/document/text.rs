//! Editable text layers over the bridge (WP B5-10): reading a text layer's
//! live source, the shared font snapshot, engine layout for the host's caret,
//! adding and editing text as one history node, and run-range edits.
//!
//! # Contract
//!
//! * Models travel as `typography::TextModel` JSON (the strict serde schema,
//!   unknown fields rejected), transforms as the row-major [`TransformMatrix`]
//!   `(a·x + b·y + c, d·x + e·y + f)` from local level-0 pixels to document
//!   pixels.
//! * Caret, selection and IME composition are host state. The host asks
//!   [`layout_text`] for the engine's own layout of its draft (the same
//!   typography layout the renderers rasterize, over the same font snapshot)
//!   and derives carets from glyph origins, advances and UTF-8 cluster
//!   offsets. UTF-16 (`NSRange`), UTF-8 clusters and run indexes are three
//!   index domains; the host converts explicitly.
//! * Typing is a draft: `set_text_layer(.., interactive: true)` (or
//!   `add_text_layer(.., interactive: true)` for a layer the draft adds)
//!   shows the complete draft on a scratch copy and records nothing. The
//!   final call with `interactive: false` — or `commit(label)` — records ONE
//!   history node. For an existing layer that node is a single
//!   `EditTextRuns` splice derived from the committed base (or `EditText`
//!   when paragraph, box, warp, path or the transform changed); a draft
//!   equal to its base records nothing. `cancel_source_preview` drops the
//!   draft with no history change.
//! * One font database snapshot (system fonts, discovered once) feeds the
//!   layout calls here and every CPU, resident and thumbnail compositor the
//!   session constructs (document/render.rs). Missing fonts are errors;
//!   there is no fallback. `ConvertToPixels` and export rasterize through
//!   the compositor's own system-font renderer, which sees the same
//!   installed fonts.

use super::{DocumentSession, DocumentUpdate, Pending, SourceOps, TransformMatrix, find};
use crate::{Result, failure};
use compositor::{Affine, DocOp, DocState, LayerId, LayerKind};
use std::sync::{OnceLock, RwLock};
use typography::{TextModel, TextRenderer, TextRun};

// ───────────────────────────── font snapshot ─────────────────────────────

static FONTS: OnceLock<RwLock<TextRenderer>> = OnceLock::new();

/// The process-wide font database: system fonts, discovered on first use.
fn fonts() -> &'static RwLock<TextRenderer> {
    FONTS.get_or_init(|| {
        let mut r = TextRenderer::new();
        r.discover_system_fonts();
        RwLock::new(r)
    })
}

fn read_fonts() -> std::sync::RwLockReadGuard<'static, TextRenderer> {
    fonts().read().unwrap_or_else(|e| e.into_inner())
}

/// A renderer over the shared snapshot, for installing into a compositor
/// (`Compositor::set_text_renderer` / `ResidentRenderer::set_text_renderer`).
pub(crate) fn shared_text_renderer() -> TextRenderer {
    let base = read_fonts();
    let mut r = TextRenderer::new();
    *r.fonts_mut() = base.fonts().clone();
    r
}

/// Adds the fonts of `dir` to the shared snapshot (deterministic tests with
/// bundled OFL fixtures). Renderers constructed afterwards see them; not a
/// host API: fonts must be installed for conversion and export to match.
#[doc(hidden)]
pub fn load_text_fonts_for_tests(dir: &str) {
    fonts()
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .load_font_dir(dir);
}

// ─────────────────────────────── records ───────────────────────────────

/// A text layer's live source.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextLayerRecord {
    pub id: u64,
    /// `typography::TextModel` JSON of the live state (the draft while one
    /// is pending).
    pub model_json: String,
    /// Local level-0 pixels → document pixels.
    pub transform: TransformMatrix,
    /// Content revision of the COMMITTED layer: pass it back as
    /// `expected_revision` so an edit fails if the layer changed meanwhile.
    pub revision: u64,
    /// A text draft of this layer is pending (not yet in history).
    pub draft_pending: bool,
    /// Point or paragraph text without warp or path: canvas caret placement
    /// is supported. Warped/path text keeps its data and rendering and edits
    /// through the labelled source editor only; vertical text is unsupported.
    pub caret_editable: bool,
    /// Human-readable limitations that apply to this layer (missing fonts,
    /// warp/path/vertical caret, colour interpretation).
    pub limitations: Vec<String>,
}

/// One installed font face.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TextFontFace {
    pub post_script_name: String,
    pub weight: u16,
    pub italic: bool,
}

/// One installed family (the Character panel's font menu).
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TextFontFamily {
    pub family: String,
    pub faces: Vec<TextFontFace>,
}

/// A positioned glyph of [`layout_text`], in local level-0 pixels.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextGlyphRecord {
    /// Run index in the model.
    pub run: u32,
    /// UTF-8 byte offset of the glyph's shaping cluster in the concatenated
    /// run text. Ligatures and combining sequences share one cluster.
    pub cluster: u32,
    /// Pen origin (x) and baseline position (y, positive down).
    pub x: f32,
    pub y: f32,
    pub advance: f32,
    /// Clockwise rotation in radians (non-zero on path text only).
    pub angle: f32,
    /// The glyph belongs to a right-to-left bidi run (visual order runs
    /// right to left through its clusters).
    pub rtl: bool,
}

/// A laid-out line: `glyph_start..glyph_end` index `glyphs` in visual order.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextLineRecord {
    /// UTF-8 source range of the line, including a trailing separator.
    pub source_start: u32,
    pub source_end: u32,
    pub glyph_start: u32,
    pub glyph_end: u32,
    pub x: f32,
    pub baseline: f32,
    pub width: f32,
    /// `None` for point text (unbounded).
    pub available_width: Option<f32>,
    /// Caret extent above / below the baseline (0.9 / 0.25 of the largest
    /// run size on the line; caret metrics, not font ascenders).
    pub ascent: f32,
    pub descent: f32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TextLayoutRecord {
    pub glyphs: Vec<TextGlyphRecord>,
    pub lines: Vec<TextLineRecord>,
    /// Paragraph text whose lines do not fit the box (hidden lines are not
    /// listed) or an unbreakable token wider than the box.
    pub overflow: bool,
    /// UTF-8 length of the concatenated source.
    pub text_len: u32,
}

/// A run-index splice ([`text_run_splice`]).
#[derive(Clone, Debug, PartialEq)]
pub struct TextRunSplice {
    pub start: usize,
    pub end: usize,
    pub runs: Vec<TextRun>,
}

// ────────────────────────────── helpers ──────────────────────────────

fn parse_model(json: &str) -> Result<TextModel> {
    let model: TextModel =
        serde_json::from_str(json).map_err(|e| failure(format!("text model JSON: {e}")))?;
    model
        .validate()
        .map_err(|e| failure(format!("text model: {e}")))?;
    Ok(model)
}

fn parse_runs(json: &str) -> Result<Vec<TextRun>> {
    serde_json::from_str(json).map_err(|e| failure(format!("text runs JSON: {e}")))
}

pub(crate) fn affine_of(m: TransformMatrix) -> Result<Affine> {
    let a = Affine {
        m: [m.a, m.b, m.c, m.d, m.e, m.f],
    };
    compositor::text_vector::validate_transform(a)?;
    Ok(a)
}

fn matrix_of(a: Affine) -> TransformMatrix {
    let [a, b, c, d, e, f] = a.m;
    TransformMatrix { a, b, c, d, e, f }
}

fn text_of(state: &DocState, id: u64) -> Result<(&TextModel, Affine, u64)> {
    let l = find(state, id)?;
    match &l.kind {
        LayerKind::Text { model, transform } => Ok((model, *transform, l.content_rev)),
        _ => Err(failure(format!("layer {id} is not a text layer"))),
    }
}

fn check_revision(id: u64, actual: u64, expected: Option<u64>) -> Result<()> {
    match expected {
        Some(e) if e != actual => Err(failure(format!(
            "text layer {id} changed since it was read (revision {actual}, expected {e})"
        ))),
        _ => Ok(()),
    }
}

/// Fields other than the runs (paragraph, box, vertical, warp, path).
fn same_frame(a: &TextModel, b: &TextModel) -> bool {
    a.paragraph == b.paragraph
        && a.text_box == b.text_box
        && a.vertical == b.vertical
        && a.warp == b.warp
        && a.path == b.path
}

/// The smallest run-index splice turning `base` into `new`: common leading
/// and trailing runs are kept, the range between them is replaced. `None`
/// when the runs are equal.
#[doc(hidden)]
pub fn text_run_splice(base: &[TextRun], new: &[TextRun]) -> Option<TextRunSplice> {
    if base == new {
        return None;
    }
    let prefix = base.iter().zip(new).take_while(|(a, b)| a == b).count();
    let max_suffix = base.len().min(new.len()) - prefix;
    let suffix = base
        .iter()
        .rev()
        .zip(new.iter().rev())
        .take(max_suffix)
        .take_while(|(a, b)| a == b)
        .count();
    Some(TextRunSplice {
        start: prefix,
        end: base.len() - suffix,
        runs: new[prefix..new.len() - suffix].to_vec(),
    })
}

/// Preview and commit ops for a draft of layer `id` against the committed
/// base: the preview is the complete draft (idempotent, rebuilt from the
/// base each time), the commit one derived splice.
fn edit_ops(
    base: &DocState,
    id: u64,
    model: TextModel,
    transform: Affine,
    expected: Option<u64>,
) -> Result<SourceOps> {
    let (old, old_t, rev) = text_of(base, id)?;
    check_revision(id, rev, expected)?;
    let commit = if *old == model && old_t == transform {
        None
    } else if old_t == transform && same_frame(old, &model) {
        text_run_splice(&old.runs, &model.runs).map(|s| DocOp::EditTextRuns {
            id: LayerId(id),
            range: s.start..s.end,
            runs: s.runs,
        })
    } else {
        Some(DocOp::EditText {
            id: LayerId(id),
            model: model.clone(),
            transform,
        })
    };
    Ok(SourceOps {
        preview: DocOp::EditText {
            id: LayerId(id),
            model,
            transform,
        },
        commit,
    })
}

/// Directions of glyphs in visual order within one line: from the engine's
/// cluster order (clusters decrease left to right in RTL runs), with strong
/// RTL scripts deciding isolated clusters.
fn glyph_rtl(text: &str, clusters: &[usize]) -> Vec<bool> {
    let strong_rtl = |c: usize| {
        text.get(c..)
            .and_then(|s| s.chars().next())
            .is_some_and(|ch| {
                matches!(ch as u32,
                    0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF
                    | 0x10800..=0x10FFF | 0x1E800..=0x1EFFF)
            })
    };
    let n = clusters.len();
    (0..n)
        .map(|i| {
            let c = clusters[i];
            let next = clusters[i + 1..].iter().find(|&&k| k != c);
            let prev = clusters[..i].iter().rev().find(|&&k| k != c);
            let by_order = match (prev, next) {
                (_, Some(&k)) if k < c => Some(true),
                (Some(&k), _) if k > c => Some(true),
                (None, None) => None,
                _ => Some(false),
            };
            strong_rtl(c) || by_order.unwrap_or(false)
        })
        .collect()
}

fn layout_record(renderer: &TextRenderer, model: &TextModel) -> Result<TextLayoutRecord> {
    let layout = match &model.path {
        Some(path) => {
            let contour = typography::TextPath::new(
                &path
                    .to_lyon()
                    .map_err(|e| failure(format!("text path: {e}")))?,
                0.01,
            )
            .map_err(|e| failure(format!("text path: {e}")))?;
            renderer.layout_on_path(model, &contour, path.offset)
        }
        None => renderer.layout(model),
    }
    .map_err(|e| failure(format!("text layout: {e}")))?;
    let text: String = model.runs.iter().map(|r| r.text.as_str()).collect();
    let mut rtl = vec![false; layout.glyphs.len()];
    for line in &layout.lines {
        let clusters: Vec<usize> = layout.glyphs[line.glyphs.clone()]
            .iter()
            .map(|g| g.cluster)
            .collect();
        for (i, r) in glyph_rtl(&text, &clusters).into_iter().enumerate() {
            rtl[line.glyphs.start + i] = r;
        }
    }
    // Largest run size on each line (runs overlapping its source range).
    let mut offsets = Vec::with_capacity(model.runs.len());
    let mut o = 0;
    for r in &model.runs {
        offsets.push((o, o + r.text.len(), r.size));
        o += r.text.len();
    }
    let fallback = model.runs.first().map_or(24.0, |r| r.size);
    Ok(TextLayoutRecord {
        glyphs: layout
            .glyphs
            .iter()
            .zip(&rtl)
            .map(|(g, &rtl)| TextGlyphRecord {
                run: g.run as u32,
                cluster: g.cluster as u32,
                x: g.x,
                y: g.y,
                advance: g.advance,
                angle: g.angle,
                rtl,
            })
            .collect(),
        lines: layout
            .lines
            .iter()
            .map(|l| {
                let size = offsets
                    .iter()
                    .filter(|(s, e, _)| {
                        (*s < l.source.end && *e > l.source.start)
                            || (l.source.is_empty() && *s <= l.source.start && *e >= l.source.start)
                    })
                    .map(|(_, _, size)| *size)
                    .fold(0.0f32, f32::max);
                let size = if size > 0.0 { size } else { fallback };
                TextLineRecord {
                    source_start: l.source.start as u32,
                    source_end: l.source.end as u32,
                    glyph_start: l.glyphs.start as u32,
                    glyph_end: l.glyphs.end as u32,
                    x: l.x,
                    baseline: l.baseline,
                    width: l.width,
                    available_width: l.available_width.is_finite().then_some(l.available_width),
                    ascent: size * 0.9,
                    descent: size * 0.25,
                }
            })
            .collect(),
        overflow: layout.overflow,
        text_len: text.len() as u32,
    })
}

// ─────────────────────────────── exports ───────────────────────────────

/// Installed font families of the shared snapshot, sorted by family name
/// (the Character panel's font menu). Faces are sorted by weight, upright
/// first.
#[uniffi::export]
pub fn available_text_fonts() -> Vec<TextFontFamily> {
    let fonts = read_fonts();
    let mut families: std::collections::BTreeMap<String, Vec<TextFontFace>> = Default::default();
    for face in fonts.fonts().faces() {
        let Some((family, _)) = face.families.first() else {
            continue;
        };
        if family.starts_with('.') {
            continue; // macOS private UI families (".SF NS", …) are not user-selectable
        }
        let italic = format!("{:?}", face.style) != "Normal";
        families
            .entry(family.clone())
            .or_default()
            .push(TextFontFace {
                post_script_name: face.post_script_name.clone(),
                weight: face.weight.0,
                italic,
            });
    }
    families
        .into_iter()
        .map(|(family, mut faces)| {
            faces.sort_by(|a, b| {
                (a.weight, a.italic, &a.post_script_name).cmp(&(
                    b.weight,
                    b.italic,
                    &b.post_script_name,
                ))
            });
            faces.dedup();
            TextFontFamily { family, faces }
        })
        .collect()
}

/// The engine's layout of a text model (local level-0 pixels) over the shared
/// font snapshot: the host's caret, selection and hit-test oracle. Path text
/// is laid out on its path (glyph angles set). Missing fonts fail.
#[uniffi::export]
pub fn layout_text(model_json: String) -> Result<TextLayoutRecord> {
    let model = parse_model(&model_json)?;
    layout_record(&read_fonts(), &model)
}

/// Why a model cannot be edited with a canvas caret, and other explicit
/// limitations (empty when none apply).
fn limitations(model: &TextModel, state: &DocState) -> (bool, Vec<String>) {
    let mut out = Vec::new();
    let mut caret = true;
    if model.vertical {
        caret = false;
        out.push("Vertical text is not supported: it keeps its data but cannot be laid out or edited on canvas.".into());
    }
    if model.path.is_some() {
        caret = false;
        out.push("Text on a path: rendering is kept; canvas caret placement is disabled, edit the source text in the Character panel.".into());
    }
    if model.warp.amount != 0.0 {
        caret = false;
        out.push("Warped text: rendering is kept; canvas caret placement is disabled, edit the source text in the Character panel.".into());
    }
    if model.paragraph.hyphenation {
        out.push("Hyphenation is stored but not applied (no dictionary hyphenation).".into());
    }
    if !model.vertical {
        let fonts = read_fonts();
        if let Err(typography::Error::MissingFont(f)) = fonts.layout(model) {
            caret = false;
            out.push(format!(
                "Missing font “{f}”: install it to render or edit this text (no substitution)."
            ));
        }
    }
    let srgb = state
        .profile
        .as_ref()
        .is_none_or(|p| p.name.to_ascii_lowercase().contains("srgb"));
    if !srgb || state.depth == compositor::Depth::F32 {
        out.push(format!(
            "Colour: text colours are sRGB bytes written directly into {} samples; live text is not colour-managed.",
            match (&state.profile, state.depth) {
                (Some(p), compositor::Depth::F32) => format!("{} 32-bit", p.name),
                (Some(p), _) => p.name.clone(),
                (None, _) => "32-bit".into(),
            }
        ));
    }
    (caret, out)
}

#[uniffi::export]
impl DocumentSession {
    /// A text layer's live source (the draft while one is pending).
    pub fn text_layer(&self, layer: u64) -> Result<TextLayerRecord> {
        let st = self.shared.lock()?;
        st.open()?;
        let live = st.live().state();
        let (model, transform, _) = text_of(live, layer)?;
        // The committed layer may not exist yet (a draft that adds it).
        let revision = find(st.doc.state(), layer).map_or(0, |l| l.content_rev);
        let pending = st
            .pending
            .iter()
            .any(|(k, _)| *k == Pending::Text(Some(layer)));
        let (caret, limits) = limitations(model, live);
        Ok(TextLayerRecord {
            id: layer,
            model_json: serde_json::to_string(model).map_err(failure)?,
            transform: matrix_of(transform),
            revision,
            draft_pending: pending,
            caret_editable: caret,
            limitations: limits,
        })
    }

    /// Adds a text layer named `name` (empty: the first line of its text) at
    /// bottom-first `index` of `parent` (`None`: root; index `None`: on top).
    /// `interactive`: a draft shown on the scratch, no history, the layer is
    /// provisional until the final call (or `commit`); the final call records
    /// one "Add Text" node whose `created` holds the new id.
    pub fn add_text_layer(
        &self,
        name: String,
        parent: Option<u64>,
        index: Option<u32>,
        model_json: String,
        transform: TransformMatrix,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let model = parse_model(&model_json)?;
        let transform = affine_of(transform)?;
        let name = if name.trim().is_empty() {
            let text: String = model.runs.iter().map(|r| r.text.as_str()).collect();
            let first = text.lines().next().unwrap_or("").trim();
            let mut n: String = first.chars().take(40).collect();
            if n.is_empty() {
                n = "Text".into();
            }
            n
        } else {
            name
        };
        self.source_edit(Pending::Text(None), interactive, move |_| {
            let op = DocOp::AddText {
                parent: parent.map(LayerId),
                index: index.map_or(usize::MAX, |i| i as usize),
                name,
                model,
                transform,
            };
            Ok(SourceOps {
                preview: op.clone(),
                commit: Some(op),
            })
        })
    }

    /// Replaces a text layer's source and transform. `interactive`: a draft
    /// (see the module docs); otherwise one history node — an
    /// `EditTextRuns` splice when only runs changed — or nothing when the
    /// model equals the committed layer. `expected_revision` (from
    /// `text_layer`) rejects edits of a layer that changed meanwhile. Locks:
    /// pixel/all reject content edits, position rejects transform changes.
    pub fn set_text_layer(
        &self,
        layer: u64,
        model_json: String,
        transform: TransformMatrix,
        interactive: bool,
        expected_revision: Option<u64>,
    ) -> Result<DocumentUpdate> {
        let model = parse_model(&model_json)?;
        let transform = affine_of(transform)?;
        self.source_edit(Pending::Text(Some(layer)), interactive, move |base| {
            edit_ops(base, layer, model, transform, expected_revision)
        })
    }

    /// Replaces runs `start_run..end_run` (half-open RUN indexes, not text
    /// offsets; an empty range inserts) with `runs_json` (a JSON array of
    /// `typography::TextRun`), as one history node. Paragraph, box, warp and
    /// path are kept. Fails without change for reversed/out-of-range
    /// indexes, a stale `expected_revision`, or while a text draft of the
    /// layer is pending (apply or cancel it first).
    pub fn edit_text_runs(
        &self,
        layer: u64,
        start_run: u32,
        end_run: u32,
        runs_json: String,
        expected_revision: Option<u64>,
    ) -> Result<DocumentUpdate> {
        let runs = parse_runs(&runs_json)?;
        {
            let st = self.shared.lock()?;
            st.open()?;
            if st
                .pending
                .iter()
                .any(|(k, _)| *k == Pending::Text(Some(layer)))
            {
                return Err(failure(format!(
                    "a text draft of layer {layer} is pending: apply or cancel it first"
                )));
            }
            let (model, _, rev) = text_of(st.doc.state(), layer)?;
            check_revision(layer, rev, expected_revision)?;
            if start_run > end_run || end_run as usize > model.runs.len() {
                return Err(failure(format!(
                    "run range {start_run}..{end_run} is outside the layer's {} runs",
                    model.runs.len()
                )));
            }
            let mut next = model.clone();
            next.runs
                .splice(start_run as usize..end_run as usize, runs.clone());
            next.validate()
                .map_err(|e| failure(format!("text model: {e}")))?;
        }
        self.edit(
            DocOp::EditTextRuns {
                id: LayerId(layer),
                range: start_run as usize..end_run as usize,
                runs,
            },
            None,
        )
    }
}
