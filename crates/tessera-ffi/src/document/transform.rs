//! Non-destructive Warp, Perspective Warp, Puppet Warp and Content-Aware
//! Scale (WP B5-12): editable `transform` stages of a smart object's filter
//! stack (`compositor::DocOp::AddTransform` / `SetTransform` carrying a
//! versioned `transform::TransformOp`).
//!
//! # Contract
//!
//! * Geometry is in the smart object's **child** level-0 pixels (pixel edges;
//!   the first pixel centre is (0.5, 0.5)). `child_to_document` (row-major
//!   `[a, b, c, d, e, f]`, `(a·x + b·y + c, d·x + e·y + f)`) places the child
//!   in the document. A pixel, text, shape, fill or group layer is wrapped at
//!   identity, so its child coordinates are document coordinates.
//! * A session is `begin_advanced_transform` → any number of
//!   `preview_advanced_transform` (scratch only, recorded nowhere) →
//!   `commit_advanced_transform` (ONE history node) or
//!   `cancel_advanced_transform` (no history change). Every call carries the
//!   session token; a token of an ended or replaced session, or of a layer
//!   changed since `begin` (undo, another edit), is rejected with no change.
//! * Existing smart objects get `AddTransform` (new stage) or `SetTransform`
//!   (re-edit: the stage keeps its enabled state, blending and stack index,
//!   neighbours untouched). Any other layer needs explicit consent on
//!   commit (`convert_to_smart_object: true`): wrapper + stage are one
//!   `Batch` (one node); without consent the commit fails and the session
//!   stays open. Cancel never creates a smart object. The wrapper keeps the
//!   layer's id, properties, styles, raster and vector masks exactly once;
//!   text and shapes stay live inside it (never flattened).
//! * Position / all locks reject geometry; converting also respects the
//!   pixel lock.
//! * Draft previews (`draft: true`) of large children render the stage on a
//!   reduced-resolution proxy of the child (geometry scaled by 2^-level), so
//!   a drag stays interactive; final previews and the commit are exact.
//! * Content-aware scale has no geometry field: it is evaluated on the CPU
//!   (a filter-worker bake in this app, off the session lock). Protection is
//!   an explicitly chosen saved alpha channel sampled onto the evaluation
//!   grid (the proxy grid for drafts, the child grid otherwise); there is no
//!   automatic skin detection. Protect masks stay Rust-side: records and
//!   JSON returned to the host strip them.
//! * Nonlinear stages are native-only: saving a PSD refuses them (compositor
//!   `to_psd`); `save_psd_rasterizing_transforms` is the explicit rasterized
//!   copy.

use super::{
    DocLayerKind, DocRect, DocumentSession, DocumentUpdate, State, TransformMatrix, blend_name,
    filtering, find, kind_of, layer_revision, raster_from_rgba,
};
use crate::{Result, failure};
use compositor::{
    Affine, DocOp, DocState, Document, Layer, LayerId, LayerKind, Rect, SmartFilter, SmartObject,
};
use std::sync::Arc;
use transform::{
    Operation, TransformOp,
    puppet::{PuppetDensity, PuppetWarp},
    seam::ContentAwareScale,
    warp::{WarpMesh, WarpPreset},
};

/// Children larger than this (level-0 pixels) get a reduced proxy for drafts.
const DRAFT_MAX_PIXELS: u64 = 1_000_000;
/// Largest child a full-resolution protect mask is built for: the engine
/// stores protect masks inline in the stage (NEEDS.md).
const PROTECT_MAX_PIXELS: u64 = 4_194_304;

// ─────────────────────────────── records ───────────────────────────────

/// The operation of a transform stage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AdvancedTransformKind {
    Warp,
    Perspective,
    Puppet,
    ContentAwareScale,
    /// A projective Free Transform stage (listed, not edited here).
    Free,
    /// A displacement field (Adaptive Wide Angle; listed, not edited here).
    Displacement,
}

impl AdvancedTransformKind {
    fn of(op: &Operation) -> Self {
        match op {
            Operation::Warp(_) => Self::Warp,
            Operation::Perspective(_) => Self::Perspective,
            Operation::Puppet(_) => Self::Puppet,
            Operation::ContentAwareScale(_) => Self::ContentAwareScale,
            Operation::Free(_) => Self::Free,
            Operation::Displacement(_) => Self::Displacement,
        }
    }

    /// History label.
    fn label(self) -> &'static str {
        match self {
            Self::Warp => "Warp",
            Self::Perspective => "Perspective Warp",
            Self::Puppet => "Puppet Warp",
            Self::ContentAwareScale => "Content-Aware Scale",
            Self::Free => "Transform",
            Self::Displacement => "Displacement",
        }
    }

    fn editable(self) -> bool {
        matches!(
            self,
            Self::Warp | Self::Perspective | Self::Puppet | Self::ContentAwareScale
        )
    }
}

/// One `transform` stage of a smart object's stack.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TransformStageRecord {
    pub layer: u64,
    /// Index in the smart filter stack (input first).
    pub index: u32,
    pub kind: AdvancedTransformKind,
    /// `transform::TransformOp` JSON; a content-aware protect mask is
    /// replaced by `null` (see `has_protection`).
    pub transform_json: String,
    pub has_protection: bool,
    pub enabled: bool,
    pub blend_mode: String,
    pub opacity: f32,
}

/// What `begin_advanced_transform` captured.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdvancedTransformInfo {
    pub token: u64,
    pub layer: u64,
    pub layer_kind: DocLayerKind,
    pub kind: AdvancedTransformKind,
    /// The stage being re-edited, or `None` for a new stage.
    pub editing_index: Option<u32>,
    /// Where a new stage goes (top of the stack).
    pub insert_index: u32,
    /// Apply wraps the layer in a smart object (needs the user's consent).
    pub needs_conversion: bool,
    /// Fixed child canvas: stage output is clipped to it.
    pub child_width: u32,
    pub child_height: u32,
    pub child_to_document: TransformMatrix,
    /// Content bounds in child pixels (`None`: empty).
    pub content_bounds: Option<DocRect>,
    pub existing: Option<TransformStageRecord>,
    /// Committed content revision captured at `begin`.
    pub revision: u64,
    /// Pyramid level of the draft proxy (0: drafts are exact).
    pub draft_level: u8,
    /// Enabled stages of the stack other than this one.
    pub other_stages: u32,
    pub limitations: Vec<String>,
}

/// A preview's frame request plus, for puppet warp, the solved mesh.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdvancedTransformPreview {
    pub update: DocumentUpdate,
    /// Puppet: deformed vertex positions `[[x, y], …]` in child pixels.
    pub deformed_json: Option<String>,
}

/// `puppet_mesh_from_layer`: a pin-less `transform::puppet::PuppetWarp`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PuppetMeshRecord {
    /// `PuppetWarp` JSON, rest vertices in child pixels.
    pub mesh_json: String,
    pub vertex_count: u32,
    pub triangle_count: u32,
    /// Grid cell size in child pixels (8 / 4 / 2 × 2^level).
    pub cell_px: u32,
    /// Level of the alpha the mesh was built from (> 0: coarsened to stay
    /// within the engine's 16,384-vertex limit).
    pub level: u8,
    pub note: Option<String>,
}

// ─────────────────────────────── session state ───────────────────────────────

/// Session-local transform state (one active session per document).
#[derive(Default)]
pub(crate) struct AdvancedState {
    next_token: u64,
    active: Option<Active>,
}

#[derive(Clone)]
enum Protection {
    None,
    /// The protect mask of the stage being re-edited (child grid).
    Inherited(Arc<Vec<f32>>),
    /// A saved alpha channel, sampled on demand onto the evaluation grid.
    Channel(u64),
}

#[derive(Clone)]
struct Proxy {
    level: u8,
    /// Smart object over the child rendered at `level` (one cache key).
    so: SmartObject,
}

#[derive(Clone)]
struct Active {
    token: u64,
    layer: u64,
    kind: AdvancedTransformKind,
    editing: Option<usize>,
    insert: usize,
    /// Pixel / text / shape / fill / group target: its identity wrapper.
    wrapper: Option<Layer>,
    child: Arc<DocState>,
    placement: Affine,
    revision: u64,
    proxy: Option<Proxy>,
    /// Blend and enabled state of a re-edited stage (drafts show it).
    stage_template: SmartFilter,
    /// The last previewed operation (what commit records).
    last: Option<TransformOp>,
    protection: Protection,
}

fn matrix(a: &Affine) -> TransformMatrix {
    let m = a.m;
    TransformMatrix {
        a: m[0],
        b: m[1],
        c: m[2],
        d: m[3],
        e: m[4],
        f: m[5],
    }
}

fn stage_record(layer: u64, index: usize, f: &SmartFilter) -> Result<TransformStageRecord> {
    let op = f
        .transform_op()?
        .ok_or_else(|| failure(format!("smart filter {index} is not a transform stage")))?;
    Ok(TransformStageRecord {
        layer,
        index: index as u32,
        kind: AdvancedTransformKind::of(&op.operation),
        transform_json: filtering::stripped_transform(&f.params).to_string(),
        has_protection: filtering::transform_protect(&f.params).is_some(),
        enabled: f.enabled,
        blend_mode: blend_name(f.blend.mode),
        opacity: f.blend.opacity,
    })
}

fn locked(l: &Layer) -> Result<()> {
    if l.props.locks.all || l.props.locks.position {
        return Err(failure(format!(
            "{:?} is locked: position and all locks reject transforms",
            l.props.name
        )));
    }
    Ok(())
}

/// The child document and its placement: the smart object's own, or the
/// identity wrapper a conversion would create.
fn source_of(s: &DocState, l: &Layer) -> Result<(Arc<DocState>, Affine, Option<Layer>)> {
    match &l.kind {
        LayerKind::SmartObject(so) => Ok((so.state.clone(), so.transform, None)),
        LayerKind::Adjustment(_) => Err(failure(
            "adjustment layers have no pixels to transform; select a pixel layer or smart object",
        )),
        _ => {
            let wrapper = filtering::smart_wrapper(s, l)?;
            let LayerKind::SmartObject(so) = &wrapper.kind else {
                return Err(failure("wrapper is not a smart object"));
            };
            Ok((so.state.clone(), so.transform, Some(wrapper)))
        }
    }
}

/// Straight RGBA of `child` at `level`.
fn render_child(child: &DocState, level: u8) -> Result<(engine_api::tile::Extent, Vec<f32>)> {
    let comp = super::fonts::compositor(64 << 20); // B5-10b: shared fonts
    Ok(comp.render_level_rgba(&Document::new(child.clone()), level)?)
}

fn proxy_for(child: &DocState, placement: &Affine) -> Result<Option<Proxy>> {
    let canvas = child.canvas;
    if canvas.area() <= DRAFT_MAX_PIXELS {
        return Ok(None);
    }
    let level = (1..8u8)
        .find(|&l| canvas.at_level(l).area() <= DRAFT_MAX_PIXELS)
        .unwrap_or(7);
    let (e, px) = render_child(child, level)?;
    let raster = raster_from_rgba(e, compositor::Depth::F32, &px, true)?;
    let mut state = DocState::new(e, compositor::Depth::F32);
    state.profile = child.profile.clone();
    let mut l = Layer::new("draft", LayerKind::Pixel(raster));
    l.id = LayerId(1);
    state.next_id = 2;
    state.root = vec![Arc::new(l)];
    let s = f64::from(1u32 << level);
    let m = placement.m;
    let scaled = Affine {
        m: [m[0] * s, m[1] * s, m[2], m[3] * s, m[4] * s, m[5]],
    };
    Ok(Some(Proxy {
        level,
        so: SmartObject::new(state, scaled),
    }))
}

/// `op` in the coordinates of a child scaled by `2^-level`.
fn scaled_op(op: &TransformOp, level: u8) -> Result<TransformOp> {
    let s = f64::from(1u32 << level);
    let k = 1.0 / s;
    let operation = match &op.operation {
        Operation::Warp(w) => {
            let mut w = w.clone();
            w.width *= k;
            w.height *= k;
            for row in &mut w.control_points {
                for p in row {
                    p[0] *= k;
                    p[1] *= k;
                }
            }
            Operation::Warp(w)
        }
        Operation::Perspective(p) => {
            let mut p = p.clone();
            for q in p
                .source_quads
                .iter_mut()
                .chain(p.destination_quads.iter_mut())
            {
                for v in q {
                    v[0] *= k;
                    v[1] *= k;
                }
            }
            Operation::Perspective(p)
        }
        Operation::Puppet(p) => {
            let mut p = p.clone();
            for v in &mut p.rest_vertices {
                v[0] *= k;
                v[1] *= k;
            }
            for pin in &mut p.pins {
                pin.target[0] *= k;
                pin.target[1] *= k;
            }
            Operation::Puppet(p)
        }
        Operation::ContentAwareScale(c) => {
            let mut c = c.clone();
            c.target_width = ((c.target_width as f64) * k).ceil().max(1.0) as usize;
            c.target_height = ((c.target_height as f64) * k).ceil().max(1.0) as usize;
            Operation::ContentAwareScale(c)
        }
        Operation::Free(f) => {
            let mut f = f.clone();
            for (i, row) in f.matrix.iter_mut().enumerate() {
                for (j, v) in row.iter_mut().enumerate() {
                    let si = if i < 2 { s } else { 1.0 };
                    let sj = if j < 2 { s } else { 1.0 };
                    *v *= sj / si;
                }
            }
            Operation::Free(f)
        }
        Operation::Displacement(_) => {
            return Err(failure("displacement stages have no draft proxy"));
        }
    };
    Ok(TransformOp {
        version: op.version,
        operation,
        kernel: op.kernel,
    })
}

/// The channel sampled onto a `w × h` child grid at `level` (pixel centres
/// mapped through the placement into the document), quantized to 1 %.
fn channel_protect(
    s: &DocState,
    channel: u64,
    placement: &Affine,
    level: u8,
    w: usize,
    h: usize,
) -> Result<Vec<f32>> {
    let c = s
        .channels
        .iter()
        .find(|c| c.id.0 == channel)
        .ok_or_else(|| failure(format!("channel {channel} not found")))?;
    let scale = f64::from(1u32 << level);
    let (cw, ch) = (c.raster.extent().width, c.raster.extent().height);
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let (dx, dy) = placement.apply((x as f64 + 0.5) * scale, (y as f64 + 0.5) * scale);
            if dx >= 0.0 && dy >= 0.0 && (dx as u32) < cw && (dy as u32) < ch {
                let v = c.raster.pixel(dx as u32, dy as u32)[0].clamp(0.0, 1.0);
                out[y * w + x] = (v * 100.0).round() / 100.0;
            }
        }
    }
    Ok(out)
}

/// Nearest resample of a child-grid mask onto the `w × h` grid of `level`.
fn resample_protect(full: &[f32], w0: usize, h0: usize, level: u8, w: usize, h: usize) -> Vec<f32> {
    let s = 1usize << level;
    let mut out = vec![0.0f32; w * h];
    for y in 0..h {
        let sy = (y * s + s / 2).min(h0 - 1);
        for x in 0..w {
            let sx = (x * s + s / 2).min(w0 - 1);
            out[y * w + x] = full[sy * w0 + sx];
        }
    }
    out
}

/// `op` with its protect mask on the grid of `level` (CAS only).
fn with_protection(
    s: &DocState,
    a: &Active,
    mut op: TransformOp,
    level: u8,
) -> Result<TransformOp> {
    let Operation::ContentAwareScale(c) = &mut op.operation else {
        return Ok(op);
    };
    if c.protect.is_some() {
        if level != 0 {
            return Err(failure(
                "explicit protect masks are child-sized; choose a channel for drafts",
            ));
        }
        return Ok(op);
    }
    let (w0, h0) = (
        a.child.canvas.width as usize,
        a.child.canvas.height as usize,
    );
    let e = a.child.canvas.at_level(level);
    let (w, h) = (e.width as usize, e.height as usize);
    c.protect = match &a.protection {
        Protection::None => None,
        Protection::Inherited(v) if level == 0 => Some((**v).clone()),
        Protection::Inherited(v) => Some(resample_protect(v, w0, h0, level, w, h)),
        Protection::Channel(id) => {
            if level == 0 && (w * h) as u64 > PROTECT_MAX_PIXELS {
                return Err(failure(format!(
                    "a protection channel needs a child of at most {} MP in this build (this one is {:.1} MP); \
                     the engine stores protect masks inline (NEEDS.md)",
                    PROTECT_MAX_PIXELS / 1_000_000,
                    (w * h) as f64 / 1e6
                )));
            }
            Some(channel_protect(s, *id, &a.placement, level, w, h)?)
        }
    };
    Ok(op)
}

fn validate(op: &TransformOp, kind: AdvancedTransformKind) -> Result<()> {
    op.validate()
        .map_err(|e| failure(format!("{}: {e}", kind.label())))?;
    let got = AdvancedTransformKind::of(&op.operation);
    if got != kind {
        return Err(failure(format!(
            "this {} session cannot preview a {} operation",
            kind.label(),
            got.label()
        )));
    }
    Ok(())
}

/// The session behind `token`, provided its layer is unchanged since `begin`.
fn checked(st: &State, token: u64) -> Result<&Active> {
    let a = st
        .advanced
        .active
        .as_ref()
        .filter(|a| a.token == token)
        .ok_or_else(|| {
            failure("stale transform session (ended or replaced): begin the transform again")
        })?;
    let l = find(st.doc.state(), a.layer)
        .map_err(|_| failure("the layer of this transform no longer exists"))?;
    let smart = matches!(l.kind, LayerKind::SmartObject(_));
    if layer_revision(l) != a.revision || smart == a.wrapper.is_some() {
        return Err(failure(
            "the layer changed since the transform began (another edit or undo): begin the transform again",
        ));
    }
    Ok(a)
}

/// The exact op recording the stage (plus the wrapper, as one Batch).
fn stage_op(s: &DocState, a: &Active, op: TransformOp) -> Result<DocOp> {
    let id = LayerId(a.layer);
    if let Some(w) = &a.wrapper {
        let (parent, index) = s.locate(id).ok_or_else(|| failure("layer not found"))?;
        return Ok(DocOp::Batch(vec![
            DocOp::RemoveLayer { id },
            DocOp::AddLayer {
                parent,
                index,
                layer: w.clone(),
            },
            DocOp::AddTransform {
                id,
                index: 0,
                transform: op,
            },
        ]));
    }
    Ok(match a.editing {
        Some(index) => DocOp::SetTransform {
            id,
            index,
            transform: op,
        },
        None => DocOp::AddTransform {
            id,
            index: a.insert,
            transform: op,
        },
    })
}

/// A draft: the layer shown as the proxy smart object with the scaled stage.
fn draft_op(s: &DocState, a: &Active, proxy: &Proxy, op: TransformOp) -> Result<DocOp> {
    let id = LayerId(a.layer);
    let l = find(s, a.layer)?;
    let (parent, index) = s.locate(id).ok_or_else(|| failure("layer not found"))?;
    let mut so = proxy.so.clone();
    let mut stage = SmartFilter::transform(op)?;
    stage.enabled = a.stage_template.enabled;
    stage.blend = a.stage_template.blend;
    so.filters = vec![stage];
    let mut nl = a.wrapper.clone().unwrap_or_else(|| l.clone());
    nl.kind = LayerKind::SmartObject(so);
    Ok(DocOp::Batch(vec![
        DocOp::RemoveLayer { id },
        DocOp::AddLayer {
            parent,
            index,
            layer: nl,
        },
    ]))
}

/// Drops a transform preview: the viewport returns to the committed document
/// plus any other pending drag.
fn drop_preview(st: &mut State) {
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
}

fn parse<T: serde::de::DeserializeOwned>(what: &str, json: &str) -> Result<T> {
    serde_json::from_str(json).map_err(|e| failure(format!("{what} JSON: {e}")))
}

fn warp_preset_of(name: &str) -> Result<WarpPreset> {
    serde_json::from_value(serde_json::Value::String(name.to_owned()))
        .map_err(|_| failure(format!("unknown warp preset {name:?}")))
}

fn density_of(name: &str) -> Result<PuppetDensity> {
    serde_json::from_value(serde_json::Value::String(name.to_owned())).map_err(|_| {
        failure(format!(
            "puppet density must be Sparse, Normal or Dense, not {name:?}"
        ))
    })
}

// ─────────────────────────────── free functions ───────────────────────────────

/// The warp presets (`Arc`, `ArcLower`, … `Twist`), menu order.
#[uniffi::export]
pub fn warp_preset_names() -> Vec<String> {
    [
        "Arc", "ArcLower", "ArcUpper", "Arch", "Bulge", "Shell", "Flag", "Wave", "Fish", "Rise",
        "Fisheye", "Inflate", "Squeeze", "Twist",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// A preset warp mesh (`transform::warp::WarpMesh` JSON) of a `width ×
/// height` child. Bend is signed [-1, 1]; zero is the identity mesh.
#[uniffi::export]
pub fn warp_preset(width: f64, height: f64, preset: String, bend: f64) -> Result<String> {
    let mesh = WarpMesh::preset(width, height, warp_preset_of(&preset)?, bend)
        .map_err(|e| failure(format!("Warp: {e}")))?;
    serde_json::to_string(&mesh).map_err(failure)
}

/// Splits the warp surface through child point `(x, y)` — a vertical split
/// (`split_u`), a horizontal one (`split_v`) or both — using exact de
/// Casteljau subdivision, so the warped image does not move. A point outside
/// the mesh, or on an existing split, fails with the mesh unchanged.
#[uniffi::export]
pub fn warp_split(
    mesh_json: String,
    x: f64,
    y: f64,
    split_u: bool,
    split_v: bool,
) -> Result<String> {
    let mesh: WarpMesh = parse("warp mesh", &mesh_json)?;
    mesh.validate().map_err(|e| failure(format!("Warp: {e}")))?;
    let uv = mesh
        .inverse([x, y])
        .ok_or_else(|| failure("the split point is outside the warp"))?;
    let mut next = mesh.clone();
    if split_u {
        next.split_u(uv[0])
            .map_err(|e| failure(format!("Warp: {e}")))?;
    }
    if split_v {
        next.split_v(uv[1])
            .map_err(|e| failure(format!("Warp: {e}")))?;
    }
    serde_json::to_string(&next).map_err(failure)
}

/// A regular `columns × rows` grid on the warp (existing splits kept, the
/// surface unchanged).
#[uniffi::export]
pub fn warp_subdivide(mesh_json: String, columns: u32, rows: u32) -> Result<String> {
    let mut mesh: WarpMesh = parse("warp mesh", &mesh_json)?;
    mesh.subdivide(columns as usize, rows as usize)
        .map_err(|e| failure(format!("Warp: {e}")))?;
    serde_json::to_string(&mesh).map_err(failure)
}

// ─────────────────────────────── session calls ───────────────────────────────

#[uniffi::export]
impl DocumentSession {
    /// Every transform stage of `layer` (none for other layer kinds).
    pub fn transform_stages(&self, layer: u64) -> Result<Vec<TransformStageRecord>> {
        let st = self.shared.lock()?;
        let l = find(st.doc.state(), layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Ok(Vec::new());
        };
        so.filters
            .iter()
            .enumerate()
            .filter(|(_, f)| f.name == filtering::TRANSFORM_STAGE)
            .map(|(i, f)| stage_record(layer, i, f))
            .collect()
    }

    /// Transform stage `index` of smart object `layer` (committed state).
    pub fn transform_stage(&self, layer: u64, index: u32) -> Result<TransformStageRecord> {
        let st = self.shared.lock()?;
        let l = find(st.doc.state(), layer)?;
        let LayerKind::SmartObject(so) = &l.kind else {
            return Err(failure(format!("layer {layer} is not a smart object")));
        };
        let f = so
            .filters
            .get(index as usize)
            .ok_or_else(|| failure(format!("no smart filter {index}")))?;
        stage_record(layer, index as usize, f)
    }

    /// Starts a Warp / Perspective / Puppet / Content-Aware Scale session on
    /// `layer`: re-editing stage `index`, or (`None`) a new stage on top of
    /// the stack. Commits another control's pending drag and ends an older
    /// transform session (its preview dropped). Locks and layer kinds are
    /// checked here; nothing changes on error.
    pub fn begin_advanced_transform(
        &self,
        layer: u64,
        index: Option<u32>,
        kind: AdvancedTransformKind,
    ) -> Result<AdvancedTransformInfo> {
        if !kind.editable() {
            return Err(failure(format!(
                "{} stages are not edited here",
                kind.label()
            )));
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let had_preview = st.advanced.active.take().is_some();
        if had_preview {
            drop_preview(&mut st);
        }
        self.commit_pending(&mut st, None)?;
        let s = st.doc.state().clone();
        let l = find(&s, layer)?;
        locked(l)?;
        let (child, placement, wrapper) = source_of(&s, l)?;
        if wrapper.is_some() {
            if index.is_some() {
                return Err(failure("only smart objects have transform stages"));
            }
            if l.props.locks.pixels {
                return Err(failure(format!(
                    "{:?} has locked pixels: converting it to a smart object is a content edit",
                    l.props.name
                )));
            }
        }
        let (editing, insert, existing, template, protection, other_stages) = match &l.kind {
            LayerKind::SmartObject(so) => {
                let editing = index.map(|i| i as usize);
                let (existing, template, protection) = match editing {
                    Some(i) => {
                        let f = so
                            .filters
                            .get(i)
                            .filter(|f| f.name == filtering::TRANSFORM_STAGE)
                            .ok_or_else(|| {
                                failure(format!("smart filter {i} is not a transform stage"))
                            })?;
                        let op = f.transform_op()?.expect("transform stage");
                        let got = AdvancedTransformKind::of(&op.operation);
                        if got != kind {
                            return Err(failure(format!(
                                "stage {i} is a {} stage, not {}",
                                got.label(),
                                kind.label()
                            )));
                        }
                        let protection = match op.operation {
                            Operation::ContentAwareScale(ContentAwareScale {
                                protect: Some(p),
                                ..
                            }) => Protection::Inherited(Arc::new(p)),
                            _ => Protection::None,
                        };
                        (Some(stage_record(layer, i, f)?), f.clone(), protection)
                    }
                    None => (None, SmartFilter::default(), Protection::None),
                };
                let others = so
                    .filters
                    .iter()
                    .enumerate()
                    .filter(|(i, f)| f.enabled && Some(*i) != editing)
                    .count();
                (
                    editing,
                    editing.unwrap_or(so.filters.len()),
                    existing,
                    template,
                    protection,
                    others,
                )
            }
            _ => (None, 0, None, SmartFilter::default(), Protection::None, 0),
        };
        let mut template = template;
        if existing.is_none() {
            template.enabled = true;
            template.blend = Default::default();
        }
        let content_bounds = match &l.kind {
            LayerKind::Pixel(r) => super::tools::content_bounds(r),
            _ => child
                .root
                .iter()
                .filter_map(|c| c.affected_bounds())
                .reduce(|a, b| a.union(&b))
                .map(|r| r.intersect(&Rect::of_extent(child.canvas))),
        };
        let proxy = if other_stages == 0 {
            proxy_for(&child, &placement)?
        } else {
            None
        };
        let mut limitations = vec![
            format!(
                "Output is clipped to the fixed {} × {} child canvas.",
                child.canvas.width, child.canvas.height
            ),
            "Warp, perspective, puppet and content-aware stages are native-only: saving a PSD refuses them \
             (File ▸ Save Rasterized PSD Copy… writes a flattened-stage copy)."
                .into(),
        ];
        if kind == AdvancedTransformKind::ContentAwareScale {
            limitations.push(
                "Content-aware scale runs on the CPU (seam carving); large changes on large images take time. \
                 Protection uses a saved alpha channel you choose; there is no automatic skin detection."
                    .into(),
            );
        }
        if let Some(p) = &proxy {
            limitations.push(format!(
                "Drags preview at 1/{} resolution; releasing the mouse renders at full resolution.",
                1u32 << p.level
            ));
        }
        if other_stages > 0 {
            limitations.push(format!(
                "{other_stages} other smart filter(s) on this layer: previews render the whole stack at full resolution."
            ));
        }
        if matches!(l.kind, LayerKind::Text { .. } | LayerKind::Shape { .. }) {
            limitations.push(
                "Applying converts the layer to a smart object; the text or shape stays editable inside it."
                    .into(),
            );
        }
        st.advanced.next_token += 1;
        let token = st.advanced.next_token;
        let info = AdvancedTransformInfo {
            token,
            layer,
            layer_kind: kind_of(l),
            kind,
            editing_index: editing.map(|i| i as u32),
            insert_index: insert as u32,
            needs_conversion: wrapper.is_some(),
            child_width: child.canvas.width,
            child_height: child.canvas.height,
            child_to_document: matrix(&placement),
            content_bounds: content_bounds.and_then(DocRect::of),
            existing,
            revision: layer_revision(l),
            draft_level: proxy.as_ref().map_or(0, |p| p.level),
            other_stages: other_stages as u32,
            limitations,
        };
        st.advanced.active = Some(Active {
            token,
            layer,
            kind,
            editing,
            insert,
            wrapper,
            child,
            placement,
            revision: info.revision,
            proxy,
            stage_template: template,
            last: None,
            protection,
        });
        if had_preview {
            self.update(&mut st, &before, None, false);
        }
        Ok(info)
    }

    /// Shows `transform_json` (a `transform::TransformOp`) on the layer,
    /// recorded nowhere. Invalid geometry (crossing or degenerate quads, a
    /// collapsed mesh, bad pins) fails with the previous preview intact.
    /// `draft`: render on the reduced proxy when the session has one.
    pub fn preview_advanced_transform(
        &self,
        token: u64,
        transform_json: String,
        draft: bool,
    ) -> Result<AdvancedTransformPreview> {
        let op: TransformOp = parse("transform", &transform_json)?;
        self.preview_op(token, op, draft)
    }

    /// Previews content-aware scale to `target_width × target_height` child
    /// pixels at `amount` (0: plain resize … 1: all seam carving), protecting
    /// saved alpha channel `channel_id` (`None`: no protection; replaces an
    /// inherited mask). The mask is sampled Rust-side on the evaluation grid.
    pub fn content_aware_scale_from_channel(
        &self,
        token: u64,
        target_width: u32,
        target_height: u32,
        amount: f32,
        channel_id: Option<u64>,
        draft: bool,
    ) -> Result<AdvancedTransformPreview> {
        {
            let mut st = self.shared.lock()?;
            st.open()?;
            if let Some(id) = channel_id {
                let c = st
                    .doc
                    .state()
                    .channels
                    .iter()
                    .find(|c| c.id.0 == id)
                    .ok_or_else(|| failure(format!("channel {id} not found")))?;
                if c.kind != compositor::channels::ChannelKind::Alpha {
                    return Err(failure(
                        "protect with a saved alpha channel, not a spot channel",
                    ));
                }
            }
            checked(&st, token)?;
            let a = st.advanced.active.as_mut().expect("checked");
            a.protection = channel_id.map_or(Protection::None, Protection::Channel);
        }
        let op = TransformOp {
            version: 1,
            operation: Operation::ContentAwareScale(ContentAwareScale {
                target_width: target_width as usize,
                target_height: target_height as usize,
                amount,
                protect: None,
            }),
            kernel: transform::Kernel::Bilinear,
        };
        self.preview_op(token, op, draft)
    }

    /// Records the last previewed operation as ONE history node ("Warp",
    /// "Perspective Warp", …) and ends the session. A layer that is not a
    /// smart object is wrapped in the same node, only with
    /// `convert_to_smart_object`; without it the call fails and the session
    /// stays open. Nothing previewed: the session ends with no node.
    pub fn commit_advanced_transform(
        &self,
        token: u64,
        convert_to_smart_object: bool,
    ) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let a = checked(&st, token)?.clone();
        let Some(op) = a.last.clone() else {
            st.advanced.active = None;
            drop_preview(&mut st);
            return Ok(self.update(&mut st, &before, None, false));
        };
        if a.wrapper.is_some() && !convert_to_smart_object {
            return Err(failure(
                "applying a transform stage converts this layer to a smart object: confirm the conversion",
            ));
        }
        let s = st.doc.state().clone();
        let op = with_protection(&s, &a, op, 0)?;
        if let Operation::Puppet(p) = &op.operation {
            p.solve()
                .map_err(|e| failure(format!("Puppet Warp: {e}")))?;
        }
        let doc_op = stage_op(&s, &a, op)?;
        st.advanced.active = None;
        st.scratch = None;
        self.commit_pending(&mut st, None)?;
        let applied = match st.doc.apply(doc_op) {
            Ok(applied) => applied,
            Err(e) => {
                st.advanced.active = Some(a);
                return Err(e.into());
            }
        };
        st.labels.insert(applied.node, a.kind.label().to_owned());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        u.created.clear();
        Ok(u)
    }

    /// Ends the session with no history change; the viewport returns to the
    /// committed document. A stale token fails without effect.
    pub fn cancel_advanced_transform(&self, token: u64) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        if !st
            .advanced
            .active
            .as_ref()
            .is_some_and(|a| a.token == token)
        {
            return Err(failure("stale transform session (ended or replaced)"));
        }
        let before = st.live().state().clone();
        st.advanced.active = None;
        drop_preview(&mut st);
        Ok(self.update(&mut st, &before, None, false))
    }

    /// A pin-less puppet mesh over the alpha of `layer`'s source (the child
    /// of a smart object, or the layer itself), in child pixels. `density`:
    /// `Sparse` / `Normal` / `Dense` (8 / 4 / 2-pixel cells); `expansion`
    /// 0…64 px. Large opaque sources are meshed from a coarser level so the
    /// mesh stays within the engine's 16,384-vertex limit (`level`, `note`).
    pub fn puppet_mesh_from_layer(
        &self,
        layer: u64,
        density: String,
        expansion: u32,
    ) -> Result<PuppetMeshRecord> {
        let density = density_of(&density)?;
        if expansion > 64 {
            return Err(failure("puppet expansion must be 0…64 px"));
        }
        let child = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.doc.state().clone();
            source_of(&s, find(&s, layer)?)?.0
        };
        let step: usize = match density {
            PuppetDensity::Sparse => 8,
            PuppetDensity::Normal => 4,
            PuppetDensity::Dense => 2,
        };
        let canvas = child.canvas;
        // Start where an opaque source would have at most 8× the vertex limit.
        let mut level = (0..8u8)
            .find(|&l| {
                let e = canvas.at_level(l);
                (e.width as usize / step + 1) * (e.height as usize / step + 1) <= 16_384 * 8
            })
            .unwrap_or(7);
        let (e, px) = render_child(&child, level)?;
        let (mut w, mut h) = (e.width as usize, e.height as usize);
        let mut alpha: Vec<u8> = px
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| (p[3].clamp(0.0, 1.0) * 255.0).round() as u8)
            .collect();
        if alpha.iter().all(|a| *a == 0) {
            return Err(failure(
                "the layer is empty: puppet warp needs visible pixels",
            ));
        }
        loop {
            let s = 1u32 << level;
            let exp = expansion.div_ceil(s);
            match PuppetWarp::from_alpha(&alpha, w, h, density, exp) {
                Ok(mut mesh) => {
                    for v in &mut mesh.rest_vertices {
                        v[0] *= f64::from(s);
                        v[1] *= f64::from(s);
                    }
                    mesh.expansion = expansion;
                    return Ok(PuppetMeshRecord {
                        vertex_count: mesh.rest_vertices.len() as u32,
                        triangle_count: mesh.triangles.len() as u32,
                        cell_px: step as u32 * s,
                        level,
                        note: (level > 0).then(|| {
                            format!(
                                "Mesh built from 1/{s} resolution: {} px cells keep it within the engine's 16,384-vertex limit.",
                                step as u32 * s
                            )
                        }),
                        mesh_json: serde_json::to_string(&mesh).map_err(failure)?,
                    });
                }
                Err(e) if level < 10 && e.to_string().contains("16384") => {
                    // Conservative 2 × 2 max-pool: occupancy never shrinks.
                    let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
                    let mut next = vec![0u8; nw * nh];
                    for y in 0..h {
                        for x in 0..w {
                            let o = &mut next[(y / 2) * nw + x / 2];
                            *o = (*o).max(alpha[y * w + x]);
                        }
                    }
                    (alpha, w, h) = (next, nw, nh);
                    level += 1;
                }
                Err(e) => return Err(failure(format!("Puppet Warp: {e}"))),
            }
        }
    }

    /// Writes a PSD / PSB copy with every smart object that has an enabled
    /// transform stage or smart filter rasterized to pixels (its whole stack
    /// applied; PSD export refuses these native-only stacks), the explicit
    /// alternative to losing a nonlinear edit. The session document (path,
    /// dirty state, history) is unchanged.
    pub fn save_psd_rasterizing_transforms(&self, path: String) -> Result<()> {
        let path = std::path::PathBuf::from(path);
        if super::io::save_kind(&path)? == super::io::SaveKind::Native {
            return Err(failure("the rasterized copy is a .psd or .psb file"));
        }
        let s = {
            let st = self.shared.lock()?;
            st.open()?;
            st.doc.state().clone()
        };
        fn walk(v: &[Arc<Layer>], out: &mut Vec<u64>) {
            for l in v {
                match &l.kind {
                    LayerKind::SmartObject(so) if so.filters.iter().any(|f| f.enabled) => {
                        out.push(l.id.0)
                    }
                    LayerKind::Group { children, .. } => walk(children, out),
                    _ => {}
                }
            }
        }
        let mut ids = Vec::new();
        walk(&s.root, &mut ids);
        let mut state = (*s).clone();
        for id in ids {
            let l = find(&s, id)?;
            let raster = filtering::rasterize_smart_stack(&s, l)?;
            state.layer_mut(LayerId(id), |x| x.kind = LayerKind::Pixel(raster));
        }
        super::io::save(&Document::new(state), &path)
    }
}

impl DocumentSession {
    fn preview_op(
        &self,
        token: u64,
        op: TransformOp,
        draft: bool,
    ) -> Result<AdvancedTransformPreview> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let a = checked(&st, token)?.clone();
        validate(&op, a.kind)?;
        let s = st.doc.state().clone();
        let deformed_json = match &op.operation {
            Operation::Puppet(p) => {
                let solved = p
                    .solve()
                    .map_err(|e| failure(format!("Puppet Warp: {e}")))?;
                Some(serde_json::to_string(&solved.vertices).map_err(failure)?)
            }
            _ => None,
        };
        let doc_op = match (&a.proxy, draft) {
            (Some(proxy), true) => {
                let scaled = with_protection(&s, &a, scaled_op(&op, proxy.level)?, proxy.level)?;
                draft_op(&s, &a, proxy, scaled)?
            }
            _ => stage_op(&s, &a, with_protection(&s, &a, op.clone(), 0)?)?,
        };
        if !st.pending.is_empty() {
            // Another control's drag is its own node (one gesture owns the scratch).
            self.commit_pending(&mut st, None)?;
        }
        let mut scratch = st.doc.clone();
        scratch.set_max_states(2);
        let applied = scratch.apply(doc_op)?;
        st.scratch = Some(scratch);
        if let Some(active) = st.advanced.active.as_mut() {
            active.last = Some(op);
        }
        let mut update = self.update(&mut st, &before, Some(&applied), false);
        update.created.clear();
        Ok(AdvancedTransformPreview {
            update,
            deformed_json,
        })
    }

    /// Whether a transform session is open (tests).
    #[doc(hidden)]
    pub fn advanced_transform_open(&self) -> bool {
        self.shared
            .lock()
            .is_ok_and(|st| st.advanced.active.is_some())
    }
}
