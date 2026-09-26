//! `DocumentSession` over UniFFI: layered documents (the `compositor` crate)
//! for the mac app's document mode — open/create, a flat layer list, edits,
//! non-linear history, GPU-resident viewport presentation into host
//! IOSurfaces, thumbnails, save and flat export.
//!
//! # Conventions (as `DevelopSession`)
//!
//! Sessions are `Arc`s created by [`Engine`]; ids and small records cross
//! UniFFI, pixels never do. A [`DocumentId`](engine_api::id::DocumentId)
//! string (`doc#N`) names a session; opening the same file (or the same
//! library image) twice returns the same session while it is open. Errors are
//! `EngineError`s surfaced as [`crate::BridgeError`].
//!
//! # Edits and history
//!
//! Every edit is exactly one [`DocOp`] through [`Document::apply`] (structural
//! edits such as merge down or group are one `DocOp::Batch`) and becomes one
//! history node. Interactive edits (`interactive: true`: an opacity or
//! adjustment slider drag) are applied to a scratch copy of the document that
//! the viewport shows; no history node is recorded until [`commit`], which
//! applies the net change of the drag as one op. A non-interactive value for
//! a control being dragged folds into the drag and commits it. Any other
//! edit, undo, redo, checkout or save commits a pending drag first.
//!
//! # Presentation
//!
//! Rendering runs on the session's render thread with the compositor's
//! `ResidentRenderer` on the engine's shared `gpu-core` Metal device (the
//! one the develop pipeline uses). Every mutation bumps the document epoch
//! and wakes the thread; mutations arriving while it works coalesce into one
//! frame, and the listener hears once per coalesced frame (`on_frame`,
//! `on_layers_changed`, `on_history_changed`), never while a session lock is
//! held. Surfaces are RGBA8 **display-encoded sRGB with straight
//! (non-premultiplied) alpha**: the host draws the transparency checkerboard
//! and composites the frame over it. Without Metal the CPU compositor writes
//! the same contract.
//!
//! [`commit`]: DocumentSession::commit

#[path = "document/io.rs"]
mod io;
#[path = "document/render.rs"]
mod render;

use crate::{Engine, Result, failure, surface::Surface};
use compositor::{
    Adjustment, BlendMode, DocOp, DocState, Document, Fill, GroupMode, Knockout, Layer, LayerId,
    LayerKind, Locks, Mask, Raster, Rect, edit::Applied,
};
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord};
use std::{
    collections::{BTreeMap, HashMap},
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard, Weak},
};

pub use io::{ExportColor, ExportFormat};

// ─────────────────────────────── records ───────────────────────────────

/// Bits per channel of a document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocDepth {
    U8,
    U16,
    F32,
}

impl From<DocDepth> for compositor::Depth {
    fn from(d: DocDepth) -> Self {
        match d {
            DocDepth::U8 => Self::U8,
            DocDepth::U16 => Self::U16,
            DocDepth::F32 => Self::F32,
        }
    }
}

impl From<compositor::Depth> for DocDepth {
    fn from(d: compositor::Depth) -> Self {
        match d {
            compositor::Depth::U8 => Self::U8,
            compositor::Depth::U16 => Self::U16,
            compositor::Depth::F32 => Self::F32,
        }
    }
}

/// What a layer is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocLayerKind {
    Pixel,
    Adjustment,
    Fill,
    Group,
    SmartObject,
    Text,
}

/// How a group composites its children.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocGroupMode {
    /// Children blend straight into the backdrop (Photoshop "Pass Through").
    PassThrough,
    /// Children composite into their own buffer, blended with the group's mode.
    Isolated,
}

impl From<DocGroupMode> for GroupMode {
    fn from(m: DocGroupMode) -> Self {
        match m {
            DocGroupMode::PassThrough => Self::PassThrough,
            DocGroupMode::Isolated => Self::Isolated,
        }
    }
}

impl From<GroupMode> for DocGroupMode {
    fn from(m: GroupMode) -> Self {
        match m {
            GroupMode::PassThrough => Self::PassThrough,
            GroupMode::Isolated => Self::Isolated,
        }
    }
}

/// A pixel rectangle `x, y, width × height` (level 0 unless stated).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocRect {
    pub x: i64,
    pub y: i64,
    pub width: i64,
    pub height: i64,
}

impl DocRect {
    fn of(r: Rect) -> Option<Self> {
        (!r.is_empty()).then(|| Self {
            x: r.x0,
            y: r.y0,
            width: r.width(),
            height: r.height(),
        })
    }
}

/// Layer lock flags (edit ops enforce them; the compositor ignores them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct LayerLocks {
    pub transparency: bool,
    pub pixels: bool,
    pub position: bool,
    pub all: bool,
}

impl From<Locks> for LayerLocks {
    fn from(l: Locks) -> Self {
        Self {
            transparency: l.transparency,
            pixels: l.pixels,
            position: l.position,
            all: l.all,
        }
    }
}

impl From<LayerLocks> for Locks {
    fn from(l: LayerLocks) -> Self {
        Self {
            transparency: l.transparency,
            pixels: l.pixels,
            position: l.position,
            all: l.all,
        }
    }
}

/// One row of the Layers panel. [`DocumentSession::layers`] lists the tree
/// flat in pre-order with siblings **top-first** (the panel's order): a group
/// row is followed by its children, topmost child first.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LayerNode {
    pub id: u64,
    /// Enclosing group (`None` at the document root).
    pub parent: Option<u64>,
    /// Compositor child index within the parent: 0 = bottom.
    pub index: u32,
    /// Nesting depth: 0 at the root.
    pub depth: u32,
    pub kind: DocLayerKind,
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub fill_opacity: f32,
    /// Stable snake_case blend mode name (`normal`, `dissolve`, `darken`,
    /// `multiply`, `color_burn`, `linear_burn`, `darker_color`, `lighten`,
    /// `screen`, `color_dodge`, `linear_dodge`, `lighter_color`, `overlay`,
    /// `soft_light`, `hard_light`, `vivid_light`, `linear_light`,
    /// `pin_light`, `hard_mix`, `difference`, `exclusion`, `subtract`,
    /// `divide`, `hue`, `saturation`, `color`, `luminosity`: COMPOSITOR.md
    /// §2 in Photoshop's menu order, see [`blend_mode_names`]), or
    /// `pass_through` for pass-through groups.
    pub blend_mode: String,
    /// Groups only.
    pub group_mode: Option<DocGroupMode>,
    pub clipped: bool,
    pub locks: LayerLocks,
    /// `none`, `shallow` or `deep`.
    pub knockout: String,
    /// The document Background layer.
    pub background: bool,
    pub has_mask: bool,
    pub mask_enabled: bool,
    /// Mask moves with the layer (the chain icon). Session state: the
    /// compositor does not model translation yet, so it is not saved.
    pub mask_linked: bool,
    pub mask_density: f32,
    /// `compositor::Adjustment` JSON (adjustment layers only).
    pub adjustment_json: Option<String>,
    /// `compositor::Fill` JSON (fill layers only).
    pub fill_json: Option<String>,
    /// Canvas region the layer can change, in whole stored tiles (256-px
    /// granular) for pixel and text layers and groups of them; `None` for
    /// layers that can change anywhere (adjustments, fills, smart objects)
    /// and for empty layers.
    pub bounds: Option<DocRect>,
    /// Changes whenever what the layer's thumbnails show changes: its
    /// pixels or content, its mask, and for groups their children (with the
    /// children's properties). The layer's own properties (opacity, blend
    /// mode, visibility, name) do not change it. The thumbnail cache key.
    pub revision: u64,
}

/// New layer content for [`DocumentSession::add_layer`].
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NewLayer {
    /// A transparent pixel layer.
    Pixel,
    /// An empty group.
    Group { mode: DocGroupMode },
    /// An adjustment layer from `compositor::Adjustment` JSON, e.g.
    /// `{"kind":"exposure","exposure":1,"offset":0,"gamma":1}`.
    Adjustment { json: String },
    /// A fill layer from `compositor::Fill` JSON, e.g.
    /// `{"kind":"solid","color":[1,0,0]}`.
    Fill { json: String },
}

/// Initial content of a new layer mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaskInit {
    RevealAll,
    HideAll,
    /// The current selection (fails without one).
    FromSelection,
}

/// Common layer properties for [`DocumentSession::set_props`]. Blend If
/// sliders and the Background flag are kept as they are.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LayerPropsRecord {
    pub name: String,
    pub visible: bool,
    pub opacity: f32,
    pub fill_opacity: f32,
    /// A [`LayerNode::blend_mode`] name; `pass_through` for groups only.
    pub blend_mode: String,
    pub clipped: bool,
    pub locks: LayerLocks,
    /// `none`, `shallow` or `deep`.
    pub knockout: String,
    pub color_tag: Option<String>,
}

/// What one edit did.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentUpdate {
    /// Layers whose row may have changed: edited, added or removed layers
    /// and their ancestor groups.
    pub layers_changed: Vec<u64>,
    /// Layers the edit created (added, duplicated, merged).
    pub created: Vec<u64>,
    /// Current history node (unchanged by interactive edits).
    pub history_head: u64,
    /// Level-0 region whose composite may have changed (`None`: nothing).
    pub dirty_rect: Option<DocRect>,
    /// Document epoch after the edit (every mutation increments it).
    pub epoch: u64,
    /// Unsaved changes.
    pub dirty: bool,
}

/// Document summary.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocumentInfo {
    /// `doc#N`: this session's document id.
    pub id: String,
    /// Where `save` writes (`.tessera-doc`, `.psd` or `.psb`); `None` for
    /// new documents and flat images until `save_as`.
    pub path: Option<String>,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub depth: DocDepth,
    /// Profile description (`None`: untagged, treated as sRGB).
    pub profile_name: Option<String>,
    pub dirty: bool,
    pub history_head: u64,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Layers selected in the Layers panel (`set_selected_layers`).
    pub selected_layer_ids: Vec<u64>,
    /// Bounds of the selection (`None`: no selection, i.e. select all).
    pub selection_bounds: Option<DocRect>,
    /// Library image this document was made from (`open_document_from_image`).
    pub source_image_id: Option<String>,
    pub layer_count: u32,
    pub epoch: u64,
    /// "Metal (<adapter>)" or "CPU".
    pub backend: String,
}

/// One history state, as the History panel lists it.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocHistoryItem {
    pub id: u64,
    pub label: String,
    /// Parent state (`None` for the opened state or after pruning).
    pub parent: Option<u64>,
    pub is_current: bool,
    /// "user" (agents and imports arrive with the MCP document tools).
    pub author: String,
}

/// Surface size to allocate for a fit-to-window viewport of `width × height`
/// device pixels: the extent of `level`, the coarsest level covering it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DocSurfacePlan {
    pub level: u8,
    pub width: u32,
    pub height: u32,
}

/// A presented frame.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DocFrameInfo {
    /// Surface written (0 when none is attached).
    pub surface_id: u32,
    pub level: u8,
    /// Presented region in `level` coordinates; its pixels sit top-left in
    /// the surface, `width × height` texels.
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    /// The same region in level-0 canvas pixels (clipped to the canvas).
    pub canvas_rect: DocRect,
    /// Extent of the whole level.
    pub level_width: u32,
    pub level_height: u32,
    /// Zoom from the last `set_viewport` (1 = 100 %), echoed back.
    pub zoom: f64,
    /// Document epoch this frame shows.
    pub epoch: u64,
    /// From the first change of this frame to the pixels being in the surface.
    pub render_ms: f64,
    /// GPU-side work: the level was recomposited in full / blocks composited.
    pub full_recomposite: bool,
    pub blocks: u32,
}

/// Callbacks run on the session's render thread, never while a session lock
/// is held; each fires at most once per coalesced frame.
#[uniffi::export(with_foreign)]
pub trait DocumentListener: Send + Sync {
    fn on_frame(&self, frame: DocFrameInfo);
    /// Rows that may have changed (re-read them with `layers()`).
    fn on_layers_changed(&self, layer_ids: Vec<u64>);
    fn on_history_changed(&self, head: u64);
    fn on_render_failed(&self, message: String);
}

/// The 27 blend mode names in Photoshop's menu order (COMPOSITOR.md §2),
/// as used by [`LayerNode::blend_mode`]. Groups add `pass_through`.
#[uniffi::export]
pub fn blend_mode_names() -> Vec<String> {
    BlendMode::ALL.iter().map(|m| blend_name(*m)).collect()
}

fn blend_name(m: BlendMode) -> String {
    serde_json::to_value(m)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn parse_blend(name: &str) -> Result<BlendMode> {
    serde_json::from_value(serde_json::Value::String(name.into()))
        .map_err(|_| failure(format!("unknown blend mode {name:?}")))
}

fn knockout_name(k: Knockout) -> String {
    match k {
        Knockout::None => "none",
        Knockout::Shallow => "shallow",
        Knockout::Deep => "deep",
    }
    .into()
}

fn parse_knockout(name: &str) -> Result<Knockout> {
    Ok(match name {
        "none" | "" => Knockout::None,
        "shallow" => Knockout::Shallow,
        "deep" => Knockout::Deep,
        _ => return Err(failure(format!("unknown knockout {name:?}"))),
    })
}

pub(crate) const PASS_THROUGH: &str = "pass_through";

// ─────────────────────────────── registry ───────────────────────────────

/// Open document sessions of one engine, and the compositor's GPU state on
/// the engine's shared device.
#[derive(Default)]
pub(crate) struct Registry {
    inner: Mutex<RegistryInner>,
    gpu: std::sync::OnceLock<Option<Arc<render::DocGpu>>>,
}

#[derive(Default)]
struct RegistryInner {
    next: u64,
    /// Source key (canonical path or `image:<id>:<developed>`) → session.
    by_key: HashMap<String, Weak<DocumentSession>>,
    by_id: HashMap<String, Weak<DocumentSession>>,
}

impl Registry {
    fn lock(&self) -> MutexGuard<'_, RegistryInner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn find(&self, key: &str) -> Option<Arc<DocumentSession>> {
        self.lock().by_key.get(key).and_then(Weak::upgrade)
    }

    fn remove(&self, id: &str) {
        let mut r = self.lock();
        r.by_id.remove(id);
        r.by_key
            .retain(|_, s| s.upgrade().is_some_and(|s| s.shared.id != id));
    }
}

impl Engine {
    fn doc_gpu(&self) -> Option<Arc<render::DocGpu>> {
        self.documents
            .gpu
            .get_or_init(|| {
                let device = self.shared_gpu()?;
                match render::DocGpu::new(&device) {
                    Ok(g) => Some(Arc::new(g)),
                    Err(e) => {
                        eprintln!("document: GPU compositor unavailable, using CPU: {e}");
                        None
                    }
                }
            })
            .clone()
    }

    /// Wraps `doc` in a session registered under `key` (or returns the
    /// session already open for it).
    fn register_document(
        self: &Arc<Self>,
        key: Option<String>,
        open: Opened,
    ) -> Arc<DocumentSession> {
        if let Some(k) = &key
            && let Some(s) = self.documents.find(k)
        {
            return s;
        }
        let id = {
            let mut r = self.documents.lock();
            r.next += 1;
            engine_api::id::DocumentId(r.next).to_string()
        };
        let session = DocumentSession::start(self, id.clone(), open);
        let mut r = self.documents.lock();
        // Another thread may have opened the same source meanwhile.
        if let Some(k) = &key
            && let Some(s) = r.by_key.get(k).and_then(Weak::upgrade)
        {
            drop(r);
            session.shutdown();
            return s;
        }
        if let Some(k) = key {
            r.by_key.insert(k, Arc::downgrade(&session));
        }
        r.by_id.insert(id, Arc::downgrade(&session));
        session
    }
}

/// A document as opened, before it has a session.
pub(crate) struct Opened {
    doc: Document,
    title: String,
    path: Option<PathBuf>,
    source_image_id: Option<String>,
    /// Label of history node 0.
    origin: &'static str,
    /// The opened state is itself unsaved (new documents, images).
    unsaved: bool,
}

#[uniffi::export]
impl Engine {
    /// A new document with one transparent pixel layer ("Layer 1").
    /// `profile`: `sRGB` (default), `Display P3`, `Adobe RGB`, `ProPhoto RGB`,
    /// `Rec. 2020`, or a path to an ICC profile.
    pub fn new_document(
        self: Arc<Self>,
        width: u32,
        height: u32,
        depth: DocDepth,
        profile: Option<String>,
    ) -> Result<Arc<DocumentSession>> {
        if width == 0 || height == 0 || width > 300_000 || height > 300_000 {
            return Err(failure("canvas must be 1–300000 pixels on each side"));
        }
        let extent = Extent::new(width, height);
        let depth: compositor::Depth = depth.into();
        let mut state = DocState::new(extent, depth);
        state.profile = io::profile(profile.as_deref())?;
        let mut layer = Layer::pixel("Layer 1", extent, depth);
        state.assign_ids_for_new(&mut layer);
        state.root.push(Arc::new(layer));
        Ok(self.register_document(
            None,
            Opened {
                doc: Document::new(state),
                title: "Untitled".into(),
                path: None,
                source_image_id: None,
                origin: "New Document",
                unsaved: true,
            },
        ))
    }

    /// Opens `.tessera-doc`, `.psd`/`.psb` (unknown PSD records are kept for
    /// save-back) or a flat JPEG/PNG/TIFF (one pixel layer named after the
    /// file). Blocking: call off the main thread. The same file opened twice
    /// returns the same session while it is open.
    pub fn open_document(self: Arc<Self>, path: String) -> Result<Arc<DocumentSession>> {
        let path = std::fs::canonicalize(&path).map_err(|e| failure(format!("{path}: {e}")))?;
        let key = path.to_string_lossy().into_owned();
        if let Some(s) = self.documents.find(&key) {
            return Ok(s);
        }
        let opened = io::open_path(&path)?;
        Ok(self.register_document(Some(key), opened))
    }

    /// A document with one pixel layer holding library image `image_id`
    /// rendered at full resolution through the export path (its develop
    /// recipe when `developed`, default settings otherwise), display
    /// orientation, 16-bit sRGB. Blocking (decodes and renders the RAW).
    pub fn open_document_from_image(
        self: Arc<Self>,
        image_id: String,
        developed: bool,
    ) -> Result<Arc<DocumentSession>> {
        let key = format!("image:{image_id}:{developed}");
        if let Some(s) = self.documents.find(&key) {
            return Ok(s);
        }
        let opened = io::open_image(&self, &image_id, developed)?;
        Ok(self.register_document(Some(key), opened))
    }

    /// An open document session by id (`doc#N`).
    pub fn document_session(&self, id: String) -> Option<Arc<DocumentSession>> {
        self.documents.lock().by_id.get(&id).and_then(Weak::upgrade)
    }

    /// Ids of the open document sessions, oldest first.
    pub fn document_ids(&self) -> Vec<String> {
        let r = self.documents.lock();
        let mut ids: Vec<(u64, String)> = r
            .by_id
            .iter()
            .filter(|(_, s)| s.upgrade().is_some())
            .filter_map(|(id, _)| Some((id.strip_prefix("doc#")?.parse().ok()?, id.clone())))
            .collect();
        ids.sort();
        ids.into_iter().map(|(_, id)| id).collect()
    }
}

/// `DocState::assign_ids` is crate-private in the compositor; new layers are
/// numbered here the same way (ids from `next_id`, descendants included).
trait AssignIds {
    fn assign_ids_for_new(&mut self, layer: &mut Layer);
}

impl AssignIds for DocState {
    fn assign_ids_for_new(&mut self, layer: &mut Layer) {
        if layer.id.0 == 0 {
            layer.id = LayerId(self.next_id);
            self.next_id += 1;
        }
        if let LayerKind::Group { children, .. } = &mut layer.kind {
            for c in children.iter_mut() {
                self.assign_ids_for_new(Arc::make_mut(c));
            }
        }
    }
}

// ─────────────────────────────── session ───────────────────────────────

/// What a pending interactive edit targets (one net op per key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Props(u64),
    Adjustment(u64),
    Fill(u64),
}

pub(crate) struct State {
    doc: Document,
    /// The document plus the interactive edits since the last commit.
    scratch: Option<Document>,
    pending: Vec<(Pending, DocOp)>,
    path: Option<PathBuf>,
    title: String,
    source_image_id: Option<String>,
    /// History node last saved or opened from disk (`None`: never saved).
    saved_node: Option<u64>,
    selected: Vec<u64>,
    epoch: u64,
    /// History label overrides (commit labels, merge down, …).
    labels: HashMap<u64, String>,
    /// Masks unlinked from their layer (session state).
    unlinked_masks: std::collections::BTreeSet<u64>,
    /// Exact bounds of the last selection `info` measured.
    selection_bounds: Option<(
        std::sync::Weak<compositor::Raster>,
        Option<compositor::Rect>,
    )>,
    closed: bool,
    pub(crate) view: render::View,
}

impl State {
    /// What the viewport and `layers()` show: the scratch while dragging.
    fn live(&self) -> &Document {
        self.scratch.as_ref().unwrap_or(&self.doc)
    }

    fn dirty(&self) -> bool {
        self.saved_node != Some(self.doc.history().current()) || !self.pending.is_empty()
    }

    fn open(&self) -> Result<()> {
        if self.closed {
            Err(failure("document is closed"))
        } else {
            Ok(())
        }
    }
}

pub(crate) struct Shared {
    id: String,
    engine: Weak<Engine>,
    state: Mutex<State>,
    render: render::Renderer,
    listener: Mutex<Option<Arc<dyn DocumentListener>>>,
}

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(failure)
    }

    fn listener(&self) -> Option<Arc<dyn DocumentListener>> {
        self.listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

#[derive(uniffi::Object)]
pub struct DocumentSession {
    shared: Arc<Shared>,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Drop for DocumentSession {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// Layer id → `Arc` address over the whole tree (for change detection).
fn layer_ptrs(state: &DocState) -> BTreeMap<u64, (usize, Option<u64>)> {
    fn go(v: &[Arc<Layer>], parent: Option<u64>, out: &mut BTreeMap<u64, (usize, Option<u64>)>) {
        for l in v {
            out.insert(l.id.0, (Arc::as_ptr(l) as usize, parent));
            if let Some(c) = l.children() {
                go(c, Some(l.id.0), out);
            }
        }
    }
    let mut out = BTreeMap::new();
    go(&state.root, None, &mut out);
    out
}

/// Layers added, removed or replaced between two states.
fn changed_layers(a: &DocState, b: &DocState) -> Vec<u64> {
    let (pa, pb) = (layer_ptrs(a), layer_ptrs(b));
    let mut out: Vec<u64> = pa
        .iter()
        .filter(|(id, p)| pb.get(id) != Some(p))
        .map(|(id, _)| *id)
        .chain(pb.keys().filter(|id| !pa.contains_key(id)).copied())
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Maximum revision over everything a layer's thumbnails show: its content,
/// its mask and, for groups, every child with its properties. The layer's
/// own properties (opacity, blend mode, visibility, name, …) are left out:
/// thumbnails ignore them, so an opacity drag does not re-render the
/// thumbnail on every step (WP M5-10b).
fn layer_revision(l: &Layer) -> u64 {
    let mut r = l.content_rev;
    if let Some(m) = &l.mask {
        r = r.max(m.raster.max_rev());
    }
    match &l.kind {
        LayerKind::Pixel(raster) => r.max(raster.max_rev()),
        LayerKind::Text(t) => r.max(t.proxy.max_rev()),
        LayerKind::Group { children, .. } => children
            .iter()
            .fold(r, |a, c| a.max(c.props_rev).max(layer_revision(c))),
        LayerKind::SmartObject(so) => r.max(so.state.rev),
        LayerKind::Adjustment(_) | LayerKind::Fill(_) => r,
    }
}

fn kind_of(l: &Layer) -> DocLayerKind {
    match l.kind {
        LayerKind::Pixel(_) => DocLayerKind::Pixel,
        LayerKind::Adjustment(_) => DocLayerKind::Adjustment,
        LayerKind::Fill(_) => DocLayerKind::Fill,
        LayerKind::Group { .. } => DocLayerKind::Group,
        LayerKind::SmartObject(_) => DocLayerKind::SmartObject,
        LayerKind::Text(_) => DocLayerKind::Text,
    }
}

fn group_mode_of(l: &Layer) -> Option<GroupMode> {
    match &l.kind {
        LayerKind::Group { mode, .. } => Some(*mode),
        _ => None,
    }
}

fn node_of(
    l: &Layer,
    parent: Option<u64>,
    index: usize,
    depth: u32,
    unlinked: &std::collections::BTreeSet<u64>,
) -> LayerNode {
    let group_mode = group_mode_of(l);
    LayerNode {
        id: l.id.0,
        parent,
        index: index as u32,
        depth,
        kind: kind_of(l),
        name: l.props.name.clone(),
        visible: l.props.visible,
        opacity: l.props.opacity,
        fill_opacity: l.props.fill_opacity,
        blend_mode: if group_mode == Some(GroupMode::PassThrough) {
            PASS_THROUGH.into()
        } else {
            blend_name(l.props.blend_mode)
        },
        group_mode: group_mode.map(Into::into),
        clipped: l.props.clipped,
        locks: l.props.locks.into(),
        knockout: knockout_name(l.props.knockout),
        background: l.props.background,
        has_mask: l.mask.is_some(),
        mask_enabled: l.mask.as_ref().is_some_and(|m| m.enabled),
        mask_linked: l.mask.is_some() && !unlinked.contains(&l.id.0),
        mask_density: l.mask.as_ref().map_or(1.0, |m| m.density),
        adjustment_json: match &l.kind {
            LayerKind::Adjustment(a) => serde_json::to_string(a).ok(),
            _ => None,
        },
        fill_json: match &l.kind {
            LayerKind::Fill(f) => serde_json::to_string(f).ok(),
            _ => None,
        },
        bounds: l.affected_bounds().and_then(DocRect::of),
        revision: layer_revision(l),
    }
}

fn flatten_nodes(
    v: &[Arc<Layer>],
    parent: Option<u64>,
    depth: u32,
    unlinked: &std::collections::BTreeSet<u64>,
    out: &mut Vec<LayerNode>,
) {
    for (i, l) in v.iter().enumerate().rev() {
        out.push(node_of(l, parent, i, depth, unlinked));
        if let Some(c) = l.children() {
            flatten_nodes(c, Some(l.id.0), depth + 1, unlinked, out);
        }
    }
}

fn find(state: &DocState, id: u64) -> Result<&Layer> {
    state
        .find(LayerId(id))
        .ok_or_else(|| failure(format!("layer {id} not found")))
}

/// A tile of `layout` in `depth` from normalized planar samples.
pub(crate) fn tile_from_f32(
    coord: TileCoord,
    layout: engine_api::tile::TileLayout,
    depth: compositor::Depth,
    data: Vec<f32>,
) -> engine_api::EngineResult<Tile> {
    match depth {
        compositor::Depth::U8 => Tile::from_samples(
            coord,
            layout,
            data.iter()
                .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
                .collect::<Vec<_>>(),
        ),
        compositor::Depth::U16 => Tile::from_samples(
            coord,
            layout,
            data.iter()
                .map(|v| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16)
                .collect::<Vec<_>>(),
        ),
        compositor::Depth::F32 => Tile::from_samples(coord, layout, data),
    }
}

impl DocumentSession {
    fn start(engine: &Arc<Engine>, id: String, open: Opened) -> Arc<Self> {
        let gpu = engine.doc_gpu();
        let saved = (!open.unsaved).then(|| open.doc.history().current());
        let mut labels = HashMap::new();
        labels.insert(open.doc.history().current(), open.origin.to_owned());
        let shared = Arc::new(Shared {
            id,
            engine: Arc::downgrade(engine),
            state: Mutex::new(State {
                doc: open.doc,
                scratch: None,
                pending: Vec::new(),
                path: open.path,
                title: open.title,
                source_image_id: open.source_image_id,
                saved_node: saved,
                selected: Vec::new(),
                epoch: 0,
                labels,
                unlinked_masks: Default::default(),
                selection_bounds: None,
                closed: false,
                view: Default::default(),
            }),
            render: render::Renderer::new(gpu),
            listener: Mutex::new(None),
        });
        let worker = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("document-render".into())
                .spawn(move || render::worker_loop(shared))
                .ok()
        };
        Arc::new(Self {
            shared,
            worker: Mutex::new(worker),
        })
    }

    fn shutdown(&self) {
        if let Ok(mut st) = self.shared.state.lock() {
            st.closed = true;
            st.view.surfaces.clear();
        }
        self.shared.render.stop();
        if let Some(w) = self.worker.lock().ok().and_then(|mut w| w.take())
            && w.thread().id() != std::thread::current().id()
        {
            let _ = w.join();
        }
    }

    /// Records the pending interactive edits as one history node.
    fn commit_pending(&self, st: &mut State, label: Option<&str>) -> Result<Option<Applied>> {
        if st.pending.is_empty() {
            st.scratch = None;
            return Ok(None);
        }
        let mut ops: Vec<DocOp> = std::mem::take(&mut st.pending)
            .into_iter()
            .map(|(_, op)| op)
            .collect();
        st.scratch = None;
        let op = if ops.len() == 1 {
            ops.pop().expect("one op")
        } else {
            DocOp::Batch(ops)
        };
        let applied = st.doc.apply(op)?;
        if let Some(l) = label.filter(|l| !l.is_empty()) {
            st.labels.insert(applied.node, l.to_owned());
        }
        Ok(Some(applied))
    }

    fn update(
        &self,
        st: &mut State,
        before: &Arc<DocState>,
        applied: Option<&Applied>,
        history: bool,
    ) -> DocumentUpdate {
        st.epoch += 1;
        let after = st.live().state().clone();
        let changed = changed_layers(before, &after);
        let dirty_rect = match applied {
            Some(a) => DocRect::of(a.damage),
            None if !changed.is_empty() || before.rev != after.rev => {
                DocRect::of(Rect::of_extent(after.canvas))
            }
            None => None,
        };
        let update = DocumentUpdate {
            layers_changed: changed.clone(),
            created: applied
                .map(|a| a.created.iter().map(|c| c.0).collect())
                .unwrap_or_default(),
            history_head: st.doc.history().current(),
            dirty_rect,
            epoch: st.epoch,
            dirty: st.dirty(),
        };
        self.shared.render.request(changed, history, st.epoch);
        update
    }

    /// Applies one op as a history node (committing a pending drag first).
    fn edit(&self, op: DocOp, label: Option<&str>) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        let applied = st.doc.apply(op)?;
        if let Some(l) = label {
            st.labels.insert(applied.node, l.to_owned());
        }
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    /// An edit of one control: live on the scratch while `interactive`,
    /// otherwise one history node (folding into a pending drag).
    fn edit_keyed(
        &self,
        key: Pending,
        interactive: bool,
        make: impl FnOnce(&DocState) -> Result<DocOp>,
    ) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let op = make(&before)?;
        if !interactive && !st.pending.iter().any(|(k, _)| *k == key) {
            // Not the control being dragged: a pending drag of another
            // control is its own history node.
            self.commit_pending(&mut st, None)?;
            let applied = st.doc.apply(op)?;
            return Ok(self.update(&mut st, &before, Some(&applied), true));
        }
        if st.scratch.is_none() {
            let mut scratch = st.doc.clone();
            scratch.set_max_states(2);
            st.scratch = Some(scratch);
        }
        let applied = st.scratch.as_mut().expect("scratch").apply(op.clone())?;
        st.pending.retain(|(k, _)| *k != key);
        st.pending.push((key, op));
        if interactive {
            return Ok(self.update(&mut st, &before, Some(&applied), false));
        }
        let committed = self.commit_pending(&mut st, None)?;
        let mut update = self.update(&mut st, &before, Some(&applied), true);
        if let Some(c) = committed {
            update.dirty_rect = DocRect::of(c.damage.union(&applied.damage));
        }
        Ok(update)
    }

    /// Replaces a layer's props through `f` (keyed: interactive-capable).
    fn edit_props(
        &self,
        id: u64,
        interactive: bool,
        f: impl FnOnce(&mut compositor::LayerProps) -> Result<()>,
    ) -> Result<DocumentUpdate> {
        self.edit_keyed(Pending::Props(id), interactive, |s| {
            let mut props = find(s, id)?.props.clone();
            f(&mut props)?;
            Ok(DocOp::SetProps {
                id: LayerId(id),
                props,
            })
        })
    }

    /// A history move (undo, redo, checkout, snapshot) after committing a drag.
    fn history_move(
        &self,
        f: impl FnOnce(&mut Document) -> Result<bool>,
    ) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        f(&mut st.doc)?;
        Ok(self.update(&mut st, &before, None, true))
    }

    /// The current mask of a layer, for mask edits.
    fn mask_of(s: &DocState, id: u64) -> Result<Mask> {
        find(s, id)?
            .mask
            .clone()
            .ok_or_else(|| failure(format!("layer {id} has no mask")))
    }
}

#[uniffi::export]
impl DocumentSession {
    // ─────────────────────────── model reads ───────────────────────────

    pub fn id(&self) -> String {
        self.shared.id.clone()
    }

    pub fn info(&self) -> Result<DocumentInfo> {
        let mut st = self.shared.lock()?;
        let selection_bounds = match st.live().state().selection.clone() {
            None => None,
            Some(sel) => match &st.selection_bounds {
                Some((seen, b)) if seen.upgrade().is_some_and(|s| Arc::ptr_eq(&s, &sel)) => *b,
                _ => {
                    let b = io::selection_bounds(&sel);
                    st.selection_bounds = Some((Arc::downgrade(&sel), b));
                    b
                }
            },
        };
        let doc = st.live();
        let s = doc.state();
        let h = st.doc.history();
        let current = h.current();
        let parent = h.nodes().find(|n| n.id == current).and_then(|n| n.parent);
        let ids = layer_ptrs(s);
        Ok(DocumentInfo {
            id: self.shared.id.clone(),
            path: st.path.as_ref().map(|p| p.to_string_lossy().into_owned()),
            title: st.title.clone(),
            width: s.canvas.width,
            height: s.canvas.height,
            depth: s.depth.into(),
            profile_name: s.profile.as_ref().map(|p| p.name.clone()),
            dirty: st.dirty(),
            history_head: current,
            can_undo: !st.pending.is_empty()
                || parent.is_some_and(|p| h.nodes().any(|n| n.id == p)),
            can_redo: st.pending.is_empty() && h.nodes().any(|n| n.parent == Some(current)),
            selected_layer_ids: st
                .selected
                .iter()
                .copied()
                .filter(|id| ids.contains_key(id))
                .collect(),
            selection_bounds: selection_bounds.and_then(DocRect::of),
            source_image_id: st.source_image_id.clone(),
            layer_count: ids.len() as u32,
            epoch: st.epoch,
            backend: self.shared.render.backend_name(),
        })
    }

    /// Every layer, flat in pre-order with siblings top-first (see
    /// [`LayerNode`]). Shows the live state while a drag is pending.
    pub fn layers(&self) -> Result<Vec<LayerNode>> {
        let st = self.shared.lock()?;
        let mut out = Vec::new();
        flatten_nodes(
            &st.live().state().root,
            None,
            0,
            &st.unlinked_masks,
            &mut out,
        );
        Ok(out)
    }

    pub fn layer(&self, id: u64) -> Result<LayerNode> {
        self.layers()?
            .into_iter()
            .find(|n| n.id == id)
            .ok_or_else(|| failure(format!("layer {id} not found")))
    }

    /// Selects layers in the Layers panel (reported by `info`).
    pub fn set_selected_layers(&self, ids: Vec<u64>) -> Result<()> {
        let mut st = self.shared.lock()?;
        let known = layer_ptrs(st.live().state());
        if let Some(id) = ids.iter().find(|id| !known.contains_key(id)) {
            return Err(failure(format!("layer {id} not found")));
        }
        st.selected = ids;
        Ok(())
    }

    pub fn set_listener(&self, listener: Option<Arc<dyn DocumentListener>>) {
        *self
            .shared
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = listener;
    }

    // ────────────────────────────── edits ──────────────────────────────

    /// Adds a layer named `name` (empty: "Layer N", "Group N", …) at `index`
    /// of `parent` (`None`: the root; index `None`: on top).
    pub fn add_layer(
        &self,
        kind: NewLayer,
        name: String,
        parent: Option<u64>,
        index: Option<u32>,
    ) -> Result<DocumentUpdate> {
        let (canvas, depth, numbered) = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            let names = layer_names(s);
            (s.canvas, s.depth, move |base: &str| {
                numbered_name(&names, base)
            })
        };
        let or = |default: String| {
            if name.is_empty() {
                default
            } else {
                name.clone()
            }
        };
        let layer = match kind {
            NewLayer::Pixel => Layer::pixel(or(numbered("Layer")), canvas, depth),
            NewLayer::Group { mode } => Layer::group(or(numbered("Group")), mode.into()),
            NewLayer::Adjustment { json } => {
                let a: Adjustment = serde_json::from_str(&json)
                    .map_err(|e| failure(format!("adjustment JSON: {e}")))?;
                Layer::new(or(numbered(adjustment_title(&a))), LayerKind::Adjustment(a))
            }
            NewLayer::Fill { json } => {
                let f: Fill =
                    serde_json::from_str(&json).map_err(|e| failure(format!("fill JSON: {e}")))?;
                Layer::new(or(numbered(fill_title(&f))), LayerKind::Fill(f))
            }
        };
        self.edit(
            DocOp::AddLayer {
                parent: parent.map(LayerId),
                index: index.map_or(usize::MAX, |i| i as usize),
                layer,
            },
            None,
        )
    }

    /// Duplicates a layer (with its children) directly above it.
    pub fn duplicate_layer(&self, id: u64) -> Result<DocumentUpdate> {
        self.edit(DocOp::DuplicateLayer { id: LayerId(id) }, None)
    }

    pub fn remove_layer(&self, id: u64) -> Result<DocumentUpdate> {
        self.edit(DocOp::RemoveLayer { id: LayerId(id) }, None)
    }

    /// Moves a layer to `index` (0 = bottom, after removal) of `parent`.
    pub fn move_layer(&self, id: u64, parent: Option<u64>, index: u32) -> Result<DocumentUpdate> {
        self.edit(
            DocOp::MoveLayer {
                id: LayerId(id),
                parent: parent.map(LayerId),
                index: index as usize,
            },
            None,
        )
    }

    /// Replaces the common properties. `pass_through` on a group switches it
    /// to pass-through; a real mode on a pass-through group isolates it.
    pub fn set_props(&self, id: u64, props: LayerPropsRecord) -> Result<DocumentUpdate> {
        let (group, current) = {
            let st = self.shared.lock()?;
            let l = find(st.live().state(), id)?;
            (group_mode_of(l), l.props.clone())
        };
        let (mode, group_mode) = resolve_blend(group, &props.blend_mode, current.blend_mode)?;
        let mut p = current;
        p.name = props.name;
        p.visible = props.visible;
        p.opacity = unit(props.opacity)?;
        p.fill_opacity = unit(props.fill_opacity)?;
        p.blend_mode = mode;
        p.clipped = props.clipped;
        p.locks = props.locks.into();
        p.knockout = parse_knockout(&props.knockout)?;
        p.color_tag = props.color_tag;
        let mut ops = vec![DocOp::SetProps {
            id: LayerId(id),
            props: p,
        }];
        if let Some(m) = group_mode.filter(|m| Some(*m) != group) {
            ops.push(DocOp::SetGroupMode {
                id: LayerId(id),
                mode: m,
            });
        }
        self.edit(batch(ops), Some("Layer Properties"))
    }

    pub fn rename_layer(&self, id: u64, name: String) -> Result<DocumentUpdate> {
        self.edit_props(id, false, |p| {
            p.name = name;
            Ok(())
        })
    }

    pub fn set_visible(&self, id: u64, visible: bool) -> Result<DocumentUpdate> {
        self.edit_props(id, false, |p| {
            p.visible = visible;
            Ok(())
        })
    }

    /// Layer opacity 0…1. `interactive`: live only, no history node until
    /// `commit` (a slider drag).
    pub fn set_opacity(&self, id: u64, value: f32, interactive: bool) -> Result<DocumentUpdate> {
        let value = unit(value)?;
        self.edit_props(id, interactive, |p| {
            p.opacity = value;
            Ok(())
        })
    }

    /// Fill opacity 0…1, like `set_opacity`.
    pub fn set_fill_opacity(
        &self,
        id: u64,
        value: f32,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let value = unit(value)?;
        self.edit_props(id, interactive, |p| {
            p.fill_opacity = value;
            Ok(())
        })
    }

    /// A [`LayerNode::blend_mode`] name (`pass_through` for groups).
    pub fn set_blend_mode(&self, id: u64, mode: String) -> Result<DocumentUpdate> {
        let (group, current) = {
            let st = self.shared.lock()?;
            let l = find(st.live().state(), id)?;
            (group_mode_of(l), l.props.clone())
        };
        let (blend, group_mode) = resolve_blend(group, &mode, current.blend_mode)?;
        match group_mode.filter(|m| Some(*m) != group) {
            None => self.edit_props(id, false, |p| {
                p.blend_mode = blend;
                Ok(())
            }),
            Some(m) => {
                let mut props = current;
                props.blend_mode = blend;
                self.edit(
                    batch(vec![
                        DocOp::SetProps {
                            id: LayerId(id),
                            props,
                        },
                        DocOp::SetGroupMode {
                            id: LayerId(id),
                            mode: m,
                        },
                    ]),
                    Some("Blending Change"),
                )
            }
        }
    }

    pub fn set_group_mode(&self, id: u64, mode: DocGroupMode) -> Result<DocumentUpdate> {
        self.edit(
            DocOp::SetGroupMode {
                id: LayerId(id),
                mode: mode.into(),
            },
            None,
        )
    }

    pub fn set_clipped(&self, id: u64, clipped: bool) -> Result<DocumentUpdate> {
        self.edit_props(id, false, |p| {
            p.clipped = clipped;
            Ok(())
        })
    }

    pub fn set_locks(&self, id: u64, locks: LayerLocks) -> Result<DocumentUpdate> {
        self.edit_props(id, false, |p| {
            p.locks = locks.into();
            Ok(())
        })
    }

    /// Replaces an adjustment layer's parameters (`compositor::Adjustment`
    /// JSON); `interactive` as for `set_opacity`.
    pub fn set_adjustment_json(
        &self,
        id: u64,
        json: String,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let adjustment: Adjustment =
            serde_json::from_str(&json).map_err(|e| failure(format!("adjustment JSON: {e}")))?;
        self.edit_keyed(Pending::Adjustment(id), interactive, |s| {
            match find(s, id)?.kind {
                LayerKind::Adjustment(_) => Ok(DocOp::SetAdjustment {
                    id: LayerId(id),
                    adjustment,
                }),
                _ => Err(failure(format!("layer {id} is not an adjustment layer"))),
            }
        })
    }

    /// Replaces a fill layer's content (`compositor::Fill` JSON);
    /// `interactive` as for `set_opacity` (colour-well drags).
    pub fn set_fill_json(
        &self,
        id: u64,
        json: String,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let fill: Fill =
            serde_json::from_str(&json).map_err(|e| failure(format!("fill JSON: {e}")))?;
        self.edit_keyed(Pending::Fill(id), interactive, |s| {
            match find(s, id)?.kind {
                LayerKind::Fill(_) => Ok(DocOp::SetFill {
                    id: LayerId(id),
                    fill,
                }),
                _ => Err(failure(format!("layer {id} is not a fill layer"))),
            }
        })
    }

    /// Adds a layer mask (replacing an existing one).
    pub fn add_mask(&self, id: u64, mask: MaskInit) -> Result<DocumentUpdate> {
        let mask = {
            let st = self.shared.lock()?;
            let s = st.live().state();
            find(s, id)?;
            match mask {
                MaskInit::RevealAll => Mask::reveal_all(s.canvas, s.depth),
                MaskInit::HideAll => Mask::hide_all(s.canvas, s.depth),
                MaskInit::FromSelection => {
                    let sel = s
                        .selection
                        .as_ref()
                        .ok_or_else(|| failure("there is no selection"))?;
                    Mask {
                        raster: io::convert_raster(sel, s.depth)?,
                        ..Mask::reveal_all(s.canvas, s.depth)
                    }
                }
            }
        };
        self.edit(
            DocOp::SetMask {
                id: LayerId(id),
                mask: Some(mask),
            },
            Some("Add Layer Mask"),
        )
    }

    pub fn remove_mask(&self, id: u64) -> Result<DocumentUpdate> {
        {
            let st = self.shared.lock()?;
            Self::mask_of(st.live().state(), id)?;
        }
        self.edit(
            DocOp::SetMask {
                id: LayerId(id),
                mask: None,
            },
            Some("Delete Layer Mask"),
        )
    }

    pub fn set_mask_enabled(&self, id: u64, enabled: bool) -> Result<DocumentUpdate> {
        let mut mask = {
            let st = self.shared.lock()?;
            Self::mask_of(st.live().state(), id)?
        };
        mask.enabled = enabled;
        self.edit(
            DocOp::SetMask {
                id: LayerId(id),
                mask: Some(mask),
            },
            Some(if enabled {
                "Enable Layer Mask"
            } else {
                "Disable Layer Mask"
            }),
        )
    }

    /// Mask density 0…1 (Properties ▸ Masks).
    pub fn set_mask_density(&self, id: u64, density: f32) -> Result<DocumentUpdate> {
        let density = unit(density)?;
        let mut mask = {
            let st = self.shared.lock()?;
            Self::mask_of(st.live().state(), id)?
        };
        mask.density = density;
        self.edit(
            DocOp::SetMask {
                id: LayerId(id),
                mask: Some(mask),
            },
            Some("Mask Density"),
        )
    }

    /// The mask's link chain. Session state only (translation is not
    /// modelled by the compositor yet, so it has no effect on pixels and is
    /// not saved); records no history.
    pub fn set_mask_linked(&self, id: u64, linked: bool) -> Result<()> {
        let mut st = self.shared.lock()?;
        Self::mask_of(st.live().state(), id)?;
        if linked {
            st.unlinked_masks.remove(&id);
        } else {
            st.unlinked_masks.insert(id);
        }
        st.epoch += 1;
        let epoch = st.epoch;
        self.shared.render.notify_layers(vec![id], epoch);
        Ok(())
    }

    /// Merges a layer into the one below it (same parent) as one history
    /// node: the pair is composited on its own (the lower layer's mask
    /// baked in, its opacity/fill applied later as before) into a pixel
    /// layer that keeps the lower layer's id, name and properties.
    pub fn merge_down(&self, id: u64) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            io::merge_down_op(st.live().state(), LayerId(id))?
        };
        self.edit(op, Some("Merge Down"))
    }

    /// Flattens every visible layer into one opaque Background layer (over
    /// white, as Photoshop does).
    pub fn flatten(&self) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            io::flatten_op(st.live().state())?
        };
        self.edit(op, Some("Flatten Image"))
    }

    /// Puts sibling layers into a new group at the topmost one's position.
    pub fn group_layers(&self, ids: Vec<u64>, name: String) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            group_op(st.live().state(), &ids, name)?
        };
        self.edit(op, Some("Group Layers"))
    }

    /// Replaces a group by its children (the group's own properties go).
    pub fn ungroup_layer(&self, id: u64) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            ungroup_op(st.live().state(), id)?
        };
        self.edit(op, Some("Ungroup Layers"))
    }

    /// A rectangular marquee selection (level-0 pixels), its edges ramped
    /// over `feather` pixels (0: hard edges). Other selection tools arrive
    /// with M5-11.
    pub fn set_selection_rect(
        &self,
        x: i64,
        y: i64,
        width: i64,
        height: i64,
        feather: f32,
    ) -> Result<DocumentUpdate> {
        let canvas = self.shared.lock()?.live().state().canvas;
        let raster = io::rect_selection(canvas, Rect::new(x, y, x + width, y + height), feather)?;
        self.edit(
            DocOp::SetSelection {
                selection: Some(raster),
            },
            Some("Rectangular Marquee"),
        )
    }

    pub fn clear_selection(&self) -> Result<DocumentUpdate> {
        self.edit(DocOp::SetSelection { selection: None }, Some("Deselect"))
    }

    /// Records the pending interactive edits as one history node labelled
    /// `label` (empty: the op's own label). Without pending edits nothing is
    /// recorded.
    pub fn commit(&self, label: String) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let applied = self.commit_pending(&mut st, Some(&label))?;
        let recorded = applied.is_some();
        let mut update = self.update(&mut st, &before, applied.as_ref(), recorded);
        if !recorded {
            update.dirty_rect = None;
        }
        Ok(update)
    }

    // ───────────────────────────── history ─────────────────────────────

    pub fn undo(&self) -> Result<DocumentUpdate> {
        self.history_move(|d| Ok(d.undo()))
    }

    pub fn redo(&self) -> Result<DocumentUpdate> {
        self.history_move(|d| Ok(d.redo()))
    }

    /// Every retained history state in creation order.
    pub fn history_items(&self) -> Result<Vec<DocHistoryItem>> {
        let st = self.shared.lock()?;
        let h = st.doc.history();
        Ok(h.nodes()
            .map(|n| DocHistoryItem {
                id: n.id,
                label: st
                    .labels
                    .get(&n.id)
                    .cloned()
                    .unwrap_or_else(|| n.label.clone()),
                parent: n.parent,
                is_current: n.id == h.current(),
                author: "user".into(),
            })
            .collect())
    }

    /// Jumps to any retained history state.
    pub fn checkout_history(&self, id: u64) -> Result<DocumentUpdate> {
        self.history_move(|d| {
            d.checkout(id)?;
            Ok(true)
        })
    }

    /// Names the current state (after committing a pending drag).
    pub fn snapshot(&self, name: String) -> Result<()> {
        if name.trim().is_empty() {
            return Err(failure("snapshot name is empty"));
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        if self.commit_pending(&mut st, None)?.is_some() {
            st.epoch += 1;
            let epoch = st.epoch;
            self.shared.render.request(Vec::new(), true, epoch);
        }
        st.doc.snapshot(name);
        Ok(())
    }

    pub fn snapshots(&self) -> Result<Vec<String>> {
        Ok(self
            .shared
            .lock()?
            .doc
            .history()
            .snapshots()
            .keys()
            .cloned()
            .collect())
    }

    pub fn restore_snapshot(&self, name: String) -> Result<DocumentUpdate> {
        self.history_move(|d| {
            d.restore_snapshot(&name)?;
            Ok(true)
        })
    }

    /// Caps retained history states (at least 2); the current state and
    /// snapshots are never pruned.
    pub fn set_max_states(&self, max_states: u32) -> Result<()> {
        self.shared.lock()?.doc.set_max_states(max_states as usize);
        Ok(())
    }

    /// Bytes of distinct tile buffers held by the retained history.
    pub fn history_memory_bytes(&self) -> Result<u64> {
        Ok(self.shared.lock()?.doc.history_bytes() as u64)
    }

    // ─────────────────────────── presentation ───────────────────────────

    /// Surface size for a fit-to-window viewport of `width × height` device
    /// pixels: the coarsest level whose extent covers it (level 0 when the
    /// canvas is smaller). Zoomed viewports allocate surfaces of their own
    /// size and name the region with `set_viewport`.
    pub fn plan_surface(&self, width: u32, height: u32) -> Result<DocSurfacePlan> {
        let canvas = self.shared.lock()?.live().state().canvas;
        let level = (0..render::MAX_VIEW_LEVEL)
            .rev()
            .find(|&l| {
                let e = canvas.at_level(l);
                e.width >= width && e.height >= height
            })
            .unwrap_or(0);
        let e = canvas.at_level(level);
        Ok(DocSurfacePlan {
            level,
            width: e.width,
            height: e.height,
        })
    }

    /// Adds an RGBA8 IOSurface (straight alpha, display-encoded sRGB) to the
    /// frame ring; attach two or three of one size so the host never samples
    /// the surface being written. A different size replaces the ring. The
    /// first surface of a ring renders a frame.
    pub fn attach_surface(&self, iosurface_id: u32, width: u32, height: u32) -> Result<()> {
        let surface = Arc::new(Surface::lookup(iosurface_id, width, height).map_err(failure)?);
        let mut st = self.shared.lock()?;
        st.open()?;
        let view = &mut st.view;
        if view
            .surfaces
            .first()
            .is_some_and(|f| f.width() != width || f.height() != height)
        {
            view.surfaces.clear();
        }
        view.surfaces.retain(|s| s.id() != surface.id());
        if view.surfaces.len() >= 3 {
            view.surfaces.remove(0);
        }
        let first = view.surfaces.is_empty();
        view.surfaces.push(surface);
        if first {
            let epoch = st.epoch;
            self.shared.render.request(Vec::new(), false, epoch);
        }
        Ok(())
    }

    /// Shows `width × height` pixels at `(x, y)` of pyramid `level` (0 = full
    /// resolution; each level halves) top-left in the surfaces, clipped to
    /// the level and the surface size. `zoom` (1 = 100 %) is echoed in
    /// frames. Until the first call the whole canvas is shown at the finest
    /// level that fits the surfaces.
    pub fn set_viewport(
        &self,
        level: u8,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        zoom: f64,
    ) -> Result<()> {
        if level >= render::MAX_VIEW_LEVEL {
            return Err(failure(format!(
                "level must be below {}",
                render::MAX_VIEW_LEVEL
            )));
        }
        if width == 0 || height == 0 {
            return Err(failure("viewport must not be empty"));
        }
        let mut st = self.shared.lock()?;
        st.view.viewport = Some(render::Viewport {
            level,
            x,
            y,
            width,
            height,
            zoom: if zoom.is_finite() && zoom > 0.0 {
                zoom
            } else {
                1.0
            },
        });
        let epoch = st.epoch;
        self.shared.render.request(Vec::new(), false, epoch);
        Ok(())
    }

    /// The EDR headroom of the display. Document frames are SDR RGBA8 for now
    /// (the compositor's RGBA16F presentation arrives with M5-08), so this is
    /// stored and has no effect yet.
    pub fn set_display_headroom(&self, headroom: f32) -> Result<()> {
        self.shared.lock()?.view.display_headroom = if headroom.is_finite() {
            headroom.max(1.0)
        } else {
            1.0
        };
        Ok(())
    }

    /// Renders a frame of the current state (first paint, or after the host
    /// discarded surface contents).
    pub fn refresh(&self) -> Result<()> {
        let st = self.shared.lock()?;
        self.shared.render.request(Vec::new(), false, st.epoch);
        Ok(())
    }

    /// Releases every surface; nothing is rendered until one is attached.
    pub fn detach_surfaces(&self) {
        if let Ok(mut st) = self.shared.lock() {
            st.view.surfaces.clear();
            st.view.next = 0;
        }
    }

    /// An RGBA8 IOSurface (straight alpha, sRGB-encoded) with the layer's own
    /// content — opacity, mode and mask ignored — at the coarsest pyramid
    /// level that fits in `max_px` (long edge; the surface is exactly that
    /// level's extent). Cached per layer revision and size: an unchanged
    /// layer returns the same surface. The session retains the surface until
    /// a newer thumbnail replaces it or the session closes; hosts that keep
    /// it longer retain it with `IOSurfaceLookup`.
    pub fn layer_thumbnail(&self, id: u64, max_px: u32) -> Result<u32> {
        render::thumbnail(&self.shared, render::ThumbKind::Layer(id), max_px)
    }

    /// The layer mask as a grey RGBA8 IOSurface (white = revealed), cached
    /// like `layer_thumbnail`.
    pub fn mask_thumbnail(&self, id: u64, max_px: u32) -> Result<u32> {
        render::thumbnail(&self.shared, render::ThumbKind::Mask(id), max_px)
    }

    /// The whole composite, like `layer_thumbnail` (cached per document state).
    pub fn composite_thumbnail(&self, max_px: u32) -> Result<u32> {
        render::thumbnail(&self.shared, render::ThumbKind::Composite, max_px)
    }

    // ────────────────────────────── output ──────────────────────────────

    /// Writes the document to its path (committing a pending drag first).
    pub fn save(&self) -> Result<()> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let path = st
            .path
            .clone()
            .ok_or_else(|| failure("the document has no file yet: use save_as"))?;
        self.save_locked(&mut st, &path)
    }

    /// Writes `.tessera-doc`, or PSD/PSB when the path ends in `.psd`/`.psb`
    /// (with the flattened composite for compatibility), and makes it the
    /// document's path.
    pub fn save_as(&self, path: String) -> Result<()> {
        let path = PathBuf::from(path);
        io::save_kind(&path)?;
        let mut st = self.shared.lock()?;
        st.open()?;
        self.save_locked(&mut st, &path)?;
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        st.title = title;
        st.path = Some(path.clone());
        drop(st);
        // Later opens of the new file find this session.
        if let (Ok(canon), Some(engine)) =
            (std::fs::canonicalize(&path), self.shared.engine.upgrade())
            && let Some(me) = engine.document_session(self.shared.id.clone())
        {
            engine
                .documents
                .lock()
                .by_key
                .insert(canon.to_string_lossy().into_owned(), Arc::downgrade(&me));
        }
        Ok(())
    }

    /// Exports the flattened composite at full resolution: PNG/TIFF keep
    /// transparency (8-bit for 8-bit documents, else 16-bit), JPEG is
    /// flattened over white; `quality` is JPEG 1–100. Colours are converted
    /// from the document profile to `color` and the profile is embedded.
    pub fn export_flat(
        &self,
        path: String,
        format: ExportFormat,
        quality: u8,
        color: ExportColor,
    ) -> Result<()> {
        let doc = {
            let st = self.shared.lock()?;
            st.open()?;
            st.live().clone()
        };
        io::export_flat(&doc, std::path::Path::new(&path), format, quality, color)
    }

    /// Stops rendering, releases the surfaces and forgets the session.
    /// Unsaved changes are discarded (the host asks first).
    pub fn close(&self) {
        if let Some(engine) = self.shared.engine.upgrade() {
            engine.documents.remove(&self.shared.id);
        }
        self.shutdown();
    }
}

impl DocumentSession {
    fn save_locked(&self, st: &mut State, path: &std::path::Path) -> Result<()> {
        if self.commit_pending(st, None)?.is_some() {
            st.epoch += 1;
            let epoch = st.epoch;
            self.shared.render.request(Vec::new(), true, epoch);
        }
        io::save(&st.doc, path)?;
        st.saved_node = Some(st.doc.history().current());
        Ok(())
    }
}

/// Test and bench support (not exported over UniFFI).
impl DocumentSession {
    /// Renders `level` of the live state synchronously and reads it back as
    /// interleaved straight f32 RGBA (`ResidentRenderer::read_level`, or
    /// the CPU compositor without Metal).
    #[doc(hidden)]
    pub fn read_level(&self, level: u8) -> Result<(u32, u32, Vec<f32>)> {
        let st = self.shared.lock()?;
        self.shared.render.read_level(st.live(), level)
    }

    /// The live document state (tests compare against the CPU compositor).
    #[doc(hidden)]
    pub fn document_state(&self) -> Result<Arc<DocState>> {
        Ok(self.shared.lock()?.live().state().clone())
    }

    /// Thumbnails rendered so far (cache misses).
    #[doc(hidden)]
    pub fn thumbnail_renders(&self) -> u64 {
        self.shared.render.thumbnail_renders()
    }

    /// Blocks until the render thread is idle.
    #[doc(hidden)]
    pub fn wait_idle(&self) {
        self.shared.render.wait_idle();
    }
}

impl Engine {
    /// Wraps an existing document in a session (tests and benches).
    #[doc(hidden)]
    pub fn adopt_document(self: &Arc<Self>, doc: Document, title: String) -> Arc<DocumentSession> {
        self.register_document(
            None,
            Opened {
                doc,
                title,
                path: None,
                source_image_id: None,
                origin: "Open",
                unsaved: false,
            },
        )
    }
}

fn batch(mut ops: Vec<DocOp>) -> DocOp {
    if ops.len() == 1 {
        ops.pop().expect("one op")
    } else {
        DocOp::Batch(ops)
    }
}

fn unit(v: f32) -> Result<f32> {
    if v.is_finite() {
        Ok(v.clamp(0.0, 1.0))
    } else {
        Err(failure("value must be finite"))
    }
}

/// Every layer name in the document (all depths).
fn layer_names(s: &DocState) -> Vec<String> {
    fn walk(layers: &[Arc<Layer>], out: &mut Vec<String>) {
        for l in layers {
            out.push(l.props.name.clone());
            if let LayerKind::Group { children, .. } = &l.kind {
                walk(children, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&s.root, &mut out);
    out
}

/// Photoshop's default names: `<base> <n>` numbered per base, one above the
/// highest `<base> <n>` in the document ("Levels 1", "Levels 2", "Layer 2").
fn numbered_name(names: &[String], base: &str) -> String {
    let prefix = format!("{base} ");
    let n = names
        .iter()
        .filter_map(|n| n.strip_prefix(&prefix)?.parse::<u64>().ok())
        .max()
        .unwrap_or(0);
    format!("{base} {}", n + 1)
}

/// Photoshop's adjustment layer names.
fn adjustment_title(a: &Adjustment) -> &'static str {
    match a {
        Adjustment::Levels { .. } => "Levels",
        Adjustment::Curves { .. } => "Curves",
        Adjustment::HueSaturation { .. } => "Hue/Saturation",
        Adjustment::Exposure { .. } => "Exposure",
        Adjustment::Invert => "Invert",
        Adjustment::Posterize { .. } => "Posterize",
        Adjustment::Threshold { .. } => "Threshold",
        Adjustment::ChannelMixer { .. } => "Channel Mixer",
    }
}

/// Photoshop's fill layer names.
fn fill_title(f: &Fill) -> &'static str {
    match f {
        Fill::Solid { .. } => "Color Fill",
        Fill::Gradient { .. } => "Gradient Fill",
        Fill::Pattern { .. } => "Pattern Fill",
    }
}

/// A blend name for a layer: `(mode, group mode to set)`. `pass_through`
/// keeps the stored mode and makes a group pass-through; a real mode on a
/// pass-through group isolates it.
fn resolve_blend(
    group: Option<GroupMode>,
    name: &str,
    current: BlendMode,
) -> Result<(BlendMode, Option<GroupMode>)> {
    if name == PASS_THROUGH {
        return match group {
            Some(_) => Ok((current, Some(GroupMode::PassThrough))),
            None => Err(failure("pass_through applies to groups only")),
        };
    }
    let mode = parse_blend(name)?;
    Ok((mode, group.map(|_| GroupMode::Isolated)))
}

fn group_op(s: &DocState, ids: &[u64], name: String) -> Result<DocOp> {
    if ids.is_empty() {
        return Err(failure("nothing to group"));
    }
    let mut located = Vec::new();
    for &id in ids {
        let (parent, index) = s
            .locate(LayerId(id))
            .ok_or_else(|| failure(format!("layer {id} not found")))?;
        located.push((parent, index, id));
    }
    let parent = located[0].0;
    if located.iter().any(|(p, _, _)| *p != parent) {
        return Err(failure("grouped layers must share a parent"));
    }
    located.sort_by_key(|(_, i, _)| *i);
    located.dedup_by_key(|(_, _, id)| *id);
    let top = located.last().map_or(0, |(_, i, _)| *i);
    let gid = LayerId(s.next_id);
    let mut group = Layer::group(
        if name.is_empty() {
            numbered_name(&layer_names(s), "Group")
        } else {
            name
        },
        GroupMode::PassThrough,
    );
    group.id = gid;
    let mut ops = vec![DocOp::AddLayer {
        parent,
        index: top + 1,
        layer: group,
    }];
    for (_, _, id) in &located {
        ops.push(DocOp::MoveLayer {
            id: LayerId(*id),
            parent: Some(gid),
            index: usize::MAX,
        });
    }
    Ok(DocOp::Batch(ops))
}

fn ungroup_op(s: &DocState, id: u64) -> Result<DocOp> {
    let layer = find(s, id)?;
    let children = layer
        .children()
        .ok_or_else(|| failure(format!("layer {id} is not a group")))?;
    let (parent, index) = s.locate(LayerId(id)).expect("found");
    let mut ops: Vec<DocOp> = children
        .iter()
        .enumerate()
        .map(|(k, c)| DocOp::MoveLayer {
            id: c.id,
            parent,
            index: index + k,
        })
        .collect();
    ops.push(DocOp::RemoveLayer { id: LayerId(id) });
    Ok(DocOp::Batch(ops))
}

/// Convenience for building rasters from normalized straight RGBA rows.
pub(crate) fn raster_from_rgba(
    extent: Extent,
    depth: compositor::Depth,
    rgba: &[f32],
    skip_transparent: bool,
) -> Result<Raster> {
    let mut r = Raster::new(extent, 4, depth, 0.0);
    let (cols, rows) = extent.tile_grid(TILE_SIZE);
    for ty in 0..rows {
        for tx in 0..cols {
            let layout = r.layout(tx, ty);
            let (w, h) = (layout.extent.width as usize, layout.extent.height as usize);
            let n = w * h;
            let mut planes = vec![0.0f32; 4 * n];
            let mut any = false;
            for y in 0..h {
                let row = (ty as usize * TILE_SIZE as usize + y) * extent.width as usize
                    + tx as usize * TILE_SIZE as usize;
                for x in 0..w {
                    let p = &rgba[(row + x) * 4..(row + x) * 4 + 4];
                    any |= p[3] != 0.0;
                    for c in 0..4 {
                        planes[c * n + y * w + x] = p[c];
                    }
                }
            }
            if skip_transparent && !any {
                continue;
            }
            let tile = tile_from_f32(TileCoord::new(0, tx, ty), layout, depth, planes)?;
            r.set_slot(tx, ty, Some(tile), 0)?;
        }
    }
    Ok(r)
}
