//! Layered-document sessions and the executor for every
//! [`DocumentToolCall`] (spec 02, spec 10 §2).
//!
//! A [`Documents`] registry owns the open documents, keyed by session
//! [`DocumentId`]. Each session pairs the compositor's [`Document`] (the
//! copy-on-write states) with the engine-api [`DocumentHistory`] (the
//! replayable [`Action`] and [`EditMeta`] of every state), one compositor
//! state per history entry (CONTRACTS.md invariants 11, 16, 17).
//!
//! Every successful `edits_document()` call applies exactly one compositor
//! op, possibly a [`DocOp::Batch`], and records exactly one entry with
//! `Author::Agent`, the request's rationale and group, and
//! `Action::from_document_tool(call)`. A call that fails leaves both the
//! state and the history untouched. `expect_head` is checked before
//! anything else.
pub mod advanced;
pub mod brush_presets;
mod dense;
mod io;
pub(crate) mod local_tools;
pub mod paint;
mod preview;
pub mod select;
mod transform;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use compositor::resident::ResidentRenderer;
use compositor::{
    Adjustment, BlendMode, DocOp, DocState, Document, Fill, GroupMode, Layer, LayerKind,
    LayerProps, Mask, PaintTarget, Raster, Rect,
};
use engine_api::action::Action;
use engine_api::document::{
    AdjustmentSpec, BrushMode, BrushParams, CanvasRect, DocumentHistory, DocumentSummary,
    LayerInfo, LayerKindTag, LayerPropsUpdate, NewLayer, SelectionMode, SelectionShape,
    StrokePoint, StrokeTarget,
};
use engine_api::id::{DocumentId, HistoryEntryId, LayerId, SelectionId};
use engine_api::recipe::history::HistoryGroup;
use engine_api::recipe::{Author, EditMeta};
use engine_api::tools::{
    DocumentToolCall, DocumentToolOutput, DocumentToolRequest, DocumentToolResponse, FieldUpdate,
};
use engine_api::{EngineError, EngineResult};
use serde::Serialize;
use serde::de::DeserializeOwned;

pub use paint::{BrushEngine, RoundBrush, StrokeCoverage};
pub use preview::{DocumentDescription, LayerThumbnail};
pub use select::{BasicSelection, SelectionEngine, SelectionGeometry};

/// Compositor states retained per document. Every state is a structurally
/// shared snapshot, so this bounds bookkeeping, not pixel memory.
const MAX_STATES: usize = 100_000;

/// A selection saved with `save_as`.
#[derive(Debug, Clone)]
pub struct SavedSelection {
    /// Id.
    pub id: SelectionId,
    /// Name.
    pub name: String,
    /// Canvas-sized f32 mask.
    pub mask: Arc<Raster>,
}

/// One open document.
pub struct DocumentSession {
    /// File it was opened from.
    pub path: PathBuf,
    doc: Document,
    history: DocumentHistory,
    /// Compositor state of each history entry (`nodes[id − 1]`).
    nodes: Vec<u64>,
    /// Compositor state as opened.
    root_node: u64,

    renderer: Option<ResidentRenderer>,
    /// Import warnings (features preserved but not rendered).
    pub warnings: Vec<String>,
}

impl DocumentSession {
    fn new(path: PathBuf, mut doc: Document, warnings: Vec<String>) -> Self {
        doc.set_max_states(MAX_STATES);
        let root_node = doc.history().current();
        Self {
            path,
            doc,
            history: DocumentHistory::default(),
            nodes: Vec::new(),
            root_node,

            renderer: None,
            warnings,
        }
    }

    /// The compositor document (current state and states).
    pub fn document(&self) -> &Document {
        &self.doc
    }

    /// The agent-facing history.
    pub fn history(&self) -> &DocumentHistory {
        &self.history
    }

    /// Saved selections.
    pub fn saved_selections(&self) -> Vec<SavedSelection> {
        self.state()
            .channels
            .iter()
            .filter(|c| matches!(c.kind, compositor::channels::ChannelKind::Alpha))
            .map(|c| SavedSelection {
                id: SelectionId(c.id.0),
                name: c.name.clone(),
                mask: Arc::new(c.raster.clone()),
            })
            .collect()
    }

    fn state(&self) -> &DocState {
        self.doc.state()
    }

    fn node_of(&self, entry: Option<HistoryEntryId>) -> EngineResult<u64> {
        match entry {
            None => Ok(self.root_node),
            Some(id) => usize::try_from(id.0)
                .ok()
                .and_then(|i| i.checked_sub(1))
                .and_then(|i| self.nodes.get(i).copied())
                .ok_or_else(|| EngineError::not_found("history entry", id)),
        }
    }

    /// Moves both histories to `target` (undo, redo, history jumps).
    pub fn checkout(&mut self, target: Option<HistoryEntryId>) -> EngineResult<()> {
        let node = self.node_of(target)?;
        self.doc.checkout(node)?;
        self.history.checkout(target)?;

        Ok(())
    }

    /// Applies `op` and records the entry for `request`.
    fn commit(
        &mut self,
        op: DocOp,
        request: &DocumentToolRequest,
    ) -> EngineResult<(HistoryEntryId, compositor::Applied)> {
        let action = Action::from_document_tool(&request.call)?;
        let expected = self.node_of(self.history.head)?;
        if self.doc.history().current() != expected {
            self.doc.checkout(expected)?;
        }
        let applied = self.doc.apply(op)?;
        let entry = self.history.record(action, meta(request));
        self.nodes.push(applied.node);

        if let Some(group) = request.group
            && !self.history.groups.iter().any(|g| g.id == group)
        {
            self.history.groups.push(HistoryGroup {
                id: group,
                name: "Agent base edit".into(),
            });
        }
        debug_assert_eq!(self.nodes.len(), self.history.entries.len());
        Ok((entry, applied))
    }

    fn layer(&self, id: LayerId) -> EngineResult<&Layer> {
        if id.is_root() {
            return Err(EngineError::invalid(
                "layer",
                "the document root is not a layer",
            ));
        }
        self.state()
            .find(id)
            .ok_or_else(|| EngineError::not_found("layer", id))
    }
}

pub(crate) fn meta(request: &DocumentToolRequest) -> EditMeta {
    EditMeta {
        label: request.call.name().into(),
        author: Author::Agent {
            name: "tessera-mcp".into(),
        },
        timestamp_ms: crate::console::now(),
        group: request.group,
        rationale: request.rationale.clone(),
    }
}

/// Converts between engine-api and compositor types whose serde forms are
/// equal (checked by the compositor's `tests/contracts.rs`).
pub(crate) fn convert<A: Serialize, B: DeserializeOwned>(what: &str, a: &A) -> EngineResult<B> {
    serde_json::from_value(serde_json::to_value(a)?)
        .map_err(|e| EngineError::invalid(what, e.to_string()))
}

fn unit(name: &str, v: f32) -> EngineResult<()> {
    if v.is_finite() && (0.0..=1.0).contains(&v) {
        Ok(())
    } else {
        Err(EngineError::invalid(name, "must be within 0..=1"))
    }
}

/// Open layered documents and the tools that edit them.
pub struct Documents {
    sessions: BTreeMap<DocumentId, DocumentSession>,
    next: u64,
    brush: Box<dyn BrushEngine>,
    selection: Box<dyn SelectionEngine>,
    segment_model: Option<Box<dyn selection::ml::SegmentModel + Send>>,
}

impl Default for Documents {
    fn default() -> Self {
        Self::new()
    }
}

impl Documents {
    /// No open documents; built-in brush and selection engines.
    pub fn new() -> Self {
        Self {
            sessions: BTreeMap::new(),
            next: 1,
            brush: Box::new(paint::RealBrush),
            selection: Box::new(select::RealSelection),
            segment_model: None,
        }
    }

    /// Installs the segmentation provider for object, subject and sky tools.
    pub fn set_segment_model(&mut self, model: Box<dyn selection::ml::SegmentModel + Send>) {
        self.segment_model = Some(model);
    }

    /// Installs a custom brush engine.
    pub fn set_brush_engine(&mut self, engine: Box<dyn BrushEngine>) {
        self.brush = engine;
    }

    /// Installs a selection engine (the selection crate, when linked).
    pub fn set_selection_engine(&mut self, engine: Box<dyn SelectionEngine>) {
        self.selection = engine;
    }

    /// An open document.
    pub fn session(&self, id: DocumentId) -> EngineResult<&DocumentSession> {
        self.sessions
            .get(&id)
            .ok_or_else(|| EngineError::not_found("document", id))
    }

    fn session_mut(&mut self, id: DocumentId) -> EngineResult<&mut DocumentSession> {
        self.sessions
            .get_mut(&id)
            .ok_or_else(|| EngineError::not_found("document", id))
    }

    /// Open document ids.
    pub fn ids(&self) -> Vec<DocumentId> {
        self.sessions.keys().copied().collect()
    }

    /// Closes a document (unsaved changes are discarded).
    pub fn close(&mut self, id: DocumentId) -> EngineResult<()> {
        self.sessions
            .remove(&id)
            .map(|_| ())
            .ok_or_else(|| EngineError::not_found("document", id))
    }

    /// Steps the document back to the parent of its head. False at the
    /// opened state.
    pub fn undo(&mut self, id: DocumentId) -> EngineResult<bool> {
        let s = self.session_mut(id)?;
        match s.history.undo_target() {
            Some(target) => s.checkout(target).map(|_| true),
            None => Ok(false),
        }
    }

    /// Steps to the newest child of the head. False at a leaf.
    pub fn redo(&mut self, id: DocumentId) -> EngineResult<bool> {
        let s = self.session_mut(id)?;
        match s.history.redo_target() {
            Some(target) => s.checkout(Some(target)).map(|_| true),
            None => Ok(false),
        }
    }

    /// Executes one request.
    pub fn execute(&mut self, request: &DocumentToolRequest) -> DocumentToolResponse {
        self.run(request).into()
    }

    pub(crate) fn run(
        &mut self,
        request: &DocumentToolRequest,
    ) -> EngineResult<DocumentToolOutput> {
        let call = &request.call;
        let Some(id) = call.document() else {
            let DocumentToolCall::OpenDocument { path } = call else {
                return Err(EngineError::internal("call without a document"));
            };
            if request.expect_head.is_some() {
                return Err(EngineError::invalid(
                    "expect_head",
                    "open_document has no history to check",
                ));
            }
            return self.open(path);
        };
        let session = self.session(id)?;
        if let Some(expected) = request.expect_head
            && expected != session.history.head
        {
            return Err(EngineError::Conflict {
                message: format!(
                    "document head is {}, expected {}",
                    fmt_head(session.history.head),
                    fmt_head(expected)
                ),
            });
        }
        match call {
            DocumentToolCall::OpenDocument { .. } => unreachable!("handled above"),
            DocumentToolCall::ListLayers { .. } => Ok(DocumentToolOutput::LayerList {
                document: id,
                layers: list_layers(session.state())?,
            }),
            DocumentToolCall::ExportDocument { settings, .. } => {
                io::export(session, settings)?;
                Ok(DocumentToolOutput::DocumentExportQueued {
                    document: id,
                    job: io::next_job(),
                })
            }
            _ => {
                let (op, layer, save) = self.plan(session, call)?;
                let session = self.session_mut(id)?;
                let op = if let Some((name, mask)) = &save {
                    DocOp::Batch(vec![
                        op,
                        DocOp::AddChannel {
                            channel: compositor::channels::DocumentChannel {
                                id: compositor::channels::ChannelId(0),
                                name: name.clone(),
                                kind: compositor::channels::ChannelKind::Alpha,
                                raster: (**mask).clone(),
                            },
                        },
                    ])
                } else {
                    op
                };
                let (entry, applied) = session.commit(op, request)?;
                let layer = layer.or_else(|| applied.created.first().copied());
                let selection = save.map(|_| {
                    SelectionId(
                        session
                            .state()
                            .channels
                            .last()
                            .expect("inserted channel")
                            .id
                            .0,
                    )
                });
                Ok(DocumentToolOutput::DocumentEdited {
                    document: id,
                    entry: Some(entry),
                    layer,
                    selection,
                })
            }
        }
    }

    /// Builds the op for an editing call without touching the document.
    /// Returns the op, the layer to report (created layers come from the
    /// applied op) and a selection to save.
    #[allow(clippy::type_complexity)]
    fn plan(
        &self,
        session: &DocumentSession,
        call: &DocumentToolCall,
    ) -> EngineResult<(DocOp, Option<LayerId>, Option<(String, Arc<Raster>)>)> {
        let state = session.state();
        Ok(match call {
            DocumentToolCall::AddLayer {
                layer,
                name,
                parent,
                above,
                props,
                ..
            } => {
                let (kind, default_name) = match layer {
                    NewLayer::Pixel => (
                        LayerKind::Pixel(Raster::new(state.canvas, 4, state.depth, 0.0)),
                        "Layer",
                    ),
                    NewLayer::Group { mode } => (
                        LayerKind::Group {
                            mode: convert::<_, GroupMode>("mode", mode)?,
                            children: Vec::new(),
                        },
                        "Group",
                    ),
                    NewLayer::SolidColor { color } => {
                        if color.iter().any(|c| !c.is_finite()) {
                            return Err(EngineError::invalid("color", "must be finite"));
                        }
                        (LayerKind::Fill(Fill::Solid { color: *color }), "Color Fill")
                    }
                };
                let mut new = Layer::new(
                    name.clone()
                        .unwrap_or_else(|| format!("{default_name} {}", state.next_id)),
                    kind,
                );
                new.props = apply_props(new.props, props)?;
                let index = insert_index(state, *parent, *above)?;
                (
                    DocOp::AddLayer {
                        parent: *parent,
                        index,
                        layer: new,
                    },
                    None,
                    None,
                )
            }
            DocumentToolCall::SetLayerProps { layer, props, .. } => {
                let l = session.layer(*layer)?;
                (
                    DocOp::SetProps {
                        id: *layer,
                        props: apply_props(l.props.clone(), props)?,
                    },
                    Some(*layer),
                    None,
                )
            }
            DocumentToolCall::PaintStroke {
                layer,
                points,
                brush,
                target,
                ..
            } => (
                self.paint(session, *layer, points, brush, *target)?,
                Some(*layer),
                None,
            ),
            DocumentToolCall::SetPixelSelection {
                shape,
                mode,
                feather,
                save_as,
                ..
            } => {
                let selection = self.selection(session, shape, *mode, *feather)?;
                let save = match save_as {
                    None => None,
                    Some(name) if name.trim().is_empty() => {
                        return Err(EngineError::invalid("save_as", "must not be empty"));
                    }
                    Some(name) => Some((
                        name.clone(),
                        Arc::new(selection.clone().unwrap_or_else(|| {
                            Raster::new(state.canvas, 1, compositor::Depth::F32, 0.0)
                        })),
                    )),
                };
                (DocOp::SetSelection { selection }, None, save)
            }
            DocumentToolCall::ApplyAdjustmentLayer {
                adjustment,
                name,
                parent,
                above,
                clipped,
                mask_from_selection,
                ..
            } => {
                validate_adjustment(adjustment)?;
                let adj: Adjustment = convert("adjustment", adjustment)?;
                let mut new = Layer::new(
                    name.clone().unwrap_or_else(|| {
                        format!("{} {}", adjustment_name(adjustment), state.next_id)
                    }),
                    LayerKind::Adjustment(adj),
                );
                new.props.clipped = *clipped;
                if *mask_from_selection && let Some(sel) = &state.selection {
                    new.mask = Some(Mask {
                        raster: dense::convert_mask(sel, state.depth)?,
                        ..Mask::reveal_all(state.canvas, state.depth)
                    });
                }
                let index = insert_index(state, *parent, *above)?;
                (
                    DocOp::AddLayer {
                        parent: *parent,
                        index,
                        layer: new,
                    },
                    None,
                    None,
                )
            }
            DocumentToolCall::TransformLayer {
                layer,
                transform,
                interpolation,
                ..
            } => {
                let t = compositor::Affine { m: transform.0 };
                if t.m.iter().any(|v| !v.is_finite()) || t.inverse().is_none() {
                    return Err(EngineError::invalid(
                        "transform",
                        "must be finite and invertible",
                    ));
                }
                let l = session.layer(*layer)?;
                if l.props.locks.position || l.props.locks.all {
                    return Err(EngineError::invalid("layer", "position is locked"));
                }
                let op = match &l.kind {
                    LayerKind::SmartObject(so) => DocOp::SetSmartTransform {
                        id: *layer,
                        transform: transform::compose(&t, &so.transform),
                    },
                    LayerKind::Text(_) => {
                        return Err(crate::unsupported(
                            "transforming text layers needs the type engine",
                        ));
                    }
                    LayerKind::Pixel(r) => {
                        let mut ops = vec![DocOp::PaintTiles {
                            id: *layer,
                            target: PaintTarget::Content,
                            tiles: transform::resample(r, &t, *interpolation)?,
                            dirty: Rect::of_extent(state.canvas),
                        }];
                        if let Some(m) = &l.mask {
                            ops.push(DocOp::PaintTiles {
                                id: *layer,
                                target: PaintTarget::Mask,
                                tiles: transform::resample(&m.raster, &t, *interpolation)?,
                                dirty: Rect::of_extent(state.canvas),
                            });
                        }
                        DocOp::Batch(ops)
                    }
                    _ => {
                        return Err(crate::unsupported(
                            "transform_layer supports pixel layers and smart objects",
                        ));
                    }
                };
                (op, Some(*layer), None)
            }
            DocumentToolCall::MergeDown { layer, .. } => {
                let (op, lower) = merge_down(state, *layer)?;
                (op, Some(lower), None)
            }
            DocumentToolCall::OpenDocument { .. }
            | DocumentToolCall::ExportDocument { .. }
            | DocumentToolCall::ListLayers { .. } => {
                return Err(EngineError::internal("not an editing call"));
            }
        })
    }

    fn paint(
        &self,
        session: &DocumentSession,
        id: LayerId,
        points: &[StrokePoint],
        brush: &BrushParams,
        target: StrokeTarget,
    ) -> EngineResult<DocOp> {
        let state = session.state();
        let layer = session.layer(id)?;
        if points.is_empty() {
            return Err(EngineError::invalid("points", "at least one point"));
        }
        if points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !(0.0..=1.0).contains(&p.pressure))
        {
            return Err(EngineError::invalid(
                "points",
                "coordinates must be finite and pressure within 0..=1",
            ));
        }
        if !(brush.size.is_finite() && brush.size > 0.0 && brush.size <= 5000.0) {
            return Err(EngineError::invalid(
                "brush.size",
                "must be within (0, 5000]",
            ));
        }
        if !(brush.spacing.is_finite() && brush.spacing > 0.0 && brush.spacing <= 10.0) {
            return Err(EngineError::invalid(
                "brush.spacing",
                "must be within (0, 10]",
            ));
        }
        unit("brush.hardness", brush.hardness)?;
        unit("brush.flow", brush.flow)?;
        unit("brush.opacity", brush.opacity)?;
        if brush.color.iter().any(|c| !c.is_finite()) {
            return Err(EngineError::invalid("brush.color", "must be finite"));
        }
        let (paint_target, mut prelude) = match target {
            StrokeTarget::Pixels => {
                if layer.raster().is_none() {
                    return Err(EngineError::invalid(
                        "layer",
                        "only pixel layers have pixels to paint",
                    ));
                }
                (PaintTarget::Content, Vec::new())
            }
            StrokeTarget::Mask => {
                let ops = if layer.mask.is_none() {
                    vec![DocOp::SetMask {
                        id,
                        mask: Some(Mask::reveal_all(state.canvas, state.depth)),
                    }]
                } else {
                    Vec::new()
                };
                (PaintTarget::Mask, ops)
            }
        };
        let base_mask = Mask::reveal_all(state.canvas, state.depth);
        let base = match paint_target {
            PaintTarget::Content => layer.raster().expect("validated pixel layer"),
            PaintTarget::Mask => &layer.mask.as_ref().unwrap_or(&base_mask).raster,
        };
        if let Some((dirty, tiles)) =
            self.brush
                .paint_tiles(base, state.selection.as_deref(), points, brush)?
        {
            let tiles = tiles
                .into_iter()
                .map(|(tx, ty, tile)| compositor::TileDelta {
                    tx,
                    ty,
                    tile: Some(tile),
                })
                .collect();
            prelude.push(DocOp::PaintTiles {
                id,
                target: paint_target,
                tiles,
                dirty,
            });
            return Ok(DocOp::Batch(prelude));
        }
        let cov = self.brush.rasterize(points, brush, state.canvas)?;
        let expected = (cov.rect.width().max(0) * cov.rect.height().max(0)) as usize;
        if cov.alpha.len() != expected {
            return Err(EngineError::internal("brush coverage size mismatch"));
        }
        let rect = cov.rect.intersect(&Rect::of_extent(state.canvas));
        if rect != cov.rect {
            return Err(EngineError::internal("brush coverage outside the canvas"));
        }
        let selection = match &state.selection {
            Some(sel) => Some(dense::read(sel, rect)?),
            None => None,
        };
        let w = rect.width().max(0) as usize;
        let alpha_at = |x: u32, y: u32| {
            let i = (i64::from(y) - rect.y0) as usize * w + (i64::from(x) - rect.x0) as usize;
            let s = selection
                .as_ref()
                .map_or(1.0, |d| d.px[i][0].clamp(0.0, 1.0));
            cov.alpha[i] * s
        };
        let color = brush.color;
        let erase = brush.mode == BrushMode::Erase;
        let luma = 0.2126 * color[0] + 0.7152 * color[1] + 0.0722 * color[2];
        let op = match paint_target {
            PaintTarget::Content => {
                compositor::paint_op(state, id, paint_target, rect, |x, y, p| {
                    let a = alpha_at(x, y);
                    if a <= 0.0 {
                        return;
                    }
                    if erase {
                        p[3] *= 1.0 - a;
                    } else {
                        let out = a + p[3] * (1.0 - a);
                        if out > 0.0 {
                            for c in 0..3 {
                                p[c] = (color[c] * a + p[c] * p[3] * (1.0 - a)) / out;
                            }
                        }
                        p[3] = out;
                    }
                })?
            }
            PaintTarget::Mask => {
                // The mask may not exist yet: paint onto a reveal-all one.
                let base;
                let raster = match &layer.mask {
                    Some(m) => &m.raster,
                    None => {
                        base = Mask::reveal_all(state.canvas, state.depth).raster;
                        &base
                    }
                };
                DocOp::PaintTiles {
                    id,
                    target: PaintTarget::Mask,
                    tiles: dense::deltas(raster, rect, |x, y, p| {
                        let a = alpha_at(x, y);
                        let v = if erase { 0.0 } else { luma };
                        p[0] += (v - p[0]) * a;
                    })?,
                    dirty: rect,
                }
            }
        };
        if prelude.is_empty() {
            Ok(op)
        } else {
            prelude.push(op);
            Ok(DocOp::Batch(prelude))
        }
    }

    /// The new selection (`None` = no selection).
    fn selection(
        &self,
        session: &DocumentSession,
        shape: &SelectionShape,
        mode: SelectionMode,
        feather: f32,
    ) -> EngineResult<Option<Raster>> {
        let state = session.state();
        let canvas = state.canvas;
        if !(feather.is_finite() && (0.0..=1000.0).contains(&feather)) {
            return Err(EngineError::invalid("feather", "must be within 0..=1000"));
        }
        let empty = || Raster::new(canvas, 1, compositor::Depth::F32, 0.0);
        let full = || Raster::new(canvas, 1, compositor::Depth::F32, 1.0);
        let check_rect = |r: &CanvasRect| {
            if r.is_empty() {
                Err(EngineError::invalid("rect", "must not be empty"))
            } else {
                Ok(())
            }
        };
        let shape_mask = match shape {
            SelectionShape::None => return Ok(None),
            SelectionShape::Inverse => {
                let current = state.selection.as_deref().cloned().unwrap_or_else(empty);
                return Ok(Some(compositor::document::selection::combine(
                    &full(),
                    &current,
                    compositor::document::selection::Combine::Subtract,
                )?));
            }
            SelectionShape::All => full(),
            SelectionShape::Rect { rect } => {
                check_rect(rect)?;
                self.selection
                    .rasterize(&SelectionGeometry::Rect(*rect), canvas)?
            }
            SelectionShape::Ellipse { rect } => {
                check_rect(rect)?;
                self.selection
                    .rasterize(&SelectionGeometry::Ellipse(*rect), canvas)?
            }
            SelectionShape::Polygon { points } => self
                .selection
                .rasterize(&SelectionGeometry::Polygon(points.clone()), canvas)?,
            SelectionShape::LayerAlpha { layer } => {
                let l = session.layer(*layer)?;
                let r = l.raster().ok_or_else(|| {
                    EngineError::invalid("layer", "only pixel layers have transparency")
                })?;
                let d = dense::read(r, Rect::of_extent(canvas))?;
                let px = d.px.iter().map(|p| [p[3], 0.0, 0.0, 0.0]).collect();
                dense::mask_raster(
                    canvas,
                    compositor::Depth::F32,
                    0.0,
                    &dense::Dense { rect: d.rect, px },
                )?
            }
            SelectionShape::Saved { selection } => session
                .saved_selections()
                .iter()
                .find(|s| s.id == *selection)
                .map(|s| (*s.mask).clone())
                .ok_or_else(|| EngineError::not_found("selection", selection))?,
        };
        let shape_mask = if feather > 0.0 {
            self.selection.feather(&shape_mask, feather)?
        } else {
            shape_mask
        };
        if shape_mask.extent() != canvas || shape_mask.channels() != 1 {
            return Err(EngineError::internal(
                "selection engine returned a wrong raster",
            ));
        }
        use compositor::document::selection::{Combine, combine};
        let current = || state.selection.as_deref().cloned().unwrap_or_else(empty);
        Ok(Some(match mode {
            SelectionMode::Replace => shape_mask,
            SelectionMode::Add => combine(&current(), &shape_mask, Combine::Add)?,
            SelectionMode::Subtract => combine(&current(), &shape_mask, Combine::Subtract)?,
            SelectionMode::Intersect => combine(&current(), &shape_mask, Combine::Intersect)?,
        }))
    }

    /// Engine names in use (brush, selection).
    pub fn engines(&self) -> (&str, &str) {
        (self.brush.name(), self.selection.name())
    }
}

fn fmt_head(h: Option<HistoryEntryId>) -> String {
    h.map_or_else(|| "the opened state".into(), |id| id.to_string())
}

/// Index in `parent`'s children for a layer inserted directly above
/// `above` (or on top).
fn insert_index(
    state: &DocState,
    parent: Option<LayerId>,
    above: Option<LayerId>,
) -> EngineResult<usize> {
    let children: &[Arc<Layer>] = match parent {
        None => &state.root,
        Some(p) => {
            let l = state
                .find(p)
                .ok_or_else(|| EngineError::not_found("layer", p))?;
            l.children()
                .ok_or_else(|| EngineError::invalid("parent", "not a group"))?
        }
    };
    match above {
        None => Ok(children.len()),
        Some(a) => children
            .iter()
            .position(|c| c.id == a)
            .map(|i| i + 1)
            .ok_or_else(|| {
                EngineError::invalid("above", format!("{a} is not a child of the parent"))
            }),
    }
}

fn apply_props(mut p: LayerProps, u: &LayerPropsUpdate) -> EngineResult<LayerProps> {
    if let Some(v) = &u.name {
        p.name = v.clone();
    }
    if let Some(v) = u.visible {
        p.visible = v;
    }
    if let Some(v) = u.opacity {
        unit("opacity", v)?;
        p.opacity = v;
    }
    if let Some(v) = u.fill_opacity {
        unit("fill_opacity", v)?;
        p.fill_opacity = v;
    }
    if let Some(v) = &u.blend_mode {
        p.blend_mode = convert::<_, BlendMode>("blend_mode", v)?;
    }
    if let Some(v) = u.clipped {
        p.clipped = v;
    }
    match &u.color_tag {
        Some(FieldUpdate::Set(t)) => p.color_tag = Some(t.clone()),
        Some(FieldUpdate::Clear) => p.color_tag = None,
        None => {}
    }
    Ok(p)
}

fn validate_adjustment(a: &AdjustmentSpec) -> EngineResult<()> {
    let finite = |name: &str, v: f32| {
        if v.is_finite() {
            Ok(())
        } else {
            Err(EngineError::invalid(name, "must be finite"))
        }
    };
    match a {
        AdjustmentSpec::Levels { master, rgb } => {
            for c in std::iter::once(master).chain(rgb) {
                for v in [c.in_black, c.in_white, c.out_black, c.out_white] {
                    unit("levels", v)?;
                }
                if !(c.gamma.is_finite() && c.gamma > 0.0) || c.in_white <= c.in_black {
                    return Err(EngineError::invalid(
                        "levels",
                        "gamma must be positive and in_white above in_black",
                    ));
                }
            }
        }
        AdjustmentSpec::Curves { master, rgb } => {
            for curve in std::iter::once(master).chain(rgb) {
                for p in curve {
                    unit("curves", p[0])?;
                    unit("curves", p[1])?;
                }
            }
        }
        AdjustmentSpec::HueSaturation {
            hue,
            saturation,
            lightness,
            ..
        } => {
            if !(-180.0..=180.0).contains(hue)
                || !(-100.0..=100.0).contains(saturation)
                || !(-100.0..=100.0).contains(lightness)
            {
                return Err(EngineError::invalid(
                    "hue_saturation",
                    "hue −180..=180, saturation and lightness −100..=100",
                ));
            }
        }
        AdjustmentSpec::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            finite("exposure", *exposure)?;
            finite("offset", *offset)?;
            if !(gamma.is_finite() && *gamma > 0.0) {
                return Err(EngineError::invalid("gamma", "must be positive"));
            }
        }
        AdjustmentSpec::Invert => {}
        AdjustmentSpec::Posterize { levels } => {
            if !(2..=255).contains(levels) {
                return Err(EngineError::invalid("levels", "must be within 2..=255"));
            }
        }
        AdjustmentSpec::Threshold { level } => unit("level", *level)?,
        AdjustmentSpec::ChannelMixer {
            matrix, constant, ..
        } => {
            for v in matrix.iter().flatten().chain(constant) {
                finite("channel_mixer", *v)?;
            }
        }
    }
    Ok(())
}

fn adjustment_name(a: &AdjustmentSpec) -> &'static str {
    match a {
        AdjustmentSpec::Levels { .. } => "Levels",
        AdjustmentSpec::Curves { .. } => "Curves",
        AdjustmentSpec::HueSaturation { .. } => "Hue/Saturation",
        AdjustmentSpec::Exposure { .. } => "Exposure",
        AdjustmentSpec::Invert => "Invert",
        AdjustmentSpec::Posterize { .. } => "Posterize",
        AdjustmentSpec::Threshold { .. } => "Threshold",
        AdjustmentSpec::ChannelMixer { .. } => "Channel Mixer",
    }
}

/// Merges `upper` into the pixel layer directly below it: the pair is
/// composited in isolation (the lower layer at full opacity, Normal, no
/// mask; the upper layer with all its properties) and the result replaces
/// the lower layer's pixels. The lower layer keeps its own properties and
/// mask; the upper layer is removed.
fn merge_down(state: &DocState, upper: LayerId) -> EngineResult<(DocOp, LayerId)> {
    let (parent, index) = state
        .locate(upper)
        .ok_or_else(|| EngineError::not_found("layer", upper))?;
    if index == 0 {
        return Err(EngineError::invalid("layer", "there is no layer below"));
    }
    let siblings = match parent {
        None => &state.root,
        Some(p) => state
            .find(p)
            .and_then(|g| g.children())
            .ok_or_else(|| EngineError::internal("parent is not a group"))?,
    };
    let (up, low) = (&siblings[index], &siblings[index - 1]);
    if !up.props.visible {
        return Err(EngineError::invalid("layer", "cannot merge a hidden layer"));
    }
    let LayerKind::Pixel(lower_raster) = &low.kind else {
        return Err(crate::unsupported(
            "merge_down needs a pixel layer below (rasterize it first)",
        ));
    };
    let mut base = (**low).clone();
    base.props = LayerProps {
        name: base.props.name.clone(),
        ..LayerProps::default()
    };
    base.mask = None;
    // A clipped upper layer clips to the lower layer's pixels in the pair.
    let top = (**up).clone();
    let mut pair = DocState::new(state.canvas, state.depth);
    pair.profile = state.profile.clone();
    let mut doc = Document::new(pair);
    for layer in [base, top] {
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer,
        })?;
    }
    let (e, rgba) = compositor::Compositor::new(64 << 20).render_level_rgba(&doc, 0)?;
    let full = Rect::of_extent(state.canvas);
    let region = match (lower_raster.bounds(), up.affected_bounds()) {
        (Some(a), Some(b)) => a.union(&b).intersect(&full),
        (None, Some(b)) => b.intersect(&full),
        _ => full,
    };
    let w = e.width as usize;
    let tiles = dense::deltas(lower_raster, region, |x, y, p| {
        let i = (y as usize * w + x as usize) * 4;
        p.copy_from_slice(&rgba[i..i + 4]);
    })?;
    Ok((
        DocOp::Batch(vec![
            DocOp::PaintTiles {
                id: low.id,
                target: PaintTarget::Content,
                tiles,
                dirty: region,
            },
            DocOp::RemoveLayer { id: upper },
        ]),
        low.id,
    ))
}

fn kind_tag(l: &Layer) -> LayerKindTag {
    match l.kind {
        LayerKind::Pixel(_) => LayerKindTag::Pixel,
        LayerKind::Adjustment(_) => LayerKindTag::Adjustment,
        LayerKind::Fill(_) => LayerKindTag::Fill,
        LayerKind::Group { .. } => LayerKindTag::Group,
        LayerKind::SmartObject(_) => LayerKindTag::SmartObject,
        LayerKind::Text(_) => LayerKindTag::Text,
    }
}

fn canvas_rect(r: Rect) -> CanvasRect {
    CanvasRect {
        x0: r.x0,
        y0: r.y0,
        x1: r.x1,
        y1: r.y1,
    }
}

fn layer_bounds(l: &Layer) -> EngineResult<Option<Rect>> {
    Ok(match &l.kind {
        LayerKind::Pixel(r) => dense::content_bounds(r, 3)?,
        LayerKind::Text(t) => dense::content_bounds(&t.proxy, 3)?,
        LayerKind::SmartObject(so) => Some(so.bounds()),
        LayerKind::Group { children, .. } => {
            let mut out: Option<Rect> = None;
            for c in children {
                if matches!(c.kind, LayerKind::Adjustment(_) | LayerKind::Fill(_)) {
                    return Ok(None);
                }
                if let Some(b) = layer_bounds(c)? {
                    out = Some(out.map_or(b, |o| o.union(&b)));
                }
            }
            out
        }
        LayerKind::Adjustment(_) | LayerKind::Fill(_) => None,
    })
}

/// `list_layers` rows: bottom to top, every group before its children.
pub(crate) fn list_layers(state: &DocState) -> EngineResult<Vec<LayerInfo>> {
    fn go(v: &[Arc<Layer>], parent: Option<LayerId>, out: &mut Vec<LayerInfo>) -> EngineResult<()> {
        for l in v {
            out.push(LayerInfo {
                id: l.id,
                parent,
                name: l.props.name.clone(),
                kind: kind_tag(l),
                visible: l.props.visible,
                opacity: l.props.opacity,
                fill_opacity: l.props.fill_opacity,
                blend_mode: convert("blend_mode", &l.props.blend_mode)?,
                group_mode: match &l.kind {
                    LayerKind::Group { mode, .. } => Some(convert("mode", mode)?),
                    _ => None,
                },
                clipped: l.props.clipped,
                has_mask: l.mask.is_some(),
                bounds: layer_bounds(l)?.map(canvas_rect),
            });
            if let Some(c) = l.children() {
                go(c, Some(l.id), out)?;
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    go(&state.root, None, &mut out)?;
    Ok(out)
}

pub(crate) fn summary(state: &DocState) -> EngineResult<DocumentSummary> {
    Ok(DocumentSummary {
        canvas: state.canvas,
        depth: convert("depth", &state.depth)?,
        layers: state.layer_ids().len() as u32,
    })
}
