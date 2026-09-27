//! Live shapes, Pen / Direct Selection path edits and vector masks (WP B5-11).
//!
//! # Models and coordinates
//!
//! A shape layer holds a `vector::ShapeModel` and a local-to-document
//! [`TransformMatrix`] (row-major `[a, b, c, d, e, f]`, mapping
//! `(a·x + b·y + c, d·x + e·y + f)`, the compositor's `Affine`). Models cross
//! the bridge as the vector crate's own serde JSON (`model_json`, `path_json`),
//! never as invented PSD descriptors. Shape geometry is in local level-zero
//! pixels; vector masks and vector paint (gradients, patterns) are in
//! document pixels, so moving a shape moves neither its paint nor its mask.
//! `kurbo::Affine` (column layout `[a, d, b, e, c, f]`) is only used inside
//! this module, through [`kurbo_affine`].
//!
//! # History
//!
//! One successful call is one history node. With `interactive: true` the
//! call previews on the session scratch (rebuilt from the committed document
//! plus the complete draft) and records nothing; the next final call of the
//! same layer records the net edit once, and [`cancel_shape_preview`] drops
//! it with no history change. Commands of [`edit_shape_path`] are absolute
//! (move an anchor TO a point) and are evaluated against the committed base,
//! so replaying a drag never compounds. Arbitrary path edits and booleans
//! clear `live_shape`, so `AddShape` / `EditShape` can never regenerate a
//! primitive over them.
//!
//! # Hit testing
//!
//! [`shape_hit_test`] is geometric: it inverts the layer affine, tests the
//! stroke outline (`Stroke::outline`: dashes, caps, joins and alignment
//! included) and then the fill with the path's fill rule. It does not look at
//! masks, opacity or rendered alpha; that would be visible-alpha hit testing,
//! which this is not.
//!
//! [`cancel_shape_preview`]: DocumentSession::cancel_shape_preview
//! [`edit_shape_path`]: DocumentSession::edit_shape_path
//! [`shape_hit_test`]: DocumentSession::shape_hit_test

use super::*;
use compositor::{Affine, VectorMask};
use vector::{FillRule, Handle, Operation, Path as VPath, Point, ShapeModel};

/// Flattening tolerance (local pixels) for booleans and stroke outlines.
const GEOMETRY_TOLERANCE: f64 = 0.01;

// ─────────────────────────────── records ───────────────────────────────

/// Axis-aligned bounds in document pixels (fractional).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ShapeBounds {
    pub x0: f64,
    pub y0: f64,
    pub x1: f64,
    pub y1: f64,
}

/// A layer's document-space vector mask.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct VectorMaskRecord {
    /// `vector::Path` JSON in level-zero document pixels.
    pub path_json: String,
    pub enabled: bool,
    /// Level-zero pixel radius, finite and ≥ 0.
    pub feather: f32,
    /// 0…1: the effective mask is `1 − density · (1 − coverage)`.
    pub density: f32,
}

/// An editable shape layer as the inspector and overlays read it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ShapeLayerRecord {
    pub layer: u64,
    /// `vector::ShapeModel` JSON (`path`, `fill`, `stroke`, `live_shape`).
    pub model_json: String,
    /// Local-to-document map.
    pub transform: TransformMatrix,
    /// The layer's thumbnail revision (changes with the source or a mask).
    pub revision: u64,
    /// `rectangle`, `ellipse`, `polygon`, `line` or `custom` while live
    /// construction parameters exist; `None` after a custom path edit.
    pub live_kind: Option<String>,
    /// Document-space bounds of the fill path (stroke outline included).
    pub bounds: Option<ShapeBounds>,
    /// Some subpath is open (Inside / Outside strokes need closed paths).
    pub has_open_subpaths: bool,
    pub vector_mask: Option<VectorMaskRecord>,
    /// Interchange and colour limitations that apply to this layer.
    pub notes: Vec<String>,
}

/// Which part of a shape a hit landed on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShapeHitPart {
    Fill,
    Stroke,
}

/// The topmost shape under a document point.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ShapeHitRecord {
    pub layer: u64,
    pub part: ShapeHitPart,
    /// The point in the layer's local coordinates.
    pub local_x: f64,
    pub local_y: f64,
}

/// Path operations (Photoshop's Combine / Subtract Front / Intersect /
/// Exclude Overlapping).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ShapePathOperation {
    Combine,
    Subtract,
    Intersect,
    Exclude,
}

impl From<ShapePathOperation> for Operation {
    fn from(o: ShapePathOperation) -> Self {
        match o {
            ShapePathOperation::Combine => Operation::Combine,
            ShapePathOperation::Subtract => Operation::Subtract,
            ShapePathOperation::Intersect => Operation::Intersect,
            ShapePathOperation::Exclude => Operation::Exclude,
        }
    }
}

// ─────────────────────────────── helpers ───────────────────────────────

pub(crate) fn affine_of(m: TransformMatrix) -> Affine {
    Affine {
        m: [m.a, m.b, m.c, m.d, m.e, m.f],
    }
}

pub(crate) fn matrix_of(a: Affine) -> TransformMatrix {
    let [a, b, c, d, e, f] = a.m;
    TransformMatrix { a, b, c, d, e, f }
}

/// Compositor row-major `(a·x + b·y + c, d·x + e·y + f)` as a kurbo affine
/// (`kurbo::Affine::new([a, d, b, e, c, f])`).
pub(crate) fn kurbo_affine(a: Affine) -> vector::Affine {
    let [a, b, c, d, e, f] = a.m;
    vector::Affine::new([a, d, b, e, c, f])
}

/// `outer ∘ inner`: apply `inner`, then `outer`.
pub(crate) fn compose(outer: Affine, inner: Affine) -> Affine {
    let [a, b, c, d, e, f] = outer.m;
    let [p, q, r, s, t, u] = inner.m;
    Affine {
        m: [
            a * p + b * s,
            a * q + b * t,
            a * r + b * u + c,
            d * p + e * s,
            d * q + e * t,
            d * r + e * u + f,
        ],
    }
}

fn checked_affine(m: TransformMatrix) -> Result<Affine> {
    let a = affine_of(m);
    if a.inverse().is_none() {
        return Err(failure("the shape transform must be finite and invertible"));
    }
    Ok(a)
}

fn parse_model(json: &str) -> Result<ShapeModel> {
    serde_json::from_str(json).map_err(|e| failure(format!("shape model JSON: {e}")))
}

fn parse_path(json: &str) -> Result<VPath> {
    serde_json::from_str(json).map_err(|e| failure(format!("path JSON: {e}")))
}

fn to_json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(failure)
}

/// Engine validation plus the checks that would otherwise only fail when
/// rendering: an Inside / Outside stroke on an open path.
fn validate_model(model: &ShapeModel, transform: Affine) -> Result<()> {
    compositor::text_vector::validate_shape(model, transform)?;
    if let Some((stroke, _)) = &model.stroke
        && stroke.alignment != vector::Alignment::Center
    {
        let path = match &model.live_shape {
            Some(shape) => shape.path().map_err(failure)?,
            None => model.path.clone(),
        };
        if path.subpaths.iter().any(|s| !s.closed) {
            return Err(failure(
                "Inside and Outside stroke alignment need a closed path; this path is open \
                 (use Center alignment for lines and open paths)",
            ));
        }
    }
    Ok(())
}

fn live_kind(model: &ShapeModel) -> Option<String> {
    model.live_shape.as_ref().map(|s| {
        match s {
            vector::Shape::Rectangle { .. } => "rectangle",
            vector::Shape::Ellipse { .. } => "ellipse",
            vector::Shape::Polygon { .. } => "polygon",
            vector::Shape::Line { .. } => "line",
            vector::Shape::Custom(_) => "custom",
        }
        .to_owned()
    })
}

fn kind_title(model: &ShapeModel) -> &'static str {
    match &model.live_shape {
        Some(vector::Shape::Rectangle { .. }) => "Rectangle",
        Some(vector::Shape::Ellipse { .. }) => "Ellipse",
        Some(vector::Shape::Polygon {
            inner_radius: Some(_),
            ..
        }) => "Star",
        Some(vector::Shape::Polygon { .. }) => "Polygon",
        Some(vector::Shape::Line { .. }) => "Line",
        _ => "Shape",
    }
}

fn shape_of(l: &Layer) -> Result<(&ShapeModel, Affine)> {
    match &l.kind {
        LayerKind::Shape { model, transform } => Ok((model, *transform)),
        _ => Err(failure(format!(
            "layer {:?} is not a shape layer",
            l.props.name
        ))),
    }
}

fn mask_record(m: &VectorMask) -> Result<VectorMaskRecord> {
    Ok(VectorMaskRecord {
        path_json: to_json(&m.path)?,
        enabled: m.enabled,
        feather: m.feather,
        density: m.density,
    })
}

fn mask_of_record(r: &VectorMaskRecord) -> Result<VectorMask> {
    let mask = VectorMask {
        enabled: r.enabled,
        path: parse_path(&r.path_json)?,
        feather: r.feather,
        density: r.density,
    };
    compositor::text_vector::validate_mask(&mask)?;
    Ok(mask)
}

fn document_bounds(model: &ShapeModel, transform: Affine) -> Option<ShapeBounds> {
    let mut path = model.path.clone();
    if let Some((stroke, _)) = &model.stroke
        && stroke.width > 0.0
        && let Ok(outline) =
            stroke.outline(&model.path, GEOMETRY_TOLERANCE.max(stroke.width / 100.0))
    {
        path.subpaths.extend(outline.subpaths);
    }
    if path.subpaths.iter().all(|s| s.anchors.is_empty()) {
        return None;
    }
    let r = path.affine(kurbo_affine(transform)).bounds();
    [r.x0, r.y0, r.x1, r.y1]
        .iter()
        .all(|v| v.is_finite())
        .then_some(ShapeBounds {
            x0: r.x0,
            y0: r.y0,
            x1: r.x1,
            y1: r.y1,
        })
}

fn has_pattern(model: &ShapeModel) -> bool {
    matches!(model.fill, Some(vector::Fill::Pattern(_)))
        || matches!(model.stroke, Some((_, vector::Fill::Pattern(_))))
}

/// Limitations that apply to one layer, stated rather than implied.
fn notes_for(s: &DocState, l: &Layer, model: &ShapeModel, transform: Affine) -> Vec<String> {
    let mut notes = Vec::new();
    if has_pattern(model) {
        notes.push(
            "Pattern paint is kept in Tessera documents, but PSD export does not write pattern \
             shape fills yet (engine limitation): the PSD keeps the rendered pixels and other \
             apps lose the editable pattern."
                .into(),
        );
    }
    if l.vector_mask.is_some() {
        notes.push(
            "In a PSD this extra vector mask is written as a raster user mask (combined with any \
             raster mask) plus Tessera's private tvMk record: Tessera reopens it as an editable \
             vector mask, other apps see the combined raster mask."
                .into(),
        );
    }
    let [a, b, _, d, e, _] = transform.m;
    let (sx, sy) = (a.hypot(d), b.hypot(e));
    let skewed = (a * b + d * e).abs() > 1e-9 * sx.max(sy).max(1.0);
    if model.stroke.is_some() && (skewed || (sx - sy).abs() > 1e-9 * sx.max(sy).max(1.0)) {
        notes.push(
            "The stroke is transformed non-uniformly: PSD stroke widths bake a uniform scale, so \
             other apps may redraw this stroke differently (Tessera restores it exactly)."
                .into(),
        );
    }
    let srgb = s
        .profile
        .as_ref()
        .is_none_or(|p| p.name.to_ascii_lowercase().contains("srgb"));
    if !srgb || s.depth == compositor::Depth::F32 {
        notes.push(
            "Shape colours are used as document samples without colour conversion (the document \
             is not an sRGB integer document)."
                .into(),
        );
    }
    notes
}

fn record_of(s: &DocState, l: &Layer) -> Result<ShapeLayerRecord> {
    let (model, transform) = shape_of(l)?;
    Ok(ShapeLayerRecord {
        layer: l.id.0,
        model_json: to_json(model)?,
        transform: matrix_of(transform),
        revision: layer_revision(l),
        live_kind: live_kind(model),
        bounds: document_bounds(model, transform),
        has_open_subpaths: model.path.subpaths.iter().any(|s| !s.closed),
        vector_mask: l.vector_mask.as_ref().map(mask_record).transpose()?,
        notes: notes_for(s, l, model, transform),
    })
}

/// Geometric hit of one shape layer at document point `(x, y)`.
fn hit_layer(
    l: &Layer,
    x: f64,
    y: f64,
    include_stroke: bool,
    tolerance: f64,
) -> Result<Option<(ShapeHitPart, Point)>> {
    let LayerKind::Shape { model, transform } = &l.kind else {
        return Ok(None);
    };
    let Some(inv) = transform.inverse() else {
        return Ok(None);
    };
    let (lx, ly) = inv.apply(x, y);
    let p = Point::new(lx, ly);
    // Document tolerance → local units (geometric mean scale of the affine).
    let scale = transform.det().abs().sqrt().max(1e-12);
    let tol = (tolerance.max(0.0) / scale).min(1e6);
    if include_stroke
        && let Some((stroke, _)) = &model.stroke
        && stroke.width > 0.0
    {
        let outline = stroke
            .outline(&model.path, GEOMETRY_TOLERANCE)
            .map_err(|e| failure(format!("stroke outline: {e}")))?;
        if outline.contains(p) || (tol > 0.0 && outline.hit_path(p, tol).is_some()) {
            return Ok(Some((ShapeHitPart::Stroke, p)));
        }
    }
    if model.fill.is_some() && model.path.contains(p) {
        return Ok(Some((ShapeHitPart::Fill, p)));
    }
    Ok(None)
}

/// A path edit (Pen / Direct Selection). Points are local shape pixels.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum PathCommand {
    /// Moves an anchor with its handles TO `(x, y)`.
    MoveAnchor {
        subpath: usize,
        anchor: usize,
        x: f64,
        y: f64,
    },
    /// Moves one handle TO `(x, y)`; `mirror` sets the opposite handle
    /// symmetrically (smooth), otherwise the handles are independent.
    SetHandle {
        subpath: usize,
        anchor: usize,
        handle: HandleName,
        x: f64,
        y: f64,
        mirror: bool,
    },
    /// Splits segment `segment` (anchor `segment` → the next) at cubic
    /// parameter `t` in (0, 1), exactly (de Casteljau).
    InsertAnchor {
        subpath: usize,
        segment: usize,
        t: f64,
    },
    /// Removes an anchor; an emptied subpath is removed.
    DeleteAnchor {
        subpath: usize,
        anchor: usize,
    },
    /// Appends a subpath (Pen: another component of the same shape).
    AddSubpath {
        subpath: vector::Subpath,
    },
    /// Opens or closes a subpath.
    SetClosed {
        subpath: usize,
        closed: bool,
    },
    SetFillRule {
        rule: FillRule,
    },
    /// Replaces the whole path.
    SetPath {
        path: VPath,
    },
}

#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
enum HandleName {
    Incoming,
    Outgoing,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Commands {
    Many(Vec<PathCommand>),
    One(PathCommand),
}

/// Merges consecutive anchors at exactly the same point (the corners of a
/// generated square-cornered rectangle), keeping the first's incoming and the
/// last's outgoing handle. Path commands address anchors of the merged path;
/// hosts apply the same rule (TesseraCore `ShapePath.mergingCoincidentAnchors`).
pub(crate) fn merge_coincident_anchors(path: &mut VPath) {
    for s in &mut path.subpaths {
        let mut out: Vec<vector::Anchor> = Vec::with_capacity(s.anchors.len());
        for a in s.anchors.drain(..) {
            match out.last_mut() {
                Some(last) if last.point == a.point => last.outgoing = a.outgoing,
                _ => out.push(a),
            }
        }
        if s.closed && out.len() > 1 && out[0].point == out[out.len() - 1].point {
            let last = out.pop().expect("two anchors");
            out[0].incoming = last.incoming;
        }
        s.anchors = out;
    }
}

fn apply_command(path: &mut VPath, c: PathCommand) -> Result<&'static str> {
    let err = |e: vector::Error| failure(format!("path edit: {e}"));
    Ok(match c {
        PathCommand::MoveAnchor {
            subpath,
            anchor,
            x,
            y,
        } => {
            path.move_anchor(subpath, anchor, Point::new(x, y))
                .map_err(err)?;
            "Move Anchor Point"
        }
        PathCommand::SetHandle {
            subpath,
            anchor,
            handle,
            x,
            y,
            mirror,
        } => {
            let h = match handle {
                HandleName::Incoming => Handle::Incoming,
                HandleName::Outgoing => Handle::Outgoing,
            };
            path.set_handle(subpath, anchor, h, Point::new(x, y), mirror)
                .map_err(err)?;
            "Move Direction Point"
        }
        PathCommand::InsertAnchor {
            subpath,
            segment,
            t,
        } => {
            if !t.is_finite() || t <= 0.0 || t >= 1.0 {
                return Err(failure(
                    "insert anchor: t must lie strictly between 0 and 1",
                ));
            }
            path.split_segment(subpath, segment, t).map_err(err)?;
            "Add Anchor Point"
        }
        PathCommand::DeleteAnchor { subpath, anchor } => {
            path.delete_anchor(subpath, anchor).map_err(err)?;
            if path.subpaths[subpath].anchors.is_empty() {
                path.subpaths.remove(subpath);
            }
            if path.subpaths.is_empty() {
                return Err(failure(
                    "that is the shape's last anchor point: delete the layer instead",
                ));
            }
            "Delete Anchor Point"
        }
        PathCommand::AddSubpath { subpath } => {
            if subpath.anchors.is_empty() {
                return Err(failure("a new subpath needs at least one anchor"));
            }
            path.subpaths.push(subpath);
            "Add Path Component"
        }
        PathCommand::SetClosed { subpath, closed } => {
            let s = path
                .subpaths
                .get_mut(subpath)
                .ok_or_else(|| failure("path edit: invalid vector input: subpath index"))?;
            if closed && s.anchors.len() < 2 {
                return Err(failure(
                    "close path: a closed path needs two or more anchors",
                ));
            }
            s.closed = closed;
            if closed { "Close Path" } else { "Open Path" }
        }
        PathCommand::SetFillRule { rule } => {
            path.fill_rule = rule;
            "Fill Rule"
        }
        PathCommand::SetPath { path: p } => {
            *path = p;
            "Edit Path"
        }
    })
}

// ─────────────────── B5-11 temporary: source-draft lifecycle ───────────────────
// B5-11 temporary: replace with B5-10 convert_to_pixels / source_edit /
// cancel_source_preview. At merge, `shape_source_edit` becomes B5-10's
// `source_edit` (with `SourceOps { preview: op.clone(), commit: Some(op) }`,
// shape ops being absolute), `Pending::Shape` / `Pending::VectorMask` join
// `Pending::is_source`, and the two exported helpers below delegate to
// `cancel_source_preview` / `convert_to_pixels`.

impl DocumentSession {
    /// A shape / mask draft under `key` (B5-10 `source_edit` semantics).
    /// Only one gesture owns the scratch: pending edits of other keys are
    /// committed first as their own node. `make` sees the COMMITTED base and
    /// returns the complete op (or `None`: the draft equals the base).
    fn shape_source_edit(
        &self,
        key: Pending,
        interactive: bool,
        make: impl FnOnce(&DocState) -> Result<Option<(DocOp, String)>>,
    ) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        if st.pending.iter().any(|(k, _)| *k != key) {
            st.pending.retain(|(k, _)| *k != key);
            self.commit_pending(&mut st, None)?;
        }
        let made = make(st.doc.state())?;
        let had = st.pending.iter().any(|(k, _)| *k == key);
        if interactive {
            let Some((op, _)) = made else {
                if had {
                    st.pending.retain(|(k, _)| *k != key);
                    st.scratch = None;
                }
                return Ok(self.update(&mut st, &before, None, false));
            };
            let mut scratch = st.doc.clone();
            scratch.set_max_states(2);
            let applied = scratch.apply(op.clone())?;
            st.scratch = Some(scratch);
            st.pending.retain(|(k, _)| *k != key);
            st.pending.push((key, op));
            return Ok(self.update(&mut st, &before, Some(&applied), false));
        }
        let saved_scratch = st.scratch.take();
        let saved_pending = std::mem::take(&mut st.pending);
        let Some((op, label)) = made else {
            return Ok(self.update(&mut st, &before, None, had));
        };
        match st.doc.apply(op) {
            Ok(applied) => {
                st.labels.insert(applied.node, label);
                Ok(self.update(&mut st, &before, Some(&applied), true))
            }
            Err(e) => {
                st.scratch = saved_scratch;
                st.pending = saved_pending;
                Err(e.into())
            }
        }
    }
}

fn is_shape_draft(k: &Pending) -> bool {
    matches!(k, Pending::Shape(_) | Pending::VectorMask(_))
}

#[uniffi::export]
impl DocumentSession {
    /// Drops a pending shape or vector-mask draft with no history change
    /// (Esc). Other pending drags are rebuilt on a fresh scratch.
    /// B5-11 temporary: replace with B5-10 cancel_source_preview.
    pub fn cancel_shape_preview(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        if !st.pending.iter().any(|(k, _)| is_shape_draft(k)) {
            return Ok(DocumentUpdate {
                layers_changed: Vec::new(),
                created: Vec::new(),
                history_head: st.doc.history().current(),
                dirty_rect: None,
                epoch: st.epoch,
                dirty: st.dirty(),
            });
        }
        st.pending.retain(|k| !is_shape_draft(&k.0));
        st.scratch = None;
        if !st.pending.is_empty() {
            let mut scratch = st.doc.clone();
            scratch.set_max_states(2);
            let mut kept = Vec::new();
            for (k, op) in std::mem::take(&mut st.pending) {
                if scratch.apply(op.clone()).is_ok() {
                    kept.push((k, op));
                }
            }
            st.pending = kept;
            st.scratch = Some(scratch);
        }
        Ok(self.update(&mut st, &before, None, false))
    }

    /// Layer ▸ Rasterize ▸ Shape: pixels at document depth in one node,
    /// keeping id, properties, styles and both masks; undo restores the live
    /// shape. B5-11 temporary: replace with B5-10 convert_to_pixels.
    pub fn convert_shape_to_pixels(&self, layer: u64) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            let l = find(st.live().state(), layer)?;
            shape_of(l)?;
        }
        self.edit(
            DocOp::ConvertToPixels { id: LayerId(layer) },
            Some("Rasterize Shape"),
        )
    }
}
// ─────────────────── B5-11 temporary end ───────────────────

#[uniffi::export]
impl DocumentSession {
    // ─────────────────────────────── reads ───────────────────────────────

    /// The shape source of `layer` (the live draft while one is pending).
    pub fn shape_layer(&self, layer: u64) -> Result<ShapeLayerRecord> {
        let st = self.shared.lock()?;
        let s = st.live().state();
        record_of(s, find(s, layer)?)
    }

    /// The document-space vector mask of any layer.
    pub fn vector_mask(&self, layer: u64) -> Result<Option<VectorMaskRecord>> {
        let st = self.shared.lock()?;
        find(st.live().state(), layer)?
            .vector_mask
            .as_ref()
            .map(mask_record)
            .transpose()
    }

    /// The topmost visible shape layer whose geometry contains document
    /// point `(x, y)`: its stroke outline (when `include_stroke`, within
    /// `tolerance` document pixels of it) and then its fill under the path's
    /// fill rule. Geometric only: masks and opacity are not considered.
    pub fn shape_hit_test(
        &self,
        x: f64,
        y: f64,
        include_stroke: bool,
        tolerance: f64,
    ) -> Result<Option<ShapeHitRecord>> {
        if !x.is_finite() || !y.is_finite() || !tolerance.is_finite() {
            return Err(failure("hit test: the point and tolerance must be finite"));
        }
        let st = self.shared.lock()?;
        fn walk(
            v: &[Arc<Layer>],
            x: f64,
            y: f64,
            stroke: bool,
            tol: f64,
        ) -> Result<Option<ShapeHitRecord>> {
            for l in v.iter().rev() {
                if !l.props.visible {
                    continue;
                }
                if let Some(c) = l.children() {
                    if let Some(h) = walk(c, x, y, stroke, tol)? {
                        return Ok(Some(h));
                    }
                    continue;
                }
                if let Some((part, p)) = hit_layer(l, x, y, stroke, tol)? {
                    return Ok(Some(ShapeHitRecord {
                        layer: l.id.0,
                        part,
                        local_x: p.x,
                        local_y: p.y,
                    }));
                }
            }
            Ok(None)
        }
        walk(&st.live().state().root, x, y, include_stroke, tolerance)
    }

    /// [`shape_hit_test`](Self::shape_hit_test) of one layer (hidden or not).
    pub fn shape_layer_hit_test(
        &self,
        layer: u64,
        x: f64,
        y: f64,
        include_stroke: bool,
        tolerance: f64,
    ) -> Result<Option<ShapeHitPart>> {
        if !x.is_finite() || !y.is_finite() || !tolerance.is_finite() {
            return Err(failure("hit test: the point and tolerance must be finite"));
        }
        let st = self.shared.lock()?;
        let l = find(st.live().state(), layer)?;
        shape_of(l)?;
        Ok(hit_layer(l, x, y, include_stroke, tolerance)?.map(|h| h.0))
    }

    // ─────────────────────────────── edits ───────────────────────────────

    /// Adds a shape layer (one node, "<Kind> Tool"): `model_json` is a
    /// `vector::ShapeModel`; with `live_shape` its path is regenerated from
    /// the construction parameters. `name` empty: "Rectangle 1", …;
    /// `parent` `None`: the root; `index` bottom-first, `None`: on top. The
    /// new id is `DocumentUpdate.created[0]`.
    pub fn add_shape_layer(
        &self,
        name: String,
        parent: Option<u64>,
        index: Option<u32>,
        model_json: String,
        transform: TransformMatrix,
    ) -> Result<DocumentUpdate> {
        let model = parse_model(&model_json)?;
        let transform = checked_affine(transform)?;
        validate_model(&model, transform)?;
        let title = kind_title(&model);
        let name = if name.is_empty() {
            let st = self.shared.lock()?;
            numbered_name(&layer_names(st.live().state()), title)
        } else {
            name
        };
        let label = if model.live_shape.is_some() && title != "Shape" {
            format!("{} Tool", if title == "Star" { "Polygon" } else { title })
        } else {
            "Pen".to_owned()
        };
        self.edit(
            DocOp::AddShape {
                parent: parent.map(LayerId),
                index: index.map_or(usize::MAX, |i| i as usize),
                name,
                model,
                transform,
            },
            Some(&label),
        )
    }

    /// Replaces a shape's model and transform. `interactive`: a live draft
    /// (inspector slider, affine handle drag) with no history until the
    /// final call of the same layer, which records the net change as one
    /// node. A draft equal to the committed source records nothing. The
    /// layer's vector mask stays where it is in the document.
    pub fn set_shape_layer(
        &self,
        layer: u64,
        model_json: String,
        transform: TransformMatrix,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let model = parse_model(&model_json)?;
        let transform = checked_affine(transform)?;
        validate_model(&model, transform)?;
        self.shape_source_edit(Pending::Shape(layer), interactive, move |s| {
            let l = find(s, layer)?;
            let (old_model, old_transform) = shape_of(l)?;
            let mut regenerated = model.clone();
            if let Some(shape) = &regenerated.live_shape {
                regenerated.path = shape.path().map_err(failure)?;
            }
            if regenerated == *old_model && transform == old_transform {
                return Ok(None);
            }
            let label = if regenerated == *old_model {
                "Transform Shape"
            } else {
                "Edit Shape"
            };
            Ok(Some((
                DocOp::EditShape {
                    id: LayerId(layer),
                    model,
                    transform,
                },
                label.to_owned(),
            )))
        })
    }

    /// A Pen / Direct Selection edit of the shape's path in local pixels:
    /// one command object or an array of them (`{"op":"move_anchor",
    /// "subpath":0,"anchor":2,"x":10,"y":4}`, `set_handle` with `handle`
    /// `incoming`/`outgoing` and `mirror`, `insert_anchor` with `segment`
    /// and `t`, `delete_anchor`, `add_subpath`, `set_closed`,
    /// `set_fill_rule`, `set_path`). Commands apply to the committed path, so
    /// an interactive drag sends absolute positions. The edit clears
    /// `live_shape`: the primitive no longer regenerates over it.
    pub fn edit_shape_path(
        &self,
        layer: u64,
        command_json: String,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let commands = match serde_json::from_str::<Commands>(&command_json)
            .map_err(|e| failure(format!("path command JSON: {e}")))?
        {
            Commands::Many(v) => v,
            Commands::One(c) => vec![c],
        };
        if commands.is_empty() {
            return Err(failure("path command JSON: no commands"));
        }
        self.shape_source_edit(Pending::Shape(layer), interactive, move |s| {
            let l = find(s, layer)?;
            let (old, transform) = shape_of(l)?;
            let mut model = old.clone();
            merge_coincident_anchors(&mut model.path);
            let mut label = "Edit Path";
            for c in commands {
                label = apply_command(&mut model.path, c)?;
            }
            model.live_shape = None;
            validate_model(&model, transform)?;
            if model == *old {
                return Ok(None);
            }
            Ok(Some((
                DocOp::EditShape {
                    id: LayerId(layer),
                    model,
                    transform,
                },
                label.to_owned(),
            )))
        })
    }

    /// The explicit linked-mask gesture: the new shape `transform` AND the
    /// layer's vector mask moved by the same document-space change
    /// (`new ∘ old⁻¹`), as ONE atomic `Batch` node (or draft). Without a
    /// vector mask this is `set_shape_layer` with the same model.
    pub fn transform_shape_with_mask(
        &self,
        layer: u64,
        transform: TransformMatrix,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let transform = checked_affine(transform)?;
        self.shape_source_edit(Pending::Shape(layer), interactive, move |s| {
            let l = find(s, layer)?;
            let (model, old) = shape_of(l)?;
            if transform == old {
                return Ok(None);
            }
            let inv = old
                .inverse()
                .ok_or_else(|| failure("the current shape transform is singular"))?;
            let delta = compose(transform, inv);
            let edit = DocOp::EditShape {
                id: LayerId(layer),
                model: model.clone(),
                transform,
            };
            Ok(Some(match &l.vector_mask {
                None => (edit, "Transform Shape".to_owned()),
                Some(mask) => {
                    let mut moved = mask.clone();
                    moved.path = mask.path.affine(kurbo_affine(delta));
                    (
                        DocOp::Batch(vec![
                            edit,
                            DocOp::SetVectorMask {
                                id: LayerId(layer),
                                mask: Some(moved),
                            },
                        ]),
                        "Transform Shape and Vector Mask".to_owned(),
                    )
                }
            }))
        })
    }

    /// Combines the paths of `operands` into `layer` (Combine, Subtract
    /// Front, Intersect, Exclude Overlapping, applied in order) and removes
    /// the operand layers: ONE history node. Operand geometry is mapped
    /// through its own affine and the inverse of the target's. The result is
    /// canonical nonzero contours (holes wind opposite); it clears
    /// `live_shape` and keeps the target's paint, stroke and transform.
    pub fn boolean_shape_paths(
        &self,
        layer: u64,
        operands: Vec<u64>,
        operation: ShapePathOperation,
    ) -> Result<DocumentUpdate> {
        if operands.is_empty() {
            return Err(failure("select two or more shape layers to combine"));
        }
        let op = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            let target = find(s, layer)?;
            let (model, transform) = shape_of(target)?;
            let inv = transform
                .inverse()
                .ok_or_else(|| failure("the shape transform is singular"))?;
            let mut path = model.path.clone();
            let mut seen = std::collections::BTreeSet::new();
            let mut ops = Vec::new();
            for &id in &operands {
                if id == layer || !seen.insert(id) {
                    return Err(failure("each operand must be a different shape layer"));
                }
                let l = find(s, id)?;
                let (m, t) = shape_of(l)?;
                if l.props.locks.all || l.props.locks.pixels {
                    return Err(failure(format!("layer {:?} is locked", l.props.name)));
                }
                let local = m.path.affine(kurbo_affine(compose(inv, t)));
                path = path
                    .boolean(&local, operation.into(), GEOMETRY_TOLERANCE)
                    .map_err(|e| failure(format!("path operation: {e}")))?;
                ops.push(DocOp::RemoveLayer { id: LayerId(id) });
            }
            if path.subpaths.is_empty() {
                return Err(failure(
                    "the path operation leaves no area: nothing would remain of the shape",
                ));
            }
            let mut model = model.clone();
            model.path = path;
            model.live_shape = None;
            validate_model(&model, transform)?;
            ops.insert(
                0,
                DocOp::EditShape {
                    id: LayerId(layer),
                    model,
                    transform,
                },
            );
            DocOp::Batch(ops)
        };
        let label = match operation {
            ShapePathOperation::Combine => "Combine Shapes",
            ShapePathOperation::Subtract => "Subtract Front Shape",
            ShapePathOperation::Intersect => "Intersect Shape Areas",
            ShapePathOperation::Exclude => "Exclude Overlapping Shapes",
        };
        self.edit(op, Some(label))
    }

    /// Adds, replaces (`Some`) or deletes (`None`) the document-space vector
    /// mask of any layer, next to its raster mask (which is untouched).
    /// `interactive` as for `set_shape_layer` (density / feather drags).
    pub fn set_vector_mask(
        &self,
        layer: u64,
        mask: Option<VectorMaskRecord>,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let mask = mask.as_ref().map(mask_of_record).transpose()?;
        self.shape_source_edit(Pending::VectorMask(layer), interactive, move |s| {
            let l = find(s, layer)?;
            if mask == l.vector_mask {
                return Ok(None);
            }
            let label = match (&l.vector_mask, &mask) {
                (None, Some(_)) => "Add Vector Mask",
                (Some(_), None) => "Delete Vector Mask",
                (Some(a), Some(b)) if a.path != b.path => "Edit Vector Mask",
                (Some(a), Some(b)) if a.enabled != b.enabled => {
                    if b.enabled {
                        "Enable Vector Mask"
                    } else {
                        "Disable Vector Mask"
                    }
                }
                (Some(a), Some(b)) if a.density != b.density => "Vector Mask Density",
                (Some(a), Some(b)) if a.feather != b.feather => "Vector Mask Feather",
                _ => "Vector Mask",
            };
            Ok(Some((
                DocOp::SetVectorMask {
                    id: LayerId(layer),
                    mask,
                },
                label.to_owned(),
            )))
        })
    }
}

/// The path a live shape primitive generates (`vector::Shape` JSON →
/// `vector::Path` JSON), for host overlays drawn before a layer exists.
#[uniffi::export]
pub fn shape_primitive_path(shape_json: String) -> Result<String> {
    let shape: vector::Shape =
        serde_json::from_str(&shape_json).map_err(|e| failure(format!("shape JSON: {e}")))?;
    to_json(&shape.path().map_err(failure)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kurbo_layout_matches_row_major() {
        let a = Affine {
            m: [1.5, 0.25, 10.0, -0.5, 2.0, -3.0],
        };
        let k = kurbo_affine(a);
        for (x, y) in [(0.0, 0.0), (3.0, -2.0), (7.5, 11.0)] {
            let (ex, ey) = a.apply(x, y);
            let p = k * Point::new(x, y);
            assert!((p.x - ex).abs() < 1e-12 && (p.y - ey).abs() < 1e-12);
        }
        let inv = a.inverse().unwrap();
        let id = compose(a, inv);
        for (v, e) in id.m.iter().zip(Affine::IDENTITY.m) {
            assert!((v - e).abs() < 1e-12);
        }
    }
}
