//! `DevelopSession` over UniFFI: live develop settings, progressive viewport
//! rendering into host IOSurfaces, histogram, history and persistence.
//!
//! # Rendering
//!
//! The session owns a [`RawImage`] and shares the engine's [`Renderer`]
//! (measured CPU/Metal selection, with `TESSERA_RENDER_BACKEND` override; see
//! `Engine::develop_renderer` for the measurements) and memo cache. Every
//! settings change bumps a generation, cancels the previous job and submits a
//! [`ProgressiveRenderJob`] at [`Priority::Viewport`] on the engine's shared
//! scheduler. Tiles are written straight into the next surface of the host's
//! ring (see [`crate::surface`] for the RGBA8 contract); when a level is
//! complete the listener's `frame_ready` names the surface and the valid
//! top-left region. Stale generations never write or notify.
//!
//! The *screen level* is the coarsest output-pyramid level that still covers
//! the viewport in device pixels ([`DevelopSession::plan_surface`]); surfaces
//! are exactly that level's extent. Tone-only changes rerun only Tone and
//! Output on the memoized WhiteBalance tiles of that level. Resident-capable
//! recipes (every panel except crop/straighten and local adjustments on the
//! Metal backend, including Texture/Clarity/Dehaze) drag at the screen level;
//! non-resident recipes on a screen level above [`DRAG_BUDGET_PX`] drag one
//! level coarser. `commit` (mouse-up) refines to the screen level.
//!
//! # Develop panels (M2-13)
//!
//! [`renderable`] passes Basic, Detail, tone curves, HSL, grading, post-crop
//! effects and crop/straighten, sanitized so a recipe can never fail a render.
//! A crop changes the output extent: frames report the cropped picture as
//! `display_width × display_height` (top-left in the uncropped-size surface).
//! Interactive renders adapt their level per [`RenderClass`]: they prefer the
//! screen level with a [`PREFERRED_BUDGET_MS`] target, drop one level only
//! when measured frames exceed [`INTERACTIVE_BUDGET_MS`] (two in a row; the
//! first frame at a level, which refills its caches, is not counted), and
//! return finer once the next finer
//! level is predicted to fit the preferred budget. Each frame's level and
//! `render_ms` are reported in [`FrameInfo`] (the render readout). Non-resident
//! heavy M2 operators start at a proxy level. A drag queues behind its
//! in-flight frame instead of cancelling it, so slow frames still show
//! progress. The crop tool renders the whole frame (`set_crop_editing`); the
//! masking preview draws the sharpening gate (`set_masking_preview`); a 1:1
//! crop renders into its own surface (`render_detail_preview`); history steps
//! can be checked out or toggled off/on (`history_items`,
//! `checkout_history`, `set_history_step_enabled`).
//!
//! # EDR presentation (M2-22)
//!
//! The host picks the surface contract per ring (see [`crate::surface`]):
//! RGBA8 rings get the SDR Output stage exactly as before; RGBA16F rings get
//! [`RenderOutput::DisplayLinear`] tone-mapped for [`presentation`]'s
//! headroom — the recipe's `output.hdr` / `output.hdr_headroom_stops`,
//! capped by the display headroom the host reports through
//! [`DevelopSession::set_display_headroom`]. HDR off on a float ring renders
//! the SDR tone curve unencoded (headroom 1). Exports, previews, the 1:1
//! detail crop and snapshots stay SDR: only viewport jobs ever request the
//! float output.
//!
//! # History and persistence
//!
//! `set_settings` changes only the live state. `commit` records one
//! [`Recipe::edit`] history entry for everything since the last commit, so a
//! whole drag is one undo step. Commits, undo/redo, reset and snapshots
//! schedule a debounced save ([`SAVE_DEBOUNCE`]) on the session's writer
//! thread: recipe JSON + XMP through `sidecar`, index refresh, and edited
//! previews keyed by the new recipe hash.

use crate::{BridgeError, Engine, Result, catalog, failure, now_ms, parse_id, surface::Surface};

#[path = "masks.rs"]
mod masks;
use engine_api::{
    color::ColorMatrix3,
    id::{HistoryEntryId, HistoryGroupId, ImageId},
    jobs::{Job, JobContext, JobHandle, Priority, Scheduler},
    recipe::{
        DevelopSettings, EditMeta, ProcessVersion, Recipe,
        history::{Author, HistoryEntry, apply_change},
        settings::{
            Curve, DemosaicMethod, DisplayTransform, GradeWheel, HighlightReconstruction, HueBands,
            ParametricCurve, WhiteBalanceMode,
        },
    },
    stage::StageId,
    tile::{TILE_SIZE, Tile},
};
use image_core::{
    Headroom, PixelRect, ProgressiveRenderJob, RawImage, RenderOutput, Renderer, Viewport,
    render::MAX_LEVEL,
};
pub(crate) use masks::SegmenterSlot;
pub use masks::*;
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread::{JoinHandle, ThreadId},
    time::{Duration, Instant},
};

#[cfg(test)]
static FAIL_AFTER_DEVELOP_RECIPE: Mutex<std::collections::BTreeMap<PathBuf, usize>> =
    Mutex::new(std::collections::BTreeMap::new());
#[cfg(test)]
type PostRecipePause = Arc<(Mutex<(bool, bool)>, Condvar)>;
#[cfg(test)]
static PAUSE_AFTER_DEVELOP_RECIPE: Mutex<std::collections::BTreeMap<PathBuf, PostRecipePause>> =
    Mutex::new(std::collections::BTreeMap::new());

#[cfg(test)]
fn injected_post_recipe_failure(path: &Path) -> Result<()> {
    let pause = PAUSE_AFTER_DEVELOP_RECIPE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(path)
        .cloned();
    if let Some(pause) = pause {
        let (state, cv) = &*pause;
        let mut state = state.lock().unwrap();
        state.0 = true;
        cv.notify_all();
        while !state.1 {
            state = cv.wait(state).unwrap();
        }
    }
    let mut fault = FAIL_AFTER_DEVELOP_RECIPE
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(remaining) = fault.get_mut(path)
        && *remaining > 0
    {
        *remaining -= 1;
        return Err(failure("injected post-recipe failure"));
    }
    Ok(())
}

/// Interactive drags on screen levels larger than this render one level
/// coarser until the drag is committed.
pub const DRAG_BUDGET_PX: u64 = 4_200_000;
/// Interactive frames should land within one display refresh: a measured
/// frame over this drops the drag one level coarser.
pub const INTERACTIVE_BUDGET_MS: f64 = 16.0;
/// Target frame time at the preferred (screen) level, leaving headroom for
/// presentation within one refresh. Finer levels are chosen only when their
/// predicted frame time fits this budget.
pub const PREFERRED_BUDGET_MS: f64 = 12.0;
/// Pixel ratio between adjacent pyramid levels (frame-time prediction).
const LEVEL_COST_RATIO: f64 = 4.0;
/// At most this many levels coarser than the screen level while dragging.
const MAX_DRAG_OFFSET: u8 = 4;
/// Quiet period after the last history change before sidecars are written.
pub const SAVE_DEBOUNCE: Duration = Duration::from_millis(400);

#[derive(Clone, Debug, uniffi::Record)]
pub struct DevelopInfo {
    pub image_id: String,
    /// Active area (level 0), sensor orientation.
    pub width: u32,
    pub height: u32,
    /// EXIF orientation 1–8; surfaces stay in sensor orientation.
    pub orientation: u16,
    /// As-shot white balance expressed on the Temperature/Tint sliders.
    pub as_shot_temperature: f32,
    pub as_shot_tint: f32,
    /// "CPU ×<threads>" or "Metal (<adapter>)".
    pub backend: String,
}

/// Surface size to allocate for a viewport: exactly the extent of `level`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SurfacePlan {
    pub level: u8,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FrameInfo {
    /// Surface written, or 0 when no surface is attached.
    pub surface_id: u32,
    pub level: u8,
    /// Valid region, anchored top-left in the surface.
    pub width: u32,
    pub height: u32,
    /// Coarsest level of this render (the readout's "L3 → L2").
    pub first_level: u8,
    /// Last level this render will deliver.
    pub is_final: bool,
    /// From the settings change to this level being complete.
    pub render_ms: f64,
    pub generation: u64,
    /// Earliest pipeline stage the change invalidated, if any.
    pub dirty_stage: Option<String>,
    /// Size of the whole picture at the render's finest level: the cropped
    /// output extent (the level extent when uncropped or in the crop tool).
    /// Hosts aspect-fit this, not the surface, so crops display correctly.
    pub display_width: u32,
    pub display_height: u32,
    /// The frame shows a diagnostic overlay (e.g. the sharpening mask), not
    /// the developed image; it has no histogram of its own.
    pub is_overlay: bool,
}

/// One step of the develop history, as shown in the History panel.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HistoryItem {
    pub id: u64,
    pub label: String,
    /// "user", "agent:<name>", "import:<source>", "preset:<style>" or "sync:<image>".
    pub author: String,
    /// Named group (e.g. one agent run), if any.
    pub group: Option<String>,
    pub timestamp_ms: i64,
    /// On the path from the base to the current state (false: undone steps
    /// that redo would reapply).
    pub applied: bool,
    pub is_head: bool,
    /// False when the step has been turned off with a step toggle.
    pub enabled: bool,
    /// The step is itself a toggle of another step (shown as such, not toggleable).
    pub toggles: Option<u64>,
    /// The step sets the amount (0...1) of a history group (the agent group's
    /// fade slider); not toggleable.
    pub group_amount: Option<f64>,
    /// Id of the named group, when `group` is set.
    pub group_id: Option<u32>,
    /// One-line explanation (agents supply one per step); markers are omitted.
    pub rationale: Option<String>,
}

/// A named history group on the current lineage (e.g. "Agent base edit"), for
/// its amount slider and per-step toggles (docs/10 §2 "fine-tune surface").
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HistoryGroupState {
    pub group_id: u32,
    pub name: String,
    /// Current amount in [0, 1] (the latest amount step, 1 without one).
    pub amount: f64,
    /// The group's steps (history entry ids), oldest first.
    pub steps: Vec<u64>,
    /// `DevelopSettings` JSON with the group at 0 % and at 100 %, everything
    /// else (other steps, toggles, other groups' amounts) as it is now. A host
    /// previews an amount `a` as `without + a · (with − without)` on numbers
    /// (rounded for integers) and `with` for other values once `a ≥ 0.5`;
    /// `commit_group_amount` records exactly that.
    pub without_json: String,
    pub with_json: String,
}

/// A rendered 1:1 detail crop.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct DetailPreview {
    /// Top-left of the crop in level-0 active-area pixels (sensor orientation).
    pub x: u32,
    pub y: u32,
    /// Pixels written, anchored top-left in the surface.
    pub width: u32,
    pub height: u32,
}

/// 256-bin histograms of the displayed (sRGB-encoded) frame.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct Histogram {
    pub red: Vec<u32>,
    pub green: Vec<u32>,
    pub blue: Vec<u32>,
    /// Rec. 709 weights on the encoded values.
    pub luminance: Vec<u32>,
    pub level: u8,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct HistoryState {
    pub can_undo: bool,
    pub can_redo: bool,
    /// Entries ever recorded (the history is append-only).
    pub entries: u32,
    /// Label of the entry at the head, if any.
    pub head_label: Option<String>,
    pub snapshots: Vec<String>,
    /// Live settings differ from the last commit.
    pub uncommitted: bool,
}

/// Callbacks run on engine worker threads, never while a session lock is held.
#[uniffi::export(with_foreign)]
pub trait DevelopListener: Send + Sync {
    fn frame_ready(&self, frame: FrameInfo);
    fn render_failed(&self, message: String);
    /// Recipe + XMP written; `recipe_hash` keys the refreshed previews.
    fn saved(&self, recipe_hash: String);
}

struct Frame {
    level: u8,
    settings: DevelopSettings,
    // Surface frames retain immutable render metadata, not unconditional pixels.
    // Saving an edited loupe/preview materializes this exact recipe on demand.
    tiles: Option<Vec<Tile>>,
}

impl Frame {
    fn pixels(
        &self,
        materialize: impl FnOnce(&DevelopSettings, u8) -> engine_api::EngineResult<Vec<Tile>>,
    ) -> engine_api::EngineResult<std::borrow::Cow<'_, [Tile]>> {
        match &self.tiles {
            Some(tiles) => Ok(std::borrow::Cow::Borrowed(tiles)),
            None => Ok(std::borrow::Cow::Owned(materialize(
                &self.settings,
                self.level,
            )?)),
        }
    }
}

struct State {
    recipe: Recipe,
    live: DevelopSettings,
    /// Calibration/model backend installed for this session, never persisted as a recipe edit.
    cfa_configured: bool,
    /// Renderable settings of the last submitted render.
    rendered: Option<DevelopSettings>,
    rendered_level: Option<u8>,
    surfaces: Vec<Arc<Surface>>,
    next_surface: SurfaceCursor,
    screen_level: u8,
    job: Option<JobHandle>,
    generation: u64,
    histogram: Histogram,
    frame: Option<Arc<Frame>>,
    closed: bool,
    closing: bool,
    /// Crop tool active: render without geometry.
    crop_editing: bool,
    render_uncorrected: bool,
    render_depth_visualisation: bool,
    /// Show the sharpening edge mask instead of the image.
    masking_preview: bool,
    /// Levels coarser than the screen level that interactive renders of
    /// each [`RenderClass`] use, adapted to the measured frame times.
    drag: [DragLevel; 2],
    /// Generation of the interactive render still in flight, if any.
    interactive_in_flight: Option<u64>,
    /// Interactive changes arrived while it ran: render the latest state
    /// when it completes instead of cancelling it (no starvation when a
    /// frame takes longer than the host's change cadence).
    interactive_pending: bool,
    /// Brush stroke in progress.
    masks: masks::MaskState,
    /// EDR headroom of the host's display (linear multiple of SDR white).
    display_headroom: f32,
}

/// Settings whose interactive cost differs by an order of magnitude: the
/// light class stays on the fused (GPU-resident) path; heavy settings use
/// the whole-level M2 operator chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderClass {
    Light = 0,
    Heavy = 1,
}

impl RenderClass {
    fn of(s: &DevelopSettings, resident: bool) -> Self {
        // Residency is backend/image-specific. Do not penalize fused curves,
        // grading or effects with the old heavy-path starting proxy. Measured
        // frame times still adapt the level if this device misses its budget.
        if resident {
            return Self::Light;
        }
        let t = &s.tone;
        if s.color == Default::default()
            && s.effects == Default::default()
            && s.geometry == Default::default()
            && t.texture == 0.0
            && t.clarity == 0.0
            && t.dehaze == 0.0
            && t.curves == Default::default()
            && s.locals.adjustments.is_empty()
        {
            Self::Light
        } else {
            Self::Heavy
        }
    }
}

/// Adaptive interactive level of one render class: the offset below the
/// screen level and a smoothed frame time measured at that offset.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct DragLevel {
    offset: u8,
    ms: Option<f64>,
    /// Consecutive frames over [`INTERACTIVE_BUDGET_MS`].
    misses: u8,
    /// The first frame at a level refills that level's caches (WB, Detail,
    /// Dehaze statistics); it is not evidence of the level's frame time.
    warm: bool,
}

impl DragLevel {
    const fn starting_at(offset: u8) -> Self {
        Self {
            offset,
            ms: None,
            misses: 0,
            warm: false,
        }
    }

    /// Records an interactive frame. The first frame at a level is ignored
    /// (cache refill). Coarser only after two consecutive frames miss the
    /// refresh ([`INTERACTIVE_BUDGET_MS`]): an isolated spike never drops the
    /// level. Finer only when the next finer level (four times the pixels) is
    /// predicted from the smoothed frame time to fit [`PREFERRED_BUDGET_MS`].
    fn record(&mut self, ms: f64) {
        if !self.warm {
            self.warm = true;
            return;
        }
        let smoothed = self.ms.map_or(ms, |m| 0.6 * m + 0.4 * ms);
        let misses = if ms > INTERACTIVE_BUDGET_MS {
            self.misses.saturating_add(1)
        } else {
            0
        };
        let next = if misses >= 2 {
            (self.offset + 1).min(MAX_DRAG_OFFSET)
        } else if smoothed * LEVEL_COST_RATIO <= PREFERRED_BUDGET_MS {
            self.offset.saturating_sub(1)
        } else {
            self.offset
        };
        *self = if next == self.offset {
            Self {
                offset: next,
                ms: Some(smoothed),
                misses,
                warm: true,
            }
        } else {
            Self::starting_at(next)
        };
    }
}

impl State {
    /// Settings the viewport draws for the live state.
    fn drawn(&self) -> DevelopSettings {
        let mut drawn = session_renderable(&self.live, !self.crop_editing, self.cfa_configured);
        if self.render_uncorrected {
            drawn.geometry = Default::default();
            drawn.lens = Default::default();
            drawn.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
            drawn.lens.remove_chromatic_aberration = false;
        }
        drawn
    }

    /// Whether the attached ring is the EDR (RGBA16F) contract.
    fn float_ring(&self) -> bool {
        self.surfaces.first().is_some_and(|s| s.is_float())
    }

    /// Display output the viewport renders for the live state.
    fn output(&self) -> RenderOutput {
        presentation(&self.live, self.float_ring(), self.display_headroom)
    }
}

/// Largest recipe HDR headroom, in stops (the `crs:HDRMaxValue` range).
pub const MAX_HDR_HEADROOM_STOPS: f32 = 16.0;

/// Recipe HDR headroom in stops, sanitized to `0..=16` (0 when invalid).
pub fn hdr_headroom_stops(s: &DevelopSettings) -> f32 {
    finite_or(s.output.hdr_headroom_stops, 0.0).clamp(0.0, MAX_HDR_HEADROOM_STOPS)
}

/// The viewport's display output. An RGBA8 ring (SDR display, or HDR off
/// when the host keeps SDR) always gets the unchanged SDR Output stage. A
/// float ring gets display-linear output tone-mapped for
/// `min(2^hdr_headroom_stops, display_headroom)` when the recipe's HDR toggle
/// is on, else for SDR white (headroom 1: the SDR tone curve, unencoded).
/// A headroom of 0 stops is "HDR on, SDR tone-mapped".
pub fn presentation(s: &DevelopSettings, float_ring: bool, display_headroom: f32) -> RenderOutput {
    if !float_ring {
        return RenderOutput::Display;
    }
    let display = finite_or(display_headroom, 1.0).max(1.0);
    let headroom = if s.output.hdr {
        hdr_headroom_stops(s).exp2().min(display)
    } else {
        1.0
    };
    RenderOutput::DisplayLinear(Headroom::new(headroom))
}

/// Advances only after presentation; cancelled work keeps its unpresented slot.
#[derive(Default)]
struct SurfaceCursor(Option<u32>);
impl SurfaceCursor {
    fn candidate(&self, ids: &[u32]) -> Option<usize> {
        if ids.is_empty() {
            return None;
        }
        Some(
            self.0
                .and_then(|id| ids.iter().position(|&s| s == id))
                .map_or(0, |i| (i + 1) % ids.len()),
        )
    }
    fn published(&mut self, id: u32) {
        self.0 = Some(id);
    }
}

/// The legacy UI sends the CFA ID for "AI Denoise On". Resolve that intent
/// once, on Off -> Neural, and persist the concrete model in normal history.
/// Saved edits, slider changes and undo/redo must never consult availability.
fn pin_new_denoise_edit(
    registry: &ml_runtime::ModelRegistry,
    before: &DevelopSettings,
    next: &mut DevelopSettings,
) -> Result<()> {
    use engine_api::recipe::settings::DenoiseMethod;
    if matches!(before.denoise.method, DenoiseMethod::Off)
        && pipeline_cpu::cfa_denoise_selected(&next.denoise)
        && let DenoiseMethod::Neural { model, .. } = &mut next.denoise.method
    {
        registry.resolve_cached_ref(model).map_err(failure)?;
        *model = registry.preferred_ai_denoise().map_err(failure)?;
    }
    Ok(())
}

type SurfaceDestination = Arc<Mutex<Option<Arc<Surface>>>>;

#[derive(Clone, Copy, Default)]
enum SaveWork {
    #[default]
    Full,
    Repair,
}

struct SaveFailure {
    error: BridgeError,
    retry: SaveWork,
    published: Option<Box<OwnerBaseline>>,
}

impl SaveFailure {
    fn full(error: impl Into<BridgeError>) -> Self {
        Self {
            error: error.into(),
            retry: SaveWork::Full,
            published: None,
        }
    }

    fn repair(error: impl Into<BridgeError>) -> Self {
        Self {
            error: error.into(),
            retry: SaveWork::Repair,
            published: None,
        }
    }

    fn after_recipe(error: impl Into<BridgeError>, published: &OwnerBaseline) -> Self {
        Self {
            error: error.into(),
            retry: SaveWork::Repair,
            published: Some(Box::new(published.clone())),
        }
    }
}

#[derive(Default)]
struct SaveState {
    due: Option<Instant>,
    due_work: SaveWork,
    retry: Option<SaveWork>,
    retry_recipe: Option<Box<Recipe>>,
    flush: bool,
    busy: bool,
    shutdown: bool,
    error: Option<String>,
    failure_seq: u64,
    error_reported: bool,
    #[cfg(test)]
    flush_waiters: usize,
}

pub(crate) struct Shared {
    engine: std::sync::Weak<Engine>,
    image_id: String,
    path: PathBuf,
    image: RawImage,
    renderer: Arc<Renderer>,
    cfa_denoiser: Mutex<Option<Arc<image_core::MlCfaDenoise>>>,
    depth_provider: Arc<image_core::depth::DepthProvider>,
    model_registry: Arc<ml_runtime::ModelRegistry>,
    automatic_cfa: Mutex<Option<(engine_api::id::ModelRef, Arc<image_core::MlCfaDenoise>)>>,
    rgb_denoiser: Arc<image_core::MlPostDemosaicDenoise>,
    backend: String,
    state: Mutex<State>,
    /// The Develop-owned fields last read or authoritatively published by
    /// this session. The save worker updates this from its published snapshot,
    /// never from newer UI state that arrived during I/O.
    owner_baseline: Mutex<OwnerBaseline>,
    /// Non-owning write capability. Reservation lifetime belongs to the
    /// session and writer closure, not render-held Shared references.
    authority: crate::recipe_write::DevelopAuthority,
    // Held through GPU completion and publication: cancelled jobs cannot
    // release an IOSurface while a submitted write is still in flight.
    render_serial: Mutex<()>,
    generation: AtomicU64,
    listener: Mutex<Option<Arc<dyn DevelopListener>>>,
    save: Mutex<SaveState>,
    save_cv: Condvar,
    #[cfg(test)]
    worker_exit_observer: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    file_hash: OnceLock<std::result::Result<blake3::Hasher, String>>,
    /// Scene-linear luminance feeding Detail at one level, for the masking
    /// preview (recomputed only when upstream settings or the level change).
    mask_source: Mutex<Option<Arc<MaskSource>>>,
    /// Local-adjustment masks: AI rasters, overlay, observed rasters.
    masks: Arc<masks::MaskShared>,
}

/// Renderer and model dependencies shared by visible Develop and read-only
/// saved-recipe consumers. This deliberately contains no session state or
/// save-worker lifecycle.
struct DevelopRenderResources {
    renderer: Arc<Renderer>,
    backend: String,
    cfa_denoiser: Mutex<Option<Arc<image_core::MlCfaDenoise>>>,
    depth_provider: Arc<image_core::depth::DepthProvider>,
    model_registry: Arc<ml_runtime::ModelRegistry>,
    automatic_cfa: Mutex<Option<(engine_api::id::ModelRef, Arc<image_core::MlCfaDenoise>)>>,
    rgb_denoiser: Arc<image_core::MlPostDemosaicDenoise>,
    masks: Arc<masks::MaskShared>,
}

impl DevelopRenderResources {
    fn renderer_snapshot(&self, settings: &DevelopSettings) -> Renderer {
        renderer_snapshot(
            &self.renderer,
            &self.depth_provider,
            &self.cfa_denoiser,
            &self.automatic_cfa,
            &self.model_registry,
            &self.rgb_denoiser,
            settings,
        )
    }

    fn depth_input(
        &self,
        image: &RawImage,
        settings: DevelopSettings,
        process_version: ProcessVersion,
    ) -> Result<pipeline_cpu::Image> {
        render_depth_input(image, settings, process_version, |settings| {
            self.renderer_snapshot(settings)
        })
    }
}

#[derive(uniffi::Object)]
pub struct DevelopSession {
    shared: Arc<Shared>,
    lease: Mutex<Option<crate::recipe_write::DevelopLease>>,
    writer: Mutex<Option<JoinHandle<()>>>,
    writer_thread: ThreadId,
    close_phase: Mutex<ClosePhase>,
}

#[derive(Clone)]
enum CloseOutcome {
    Success,
    Failure(String),
}

struct CloseAttempt {
    state: Mutex<CloseAttemptState>,
    cv: Condvar,
}

struct CloseAttemptState {
    outcome: Option<CloseOutcome>,
    #[cfg(test)]
    waiters: usize,
}

enum ClosePhase {
    Open,
    Closing(Arc<CloseAttempt>),
    Closed,
}

// ─────────────────────────── settings helpers ───────────────────────────

fn session_renderable(
    s: &DevelopSettings,
    geometry: bool,
    _denoiser_configured: bool,
) -> DevelopSettings {
    let mut drawn = renderable_with(s, geometry);
    // Runtime adapters estimate raw noise automatically. Missing weights are a
    // render error for the panel, never a silently discarded denoise request.
    if pipeline_cpu::validate_denoise(&s.denoise).is_ok() {
        drawn.denoise = s.denoise.clone();
    }
    drawn.effects.lens_blur = s.effects.lens_blur.clone();
    drawn
}

/// The controls this pipeline renders: Basic, Detail, tone curves, HSL,
/// grading, post-crop effects and crop/straighten. Values are sanitized to
/// the operators' declared ranges so a hand-edited or imported recipe can
/// never fail a render: invalid curves or split points fall back to
/// identity/defaults. Every other field keeps its native defaults.
/// Unsupported settings stay in the recipe and XMP but are not drawn; see
/// [`ignored_settings`].
pub fn renderable(s: &DevelopSettings) -> DevelopSettings {
    renderable_with(s, true)
}

/// [`renderable`], optionally without geometry (the crop tool shows the
/// whole, unrotated frame and draws the crop over it).
pub fn renderable_with(s: &DevelopSettings, geometry: bool) -> DevelopSettings {
    let mut r = DevelopSettings::default();
    r.linearize.highlight_reconstruction = match s.linearize.highlight_reconstruction {
        m @ (HighlightReconstruction::Clip | HighlightReconstruction::ReconstructColor) => m,
        _ => r.linearize.highlight_reconstruction,
    };
    r.demosaic.method = match s.demosaic.method {
        m @ (DemosaicMethod::Auto | DemosaicMethod::Bilinear) => m,
        _ => DemosaicMethod::Auto,
    };
    r.white_balance = s.white_balance.clone();
    if r.white_balance.mode == WhiteBalanceMode::Auto {
        r.white_balance.mode = WhiteBalanceMode::AsShot;
    }
    r.white_balance.temperature =
        finite_or(s.white_balance.temperature, 5500.0).clamp(1667.0, 25000.0);
    r.white_balance.tint = finite_or(s.white_balance.tint, 0.0).clamp(-150.0, 150.0);
    let t = &s.tone;
    r.tone.exposure = finite_or(t.exposure, 0.0).clamp(-10.0, 10.0);
    for (dst, src) in [
        (&mut r.tone.contrast, t.contrast),
        (&mut r.tone.highlights, t.highlights),
        (&mut r.tone.shadows, t.shadows),
        (&mut r.tone.whites, t.whites),
        (&mut r.tone.blacks, t.blacks),
        (&mut r.tone.texture, t.texture),
        (&mut r.tone.clarity, t.clarity),
        (&mut r.tone.dehaze, t.dehaze),
        (&mut r.color.vibrance, s.color.vibrance),
        (&mut r.color.saturation, s.color.saturation),
    ] {
        *dst = bipolar(src);
    }
    r.tone.display_transform = match t.display_transform {
        d @ (DisplayTransform::Native | DisplayTransform::Sigmoid) => d,
        _ => r.tone.display_transform,
    };
    // Tone curves.
    let (pc, rc) = (&t.curves.parametric, &mut r.tone.curves.parametric);
    rc.shadows = bipolar(pc.shadows);
    rc.darks = bipolar(pc.darks);
    rc.lights = bipolar(pc.lights);
    rc.highlights = bipolar(pc.highlights);
    let splits = [pc.shadow_split, pc.midtone_split, pc.highlight_split];
    if splits.iter().all(|v| v.is_finite())
        && 0.0 < splits[0]
        && splits[0] < splits[1]
        && splits[1] < splits[2]
        && splits[2] < 100.0
    {
        (rc.shadow_split, rc.midtone_split, rc.highlight_split) = (splits[0], splits[1], splits[2]);
    } else {
        let d = ParametricCurve::default();
        (rc.shadow_split, rc.midtone_split, rc.highlight_split) =
            (d.shadow_split, d.midtone_split, d.highlight_split);
    }
    r.tone.curves.rgb = point_curve(&t.curves.rgb);
    r.tone.curves.red = point_curve(&t.curves.red);
    r.tone.curves.green = point_curve(&t.curves.green);
    r.tone.curves.blue = point_curve(&t.curves.blue);
    r.tone.curves.luminance = point_curve(&t.curves.luminance);
    // HSL and colour grading.
    let hsl = &s.color.hsl;
    r.color.hsl.hue = bands(&hsl.hue);
    r.color.hsl.saturation = bands(&hsl.saturation);
    r.color.hsl.luminance = bands(&hsl.luminance);
    let g = &s.color.grading;
    let rg = &mut r.color.grading;
    rg.shadows = wheel(&g.shadows);
    rg.midtones = wheel(&g.midtones);
    rg.highlights = wheel(&g.highlights);
    rg.global = wheel(&g.global);
    rg.blending = finite_or(g.blending, 50.0).clamp(0.0, 100.0);
    rg.balance = bipolar(g.balance);
    // Detail.
    let (sh, rs) = (&s.detail.sharpening, &mut r.detail.sharpening);
    rs.amount = finite_or(sh.amount, 40.0).clamp(0.0, 150.0);
    rs.radius = finite_or(sh.radius, 1.0).clamp(0.5, 3.0);
    rs.detail = unit(sh.detail, 25.0);
    rs.masking = unit(sh.masking, 0.0);
    let (nr, rn) = (&s.detail.noise_reduction, &mut r.detail.noise_reduction);
    rn.luminance = unit(nr.luminance, 0.0);
    rn.luminance_detail = unit(nr.luminance_detail, 50.0);
    rn.luminance_contrast = unit(nr.luminance_contrast, 0.0);
    rn.color = unit(nr.color, 25.0);
    rn.color_detail = unit(nr.color_detail, 50.0);
    rn.color_smoothness = unit(nr.color_smoothness, 50.0);
    // Effects (lens blur is not in this pipeline).
    let (v, rv) = (&s.effects.vignette, &mut r.effects.vignette);
    rv.style = v.style;
    rv.amount = bipolar(v.amount);
    rv.midpoint = unit(v.midpoint, 50.0);
    rv.roundness = bipolar(v.roundness);
    rv.feather = unit(v.feather, 50.0);
    rv.highlights = unit(v.highlights, 0.0);
    let (gr, rgr) = (&s.effects.grain, &mut r.effects.grain);
    rgr.amount = unit(gr.amount, 0.0);
    rgr.size = unit(gr.size, 25.0);
    rgr.roughness = unit(gr.roughness, 50.0);
    // Crop and straighten; the aspect lock is a UI hint that draws nothing.
    let crop = &s.geometry.crop;
    if geometry && crop.rect.is_valid() && crop.angle.is_finite() && crop.angle.abs() <= 45.0 {
        r.geometry.crop.rect = crop.rect;
        r.geometry.crop.angle = crop.angle;
    }

    // Native composed optics/geometry are now shared with export. Validate
    // before retaining controls so malformed imported parameters do not leak
    // into the sanitized interactive recipe.
    let mut optics = r.clone();
    optics.lens = s.lens.clone();
    if pipeline_cpu::validate_settings(&optics).is_ok() {
        r.lens = optics.lens;
    }
    if geometry {
        let mut composed = r.clone();
        composed.geometry = s.geometry.clone();
        composed.geometry.crop = r.geometry.crop.clone();
        // Constrain Crop and EXIF-orientation overrides are not implemented by
        // the scalar geometry operator; keep them out of the rendered recipe so
        // they are reported by `ignored_settings` instead of failing frames.
        composed.geometry.constrain_crop = false;
        composed.geometry.orientation = 1;
        if pipeline_cpu::validate_settings(&composed).is_ok() {
            r.geometry = composed.geometry;
        }
    }

    r.output.gamut_mapping = s.output.gamut_mapping;
    r.locals = masks::renderable_locals(&s.locals);
    r
}

fn bipolar(v: f32) -> f32 {
    finite_or(v, 0.0).clamp(-100.0, 100.0)
}

fn unit(v: f32, default: f32) -> f32 {
    finite_or(v, default).clamp(0.0, 100.0)
}

fn bands(b: &HueBands) -> HueBands {
    HueBands {
        red: bipolar(b.red),
        orange: bipolar(b.orange),
        yellow: bipolar(b.yellow),
        green: bipolar(b.green),
        aqua: bipolar(b.aqua),
        blue: bipolar(b.blue),
        purple: bipolar(b.purple),
        magenta: bipolar(b.magenta),
    }
}

fn wheel(w: &GradeWheel) -> GradeWheel {
    GradeWheel {
        hue: finite_or(w.hue, 0.0).rem_euclid(360.0),
        saturation: unit(w.saturation, 0.0),
        luminance: bipolar(w.luminance),
    }
}

/// A point curve the operator accepts (finite knots in [0,1], strictly
/// increasing x, nondecreasing y), or identity. Identity curves render as the
/// empty default so they keep the fast path and a clean recipe hash.
fn point_curve(c: &Curve) -> Curve {
    let valid =
        c.0.iter()
            .all(|p| (0.0..=1.0).contains(&p.x) && (0.0..=1.0).contains(&p.y))
            && c.0.windows(2).all(|w| w[0].x < w[1].x && w[0].y <= w[1].y);
    if !valid || c.is_identity() {
        return Curve::default();
    }
    c.clone()
}

fn finite_or(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

/// JSON pointers of settings present in `s` that the viewport does not render.
pub fn ignored_settings(s: &DevelopSettings) -> Vec<String> {
    let mut drawn = renderable(s);
    // The aspect lock is a crop-tool hint, not an undrawn setting.
    drawn.geometry.crop.aspect = s.geometry.crop.aspect;
    // HDR is drawn by the viewport's presentation (float rings), not by the
    // renderer's settings, which stay SDR for every rendition.
    drawn.output.hdr = s.output.hdr;
    drawn.output.hdr_headroom_stops = s.output.hdr_headroom_stops;
    let (Ok(a), Ok(b)) = (serde_json::to_value(s), serde_json::to_value(drawn)) else {
        return Vec::new();
    };
    engine_api::recipe::history::diff(&b, &a)
        .into_iter()
        .map(|c| c.path().to_owned())
        .collect()
}

/// RFC 7386 JSON merge patch.
fn merge_patch(target: &mut Value, patch: &Value) {
    match patch {
        Value::Object(members) => {
            if !target.is_object() {
                *target = Value::Object(Default::default());
            }
            let object = target.as_object_mut().expect("object");
            for (key, value) in members {
                if value.is_null() {
                    object.remove(key);
                } else {
                    merge_patch(object.entry(key.clone()).or_insert(Value::Null), value);
                }
            }
        }
        _ => *target = patch.clone(),
    }
}

/// Correlated colour temperature and tint of the as-shot white, on the same
/// unrounded CCT/perpendicular-Duv scale as `pipeline_cpu::temperature_white`.
fn as_shot_white(image: &RawImage) -> (f32, f32) {
    let m = image.metadata();
    let Ok(camera_xyz) = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
        m.cam_xyz[r].map(f64::from)
    }))) else {
        return (5500.0, 0.0);
    };
    pipeline_cpu::as_shot_temperature_tint(camera_xyz, m.as_shot_wb).unwrap_or((5500.0, 0.0))
}

fn stage_name(stage: StageId) -> String {
    format!("{stage:?}")
}

// ─────────────────────────────── engine ───────────────────────────────

struct DevelopDiskSnapshot {
    image_id: ImageId,
    path: PathBuf,
    recipe: Recipe,
    owner_baseline: OwnerBaseline,
    lease: Option<crate::recipe_write::DevelopLease>,
}

#[uniffi::export]
impl Engine {
    /// Blocking depth histogram for an indexed image using its saved recipe,
    /// without opening an editable Develop session.
    pub fn depth_histogram(self: Arc<Self>, image_id: String) -> Result<Vec<u64>> {
        let snapshot = self.develop_disk_snapshot(&image_id, false)?;
        let image = RawImage::open(snapshot.image_id, &snapshot.path)?;
        let mut recipe = snapshot.recipe;
        recipe.source_kind = if image.source_kind() == "rgb" {
            engine_api::recipe::SourceKind::Rgb
        } else {
            engine_api::recipe::SourceKind::Raw
        };
        let resources = self.develop_render_resources(&image)?;
        let settings = session_renderable(&recipe.settings, true, false);
        let input = resources.depth_input(&image, settings, recipe.process_version)?;
        Ok(resources
            .depth_provider
            .estimate(&input)?
            .histogram()
            .to_vec())
    }

    /// Opens a develop session on an indexed RAW or rendered RGB image.
    /// Blocking decode: call off the main thread. One session per visible image.
    pub fn open_develop_session(self: Arc<Self>, image_id: String) -> Result<Arc<DevelopSession>> {
        let snapshot = self.develop_disk_snapshot(&image_id, true)?;
        let path = snapshot.path;
        let mut recipe = snapshot.recipe;
        let image = RawImage::open(snapshot.image_id, &path)?;
        recipe.source_kind = if image.source_kind() == "rgb" {
            engine_api::recipe::SourceKind::Rgb
        } else {
            engine_api::recipe::SourceKind::Raw
        };
        let resources = self.develop_render_resources(&image)?;
        let screen_level = default_level(&image);
        let shared = Arc::new(Shared {
            engine: Arc::downgrade(&self),
            image_id,
            path,
            image,
            renderer: resources.renderer,
            cfa_denoiser: resources.cfa_denoiser,
            automatic_cfa: resources.automatic_cfa,
            rgb_denoiser: resources.rgb_denoiser,
            model_registry: resources.model_registry,
            depth_provider: resources.depth_provider,
            backend: resources.backend,
            owner_baseline: Mutex::new(snapshot.owner_baseline),
            authority: snapshot
                .lease
                .as_ref()
                .expect("editor snapshot reserves lease")
                .authority(),
            state: Mutex::new(State {
                live: recipe.settings.clone(),
                cfa_configured: false,
                recipe,
                rendered: None,
                rendered_level: None,
                surfaces: Vec::new(),
                next_surface: SurfaceCursor::default(),
                screen_level,
                job: None,
                generation: 0,
                histogram: Histogram::default(),
                frame: None,
                closed: false,
                closing: false,
                crop_editing: false,
                render_uncorrected: false,
                render_depth_visualisation: false,
                masking_preview: false,
                // Heavy drags start coarser and adapt from there.
                drag: [DragLevel::starting_at(0), DragLevel::starting_at(2)],
                interactive_in_flight: None,
                interactive_pending: false,
                masks: Default::default(),
                display_headroom: 1.0,
            }),
            render_serial: Mutex::new(()),
            generation: AtomicU64::new(0),
            listener: Mutex::new(None),
            save: Mutex::new(SaveState::default()),
            save_cv: Condvar::new(),
            #[cfg(test)]
            worker_exit_observer: Mutex::new(None),
            file_hash: OnceLock::new(),
            mask_source: Mutex::new(None),
            masks: resources.masks,
        });
        let writer = {
            let shared = shared.clone();
            #[cfg(test)]
            let exit_observer = shared.clone();
            let worker_lease = snapshot
                .lease
                .clone()
                .expect("editor snapshot reserves lease");
            std::thread::Builder::new()
                .name("develop-save".into())
                .spawn(move || {
                    shared.writer_loop();
                    drop(worker_lease);
                    #[cfg(test)]
                    exit_observer.notify_worker_exit_for_test();
                })?
        };
        #[cfg(test)]
        self.develop_writer_constructions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let writer_thread = writer.thread().id();
        Ok(Arc::new(DevelopSession {
            shared,
            lease: Mutex::new(snapshot.lease),
            writer: Mutex::new(Some(writer)),
            writer_thread,
            close_phase: Mutex::new(ClosePhase::Open),
        }))
    }
}

impl Engine {
    fn develop_disk_snapshot(
        &self,
        image_id: &str,
        reserve_editor: bool,
    ) -> Result<DevelopDiskSnapshot> {
        let id = parse_id(image_id)?;
        let path = {
            let c = self.lock()?;
            PathBuf::from(Self::path(&c, image_id)?)
        };
        let (lease, (recipe, owner_baseline)) = {
            let gate = crate::recipe_write::gate_for(&path)?;
            if reserve_editor {
                let (lease, snapshot) = gate.reserve_develop(&path, || {
                    let c = self.lock()?;
                    if Path::new(&Self::path(&c, image_id)?) != path {
                        return Err(failure("image path changed before Develop open"));
                    }
                    let recipe = catalog::document(&path, id)?.recipe;
                    let baseline = OwnerBaseline::from_disk(&path, recipe.clone())?;
                    Ok((recipe, baseline))
                })?;
                (Some(lease), snapshot)
            } else {
                let _read = gate.begin_read()?;
                let c = self.lock()?;
                if Path::new(&Self::path(&c, image_id)?) != path {
                    return Err(failure("image path changed before Develop open"));
                }
                let recipe = catalog::document(&path, id)?.recipe;
                let baseline = OwnerBaseline::from_disk(&path, recipe.clone())?;
                (None, (recipe, baseline))
            }
        };
        Ok(DevelopDiskSnapshot {
            image_id: id,
            path,
            recipe,
            owner_baseline,
            lease,
        })
    }

    fn develop_render_resources(&self, image: &RawImage) -> Result<DevelopRenderResources> {
        let (renderer, backend) = self.develop_renderer(image);
        let masks = masks::MaskShared::new(image);
        renderer
            .mask_cache()
            .set_hooks(Some(Arc::new(masks::Hooks(masks.clone()))));
        let support = self
            .db
            .parent()
            .ok_or_else(|| failure("missing support directory"))?;
        let model_registry =
            Arc::new(ml_runtime::ModelRegistry::from_support(support).map_err(failure)?);
        Ok(DevelopRenderResources {
            renderer,
            backend,
            cfa_denoiser: Mutex::new(None),
            depth_provider: Arc::new(image_core::depth::DepthProvider::from_support(support)?),
            rgb_denoiser: Arc::new(image_core::MlPostDemosaicDenoise::new(
                model_registry.clone(),
                Default::default(),
            )),
            model_registry,
            automatic_cfa: Mutex::new(None),
            masks,
        })
    }

    /// Writes develop state into the image's recipe document without touching
    /// fields other writers own (selection, unknown members), then the XMP
    /// (crs: develop values + selection) and the index. Returns the new
    /// recipe hash.
    fn save_develop(
        &self,
        image_id: &str,
        path: &Path,
        recipe: &Recipe,
        baseline: &OwnerBaseline,
        authority: &crate::recipe_write::DevelopAuthority,
    ) -> std::result::Result<(String, OwnerBaseline), SaveFailure> {
        let id = parse_id(image_id).map_err(SaveFailure::full)?;
        let gate = crate::recipe_write::gate_for(path).map_err(SaveFailure::full)?;
        let _write = gate
            .begin_develop_write(authority, path)
            .map_err(SaveFailure::full)?;
        let mut c = self.lock().map_err(SaveFailure::full)?;
        if Path::new(&Self::path(&c, image_id).map_err(SaveFailure::full)?) != path {
            return Err(SaveFailure::full(failure(
                "image path changed before Develop save",
            )));
        }
        let mut doc = catalog::document(path, id).map_err(SaveFailure::full)?;
        let current =
            OwnerBaseline::from_disk(path, doc.recipe.clone()).map_err(SaveFailure::full)?;
        if baseline.has_unrepresented_nested || current.has_unrepresented_nested {
            return Err(SaveFailure::full(failure(
                "conflict: unsupported nested Develop recipe fields would be lost",
            )));
        }
        if !baseline.matches(&current) {
            return Err(SaveFailure::full(failure(
                "conflict: newer Develop-owned recipe fields changed on disk",
            )));
        }
        doc.recipe.process_version = recipe.process_version;
        doc.recipe.settings = recipe.settings.clone();
        doc.recipe.history = recipe.history.clone();
        // Develop owns the source-kind field; all other unknown members from
        // concurrent writers are preserved.
        doc.recipe.source_kind = if image_core::RgbSource::recognizes(path) {
            engine_api::recipe::SourceKind::Rgb
        } else {
            engine_api::recipe::SourceKind::Raw
        };
        doc.recipe.ids.next_mask = doc.recipe.ids.next_mask.max(recipe.ids.next_mask);
        doc.recipe.ids.next_retouch = doc.recipe.ids.next_retouch.max(recipe.ids.next_retouch);
        doc.record_write("tessera-mac", now_ms())
            .map_err(SaveFailure::full)?;
        let packet = catalog::selection_packet(path, &doc)
            .and_then(|packet| packet.with_recipe(&doc.recipe))
            .map_err(SaveFailure::full)?;
        let published = OwnerBaseline::published(doc.recipe.clone()).map_err(SaveFailure::full)?;
        sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(path).recipe, &doc)
            .map_err(SaveFailure::full)?;
        #[cfg(test)]
        injected_post_recipe_failure(path)
            .map_err(|error| SaveFailure::after_recipe(error, &published))?;
        sidecar::Sidecar::write_xmp(catalog::xmp_path(path), &packet)
            .map_err(|error| SaveFailure::after_recipe(error, &published))?;
        c.index
            .scan(
                path.parent().ok_or_else(|| {
                    SaveFailure::after_recipe(failure("image has no folder"), &published)
                })?,
                &catalog::Sidecars,
                &catalog::EmbeddedMetadata,
            )
            .map_err(|error| SaveFailure::after_recipe(error, &published))?;
        Ok((doc.recipe.recipe_hash().to_string(), published))
    }

    /// Complete auxiliary outputs after a recipe was already published. Read
    /// the current document so a retry never republishes stale session state.
    fn repair_develop(
        &self,
        image_id: &str,
        path: &Path,
        authority: &crate::recipe_write::DevelopAuthority,
    ) -> Result<Recipe> {
        let id = parse_id(image_id)?;
        let gate = crate::recipe_write::gate_for(path)?;
        let _write = gate.begin_develop_write(authority, path)?;
        let mut c = self.lock()?;
        if Path::new(&Self::path(&c, image_id)?) != path {
            return Err(failure("image path changed before Develop repair"));
        }
        let doc = catalog::document(path, id)?;
        let packet = catalog::selection_packet(path, &doc)?.with_recipe(&doc.recipe)?;
        #[cfg(test)]
        injected_post_recipe_failure(path)?;
        sidecar::Sidecar::write_xmp(catalog::xmp_path(path), &packet)?;
        c.index.scan(
            path.parent()
                .ok_or_else(|| failure("image has no folder"))?,
            &catalog::Sidecars,
            &catalog::EmbeddedMetadata,
        )?;
        Ok(doc.recipe)
    }
}

fn same_develop_fields(left: &Recipe, right: &Recipe) -> bool {
    left.process_version == right.process_version
        && left.source_kind == right.source_kind
        && left.settings == right.settings
        && left.history == right.history
}

/// The editor's disk baseline is distinct from the repair path's comparison.
/// Selection and top-level unknown members belong to other writers and are
/// merged from the latest disk document; allocation counters are owner state.
fn same_develop_owner_fields(left: &Recipe, right: &Recipe) -> bool {
    same_develop_fields(left, right)
        && left.ids.next_mask == right.ids.next_mask
        && left.ids.next_retouch == right.ids.next_retouch
}

/// Known owner fields use the same defaulting and XMP fallback as
/// `catalog::document`. Raw nested members not represented by the current
/// serializer are retained in this comparison, then rejected before a write
/// because publishing a typed Recipe would otherwise erase them.
#[derive(Clone)]
struct OwnerBaseline {
    recipe: Recipe,
    projection: Value,
    has_unrepresented_nested: bool,
}

impl OwnerBaseline {
    fn published(recipe: Recipe) -> Result<Self> {
        Ok(Self {
            projection: canonical_owner_projection(&recipe)?,
            recipe,
            has_unrepresented_nested: false,
        })
    }

    fn from_disk(path: &Path, recipe: Recipe) -> Result<Self> {
        let mut projection = canonical_owner_projection(&recipe)?;
        let raw = match std::fs::read(sidecar::Sidecar::paths(path).recipe) {
            Ok(bytes) => Some(serde_json::from_slice::<Value>(&bytes).map_err(failure)?),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let mut has_unrepresented_nested = false;
        if let Some(raw_recipe) = raw.as_ref().and_then(|value| value.get("recipe")) {
            for key in [
                "process_version",
                "source_kind",
                "settings",
                "history",
                "ids",
            ] {
                if let Some(raw_field) = raw_recipe.get(key) {
                    has_unrepresented_nested |=
                        merge_unrepresented(raw_field, &mut projection[key]);
                }
            }
        }
        Ok(Self {
            recipe,
            projection,
            has_unrepresented_nested,
        })
    }

    fn matches(&self, current: &Self) -> bool {
        !self.has_unrepresented_nested
            && !current.has_unrepresented_nested
            && same_develop_owner_fields(&self.recipe, &current.recipe)
            && self.projection == current.projection
    }
}

fn canonical_owner_projection(recipe: &Recipe) -> Result<Value> {
    let value = serde_json::to_value(recipe).map_err(failure)?;
    let mut owner = serde_json::Map::new();
    for key in [
        "process_version",
        "source_kind",
        "settings",
        "history",
        "ids",
    ] {
        owner.insert(key.to_owned(), value[key].clone());
    }
    Ok(Value::Object(owner))
}

fn merge_unrepresented(raw: &Value, normalized: &mut Value) -> bool {
    match (raw, normalized) {
        (Value::Object(raw), Value::Object(normalized)) => {
            let mut found = false;
            for (key, value) in raw {
                if let Some(known) = normalized.get_mut(key) {
                    found |= merge_unrepresented(value, known);
                } else {
                    normalized.insert(key.clone(), value.clone());
                    found = true;
                }
            }
            found
        }
        (Value::Array(raw), Value::Array(normalized)) => {
            if raw.len() != normalized.len() {
                *normalized = raw.clone();
                return true;
            }
            raw.iter()
                .zip(normalized.iter_mut())
                .fold(false, |found, (value, known)| {
                    merge_unrepresented(value, known) || found
                })
        }
        _ => false,
    }
}

/// Before a surface is attached: the level whose long edge is ≤ 2048 px.
fn default_level(image: &RawImage) -> u8 {
    (0..=MAX_LEVEL)
        .find(|&l| {
            let e = image.level_extent(l);
            e.width.max(e.height) <= 2048
        })
        .unwrap_or(MAX_LEVEL)
}

fn renderer_snapshot(
    renderer: &Arc<Renderer>,
    depth_provider: &Arc<image_core::depth::DepthProvider>,
    cfa_denoiser: &Mutex<Option<Arc<image_core::MlCfaDenoise>>>,
    automatic_cfa: &Mutex<Option<(engine_api::id::ModelRef, Arc<image_core::MlCfaDenoise>)>>,
    model_registry: &Arc<ml_runtime::ModelRegistry>,
    rgb_denoiser: &Arc<image_core::MlPostDemosaicDenoise>,
    settings: &DevelopSettings,
) -> Renderer {
    let renderer = (**renderer).clone().with_depth(depth_provider.clone());
    if let Some(adapter) = cfa_denoiser
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        return renderer.with_cfa_denoise(adapter.clone());
    }
    if pipeline_cpu::cfa_denoise_selected(&settings.denoise)
        && let engine_api::recipe::settings::DenoiseMethod::Neural { model, .. } =
            &settings.denoise.method
    {
        let mut slot = automatic_cfa.lock().unwrap_or_else(|e| e.into_inner());
        if slot.as_ref().is_none_or(|(key, _)| key != model) {
            *slot = Some((
                model.clone(),
                Arc::new(image_core::MlCfaDenoise::automatic(
                    model_registry.clone(),
                    Default::default(),
                    model.clone(),
                )),
            ));
        }
        return renderer.with_cfa_denoise(slot.as_ref().expect("installed above").1.clone());
    }
    renderer.with_post_demosaic_denoise(rgb_denoiser.clone())
}

fn render_depth_input(
    image: &RawImage,
    mut settings: DevelopSettings,
    process_version: ProcessVersion,
    snapshot: impl FnOnce(&DevelopSettings) -> Renderer,
) -> Result<pipeline_cpu::Image> {
    settings.effects = Default::default();
    settings.geometry = Default::default();
    settings.lens.distortion_scale = 0.0;
    settings.lens.manual_distortion = 0.0;
    let level = (0..=MAX_LEVEL)
        .find(|&l| {
            let e = image.level_extent(l);
            e.width.max(e.height) <= 1024
        })
        .unwrap_or(MAX_LEVEL);
    let e = image.level_extent(level);
    let tiles = snapshot(&settings)
        .for_process_version(process_version)
        .render_region_as(
            image,
            &settings,
            level,
            PixelRect::full(e),
            RenderOutput::SceneLinear,
        )?;
    let mut output =
        pipeline_cpu::Image::new(e.width, e.height, vec![vec![0.; e.area() as usize]; 3])?;
    for tile in tiles {
        let coord = engine_api::tile::TileCoord::new(0, tile.coord().x, tile.coord().y);
        output.put(&Tile::from_samples(
            coord,
            tile.layout(),
            tile.samples::<f32>()?.to_vec(),
        )?)?;
    }
    Ok(output)
}

// ─────────────────────────────── session ───────────────────────────────

impl Shared {
    #[cfg(test)]
    fn notify_worker_exit_for_test(&self) {
        if let Some(sender) = self.worker_exit_observer.lock().unwrap().take() {
            let _ = sender.send(());
        }
    }

    fn renderer_snapshot(&self, settings: &DevelopSettings) -> Renderer {
        renderer_snapshot(
            &self.renderer,
            &self.depth_provider,
            &self.cfa_denoiser,
            &self.automatic_cfa,
            &self.model_registry,
            &self.rgb_denoiser,
            settings,
        )
    }

    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(failure)
    }

    /// A state mutation and any matching save enqueue must both finish while
    /// this guard is held. Close takes the same lock to stop new mutations.
    fn edit_lock(&self) -> Result<MutexGuard<'_, State>> {
        let st = self.lock()?;
        if st.closed || st.closing {
            return Err(failure("Develop session is closing or closed"));
        }
        Ok(st)
    }

    fn depth_input(&self) -> Result<pipeline_cpu::Image> {
        let (settings, process_version) = {
            let state = self.lock()?;
            (state.drawn(), state.recipe.process_version)
        };
        render_depth_input(&self.image, settings, process_version, |settings| {
            self.renderer_snapshot(settings)
        })
    }

    fn listener(&self) -> Option<Arc<dyn DevelopListener>> {
        self.listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn full_rect(&self) -> PixelRect {
        PixelRect::full(self.image.active_extent())
    }

    /// Cancels the running render and submits a new one for the live settings.
    fn render(self: &Arc<Self>, st: &mut State, interactive: bool) {
        if st.closed {
            return;
        }
        // A drag never cancels its own in-flight frame: the latest state
        // follows as soon as it lands, so slow frames still show progress.
        if interactive
            && !st.masking_preview
            && st.job.is_some()
            && st.interactive_in_flight == Some(st.generation)
        {
            st.interactive_pending = true;
            return;
        }
        st.interactive_pending = false;
        st.generation += 1;
        let generation = st.generation;
        self.generation.store(generation, Ordering::SeqCst);
        if let Some(job) = st.job.take() {
            job.cancel();
        }
        if st.masking_preview {
            // The overlay does not change `rendered`: leaving the preview
            // re-renders the image from the same baseline.
            let job = MaskJob {
                shared: Arc::downgrade(self),
                generation,
                level: st.screen_level,
                upstream: mask_upstream(&st.live),
                masking: renderable(&st.live).detail.sharpening.masking,
            };
            if let Some(engine) = self.engine.upgrade() {
                st.job = Some(engine.jobs.submit(Box::new(job), None));
            }
            return;
        }
        let settings = st.drawn();
        let renderer = Arc::new(
            self.renderer_snapshot(&settings)
                .for_recipe(&st.recipe)
                .with_depth_visualisation(st.render_depth_visualisation),
        );
        masks::ensure_ai_jobs(self, &settings);
        let dirty = match &st.rendered {
            Some(prev) => prev.first_dirty_stage(&settings),
            None => Some(StageId::Decode),
        };
        st.rendered = Some(settings.clone());
        let target = st.screen_level;
        let area = self.image.level_extent(target).area();
        let resident = renderer
            .can_render_resident(&self.image, &settings)
            .unwrap_or(false);
        let class = RenderClass::of(&settings, resident);
        let first = if interactive {
            // Measured adaptation governs resident renders; the static pixel
            // budget remains a prior only for non-resident heavy recipes.
            let budget = if !resident && area > DRAG_BUDGET_PX {
                1
            } else {
                0
            };
            (target + st.drag[class as usize].offset.max(budget)).min(MAX_LEVEL)
        } else {
            target
        };
        let finest = if interactive { first } else { target };
        let viewport = Viewport {
            rect: self.full_rect(),
            finest_level: finest,
            coarsest_level: first,
        };
        let expected: Vec<(u8, usize)> = (finest..=first)
            .map(|l| {
                let (cols, rows) = Renderer::output_extent(&self.image, &settings, l)
                    .unwrap_or_else(|_| self.image.level_extent(l))
                    .tile_grid(TILE_SIZE);
                (l, (cols * rows) as usize)
            })
            .collect();
        let output = st.output();
        let surface = Arc::new(Mutex::new(None));
        let sink = Arc::new(Mutex::new(LevelSink {
            output,
            surface: surface.clone(),
            shared: Arc::downgrade(self),
            generation,
            started: Instant::now(),
            first_level: first,
            finest_level: finest,
            expected,
            settings: settings.clone(),
            dirty: dirty.map(stage_name),
            current: None,
            screen_level: target,
            adapt: interactive.then_some(class),
        }));
        let cpu_sink = sink.clone();
        let job = DevelopJob {
            sink,
            surface,
            inner: ProgressiveRenderJob {
                renderer,
                image: self.image.clone(),
                settings,
                viewport,
                output,
                priority: Priority::Viewport,
                sink: Box::new(move |t| {
                    cpu_sink.lock().unwrap_or_else(|e| e.into_inner()).accept(t)
                }),
            },
            shared: Arc::downgrade(self),
            generation,
        };
        st.interactive_in_flight = interactive.then_some(generation);
        if let Some(engine) = self.engine.upgrade() {
            st.job = Some(engine.jobs.submit(Box::new(job), None));
        }
    }

    /// An interactive render finished (or failed): start the one queued
    /// behind it, if any.
    fn interactive_done(self: &Arc<Self>, st: &mut State, generation: u64) {
        if st.interactive_in_flight == Some(generation) {
            st.interactive_in_flight = None;
            if std::mem::take(&mut st.interactive_pending) {
                self.render(st, true);
            }
        }
    }

    /// Records live changes as one history entry. Returns whether anything
    /// was recorded.
    fn commit_pending(&self, st: &mut State, label: &str) -> Result<bool> {
        let live = st.live.clone();
        let label = if label.is_empty() { "Edit" } else { label };
        let recorded = st
            .recipe
            .edit(EditMeta::user(label, now_ms()), |s| *s = live)?
            .is_some();
        Ok(recorded)
    }

    fn schedule_save(&self) {
        let mut s = self.save.lock().unwrap_or_else(|e| e.into_inner());
        s.due = Some(Instant::now() + SAVE_DEBOUNCE);
        s.due_work = SaveWork::Full;
        self.save_cv.notify_all();
    }

    fn writer_loop(self: Arc<Self>) {
        loop {
            let mut s = self.save.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match s.due {
                    None if s.shutdown => return,
                    None => s = self.save_cv.wait(s).unwrap_or_else(|e| e.into_inner()),
                    Some(due) => {
                        let now = Instant::now();
                        if s.flush || s.shutdown || now >= due {
                            break;
                        }
                        s = self
                            .save_cv
                            .wait_timeout(s, due - now)
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                }
            }
            let work = s.due_work;
            let retry_recipe = s.retry_recipe.clone();
            s.due = None;
            s.due_work = SaveWork::Full;
            s.flush = false;
            s.busy = true;
            drop(s);
            let result = self.save_now(work, retry_recipe);
            let mut s = self.save.lock().unwrap_or_else(|e| e.into_inner());
            s.busy = false;
            match result {
                Ok(()) => {
                    s.retry = None;
                    s.retry_recipe = None;
                }
                Err(failed) => {
                    s.retry = Some(failed.retry);
                    if matches!(failed.retry, SaveWork::Full) {
                        s.retry_recipe = None;
                    } else if let Some(published) = failed.published {
                        s.retry_recipe = Some(Box::new(published.recipe.clone()));
                    }
                    s.failure_seq = s.failure_seq.checked_add(1).expect("save failure sequence");
                    s.error = Some(failed.error.to_string());
                    s.error_reported = false;
                }
            }
            self.save_cv.notify_all();
        }
    }

    fn save_now(
        &self,
        work: SaveWork,
        retry_recipe: Option<Box<Recipe>>,
    ) -> std::result::Result<(), SaveFailure> {
        let authority = self.authority.clone();
        let engine = self
            .engine
            .upgrade()
            .ok_or_else(|| SaveFailure::full(failure("engine closed")))?;
        if matches!(work, SaveWork::Repair) {
            let committed = retry_recipe
                .ok_or_else(|| SaveFailure::repair(failure("missing Develop repair recipe")))?;
            let disk_recipe = engine
                .repair_develop(&self.image_id, &self.path, &authority)
                .map_err(SaveFailure::repair)?;
            let (session_recipe, frame) = {
                let st = self.lock().map_err(SaveFailure::repair)?;
                (st.recipe.clone(), st.frame.clone())
            };
            if same_develop_fields(&disk_recipe, &committed)
                && same_develop_fields(&session_recipe, &committed)
            {
                if let Some(frame) = frame.filter(|f| {
                    f.settings == session_renderable(&disk_recipe.settings, true, false)
                }) && let Err(error) = self.store_previews(&engine, &disk_recipe, &frame)
                {
                    eprintln!("develop: edited preview not stored: {error}");
                }
                if let Some(listener) = self.listener() {
                    listener.saved(disk_recipe.recipe_hash().to_string());
                }
            }
            return Ok(());
        }
        let (recipe, frame) = {
            let st = self.lock().map_err(SaveFailure::full)?;
            (st.recipe.clone(), st.frame.clone())
        };
        let baseline = self
            .owner_baseline
            .lock()
            .map_err(|error| SaveFailure::full(failure(error)))?
            .clone();
        let (hash, published) =
            match engine.save_develop(&self.image_id, &self.path, &recipe, &baseline, &authority) {
                Ok(saved) => saved,
                Err(failed) => {
                    if let Some(published) = &failed.published {
                        *self
                            .owner_baseline
                            .lock()
                            .map_err(|error| SaveFailure::full(failure(error)))? =
                            (**published).clone();
                    }
                    return Err(failed);
                }
            };
        *self
            .owner_baseline
            .lock()
            .map_err(|error| SaveFailure::full(failure(error)))? = published;
        if let Some(frame) =
            frame.filter(|f| f.settings == session_renderable(&recipe.settings, true, false))
            && let Err(e) = self.store_previews(&engine, &recipe, &frame)
        {
            eprintln!("develop: edited preview not stored: {e}");
        }
        if let Some(listener) = self.listener() {
            listener.saved(hash);
        }
        Ok(())
    }

    /// Stores the current frame as the edited preview for every tier the app
    /// has requested for this image, keyed like `PreviewStore::from_raw`
    /// (file bytes + max_px) with the new recipe hash.
    fn store_previews(&self, engine: &Engine, recipe: &Recipe, frame: &Frame) -> Result<()> {
        let sizes = engine.requested_preview_sizes(&self.image_id);
        if sizes.is_empty() {
            return Ok(());
        }
        let hasher = self
            .file_hash
            .get_or_init(|| {
                let mut h = blake3::Hasher::new();
                std::fs::File::open(&self.path)
                    .and_then(|f| h.update_reader(f).map(|_| ()))
                    .map(|()| h.clone())
                    .map_err(|e| e.to_string())
            })
            .as_ref()
            .map_err(failure)?;
        // The cropped picture when the recipe crops.
        let e = Renderer::output_extent(&self.image, &frame.settings, frame.level)?;
        let mut rgb = image::RgbImage::new(e.width, e.height);
        // Pixel consumers render the immutable recipe, never a mutable surface ring.
        let tiles = frame.pixels(|settings, level| {
            self.renderer_snapshot(settings)
                .for_process_version(recipe.process_version)
                .render_region(&self.image, settings, level, PixelRect::full(e))
        })?;
        for t in tiles.iter() {
            let l = t.layout();
            let n = l.plane_len();
            let d = t.samples::<u8>()?;
            let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
            for y in 0..l.extent.height {
                for x in 0..l.extent.width {
                    let i = (y * l.extent.width + x) as usize;
                    rgb.put_pixel(ox + x, oy + y, image::Rgb([d[i], d[n + i], d[2 * n + i]]));
                }
            }
        }
        let orientation = self.image.metadata().orientation as u8;
        for max_px in sizes {
            let mut h = hasher.clone();
            h.update(&max_px.to_le_bytes());
            let key = previews::PreviewKey {
                file_hash: *h.finalize().as_bytes(),
                orientation,
                recipe_hash: recipe.recipe_hash().0.0,
            };
            engine
                .previews
                .put_image(&key, &rgb, max_px)
                .map_err(failure)?;
        }
        Ok(())
    }

    fn close(&self) {
        if let Ok(mut st) = self.state.lock() {
            st.closed = true;
            st.closing = false;
            st.generation += 1;
            self.generation.store(st.generation, Ordering::SeqCst);
            if let Some(job) = st.job.take() {
                job.cancel();
            }
            st.surfaces.clear();
        }
    }
}

/// Collects one render's tiles level by level, writing them into the ring.
struct LevelSink {
    /// SDR encoded or EDR display-linear tiles (matches the ring).
    output: RenderOutput,
    surface: SurfaceDestination,
    shared: std::sync::Weak<Shared>,
    generation: u64,
    started: Instant,
    first_level: u8,
    finest_level: u8,
    expected: Vec<(u8, usize)>,
    settings: DevelopSettings,
    dirty: Option<String>,
    current: Option<LevelWriter>,
    /// Level the host's surfaces are planned for (the frame's display size).
    screen_level: u8,
    /// Interactive render: its final frame time adapts the drag level.
    adapt: Option<RenderClass>,
}

struct LevelWriter {
    level: u8,
    surface: Option<Arc<Surface>>,
    remaining: usize,
    tiles: Vec<Tile>,
}

impl LevelSink {
    fn accept(&mut self, tile: Tile) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if shared.generation.load(Ordering::SeqCst) != self.generation {
            return;
        }
        let level = tile.coord().level;
        if self.current.as_ref().map(|c| c.level) != Some(level) {
            let surface = self
                .surface
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            let remaining = self
                .expected
                .iter()
                .find(|(l, _)| *l == level)
                .map_or(0, |(_, n)| *n);
            self.current = Some(LevelWriter {
                level,
                surface,
                remaining,
                tiles: Vec::with_capacity(remaining),
            });
        }
        let current = self.current.as_mut().expect("level started");
        current.tiles.push(tile);
        current.remaining = current.remaining.saturating_sub(1);
        if current.remaining == 0 {
            let done = self.current.take().expect("level started");
            self.finish(&shared, done);
        }
    }

    fn finish(&self, shared: &Arc<Shared>, done: LevelWriter) {
        // A cached level arrives as one chunk, so writing at the end loses no
        // overlap; bands of tile rows are written in parallel.
        let hist = match write_level(done.surface.as_deref(), &done.tiles) {
            Ok(h) => h,
            Err(e) => {
                if let Some(l) = shared.listener() {
                    l.render_failed(e);
                }
                return;
            }
        };
        self.publish(shared, done, hist, false);
    }

    fn finish_surface(&self, level: u8, surface: Arc<Surface>, hist: Hist) {
        if let Some(shared) = self.shared.upgrade() {
            self.publish(
                &shared,
                LevelWriter {
                    level,
                    surface: Some(surface),
                    remaining: 0,
                    tiles: Vec::new(),
                },
                hist,
                true,
            );
        }
    }

    fn publish(&self, shared: &Arc<Shared>, done: LevelWriter, hist: Hist, lazy_pixels: bool) {
        let render_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let output = |level| {
            Renderer::output_extent(&shared.image, &self.settings, level)
                .unwrap_or_else(|_| shared.image.level_extent(level))
        };
        let extent = output(done.level);
        let display = output(self.screen_level);
        let (width, height) = match &done.surface {
            Some(s) => (extent.width.min(s.width()), extent.height.min(s.height())),
            None => (extent.width, extent.height),
        };
        let [r, g, b, y] = hist;
        let is_overlay;
        {
            let Ok(mut st) = shared.state.lock() else {
                return;
            };
            if st.generation != self.generation {
                return;
            }
            if let Some(surface) = &done.surface {
                st.next_surface.published(surface.id());
            }
            is_overlay = st.render_depth_visualisation;
            if !is_overlay {
                st.histogram = Histogram {
                    red: r.to_vec(),
                    green: g.to_vec(),
                    blue: b.to_vec(),
                    luminance: y.to_vec(),
                    level: done.level,
                    generation: self.generation,
                };
            }
            st.rendered_level = Some(done.level);
            if let Some(class) = self.adapt.filter(|_| done.level == self.finest_level) {
                st.drag[class as usize].record(render_ms);
            }
            if done.level == self.finest_level {
                shared.interactive_done(&mut st, self.generation);
            }
            // Pixel consumers (previews) take SDR tiles: EDR frames are
            // materialized again through the SDR Output stage on demand.
            let sdr_tiles = !lazy_pixels && self.output == RenderOutput::Display;
            st.frame = if is_overlay {
                None
            } else {
                Some(Arc::new(Frame {
                    level: done.level,
                    settings: self.settings.clone(),
                    tiles: if sdr_tiles { Some(done.tiles) } else { None },
                }))
            };
        }
        if let Some(listener) = shared.listener() {
            listener.frame_ready(FrameInfo {
                surface_id: done.surface.as_ref().map_or(0, |s| s.id()),
                level: done.level,
                width,
                height,
                first_level: self.first_level,
                is_final: done.level == self.finest_level,
                render_ms,
                generation: self.generation,
                dirty_stage: self.dirty.clone(),
                display_width: display.width,
                display_height: display.height,
                is_overlay,
            });
        }
        if !is_overlay {
            masks::publish_overlay(shared, &self.settings, done.level, self.generation);
        }
    }
}

type Hist = [[u32; 256]; 4];

/// Writes a complete level into `surface` (if any) and returns its
/// histograms. Each thread owns one band of 256 surface rows (one tile row),
/// so the bands are disjoint slices of the locked surface.
pub(crate) fn write_level(
    surface: Option<&Surface>,
    tiles: &[Tile],
) -> std::result::Result<Hist, String> {
    let mut rows: std::collections::BTreeMap<u32, Vec<&Tile>> = Default::default();
    for t in tiles {
        rows.entry(t.coord().y).or_default().push(t);
    }
    let float = surface.is_some_and(Surface::is_float);
    let band = |pixels: Option<(&mut [u8], usize, u32, u32)>, tiles: &[&Tile]| {
        let mut hist: Hist = [[0; 256]; 4];
        let mut error = None;
        let mut pixels = pixels;
        for t in tiles {
            if let Some((px, stride, first_row, width)) = pixels.as_mut()
                && let Err(e) =
                    crate::surface::write_display(px, *stride, *first_row, *width, float, t)
            {
                error = Some(e);
            }
            accumulate(&mut hist, t);
        }
        (hist, error)
    };
    let results: Vec<(Hist, Option<String>)> = match surface {
        Some(s) => s.with_pixels(|px, stride| {
            let width = s.width();
            let mut bands: Vec<(u32, &mut [u8])> = px
                .chunks_mut(stride * TILE_SIZE as usize)
                .enumerate()
                .map(|(i, b)| (i as u32, b))
                .collect();
            std::thread::scope(|scope| {
                let handles: Vec<_> = bands
                    .drain(..)
                    .filter_map(|(i, b)| rows.get(&i).map(|t| (i, b, t)))
                    .map(|(i, b, t)| {
                        scope.spawn(move || band(Some((b, stride, i * TILE_SIZE, width)), t))
                    })
                    .collect();
                handles
                    .into_iter()
                    .map(|h| h.join().expect("band writer"))
                    .collect()
            })
        })?,
        None => std::thread::scope(|scope| {
            let handles: Vec<_> = rows
                .values()
                .map(|t| scope.spawn(move || band(None, t)))
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().expect("band histogram"))
                .collect()
        }),
    };
    let mut total: Hist = [[0; 256]; 4];
    for (hist, error) in results {
        if let Some(e) = error {
            return Err(e);
        }
        for (t, h) in total.iter_mut().zip(hist.iter()) {
            for (a, b) in t.iter_mut().zip(h.iter()) {
                *a += b;
            }
        }
    }
    Ok(total)
}

/// Display-encoded sRGB bin of a display-linear value: the SDR code value
/// `round(oetf(clamp(v, 0, 1)) · 255)`, so EDR highlights land in bin 255.
/// Exact by bisection over the 255 bin boundaries (no per-sample `powf`).
fn encoded_bin(v: f32) -> u8 {
    static EDGES: OnceLock<[f32; 255]> = OnceLock::new();
    let edges = EDGES.get_or_init(|| {
        let code = |x: f32| (pipeline_cpu::srgb_oetf(x) * 255.0 + 0.5).floor();
        std::array::from_fn(|k| {
            // Smallest non-negative f32 whose code value is at least k + 1
            // (positive f32 bit patterns are ordered like their values).
            let target = (k + 1) as f32;
            let (mut lo, mut hi) = (0u32, 1.0f32.to_bits());
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if code(f32::from_bits(mid)) >= target {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            f32::from_bits(hi)
        })
    });
    if v.is_nan() {
        return 0;
    }
    edges.partition_point(|&e| e <= v) as u8
}

fn accumulate(hist: &mut Hist, tile: &Tile) {
    if let Ok(d) = tile.samples::<f32>() {
        let n = tile.layout().plane_len();
        let (r, rest) = d.split_at(n);
        let (g, b) = rest.split_at(n);
        let [hr, hg, hb, hy] = hist;
        for i in 0..n {
            let (rv, gv, bv) = (encoded_bin(r[i]), encoded_bin(g[i]), encoded_bin(b[i]));
            hr[rv as usize] += 1;
            hg[gv as usize] += 1;
            hb[bv as usize] += 1;
            let yv = (54 * u32::from(rv) + 183 * u32::from(gv) + 19 * u32::from(bv)) >> 8;
            hy[yv as usize] += 1;
        }
        return;
    }
    let Ok(d) = tile.samples::<u8>() else {
        return;
    };
    let n = tile.layout().plane_len();
    let (r, rest) = d.split_at(n);
    let (g, b) = rest.split_at(n);
    let [hr, hg, hb, hy] = hist;
    for i in 0..n {
        let (rv, gv, bv) = (r[i], g[i], b[i]);
        hr[rv as usize] += 1;
        hg[gv as usize] += 1;
        hb[bv as usize] += 1;
        // Rec. 709 luma weights in 8.8 fixed point (54 + 183 + 19 = 256).
        let yv = (54 * u32::from(rv) + 183 * u32::from(gv) + 19 * u32::from(bv)) >> 8;
        hy[yv as usize] += 1;
    }
}

/// Reports failures of the wrapped render to the session listener.
struct DevelopJob {
    sink: Arc<Mutex<LevelSink>>,
    surface: SurfaceDestination,
    inner: ProgressiveRenderJob,
    shared: std::sync::Weak<Shared>,
    generation: u64,
}

impl Job for DevelopJob {
    fn label(&self) -> &str {
        "develop viewport"
    }
    fn priority(&self) -> Priority {
        self.inner.priority
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        let shared = self.shared.clone();
        let generation = self.generation;
        let owner = shared.upgrade();
        let _serial = owner
            .as_ref()
            .map(|s| s.render_serial.lock().unwrap_or_else(|e| e.into_inner()));
        let result = (|| {
            ctx.cancellation.check()?;
            let destination = if let Some(owner) = &owner {
                let st = owner.state.lock().unwrap_or_else(|e| e.into_inner());
                if st.generation != generation {
                    return Err(engine_api::EngineError::Cancelled);
                }
                st.next_surface
                    .candidate(&st.surfaces.iter().map(|s| s.id()).collect::<Vec<_>>())
                    .map(|i| st.surfaces[i].clone())
            } else {
                return Err(engine_api::EngineError::Cancelled);
            };
            // A ring of the other contract was attached after submission:
            // its attach already queued the render for that contract.
            if destination.as_ref().is_some_and(|d| {
                d.is_float() != matches!(self.inner.output, RenderOutput::DisplayLinear(_))
            }) {
                return Err(engine_api::EngineError::Cancelled);
            }
            *self.surface.lock().unwrap_or_else(|e| e.into_inner()) = destination.clone();
            if let Some(surface) = &destination
                && self.inner.viewport.finest_level == self.inner.viewport.coarsest_level
                && let Some(hist) = self.inner.renderer.render_surface_as(
                    &self.inner.image,
                    &self.inner.settings,
                    self.inner.viewport.finest_level,
                    surface.id(),
                    self.inner.output,
                    &ctx.cancellation,
                )?
            {
                ctx.cancellation.check()?;
                self.sink
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .finish_surface(self.inner.viewport.finest_level, surface.clone(), hist);
                ctx.report_progress(1.0, None);
                return Ok(());
            }
            Box::new(self.inner).run(ctx)
        })();
        if let Err(e) = &result
            && !matches!(e, engine_api::EngineError::Cancelled)
            && let Some(s) = shared.upgrade()
            && s.generation.load(Ordering::SeqCst) == generation
        {
            if let Ok(mut st) = s.state.lock() {
                s.interactive_done(&mut st, generation);
            }
            if let Some(l) = s.listener() {
                l.render_failed(e.to_string());
            }
        }
        result
    }
}

impl Drop for DevelopSession {
    fn drop(&mut self) {
        self.shared.close();
        self.stop_writer();
    }
}

impl DevelopSession {
    fn on_save_worker(&self) -> bool {
        std::thread::current().id() == self.writer_thread
    }

    fn stop_writer(&self) {
        {
            let mut s = self.shared.save.lock().unwrap_or_else(|e| e.into_inner());
            s.shutdown = true;
            self.shared.save_cv.notify_all();
        }
        let writer = self.writer.lock().ok().and_then(|mut w| w.take());
        if let Some(writer) = writer {
            // The final session Arc may be released by a listener on this
            // worker. Dropping its handle detaches it; joining self deadlocks.
            if writer.thread().id() != std::thread::current().id() {
                let _ = writer.join();
            }
        }
    }

    fn wait_close_attempt(attempt: &CloseAttempt) -> Result<()> {
        let mut state = attempt.state.lock().map_err(failure)?;
        #[cfg(test)]
        {
            state.waiters += 1;
            attempt.cv.notify_all();
        }
        while state.outcome.is_none() {
            state = attempt.cv.wait(state).map_err(failure)?;
        }
        match state.outcome.as_ref().expect("close attempt completed") {
            CloseOutcome::Success => Ok(()),
            CloseOutcome::Failure(message) => Err(failure(message.clone())),
        }
    }

    fn finish_close_attempt(&self, attempt: &CloseAttempt, result: &Result<()>) {
        let outcome = match result {
            Ok(()) => CloseOutcome::Success,
            Err(error) => CloseOutcome::Failure(error.to_string()),
        };
        attempt
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .outcome = Some(outcome);
        *self.close_phase.lock().unwrap_or_else(|e| e.into_inner()) = if result.is_ok() {
            ClosePhase::Closed
        } else {
            ClosePhase::Open
        };
        attempt.cv.notify_all();
    }

    fn flush_inner(&self, close_owned: bool) -> Result<()> {
        {
            let mut st = if close_owned {
                self.shared.lock()?
            } else {
                self.shared.edit_lock()?
            };
            if self.shared.commit_pending(&mut st, "Edit")? {
                self.shared.schedule_save();
            }
        }
        let mut s = self.shared.save.lock().map_err(failure)?;
        let starting_failure_seq = s.failure_seq;
        let unreported_error = (!s.error_reported).then(|| s.error.clone()).flatten();
        if s.due.is_none()
            && !s.busy
            && let Some(retry) = s.retry
        {
            s.due = Some(Instant::now());
            s.due_work = retry;
        }
        if s.due.is_some() {
            s.flush = true;
            self.shared.save_cv.notify_all();
        }
        #[cfg(test)]
        {
            s.flush_waiters += 1;
            self.shared.save_cv.notify_all();
        }
        while s.due.is_some() || s.busy {
            s = self.shared.save_cv.wait(s).map_err(failure)?;
        }
        #[cfg(test)]
        {
            s.flush_waiters -= 1;
            self.shared.save_cv.notify_all();
        }
        let observed_error = if s.failure_seq != starting_failure_seq {
            s.error.clone()
        } else {
            unreported_error
        };
        if let Some(error) = observed_error {
            s.error_reported = true;
            Err(failure(error))
        } else {
            Ok(())
        }
    }
}

/// Explicit caller-owned calibration in normalized linear sensor units. The
/// four noise coefficients are canonical RGGB sites; ISO alone is insufficient.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CfaDenoiseConfig {
    pub manifest_path: String,
    pub cache_path: String,
    pub model_id: String,
    pub model_digest: String,
    pub shot: Vec<f32>,
    pub read: Vec<f32>,
    /// Optional full-sensor raster, one independent coverage per Bayer site.
    pub mask: Option<Vec<f32>>,
}

#[uniffi::export]
impl DevelopSession {
    /// Installs a lazy, pinned CFA backend for this develop session. Does not
    /// invent a noise calibration, download weights, or enable denoise in the
    /// recipe. Subsequent Amount/tone edits share inference and resident caches.
    pub fn configure_cfa_denoise(&self, config: CfaDenoiseConfig) -> Result<()> {
        let shot: [f32; 4] = config
            .shot
            .try_into()
            .map_err(|_| failure("CFA shot requires four sites"))?;
        let read: [f32; 4] = config
            .read
            .try_into()
            .map_err(|_| failure("CFA read requires four sites"))?;
        if shot.iter().chain(&read).any(|v| !v.is_finite() || *v < 0.0) {
            return Err(failure("CFA noise must be finite and nonnegative"));
        }
        let model = engine_api::id::ModelRef {
            id: config.model_id.as_str().into(),
            version: config.model_digest,
        };
        let settings = engine_api::recipe::settings::DenoiseSettings {
            method: engine_api::recipe::settings::DenoiseMethod::Neural {
                model: model.clone(),
                joint_demosaic: false,
            },
            ..Default::default()
        };
        pipeline_cpu::validate_denoise(&settings)?;
        if !pipeline_cpu::cfa_denoise_selected(&settings) {
            return Err(failure("CFA configuration requires a pinned CFA model"));
        }
        let registry = Arc::new(
            ml_runtime::ModelRegistry::open(config.manifest_path, config.cache_path)
                .map_err(failure)?
                .with_downloads_allowed(false),
        );
        if !registry.models().iter().any(|s| {
            s.id == model.id.as_str()
                && s.version == model.version
                && s.sha256 == model.version
                && s.task == "cfa-denoise"
        }) {
            return Err(failure("CFA model/digest missing from manifest"));
        }
        let mut adapter = image_core::MlCfaDenoise::new(
            registry,
            Default::default(),
            model,
            ml_enhance::CfaNoise { shot, read },
        );
        if let Some(mask) = config.mask {
            let frame = self.shared.image.sensor_extent();
            adapter = adapter.with_mask(frame.width, frame.height, mask)?;
        }
        let mut st = self.shared.edit_lock()?;
        *self
            .shared
            .cfa_denoiser
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(adapter));
        st.cfa_configured = true;
        st.rendered = None;
        st.frame = None;
        self.shared.render(&mut st, false);
        Ok(())
    }

    pub fn info(&self) -> DevelopInfo {
        let s = &self.shared;
        let e = s.image.active_extent();
        let (as_shot_temperature, as_shot_tint) = as_shot_white(&s.image);
        DevelopInfo {
            image_id: s.image_id.clone(),
            width: e.width,
            height: e.height,
            orientation: s.image.metadata().orientation,
            as_shot_temperature,
            as_shot_tint,
            backend: s.backend.clone(),
        }
    }

    pub fn set_listener(&self, listener: Option<Arc<dyn DevelopListener>>) {
        *self
            .shared
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = listener;
    }

    /// Surface size for a viewport of `width × height` device pixels, both in
    /// sensor orientation (swap them for orientations 5–8): the coarsest
    /// level covering the viewport, or level 0 when the image is smaller.
    pub fn plan_surface(&self, width: u32, height: u32) -> SurfacePlan {
        let image = &self.shared.image;
        let level = (0..=MAX_LEVEL)
            .rev()
            .find(|&l| {
                let e = image.level_extent(l);
                e.width >= width && e.height >= height
            })
            .unwrap_or(0);
        let e = image.level_extent(level);
        SurfacePlan {
            level,
            width: e.width,
            height: e.height,
        }
    }

    /// Adds an RGBA8 (SDR) or `'RGhA'` RGBA16F (EDR) IOSurface (see
    /// `surface.rs`) to the frame ring; call two or three times with
    /// surfaces of one `plan_surface` size and one format so the host never
    /// samples the surface being written. A different size or format
    /// replaces the ring. The first surface of a ring starts a render at the
    /// new screen level in the ring's contract.
    pub fn attach_surface(&self, iosurface_id: u32, width: u32, height: u32) -> Result<()> {
        let s = &self.shared;
        let level = (0..=MAX_LEVEL)
            .find(|&l| {
                let e = s.image.level_extent(l);
                e.width == width && e.height == height
            })
            .ok_or_else(|| failure("surface size must come from plan_surface"))?;
        let surface =
            Arc::new(Surface::lookup_presentation(iosurface_id, width, height).map_err(failure)?);
        let mut st = s.edit_lock()?;
        if st.surfaces.first().is_some_and(|f| {
            f.width() != width || f.height() != height || f.kind() != surface.kind()
        }) {
            st.surfaces.clear();
        }
        st.surfaces.retain(|x| x.id() != surface.id());
        if st.surfaces.len() >= 3 {
            st.surfaces.remove(0);
        }
        // The first surface of a ring starts a render at the new screen
        // level; further surfaces of the same ring only join the rotation.
        let first_of_ring = st.surfaces.is_empty();
        let level_changed = st.screen_level != level;
        st.surfaces.push(surface);
        st.screen_level = level;
        if first_of_ring || level_changed {
            s.render(&mut st, false);
        }
        Ok(())
    }

    /// Reports the EDR headroom of the display showing the viewport: the
    /// screen's current `maximumExtendedDynamicRangeColorComponentValue`
    /// (1.0 on SDR displays). Float rings tone-map for at most this; a
    /// change re-renders only when it changes the frame's headroom.
    pub fn set_display_headroom(&self, headroom: f32) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        let before = st.output();
        st.display_headroom = finite_or(headroom, 1.0).max(1.0);
        if st.output() != before && !st.surfaces.is_empty() {
            self.shared.render(&mut st, false);
        }
        Ok(())
    }

    /// Headroom (linear multiple of SDR white) the viewport renders for:
    /// 0 for the SDR RGBA8 contract, else the float frames' peak.
    pub fn presentation_headroom(&self) -> Result<f32> {
        Ok(match self.shared.lock()?.output() {
            RenderOutput::DisplayLinear(h) => h.get(),
            _ => 0.0,
        })
    }

    /// Releases every surface. Renders continue (histogram only) until a new
    /// surface is attached.
    pub fn detach_surfaces(&self) {
        if let Ok(mut st) = self.shared.lock() {
            st.surfaces.clear();
            st.next_surface = SurfaceCursor::default();
        }
    }

    /// Re-renders the live settings at the screen level (first paint).
    pub fn refresh(&self) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        self.shared.render(&mut st, false);
        Ok(())
    }

    /// Merges an RFC 7386 JSON patch into the live settings and renders.
    /// Setting `white_balance.temperature`/`tint` without a mode switches to
    /// `custom`. Not recorded in history until `commit`.
    pub fn set_settings(&self, json_patch: String, interactive: bool) -> Result<()> {
        let patch: Value = serde_json::from_str(&json_patch).map_err(failure)?;
        let mut st = self.shared.edit_lock()?;
        let mut value = serde_json::to_value(&st.live).map_err(failure)?;
        merge_patch(&mut value, &patch);
        if let Some(wb) = patch.get("white_balance").and_then(Value::as_object)
            && (wb.contains_key("temperature") || wb.contains_key("tint"))
            && !wb.contains_key("mode")
        {
            if st.live.white_balance.mode == WhiteBalanceMode::AsShot {
                let (temperature, tint) = as_shot_white(&self.shared.image);
                if !wb.contains_key("temperature") {
                    value["white_balance"]["temperature"] = serde_json::json!(temperature);
                }
                if !wb.contains_key("tint") {
                    value["white_balance"]["tint"] = serde_json::json!(tint);
                }
            }
            value["white_balance"]["mode"] = Value::String("custom".into());
        }
        let mut next: DevelopSettings = serde_json::from_value(value).map_err(failure)?;
        pin_new_denoise_edit(&self.shared.model_registry, &st.live, &mut next)?;
        if next == st.live && st.rendered.is_some() && !interactive {
            return Ok(());
        }
        // Changes that draw nothing (the aspect lock, crop edits while the crop
        // tool shows the whole frame) update the live state without a render.
        // HDR toggle/headroom draw only through a float ring's presentation.
        let unchanged = !st.masking_preview
            && st.rendered_level == Some(st.screen_level)
            && st.rendered.as_ref() == Some(&renderable_with(&next, !st.crop_editing))
            && presentation(&next, st.float_ring(), st.display_headroom) == st.output();
        st.live = next;
        if !unchanged {
            self.shared.render(&mut st, interactive);
        }
        Ok(())
    }

    /// Records the live changes since the last commit as one undo step
    /// labelled `label`, refines the viewport and schedules a save.
    pub fn commit(&self, label: String) -> Result<bool> {
        let mut st = self.shared.edit_lock()?;
        let recorded = self.shared.commit_pending(&mut st, &label)?;
        if st.rendered_level != Some(st.screen_level) || st.rendered.as_ref() != Some(&st.drawn()) {
            self.shared.render(&mut st, false);
        }
        if recorded {
            self.shared.schedule_save();
        }
        Ok(recorded)
    }

    pub fn get_settings_json(&self) -> Result<String> {
        serde_json::to_string(&self.shared.lock()?.live).map_err(failure)
    }

    /// JSON ProcessVersion (`family`, `revision`) for this session's recipe.
    pub fn get_process_version(&self) -> Result<String> {
        serde_json::to_string(&self.shared.lock()?.recipe.process_version).map_err(failure)
    }

    /// Switch operator sets as a persisted undo step. Pending sliders commit first.
    pub fn set_process_version(&self, json: String) -> Result<bool> {
        use engine_api::recipe::{ProcessFamily, ProcessVersion};
        let version: ProcessVersion = serde_json::from_str(&json).map_err(failure)?;
        if (version.family == ProcessFamily::Adobe && !(3..=6).contains(&version.revision))
            || (version.family == ProcessFamily::Native
                && version != ProcessVersion::NATIVE_CURRENT)
        {
            return Err(failure("unsupported process version"));
        }
        let mut st = self.shared.edit_lock()?;
        if st.recipe.process_version == version {
            return Ok(false);
        }
        self.shared.commit_pending(&mut st, "Edit")?;
        crate::backend::record_process(&mut st.recipe, version, now_ms())?;
        st.rendered = None;
        st.frame = None;
        self.shared.render(&mut st, false);
        self.shared.schedule_save();
        Ok(true)
    }

    /// Settings kept in the recipe but not rendered by this pipeline version.
    pub fn ignored_settings(&self) -> Result<Vec<String>> {
        let st = self.shared.lock()?;
        let mut ignored = ignored_settings(&st.live);
        ignored.retain(|path| !path.starts_with("/effects/lens_blur"));
        if pipeline_cpu::validate_denoise(&st.live.denoise).is_ok() {
            ignored.retain(|path| !path.starts_with("/denoise"));
        }
        Ok(ignored)
    }

    /// Histogram of the last completed frame (empty before the first one).
    pub fn get_histogram(&self) -> Result<Histogram> {
        Ok(self.shared.lock()?.histogram.clone())
    }

    pub fn history_state(&self) -> Result<HistoryState> {
        let st = self.shared.lock()?;
        let h = &st.recipe.history;
        Ok(HistoryState {
            can_undo: h.undo_target().is_some() || st.live != st.recipe.settings,
            can_redo: h.redo_target().is_some() && st.live == st.recipe.settings,
            entries: h.entries.len() as u32,
            head_label: h
                .head
                .and_then(|id| h.entry(id))
                .map(|e| e.meta.label.clone()),
            snapshots: h.snapshots.iter().map(|s| s.name.clone()).collect(),
            uncommitted: st.live != st.recipe.settings,
        })
    }

    /// Steps back one history entry (committing pending live changes first).
    pub fn undo(&self) -> Result<bool> {
        self.history_move(|r| r.undo())
    }

    pub fn redo(&self) -> Result<bool> {
        self.history_move(|r| r.redo())
    }

    /// Resets every setting to its default as one undo step.
    pub fn reset(&self) -> Result<bool> {
        let mut st = self.shared.edit_lock()?;
        self.shared.commit_pending(&mut st, "Edit")?;
        let changed = st
            .recipe
            .edit(EditMeta::user("Reset", now_ms()), |s| {
                *s = DevelopSettings::default()
            })?
            .is_some();
        if changed {
            st.live = st.recipe.settings.clone();
            self.shared.render(&mut st, false);
            self.shared.schedule_save();
        }
        Ok(changed)
    }

    /// Names the current state (after committing pending changes).
    pub fn snapshot(&self, name: String) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        self.shared.commit_pending(&mut st, "Edit")?;
        st.recipe.create_snapshot(name, now_ms())?;
        self.shared.schedule_save();
        Ok(())
    }

    pub fn snapshots(&self) -> Result<Vec<String>> {
        Ok(self
            .shared
            .lock()?
            .recipe
            .history
            .snapshots
            .iter()
            .map(|s| s.name.clone())
            .collect())
    }

    /// Moves to a snapshot's state; later edits branch from it.
    pub fn restore_snapshot(&self, name: String) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        self.shared.commit_pending(&mut st, "Edit")?;
        st.recipe.restore_snapshot(&name)?;
        crate::backend::sync_process(&mut st.recipe)?;
        st.frame = None;
        st.live = st.recipe.settings.clone();
        self.shared.render(&mut st, false);
        self.shared.schedule_save();
        Ok(())
    }

    /// Crop tool on/off. While on, the viewport renders the whole unrotated
    /// frame (geometry is kept in the settings) so the host can draw and
    /// edit the crop over it; off renders the cropped result again.
    pub fn set_crop_editing(&self, editing: bool) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        if st.crop_editing != editing {
            st.crop_editing = editing;
            self.shared.render(&mut st, false);
        }
        Ok(())
    }

    /// Session-only uncorrected image for Guided Upright placement. Saved
    /// lens, Upright, manual transform and crop controls remain intact.
    pub fn set_render_uncorrected(&self, enabled: bool) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        if st.render_uncorrected != enabled {
            st.render_uncorrected = enabled;
            st.rendered = None;
            st.frame = None;
            self.shared.render(&mut st, false);
        }
        Ok(())
    }

    /// Session-only grayscale depth overlay. Missing weights surface through
    /// render_failed just like other rendering errors; this never downloads.
    pub fn set_render_depth_visualisation(&self, enabled: bool) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        if st.render_depth_visualisation != enabled {
            st.render_depth_visualisation = enabled;
            st.rendered = None;
            st.frame = None;
            self.shared.render(&mut st, false);
        }
        Ok(())
    }

    /// 256 near-to-far bins for the current recipe's pre-geometry image.
    /// Reuses the content-addressed depth cache. This is a blocking worker API.
    pub fn depth_histogram(&self) -> Result<Vec<u64>> {
        let input = self.shared.depth_input()?;
        // The provider's latest result can belong to an older recipe while a
        // new render is pending. Return this snapshot's estimate directly.
        Ok(self
            .shared
            .depth_provider
            .estimate(&input)?
            .histogram()
            .to_vec())
    }

    /// Focus the inclusive range around the segmented subject's depth. Uses
    /// cached models only and leaves all controls unchanged on failure.
    pub fn focus_lens_blur_on_subject(&self) -> Result<Vec<f32>> {
        let input = self.shared.depth_input()?;
        let range = self.shared.depth_provider.subject_focus(&input)?;
        self.set_settings(
            serde_json::json!({"effects":{"lens_blur":{"focus_range":range}}}).to_string(),
            false,
        )?;
        Ok(range.to_vec())
    }

    /// Shows the sharpening Masking gate (white = sharpened) instead of the
    /// image while on; frames report `is_overlay`. Settings changes keep
    /// updating the overlay.
    pub fn set_masking_preview(&self, enabled: bool) -> Result<()> {
        let mut st = self.shared.edit_lock()?;
        if st.masking_preview != enabled {
            st.masking_preview = enabled;
            self.shared.render(&mut st, false);
        }
        Ok(())
    }

    /// The history as a list: the applied steps from the oldest, then the
    /// undone steps redo would reapply.
    pub fn history_items(&self) -> Result<Vec<HistoryItem>> {
        Ok(history_items(&self.shared.lock()?.recipe)?)
    }

    /// Moves to the state after history step `id` (`None`: the base state),
    /// committing pending changes first. Later steps stay listed until a
    /// new edit branches from there.
    pub fn checkout_history(&self, id: Option<u64>) -> Result<bool> {
        self.history_move(|r| {
            let target = id.map(HistoryEntryId);
            if let Some(t) = target {
                r.history
                    .entry(t)
                    .ok_or_else(|| engine_api::EngineError::not_found("history entry", t))?;
            }
            if r.history.head == target {
                return Ok(false);
            }
            r.checkout(target).map(|()| true)
        })
    }

    /// Turns an applied step's changes off (or back on) as a new undoable
    /// step: the state is the history replayed without the disabled steps.
    pub fn set_history_step_enabled(&self, id: u64, enabled: bool) -> Result<bool> {
        let changed = {
            let mut st = self.shared.edit_lock()?;
            self.shared.commit_pending(&mut st, "Edit")?;
            let h = &st.recipe.history;
            let lineage = h.lineage(h.head)?;
            let Some(step) = lineage.iter().find(|e| e.id.0 == id) else {
                return Err(failure("only applied history steps can be toggled"));
            };
            if toggle_of(step).is_some() {
                return Err(failure("a step toggle cannot itself be toggled"));
            }
            if amount_of(step).is_some() {
                return Err(failure("a group amount step cannot be toggled"));
            }
            let mut off = disabled_steps(&lineage);
            if off.contains(&id) != enabled {
                return Ok(false);
            }
            if enabled {
                off.remove(&id);
            } else {
                off.insert(id);
            }
            let next = replay_state(&h.base, &lineage, &off, &Default::default())?;
            let label = format!(
                "{} {}",
                if enabled { "Turn On" } else { "Turn Off" },
                display_label(step)
            );
            let meta = EditMeta {
                rationale: Some(format!(
                    "{TOGGLE_MARKER}:{id}:{}",
                    if enabled { "on" } else { "off" }
                )),
                ..EditMeta::user(label, now_ms())
            };
            // A step that later steps fully override changes nothing and is
            // not recorded.
            let recorded = st
                .recipe
                .edit(meta.clone(), |s| *s = next.clone())?
                .is_some();
            if !recorded {
                return Ok(false);
            }
            st.live = st.recipe.settings.clone();
            self.shared.render(&mut st, false);
            self.shared.schedule_save();
            true
        };
        Ok(changed)
    }

    /// Named groups on the current lineage with their amount and the settings
    /// at 0 % and 100 % (see `HistoryGroupState`), oldest group first.
    pub fn history_groups(&self) -> Result<Vec<HistoryGroupState>> {
        Ok(history_groups(&self.shared.lock()?.recipe)?)
    }

    /// Records the amount (0...1) of history group `group_id` as one undoable
    /// step: the state is the history replayed with that group blended in at
    /// `amount` (numbers interpolate between the group off and on; other
    /// values switch at 50 %), later steps and toggles kept. Uncommitted live
    /// changes (an amount preview) are replaced by the recorded state.
    pub fn commit_group_amount(&self, group_id: u32, amount: f64) -> Result<bool> {
        if !(0.0..=1.0).contains(&amount) {
            return Err(failure("amount must be in [0, 1]"));
        }
        let changed = {
            let mut st = self.shared.edit_lock()?;
            let changed = record_group_amount(&mut st.recipe, group_id, amount, now_ms())?;
            st.live = st.recipe.settings.clone();
            self.shared.render(&mut st, false);
            if changed {
                self.shared.schedule_save();
            }
            changed
        };
        Ok(changed)
    }

    /// Renders a 1:1 (level 0) crop of the live settings centred on
    /// (`center_x`, `center_y`), normalized active-area coordinates in sensor
    /// orientation, into an RGBA8 IOSurface of `width × height`. Blocking:
    /// call off the main thread. Crop and post-crop effects are not applied
    /// (the crop shows sensor pixels); global dehaze statistics come from the
    /// window (the full sensor when learned denoise is active).
    pub fn render_detail_preview(
        &self,
        iosurface_id: u32,
        width: u32,
        height: u32,
        center_x: f32,
        center_y: f32,
    ) -> Result<DetailPreview> {
        let s = &self.shared;
        let surface = Surface::lookup(iosurface_id, width, height).map_err(failure)?;
        let (mut settings, version) = {
            let st = s.edit_lock()?;
            (st.drawn(), st.recipe.process_version)
        };
        settings.geometry = Default::default();
        settings.effects = Default::default();
        let e = s.image.active_extent();
        let (w, h) = (width.min(e.width), height.min(e.height));
        let origin = |center: f32, size: u32, total: u32| {
            let c = if center.is_finite() {
                center.clamp(0.0, 1.0)
            } else {
                0.5
            };
            ((c * total as f32 - size as f32 / 2.0).round().max(0.0) as u32).min(total - size)
        };
        let (x, y) = (origin(center_x, w, e.width), origin(center_y, h, e.height));
        // Window with a margin of real neighbours, clamped to the active area.
        let wx = x.saturating_sub(DETAIL_MARGIN);
        let wy = y.saturating_sub(DETAIL_MARGIN);
        let ww = (x + w + DETAIL_MARGIN).min(e.width) - wx;
        let wh = (y + h + DETAIL_MARGIN).min(e.height) - wy;
        // Learned CFA padding and masks belong to the complete sensor. Keep
        // its identity for detail crops so they share the viewport inference.
        let (window, ox, oy) = if pipeline_cpu::denoise_active(&settings.denoise) {
            (s.image.clone(), x, y)
        } else {
            settings.locals = masks::window_locals(&settings.locals, e, (wx, wy, ww, wh));
            (window_image(&s.image, wx, wy, ww, wh)?, x - wx, y - wy)
        };
        let tiles = s
            .renderer_snapshot(&settings)
            .for_process_version(version)
            .render_region(&window, &settings, 0, PixelRect::new(ox, oy, w, h))?;
        surface
            .with_pixels(|px, stride| {
                for t in &tiles {
                    let l = t.layout();
                    let n = l.plane_len();
                    let Ok(d) = t.samples::<u8>() else { continue };
                    let (tx, ty) = t.coord().pixel_origin(TILE_SIZE);
                    for row in 0..l.extent.height {
                        let gy = ty + row;
                        if gy < oy || gy >= oy + h {
                            continue;
                        }
                        let line = &mut px[(gy - oy) as usize * stride..];
                        for col in 0..l.extent.width {
                            let gx = tx + col;
                            if gx < ox || gx >= ox + w {
                                continue;
                            }
                            let i = (row * l.extent.width + col) as usize;
                            let o = 4 * (gx - ox) as usize;
                            line[o..o + 4].copy_from_slice(&[d[i], d[n + i], d[2 * n + i], 255]);
                        }
                    }
                }
            })
            .map_err(failure)?;
        Ok(DetailPreview {
            x,
            y,
            width: w,
            height: h,
        })
    }

    /// Writes pending changes now and waits for the save to finish.
    pub fn flush(&self) -> Result<()> {
        if self.on_save_worker() {
            return Err(failure("Develop save worker cannot flush itself"));
        }
        self.flush_inner(false)
    }

    /// Writes pending changes, drains the save worker and stops rendering.
    /// A failed write leaves this session open for a later retry.
    pub fn close(&self) -> Result<()> {
        if self.on_save_worker() {
            return Err(failure("Develop save worker cannot close itself"));
        }
        let (attempt, leader) = {
            let mut phase = self.close_phase.lock().map_err(failure)?;
            match &*phase {
                ClosePhase::Closed => return Ok(()),
                ClosePhase::Closing(attempt) => (attempt.clone(), false),
                ClosePhase::Open => {
                    let attempt = Arc::new(CloseAttempt {
                        state: Mutex::new(CloseAttemptState {
                            outcome: None,
                            #[cfg(test)]
                            waiters: 0,
                        }),
                        cv: Condvar::new(),
                    });
                    *phase = ClosePhase::Closing(attempt.clone());
                    (attempt, true)
                }
            }
        };
        if !leader {
            return Self::wait_close_attempt(&attempt);
        }
        let result = (|| {
            self.shared.lock()?.closing = true;
            self.flush_inner(true)?;
            self.stop_writer();
            self.shared.close();
            let released = self.lease.lock().map_err(failure)?.take();
            drop(released);
            Ok(())
        })();
        if result.is_err()
            && let Ok(mut st) = self.shared.lock()
        {
            st.closing = false;
        }
        self.finish_close_attempt(&attempt, &result);
        result
    }
}

// ─────────────────────────── masking preview ───────────────────────────

/// Settings of the stages before Detail: what the sharpening mask sees.
fn mask_upstream(live: &DevelopSettings) -> DevelopSettings {
    let mut base = renderable_with(live, false);
    base.detail = Default::default();
    base.detail.sharpening.amount = 0.0;
    base.detail.noise_reduction.color = 0.0;
    base.tone = Default::default();
    base.color = Default::default();
    base.effects = Default::default();
    base.locals = Default::default();
    base
}

/// Per-pixel edge strength `mean_sobel / (|Y| + .1)` of the Detail input at
/// one level (the operator's masking statistic), so a Masking drag only
/// re-thresholds it.
struct MaskSource {
    level: u8,
    upstream: DevelopSettings,
    width: u32,
    height: u32,
    edge: Vec<f32>,
}

impl MaskSource {
    fn compute(
        shared: &Shared,
        level: u8,
        upstream: &DevelopSettings,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> engine_api::EngineResult<Self> {
        let e = shared.image.level_extent(level);
        let tiles = shared.renderer_snapshot(upstream).render_region_as(
            &shared.image,
            upstream,
            level,
            PixelRect::full(e),
            image_core::RenderOutput::SceneLinear,
        )?;
        cancel.check()?;
        let (w, h) = (e.width as usize, e.height as usize);
        let mut y = vec![0f32; w * h];
        for t in &tiles {
            let l = t.layout();
            let n = l.plane_len();
            let d = t.samples::<f32>()?;
            let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
            let (tw, th) = (l.extent.width as usize, l.extent.height as usize);
            for row in 0..th {
                for col in 0..tw {
                    let i = row * tw + col;
                    let v = 0.2627 * d[i] + 0.6780 * d[n + i] + 0.0593 * d[2 * n + i];
                    y[(oy as usize + row) * w + ox as usize + col] = v;
                }
            }
        }
        cancel.check()?;
        let mut edge = vec![0f32; w * h];
        let threads = std::thread::available_parallelism()
            .map_or(4, |n| n.get())
            .min(8);
        let band = h.div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            for (b, out) in edge.chunks_mut(band * w).enumerate() {
                let y = &y;
                scope.spawn(move || {
                    let at = |x: isize, r: isize| {
                        y[r.clamp(0, h as isize - 1) as usize * w
                            + x.clamp(0, w as isize - 1) as usize]
                    };
                    for (k, o) in out.iter_mut().enumerate() {
                        let (r, x) = ((b * band + k / w) as isize, (k % w) as isize);
                        let mut sum = 0.0;
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                let (cx, cy) = (x + dx, r + dy);
                                let gx =
                                    at(cx + 1, cy - 1) + 2.0 * at(cx + 1, cy) + at(cx + 1, cy + 1)
                                        - at(cx - 1, cy - 1)
                                        - 2.0 * at(cx - 1, cy)
                                        - at(cx - 1, cy + 1);
                                let gy =
                                    at(cx - 1, cy + 1) + 2.0 * at(cx, cy + 1) + at(cx + 1, cy + 1)
                                        - at(cx - 1, cy - 1)
                                        - 2.0 * at(cx, cy - 1)
                                        - at(cx + 1, cy - 1);
                                sum += gx.hypot(gy) / 8.0;
                            }
                        }
                        *o = sum / 9.0 / (at(x, r).abs() + 0.1);
                    }
                });
            }
        });
        Ok(Self {
            level,
            upstream: upstream.clone(),
            width: e.width,
            height: e.height,
            edge,
        })
    }

    /// The operator's gate: white where sharpening applies.
    fn gate(edge: f32, masking: f32) -> u8 {
        if masking <= 0.0 {
            return 255;
        }
        let t = ((edge - 0.15 * masking / 100.0) / 0.05).clamp(0.0, 1.0);
        (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8
    }
}

/// Renders the sharpening edge mask (Masking slider with ⌥) into the ring.
struct MaskJob {
    shared: std::sync::Weak<Shared>,
    generation: u64,
    level: u8,
    upstream: DevelopSettings,
    masking: f32,
}

impl Job for MaskJob {
    fn label(&self) -> &str {
        "develop masking preview"
    }
    fn priority(&self) -> Priority {
        Priority::Viewport
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(());
        };
        let started = Instant::now();
        let _serial = shared
            .render_serial
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let current = |shared: &Shared| shared.generation.load(Ordering::SeqCst) == self.generation;
        if !current(&shared) {
            return Err(engine_api::EngineError::Cancelled);
        }
        let cached = shared
            .mask_source
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .filter(|m| m.level == self.level && m.upstream == self.upstream);
        let source = match cached {
            Some(m) => m,
            None => {
                let m = Arc::new(MaskSource::compute(
                    &shared,
                    self.level,
                    &self.upstream,
                    &ctx.cancellation,
                )?);
                *shared.mask_source.lock().unwrap_or_else(|e| e.into_inner()) = Some(m.clone());
                m
            }
        };
        ctx.cancellation.check()?;
        let surface = {
            let st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.generation != self.generation {
                return Err(engine_api::EngineError::Cancelled);
            }
            st.next_surface
                .candidate(&st.surfaces.iter().map(|s| s.id()).collect::<Vec<_>>())
                .map(|i| st.surfaces[i].clone())
        };
        let (w, h) = match &surface {
            Some(s) => (source.width.min(s.width()), source.height.min(s.height())),
            None => (source.width, source.height),
        };
        if let Some(surface) = &surface {
            let masking = self.masking;
            let float = surface.is_float();
            surface
                .with_pixels(|px, stride| {
                    for (r, row) in px.chunks_mut(stride).take(h as usize).enumerate() {
                        let src = &source.edge[r * source.width as usize..][..w as usize];
                        for (x, &e) in src.iter().enumerate() {
                            let g = MaskSource::gate(e, masking);
                            if float {
                                // Same grey as the RGBA8 path, as linear light.
                                let v = f32::from(g) / 255.0;
                                let l = if v <= 0.04045 {
                                    v / 12.92
                                } else {
                                    ((v + 0.055) / 1.055).powf(2.4)
                                };
                                let l = half::f16::from_f32(l).to_le_bytes();
                                let one = half::f16::ONE.to_le_bytes();
                                row[8 * x..8 * x + 8].copy_from_slice(&[
                                    l[0], l[1], l[0], l[1], l[0], l[1], one[0], one[1],
                                ]);
                            } else {
                                row[4 * x..4 * x + 4].copy_from_slice(&[g, g, g, 255]);
                            }
                        }
                    }
                })
                .map_err(engine_api::EngineError::internal)?;
        }
        {
            let mut st = shared.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.generation != self.generation {
                return Err(engine_api::EngineError::Cancelled);
            }
            if let Some(s) = &surface {
                st.next_surface.published(s.id());
            }
        }
        if let Some(listener) = shared.listener() {
            listener.frame_ready(FrameInfo {
                surface_id: surface.as_ref().map_or(0, |s| s.id()),
                level: self.level,
                width: w,
                height: h,
                first_level: self.level,
                is_final: true,
                render_ms: started.elapsed().as_secs_f64() * 1000.0,
                generation: self.generation,
                dirty_stage: Some(stage_name(StageId::Detail)),
                display_width: source.width,
                display_height: source.height,
                is_overlay: true,
            });
        }
        Ok(())
    }
}

// ─────────────────────────────── history ───────────────────────────────

/// Marker in [`EditMeta::rationale`] of a step-toggle entry.
const TOGGLE_MARKER: &str = "tessera:step-toggle";

pub(crate) fn toggle_of(e: &HistoryEntry) -> Option<(u64, bool)> {
    let rest = e.meta.rationale.as_deref()?.strip_prefix(TOGGLE_MARKER)?;
    let mut parts = rest.trim_start_matches(':').split(':');
    let id = parts.next()?.parse().ok()?;
    let enabled = match parts.next()? {
        "on" => true,
        "off" => false,
        _ => return None,
    };
    Some((id, enabled))
}

/// Steps turned off along `lineage` (toggle entries replayed in order).
pub(crate) fn disabled_steps(lineage: &[&HistoryEntry]) -> std::collections::BTreeSet<u64> {
    let mut off = std::collections::BTreeSet::new();
    for e in lineage {
        if let Some((id, enabled)) = toggle_of(e) {
            if enabled {
                off.remove(&id);
            } else {
                off.insert(id);
            }
        }
    }
    off
}

const AMOUNT_MARKER: &str = "tessera:group-amount";

/// `(group, amount)` of a group-amount step.
pub(crate) fn amount_of(e: &HistoryEntry) -> Option<(u32, f64)> {
    let rest = e.meta.rationale.as_deref()?.strip_prefix(AMOUNT_MARKER)?;
    let mut parts = rest.trim_start_matches(':').split(':');
    let group = parts.next()?.parse().ok()?;
    let amount: f64 = parts.next()?.parse().ok()?;
    (0.0..=1.0).contains(&amount).then_some((group, amount))
}

/// Replays `lineage` from `base`, skipping toggle entries, group-amount
/// entries and `off` steps; groups with an amount below 1 (the latest amount
/// step on the lineage, or `overrides`) are blended in at that amount.
pub(crate) fn replay_state(
    base: &DevelopSettings,
    lineage: &[&HistoryEntry],
    off: &std::collections::BTreeSet<u64>,
    overrides: &std::collections::BTreeMap<u32, f64>,
) -> engine_api::EngineResult<DevelopSettings> {
    let mut amounts = std::collections::BTreeMap::new();
    for e in lineage {
        if let Some((group, amount)) = amount_of(e) {
            amounts.insert(group, amount);
        }
    }
    amounts.extend(overrides.iter().map(|(g, a)| (*g, *a)));
    let faded: Vec<(u32, f64)> = amounts.into_iter().filter(|(_, a)| *a < 1.0).collect();
    let replay = |include: Option<u32>| -> engine_api::EngineResult<Value> {
        let mut value = serde_json::to_value(base)?;
        for e in lineage {
            if toggle_of(e).is_some() || amount_of(e).is_some() || off.contains(&e.id.0) {
                continue;
            }
            if let Some(g) = e.meta.group.map(|g| g.0)
                && Some(g) != include
                && faded.iter().any(|(f, _)| *f == g)
            {
                continue;
            }
            for change in &e.changes {
                apply_change(&mut value, change)?;
            }
        }
        Ok(value)
    };
    let without = replay(None)?;
    let mut out = without.clone();
    for &(group, amount) in &faded {
        if amount > 0.0 {
            blend_into(&mut out, &without, &replay(Some(group))?, amount);
        }
    }
    Ok(serde_json::from_value(out)?)
}

/// Adds `amount · (with − without)` to `out` on numbers (rounded when both
/// are integers); other differing values take `with` from 50 %.
fn blend_into(out: &mut Value, without: &Value, with: &Value, amount: f64) {
    match (without, with) {
        (Value::Number(a), Value::Number(b)) => {
            let (Some(x), Some(y)) = (a.as_f64(), b.as_f64()) else {
                return;
            };
            let current = out.as_f64().unwrap_or(x);
            let next = current + amount * (y - x);
            *out = if a.is_f64() || b.is_f64() {
                serde_json::Number::from_f64(next).map_or(Value::Null, Value::Number)
            } else {
                Value::from(next.round() as i64)
            };
        }
        (Value::Object(a), Value::Object(b)) => {
            let Value::Object(target) = out else {
                if amount >= 0.5 {
                    *out = with.clone();
                }
                return;
            };
            for (key, vb) in b {
                match a.get(key) {
                    Some(va) => {
                        let slot = target.entry(key.clone()).or_insert_with(|| va.clone());
                        blend_into(slot, va, vb, amount);
                    }
                    None if amount >= 0.5 => {
                        target.insert(key.clone(), vb.clone());
                    }
                    None => {}
                }
            }
            if amount >= 0.5 {
                for key in a.keys().filter(|k| !b.contains_key(*k)) {
                    target.remove(key);
                }
            }
        }
        (a, b) if a == b => {}
        _ if amount >= 0.5 => *out = with.clone(),
        _ => {}
    }
}

/// Applies `amount` to `group` as one recorded step (see `commit_group_amount`).
pub(crate) fn record_group_amount(
    recipe: &mut Recipe,
    group: u32,
    amount: f64,
    timestamp_ms: i64,
) -> engine_api::EngineResult<bool> {
    let h = &recipe.history;
    let lineage = h.lineage(h.head)?;
    if !lineage
        .iter()
        .any(|e| e.meta.group.map(|g| g.0) == Some(group) && amount_of(e).is_none())
    {
        return Err(engine_api::EngineError::not_found(
            "history group",
            HistoryGroupId(group),
        ));
    }
    let current = lineage
        .iter()
        .rev()
        .find_map(|e| amount_of(e).filter(|(g, _)| *g == group))
        .map_or(1.0, |(_, a)| a);
    if (current - amount).abs() < 1e-9 {
        return Ok(false);
    }
    let off = disabled_steps(&lineage);
    let next = replay_state(&h.base, &lineage, &off, &[(group, amount)].into())?;
    let name = h
        .groups
        .iter()
        .find(|g| g.id.0 == group)
        .map_or_else(|| "Group".to_owned(), |g| g.name.clone());
    let meta = EditMeta {
        rationale: Some(format!("{AMOUNT_MARKER}:{group}:{amount}")),
        ..EditMeta::user(format!("{name} {:.0}%", amount * 100.0), timestamp_ms)
    };
    if recipe.edit(meta.clone(), |s| *s = next)?.is_none() {
        // The amount changes nothing visible but is still the new state.
        let history = &mut recipe.history;
        let id = HistoryEntryId(history.entries.len() as u64 + 1);
        history.entries.push(HistoryEntry {
            id,
            parent: history.head,
            meta,
            changes: vec![],
        });
        history.head = Some(id);
    }
    Ok(true)
}

/// "Exposure, Contrast": the controls a step sets (agent steps are labelled
/// with tool names such as `set_tone`).
pub(crate) fn changes_title(changes: &[engine_api::recipe::history::ParamChange]) -> String {
    let mut names: Vec<String> = Vec::new();
    for change in changes {
        let token = change.path().rsplit('/').next().unwrap_or_default();
        let name = match token {
            "temperature" => "Temperature".to_owned(),
            "mode" => continue,
            t => {
                let mut c = t.replace('_', " ").chars().collect::<Vec<_>>();
                if let Some(first) = c.first_mut() {
                    *first = first.to_ascii_uppercase();
                }
                c.into_iter().collect()
            }
        };
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    if names.is_empty() {
        "No change".into()
    } else {
        names.join(", ")
    }
}

/// History label: agent steps named after an engine tool read as their controls.
fn display_label(e: &HistoryEntry) -> String {
    let tool = !e.meta.label.is_empty()
        && e.meta
            .label
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_');
    if matches!(e.meta.author, Author::Agent { .. }) && tool && !e.changes.is_empty() {
        changes_title(&e.changes)
    } else {
        e.meta.label.clone()
    }
}

pub(crate) fn history_groups(recipe: &Recipe) -> engine_api::EngineResult<Vec<HistoryGroupState>> {
    let h = &recipe.history;
    let lineage = h.lineage(h.head)?;
    let off = disabled_steps(&lineage);
    let mut order: Vec<u32> = Vec::new();
    for e in &lineage {
        if let Some(g) = e.meta.group.map(|g| g.0)
            && amount_of(e).is_none()
            && toggle_of(e).is_none()
            && !order.contains(&g)
        {
            order.push(g);
        }
    }
    order
        .into_iter()
        .map(|group| {
            let amount = lineage
                .iter()
                .rev()
                .find_map(|e| amount_of(e).filter(|(g, _)| *g == group))
                .map_or(1.0, |(_, a)| a);
            let at = |a: f64| -> engine_api::EngineResult<String> {
                let s = replay_state(&h.base, &lineage, &off, &[(group, a)].into())?;
                Ok(serde_json::to_string(&s)?)
            };
            Ok(HistoryGroupState {
                group_id: group,
                name: h
                    .groups
                    .iter()
                    .find(|g| g.id.0 == group)
                    .map_or_else(|| "Group".to_owned(), |g| g.name.clone()),
                amount,
                steps: lineage
                    .iter()
                    .filter(|e| {
                        e.meta.group.map(|g| g.0) == Some(group)
                            && amount_of(e).is_none()
                            && toggle_of(e).is_none()
                    })
                    .map(|e| e.id.0)
                    .collect(),
                without_json: at(0.0)?,
                with_json: at(1.0)?,
            })
        })
        .collect()
}

fn history_items(recipe: &Recipe) -> engine_api::EngineResult<Vec<HistoryItem>> {
    let h = &recipe.history;
    let lineage = h.lineage(h.head)?;
    let off = disabled_steps(&lineage);
    let author = |e: &HistoryEntry| match &e.meta.author {
        Author::User => "user".to_owned(),
        Author::Agent { name } => format!("agent:{name}"),
        Author::Import { source } => format!("import:{source}"),
        Author::Preset { style } => format!("preset:{style}"),
        Author::Sync { from } => format!("sync:{from}"),
    };
    let group = |e: &HistoryEntry| {
        e.meta
            .group
            .and_then(|g| h.groups.iter().find(|x| x.id == g))
            .map(|g| g.name.clone())
    };
    let item = |e: &HistoryEntry, applied: bool| HistoryItem {
        id: e.id.0,
        label: display_label(e),
        author: author(e),
        group: group(e),
        timestamp_ms: e.meta.timestamp_ms,
        applied,
        is_head: h.head == Some(e.id),
        enabled: !off.contains(&e.id.0),
        toggles: toggle_of(e).map(|(id, _)| id),
        group_amount: amount_of(e).map(|(_, a)| a),
        group_id: e.meta.group.map(|g| g.0),
        rationale: e
            .meta
            .rationale
            .clone()
            .filter(|_| toggle_of(e).is_none() && amount_of(e).is_none()),
    };
    let mut items: Vec<HistoryItem> = lineage.iter().map(|e| item(e, true)).collect();
    // Undone steps redo would reapply, newest child first at each step.
    let mut cursor = h.head;
    while let Some(next) = h
        .entries
        .iter()
        .rev()
        .find(|e| e.parent == cursor)
        .filter(|_| items.len() <= h.entries.len())
    {
        items.push(item(next, false));
        cursor = Some(next.id);
    }
    Ok(items)
}

// ─────────────────────────── detail preview ───────────────────────────

/// Margin rendered around a 1:1 detail crop so neighbourhood operators see
/// real pixels (Detail support ≤ 9, Texture/Clarity ≤ 16).
const DETAIL_MARGIN: u32 = 32;

/// `image` restricted to a window of its active area, with its own identity
/// so memoized output-frame tiles never alias the full image's.
fn window_image(image: &RawImage, x: u32, y: u32, w: u32, h: u32) -> Result<RawImage> {
    let mut m = image.metadata().clone();
    let [left, top, _, _] = m.default_crop;
    m.default_crop = [left + x, top + y, w, h];
    let key = blake3::hash(
        &[
            &image.id().0.to_le_bytes()[..],
            &x.to_le_bytes(),
            &y.to_le_bytes(),
            &w.to_le_bytes(),
            &h.to_le_bytes(),
        ]
        .concat(),
    );
    let id = ImageId(u128::from_le_bytes(
        key.as_bytes()[..16].try_into().expect("16 bytes"),
    ));
    Ok(image.with_metadata(id, Arc::new(m))?)
}

impl DevelopSession {
    fn history_move(
        &self,
        f: impl FnOnce(&mut Recipe) -> engine_api::EngineResult<bool>,
    ) -> Result<bool> {
        let moved = {
            let mut st = self.shared.edit_lock()?;
            self.shared.commit_pending(&mut st, "Edit")?;
            let moved = f(&mut st.recipe)?;
            if moved {
                crate::backend::sync_process(&mut st.recipe)?;
                st.frame = None;
                st.live = st.recipe.settings.clone();
                self.shared.render(&mut st, false);
                self.shared.schedule_save();
            }
            moved
        };
        Ok(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edited_preview_key(
        photo: &Path,
        recipe: &Recipe,
        orientation: u8,
        size: u32,
    ) -> previews::PreviewKey {
        let mut hash = blake3::Hasher::new();
        hash.update(&std::fs::read(photo).unwrap());
        hash.update(&size.to_le_bytes());
        previews::PreviewKey {
            file_hash: *hash.finalize().as_bytes(),
            orientation,
            recipe_hash: recipe.recipe_hash().0.0,
        }
    }

    #[derive(Default)]
    struct SavedHashes(Mutex<Vec<String>>);

    impl DevelopListener for SavedHashes {
        fn frame_ready(&self, _: FrameInfo) {}
        fn render_failed(&self, _: String) {}
        fn saved(&self, recipe_hash: String) {
            self.0.lock().unwrap().push(recipe_hash);
        }
    }

    struct DropSessionOnSaveWorker {
        slot: Arc<Mutex<Option<Arc<DevelopSession>>>>,
        entered: std::sync::mpsc::Sender<()>,
        drop_ack: Mutex<std::sync::mpsc::Receiver<()>>,
        dropped: std::sync::mpsc::Sender<bool>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl DevelopListener for DropSessionOnSaveWorker {
        fn frame_ready(&self, _: FrameInfo) {}
        fn render_failed(&self, _: String) {}
        fn saved(&self, _: String) {
            let _ = self.entered.send(());
            let _ = self
                .drop_ack
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(Duration::from_secs(5));
            let final_session = self.slot.lock().unwrap().take();
            let dropped_on_writer = final_session.as_ref().is_some_and(|session| {
                Arc::strong_count(session) == 1
                    && std::thread::current().id() == session.writer_thread
            });
            drop(final_session);
            let _ = self.dropped.send(dropped_on_writer);
            let _ = self
                .release
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(Duration::from_secs(5));
        }
    }

    struct ReleaseWorkerCallback {
        ack: Option<std::sync::mpsc::Sender<()>>,
        release: Option<std::sync::mpsc::Sender<()>>,
    }

    impl ReleaseWorkerCallback {
        fn ack(&mut self) {
            if let Some(sender) = self.ack.take() {
                let _ = sender.send(());
            }
        }

        fn release(&mut self) {
            if let Some(sender) = self.release.take() {
                let _ = sender.send(());
            }
        }
    }

    impl Drop for ReleaseWorkerCallback {
        fn drop(&mut self) {
            self.ack();
            self.release();
        }
    }

    struct BlockingSaved {
        entered: std::sync::mpsc::Sender<()>,
        release: Mutex<std::sync::mpsc::Receiver<()>>,
    }

    impl DevelopListener for BlockingSaved {
        fn frame_ready(&self, _: FrameInfo) {}
        fn render_failed(&self, _: String) {}
        fn saved(&self, _: String) {
            let _ = self.entered.send(());
            let _ = self
                .release
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .recv_timeout(Duration::from_secs(5));
        }
    }

    struct ReleasePause(PostRecipePause);
    impl Drop for ReleasePause {
        fn drop(&mut self) {
            let (state, cv) = &*self.0;
            state.lock().unwrap_or_else(|e| e.into_inner()).1 = true;
            cv.notify_all();
        }
    }

    fn tiny_develop_session(
        name: &str,
    ) -> (
        tempfile::TempDir,
        PathBuf,
        Arc<Engine>,
        String,
        Arc<DevelopSession>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join(name);
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let mut rows = engine.list_images(crate::ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 1, "fixture must index only its photo");
        let row = rows.remove(0);
        let session = engine.clone().open_develop_session(row.id.clone()).unwrap();
        (dir, photo, engine, row.id, session)
    }

    #[test]
    fn open_develop_editor_does_not_overwrite_foreign_disk_settings() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("owner-conflict.jpg");
        let mut newer: Recipe =
            serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
        newer
            .edit(EditMeta::user("Newer edit", now_ms()), |settings| {
                settings.tone.exposure = 1.2;
            })
            .unwrap();
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let mut foreign = catalog::document(&photo, parse_id(&id).unwrap()).unwrap();
        foreign.recipe = newer;
        sidecar::Sidecar::write_recipe(&recipe_path, &foreign).unwrap();
        let packet = catalog::selection_packet(&photo, &foreign)
            .unwrap()
            .with_recipe(&foreign.recipe)
            .unwrap();
        sidecar::Sidecar::write_xmp(catalog::xmp_path(&photo), &packet).unwrap();
        let published_bytes = std::fs::read(&recipe_path).unwrap();
        let xmp_path = catalog::xmp_path(&photo);
        let published_xmp = std::fs::read(&xmp_path).unwrap();

        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        let error = session.flush().expect_err("stale editor must not publish");
        assert!(error.to_string().contains("conflict"), "{error}");
        assert_eq!(std::fs::read(&recipe_path).unwrap(), published_bytes);
        assert_eq!(std::fs::read(&xmp_path).unwrap(), published_xmp);
        assert!(!session.shared.lock().unwrap().closed);
    }

    #[test]
    fn selection_during_develop_does_not_conflict_with_edited_settings() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("owner-selection.jpg");
        engine
            .set_selection(
                id.clone(),
                crate::Selection {
                    decision: crate::Decision::Keep,
                    grade: Some(2),
                    mark: None,
                },
            )
            .unwrap();
        session
            .set_settings(r#"{"tone":{"exposure":0.4}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        let saved = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe).unwrap();
        assert_eq!(saved.recipe.settings.tone.exposure, 0.4);
        assert_eq!(
            saved.recipe.selection.grade,
            Some(engine_api::recipe::Grade::Two)
        );
    }

    #[test]
    fn legacy_rgb_disk_baseline_does_not_conflict_on_first_develop_save() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("legacy.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let rows = engine.list_images(crate::ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        let id = rows[0].id.clone();
        let mut legacy = catalog::document(&photo, parse_id(&id).unwrap()).unwrap();
        legacy.recipe.source_kind = engine_api::recipe::SourceKind::Raw;
        sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(&photo).recipe, &legacy).unwrap();
        let session = engine.clone().open_develop_session(id).unwrap();
        session
            .set_settings(r#"{"tone":{"exposure":0.3}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        let saved = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe).unwrap();
        assert_eq!(
            saved.recipe.source_kind,
            engine_api::recipe::SourceKind::Rgb
        );
        assert_eq!(saved.recipe.settings.tone.exposure, 0.3);
    }

    #[test]
    fn foreign_unknown_nested_develop_field_cannot_be_erased_by_open_editor() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("owner-unknown.jpg");
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let doc = catalog::document(&photo, parse_id(&id).unwrap()).unwrap();
        sidecar::Sidecar::write_recipe(&recipe_path, &doc).unwrap();
        // Reopen after the canonical sidecar exists, so this tests a nested
        // change rather than merely detecting absent -> present sidecar.
        session.close().unwrap();
        let editor = engine.clone().open_develop_session(id).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&recipe_path).unwrap()).unwrap();
        value["recipe"]["settings"]["tone"]["future_curve"] = serde_json::json!({"points": [1, 2]});
        std::fs::write(&recipe_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let foreign_bytes = std::fs::read(&recipe_path).unwrap();

        editor
            .set_settings(r#"{"tone":{"exposure":0.5}}"#.into(), false)
            .unwrap();
        let error = editor
            .flush()
            .expect_err("unknown owner field must be preserved");
        assert!(error.to_string().contains("conflict"), "{error}");
        assert_eq!(std::fs::read(&recipe_path).unwrap(), foreign_bytes);
    }

    #[test]
    fn preexisting_unknown_nested_develop_field_is_not_silently_dropped() {
        let (_dir, photo, engine, id, original) = tiny_develop_session("owner-unknown-open.jpg");
        original.close().unwrap();
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let doc = catalog::document(&photo, parse_id(&id).unwrap()).unwrap();
        sidecar::Sidecar::write_recipe(&recipe_path, &doc).unwrap();
        let mut value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&recipe_path).unwrap()).unwrap();
        value["recipe"]["settings"]["tone"]["future_curve"] = serde_json::json!("preserve");
        std::fs::write(&recipe_path, serde_json::to_vec(&value).unwrap()).unwrap();
        let original_bytes = std::fs::read(&recipe_path).unwrap();
        let editor = engine.clone().open_develop_session(id).unwrap();

        editor
            .set_settings(r#"{"tone":{"exposure":0.6}}"#.into(), false)
            .unwrap();
        let error = editor
            .flush()
            .expect_err("unrepresented owner field must fail closed");
        assert!(error.to_string().contains("conflict"), "{error}");
        assert_eq!(std::fs::read(&recipe_path).unwrap(), original_bytes);
    }

    #[test]
    fn second_develop_editor_cannot_replace_first_edit() {
        let (dir, photo, first_engine, id, first) = tiny_develop_session("two-editors.jpg");
        let second_engine =
            Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        let writer_count = second_engine
            .develop_writer_constructions
            .load(Ordering::Relaxed);
        let second = match second_engine.clone().open_develop_session(id.clone()) {
            Ok(session) => {
                session.close().unwrap();
                panic!("same-destination editor admission must conflict");
            }
            Err(error) => error,
        };
        assert_eq!(
            second.to_string(),
            "conflict: Develop destination already has an active editor"
        );
        assert_eq!(
            second_engine
                .develop_writer_constructions
                .load(Ordering::Relaxed),
            writer_count,
            "rejected admission must not construct a writer"
        );
        first
            .set_settings(r#"{"tone":{"exposure":0.8}}"#.into(), false)
            .unwrap();
        first.flush().unwrap();
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let first_bytes = std::fs::read(&recipe_path).unwrap();
        let first_id = first.shared.image_id.clone();
        let stale_first = first.clone();
        first.close().unwrap();
        let reopened = second_engine.clone().open_develop_session(id).unwrap();
        stale_first.close().unwrap();
        stale_first.close().unwrap();
        assert_eq!(
            serde_json::from_str::<Recipe>(&first_engine.get_recipe(first_id.clone()).unwrap())
                .unwrap()
                .settings
                .tone
                .exposure,
            0.8
        );
        assert_eq!(std::fs::read(&recipe_path).unwrap(), first_bytes);
        drop(stale_first);
        drop(first);
        let still_owned = match second_engine.clone().open_develop_session(first_id) {
            Ok(opened) => {
                opened.close().unwrap();
                panic!("stale closed editor release cleared the current owner's lease");
            }
            Err(error) => error,
        };
        assert_eq!(
            still_owned.to_string(),
            "conflict: Develop destination already has an active editor"
        );
        reopened.close().unwrap();
    }

    #[test]
    fn final_session_drop_on_writer_keeps_lease_until_writer_exit() {
        let (dir, _photo, _engine, id, session) = tiny_develop_session("worker-final-arc.jpg");
        let second_engine =
            Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        let weak = Arc::downgrade(&session);
        let held_shared = session.shared.clone();
        let slot = Arc::new(Mutex::new(Some(session.clone())));
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (drop_ack_tx, drop_ack_rx) = std::sync::mpsc::channel();
        let (dropped_tx, dropped_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut release_callback = ReleaseWorkerCallback {
            ack: Some(drop_ack_tx),
            release: Some(release_tx),
        };
        let (worker_exit_tx, worker_exit_rx) = std::sync::mpsc::channel();
        *session.shared.worker_exit_observer.lock().unwrap() = Some(worker_exit_tx);
        session.set_listener(Some(Arc::new(DropSessionOnSaveWorker {
            slot: slot.clone(),
            entered: entered_tx,
            drop_ack: Mutex::new(drop_ack_rx),
            dropped: dropped_tx,
            release: Mutex::new(release_rx),
        })));
        session.shared.schedule_save();
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(session);
        release_callback.ack();
        assert!(
            dropped_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "the final Arc must be destroyed by the save worker"
        );
        assert!(
            weak.upgrade().is_none(),
            "callback must drop final session Arc on writer thread"
        );

        let contender_engine = second_engine.clone();
        let contender_id = id.clone();
        let (contender_tx, contender_rx) = std::sync::mpsc::channel();
        let contender = thread::spawn(move || {
            let outcome = match contender_engine.open_develop_session(contender_id) {
                Ok(opened) => {
                    opened.close().unwrap();
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            };
            contender_tx.send(outcome).unwrap();
        });
        let conflict = contender_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        contender.join().unwrap();
        assert_eq!(
            conflict,
            "conflict: Develop destination already has an active editor"
        );
        release_callback.release();
        worker_exit_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let reopened = second_engine.open_develop_session(id).unwrap();
        reopened.close().unwrap();
        drop(held_shared);
    }

    #[test]
    fn admission_remains_exclusive_while_close_waits_after_save() {
        let (dir, _photo, _engine, id, session) = tiny_develop_session("close-in-flight-lease.jpg");
        let second_engine =
            Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let mut release_callback = ReleaseWorkerCallback {
            ack: None,
            release: Some(release_tx),
        };
        session.set_listener(Some(Arc::new(BlockingSaved {
            entered: entered_tx,
            release: Mutex::new(release_rx),
        })));
        session
            .set_settings(r#"{"tone":{"exposure":0.55}}"#.into(), false)
            .unwrap();
        let closing = session.clone();
        let (close_tx, close_rx) = std::sync::mpsc::channel();
        let close_thread = thread::spawn(move || close_tx.send(closing.close()).unwrap());
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let contender_engine = second_engine.clone();
        let contender_id = id.clone();
        let (contender_tx, contender_rx) = std::sync::mpsc::channel();
        let contender = thread::spawn(move || {
            let outcome = match contender_engine.open_develop_session(contender_id) {
                Ok(opened) => {
                    opened.close().unwrap();
                    Ok(())
                }
                Err(error) => Err(error.to_string()),
            };
            let _ = contender_tx.send(outcome);
        });
        let admission_error = contender_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        contender.join().unwrap();
        assert_eq!(
            admission_error,
            "conflict: Develop destination already has an active editor"
        );
        release_callback.release();
        close_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        close_thread.join().unwrap();
        let reopened = second_engine.open_develop_session(id).unwrap();
        reopened.close().unwrap();
    }

    fn optional_file_bytes(path: &Path) -> Option<Vec<u8>> {
        match std::fs::read(path) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("failed to read {}: {error}", path.display()),
        }
    }

    #[test]
    fn direct_recipe_replacement_conflicts_without_publishing_during_develop() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("setter-conflict.jpg");
        engine
            .set_selection(
                id.clone(),
                crate::Selection {
                    decision: crate::Decision::Keep,
                    grade: Some(2),
                    mark: None,
                },
            )
            .unwrap();
        let mut replacement: Recipe =
            serde_json::from_str(&engine.get_recipe(id.clone()).unwrap()).unwrap();
        replacement
            .edit(EditMeta::user("External edit", now_ms()), |settings| {
                settings.tone.exposure = 1.1;
            })
            .unwrap();
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let xmp_path = catalog::xmp_path(&photo);
        let recipe_before = optional_file_bytes(&recipe_path);
        let xmp_before = optional_file_bytes(&xmp_path);
        let indexed_hash_before = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .into_iter()
            .find(|row| row.id == id)
            .unwrap()
            .recipe_hash;

        let result = engine.set_recipe_json(
            id.clone(),
            String::from_utf8(replacement.to_json().unwrap()).unwrap(),
        );
        assert!(
            result.is_err(),
            "active Develop must reject direct recipe replacement"
        );
        let error = result.unwrap_err();
        assert_eq!(
            error.to_string(),
            "conflict: Develop destination already has an active editor"
        );
        assert_eq!(optional_file_bytes(&recipe_path), recipe_before);
        assert_eq!(optional_file_bytes(&xmp_path), xmp_before);
        assert_eq!(
            engine
                .list_images(crate::ImageQuery::default())
                .unwrap()
                .into_iter()
                .find(|row| row.id == id)
                .unwrap()
                .recipe_hash,
            indexed_hash_before,
            "rejected setter must not publish index state"
        );
        session.close().unwrap();
        engine
            .set_recipe_json(
                id,
                String::from_utf8(replacement.to_json().unwrap()).unwrap(),
            )
            .unwrap();
    }

    #[test]
    fn failed_editor_snapshot_and_raw_decode_release_reservation() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("failed-open-release.jpg");
        session
            .set_settings(r#"{"tone":{"exposure":0.4}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        session.close().unwrap();
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let good_recipe = std::fs::read(&recipe_path).unwrap();
        std::fs::write(&recipe_path, b"{ malformed").unwrap();
        let writer_count_before_snapshot =
            engine.develop_writer_constructions.load(Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        let snapshot_engine = engine.clone();
        let snapshot_id = id.clone();
        let snapshot_thread = thread::spawn(move || {
            tx.send(
                snapshot_engine
                    .open_develop_session(snapshot_id)
                    .map(|_| ()),
            )
            .unwrap();
        });
        let snapshot_error = rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap_err();
        assert!(
            snapshot_error.to_string().contains("key must be a string"),
            "unexpected snapshot error: {snapshot_error}"
        );
        assert_eq!(
            engine.develop_writer_constructions.load(Ordering::Relaxed),
            writer_count_before_snapshot
        );
        snapshot_thread.join().unwrap();
        std::fs::write(&recipe_path, &good_recipe).unwrap();
        let snapshot_reopen = engine.clone().open_develop_session(id.clone()).unwrap();
        snapshot_reopen.close().unwrap();

        let image_bytes = std::fs::read(&photo).unwrap();
        std::fs::remove_file(&photo).unwrap();
        let writer_count = engine.develop_writer_constructions.load(Ordering::Relaxed);
        let decode_error = match engine.clone().open_develop_session(id.clone()) {
            Ok(opened) => {
                opened.close().unwrap();
                panic!("missing image unexpectedly decoded");
            }
            Err(error) => error,
        };
        assert!(
            !decode_error.to_string().contains("active editor"),
            "decode failure was masked by stale lease: {decode_error}"
        );
        assert!(
            decode_error
                .to_string()
                .contains(&photo.display().to_string()),
            "missing-image I/O failure did not identify fixture: {decode_error}"
        );
        assert_eq!(
            engine.develop_writer_constructions.load(Ordering::Relaxed),
            writer_count
        );
        std::fs::write(&photo, image_bytes).unwrap();
        let reopened = engine.clone().open_develop_session(id).unwrap();
        reopened.close().unwrap();
    }

    #[test]
    fn post_recipe_failure_advances_owner_baseline_for_newer_local_edit() {
        let (_dir, photo, engine, id, session) = tiny_develop_session("owner-retry-newer.jpg");
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        assert!(session.flush().is_err());
        assert_eq!(
            serde_json::from_str::<Recipe>(&engine.get_recipe(id).unwrap())
                .unwrap()
                .settings
                .tone
                .exposure,
            0.7
        );
        session
            .set_settings(r#"{"tone":{"exposure":0.8}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        assert_eq!(
            sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe)
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.8
        );
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.8
        );
    }

    #[test]
    fn develop_save_waits_for_destination_without_holding_catalog_mutex() {
        use std::{sync::mpsc, thread};

        let (_dir, photo, engine, _id, session) = tiny_develop_session("owner-gated.jpg");
        let other = photo.with_file_name("other.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([30, 60, 90]))
            .save(&other)
            .unwrap();
        engine
            .index_folder(other.parent().unwrap().to_string_lossy().into_owned())
            .unwrap();
        let rows = engine.list_images(crate::ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 2, "both photos must be indexed");
        let other_id = rows
            .into_iter()
            .find(|row| row.path.ends_with("/other.jpg"))
            .unwrap()
            .id;
        let gate = crate::recipe_write::gate_for(&photo).unwrap();
        let held = gate.begin_read().unwrap();
        let contended = gate.observe_next_contended_gate_attempt();
        session
            .set_settings(r#"{"tone":{"exposure":0.4}}"#.into(), false)
            .unwrap();
        let flushing = session.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let flush_thread = thread::spawn(move || done_tx.send(flushing.flush()).unwrap());
        contended.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(done_rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let unrelated_engine = engine.clone();
        let (other_tx, other_rx) = mpsc::channel();
        let other_thread = thread::spawn(move || {
            other_tx
                .send(unrelated_engine.set_selection(
                    other_id,
                    crate::Selection {
                        decision: crate::Decision::Keep,
                        grade: Some(1),
                        mark: None,
                    },
                ))
                .unwrap();
        });
        other_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        other_thread.join().unwrap();
        drop(held);
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        flush_thread.join().unwrap();
    }

    #[test]
    fn failed_close_repairs_without_a_new_edit_and_only_then_drains_worker() {
        let (dir, photo, engine, id, session) = tiny_develop_session("close-repair.jpg");
        let second_engine =
            Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();

        let first = session.close().unwrap_err();
        assert!(first.to_string().contains("injected post-recipe failure"));
        assert!(!session.shared.lock().unwrap().closed);
        assert!(session.writer.lock().unwrap().is_some());
        let rejected = match second_engine.clone().open_develop_session(id.clone()) {
            Ok(opened) => {
                opened.close().unwrap();
                panic!("lease released before repaired close");
            }
            Err(error) => error,
        };
        assert_eq!(
            rejected.to_string(),
            "conflict: Develop destination already has an active editor"
        );
        let committed = std::fs::read(sidecar::Sidecar::paths(&photo).recipe).unwrap();
        assert_eq!(
            sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe)
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );

        session.close().unwrap();
        let reopened = second_engine.open_develop_session(id.clone()).unwrap();
        reopened.close().unwrap();
        assert_eq!(
            std::fs::read(sidecar::Sidecar::paths(&photo).recipe).unwrap(),
            committed
        );
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );
        assert_eq!(
            engine
                .list_images(crate::ImageQuery::default())
                .unwrap()
                .into_iter()
                .find(|row| row.id == id)
                .unwrap()
                .recipe_hash,
            sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe)
                .unwrap()
                .recipe
                .recipe_hash()
                .to_string()
        );
        assert!(session.writer.lock().unwrap().is_none());
        session.close().unwrap();
    }

    #[test]
    fn failed_close_retains_live_session_for_a_later_edit() {
        let (_dir, photo, _engine, _id, session) = tiny_develop_session("close-edit.jpg");
        let retained = session.clone();
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        assert!(session.close().is_err());
        assert!(!session.shared.lock().unwrap().closed);

        retained
            .set_settings(r#"{"tone":{"exposure":1.2}}"#.into(), false)
            .unwrap();
        assert!(retained.commit("After failed close".into()).unwrap());
        retained.close().unwrap();
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            1.2
        );
    }

    #[test]
    fn successful_close_rejects_retained_arc_mutations_and_flush() {
        let (_dir, photo, _engine, _id, session) = tiny_develop_session("close-closed.jpg");
        let retained = session.clone();
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        session.close().unwrap();
        assert!(session.shared.lock().unwrap().closed);
        assert!(session.writer.lock().unwrap().is_none());
        assert!(
            retained
                .set_settings(r#"{"tone":{"exposure":1.2}}"#.into(), false)
                .is_err()
        );
        assert!(retained.commit("After close".into()).is_err());
        assert!(retained.undo().is_err());
        assert!(retained.flush().is_err());
        assert_eq!(
            sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&photo).recipe)
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );
        let save = session.shared.save.lock().unwrap();
        assert!(save.due.is_none() && !save.busy && save.retry.is_none());
    }

    #[test]
    fn concurrent_closes_share_one_failed_attempt_and_later_retry() {
        let (_dir, photo, _engine, _id, session) = tiny_develop_session("close-two.jpg");
        let path = session.shared.path.clone();
        let pause: PostRecipePause = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let _release = ReleasePause(pause.clone());
        PAUSE_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(path.clone(), pause.clone());
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        let first = {
            let session = session.clone();
            std::thread::spawn(move || session.close())
        };
        let (pause_state, pause_cv) = &*pause;
        let mut state = pause_state.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.0 {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("first close did not reach the save boundary");
            state = pause_cv.wait_timeout(state, remaining).unwrap().0;
        }
        drop(state);
        let attempt = match &*session.close_phase.lock().unwrap() {
            ClosePhase::Closing(attempt) => attempt.clone(),
            _ => panic!("first close was not in flight"),
        };
        let second = {
            let session = session.clone();
            std::thread::spawn(move || session.close())
        };
        let mut attempt_state = attempt.state.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while attempt_state.waiters != 1 {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("second close did not join first attempt");
            attempt_state = attempt.cv.wait_timeout(attempt_state, remaining).unwrap().0;
        }
        drop(attempt_state);
        let before = session.get_settings_json().unwrap();
        assert!(session.shared.save.lock().unwrap().due.is_none());
        assert!(
            session
                .set_settings(r#"{"tone":{"exposure":1.2}}"#.into(), false)
                .is_err()
        );
        assert!(session.commit("Closing edit".into()).is_err());
        assert!(session.flush().is_err());
        assert_eq!(session.get_settings_json().unwrap(), before);
        assert!(session.shared.save.lock().unwrap().due.is_none());
        let mut state = pause_state.lock().unwrap();
        state.1 = true;
        pause_cv.notify_all();
        drop(state);
        PAUSE_AFTER_DEVELOP_RECIPE.lock().unwrap().remove(&path);
        let first_error = first.join().unwrap().unwrap_err().to_string();
        let second_error = second.join().unwrap().unwrap_err().to_string();
        assert_eq!(first_error, second_error);
        assert!(first_error.contains("injected post-recipe failure"));
        assert!(!session.shared.lock().unwrap().closed);
        session.close().unwrap();
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );
    }

    struct ReentrantSaved {
        session: std::sync::Weak<DevelopSession>,
        results: Mutex<Vec<String>>,
    }

    impl DevelopListener for ReentrantSaved {
        fn frame_ready(&self, _: FrameInfo) {}
        fn render_failed(&self, _: String) {}
        fn saved(&self, _: String) {
            let session = self.session.upgrade().expect("listener session retained");
            let mut results = self.results.lock().unwrap();
            results.push(session.close().unwrap_err().to_string());
            results.push(session.flush().unwrap_err().to_string());
        }
    }

    #[test]
    fn save_listener_cannot_self_join_or_self_flush() {
        let (_dir, _photo, _engine, _id, session) = tiny_develop_session("close-callback.jpg");
        let listener = Arc::new(ReentrantSaved {
            session: Arc::downgrade(&session),
            results: Mutex::new(Vec::new()),
        });
        session.set_listener(Some(listener.clone()));
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        let results = listener.results.lock().unwrap();
        assert_eq!(results.len(), 2);
        assert!(results[0].contains("cannot close itself"));
        assert!(results[1].contains("cannot flush itself"));
        drop(results);
        session.close().unwrap();
    }

    #[test]
    fn temporary_depth_histogram_session_coexists_with_editor_and_closed_editor() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([120, 80, 40]))
            .save(photos.join("depth.jpg"))
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let mut rows = engine.list_images(crate::ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        let id = rows.remove(0).id;
        let editor = engine.clone().open_develop_session(id.clone()).unwrap();
        let writer_count = engine.develop_writer_constructions.load(Ordering::Relaxed);
        let input = editor.shared.depth_input().unwrap();
        let rgb = image_core::depth::model_input(&input).unwrap();
        let depth = image_core::ml_depth::DepthMap::from_prediction(
            input.width(),
            input.height(),
            vec![0.; input.width() as usize * input.height() as usize],
        )
        .unwrap();
        let store = image_core::ml_depth::DepthStore::new(
            engine.db.parent().unwrap().join("previews/depth-cache"),
            1 << 20,
        )
        .unwrap();
        depth
            .store(
                &store,
                &image_core::ml_depth::cache_key(&rgb, image_core::ml_depth::MODEL_VERSION),
            )
            .unwrap();

        let while_open = engine.clone().depth_histogram(id.clone()).unwrap();
        assert_eq!(while_open.len(), 256);
        assert_eq!(
            engine.develop_writer_constructions.load(Ordering::Relaxed),
            writer_count
        );
        let second_open = match engine.clone().open_develop_session(id.clone()) {
            Ok(opened) => {
                opened.close().unwrap();
                panic!("read-only histogram released the active editor lease");
            }
            Err(error) => error,
        };
        assert_eq!(
            second_open.to_string(),
            "conflict: Develop destination already has an active editor"
        );
        assert!(!editor.shared.lock().unwrap().closed);
        editor.close().unwrap();
        let after_close = engine.depth_histogram(id).unwrap();
        assert_eq!(after_close, while_open);
    }

    #[test]
    fn flush_without_new_edit_repairs_sidecars_after_recipe_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("retry.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id.clone()).unwrap();
        let saved = Arc::new(SavedHashes::default());
        session.set_listener(Some(saved.clone()));
        engine
            .request_raw(
                row.id.clone(),
                row.path.clone(),
                16,
                row.recipe_hash.clone(),
            )
            .unwrap();
        assert_eq!(engine.requested_preview_sizes(&row.id), vec![16]);
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        let first = session.flush().unwrap_err();
        assert!(first.to_string().contains("injected post-recipe failure"));
        assert!(saved.0.lock().unwrap().is_empty());
        {
            let mut state = session.shared.lock().unwrap();
            state.frame = Some(Arc::new(Frame {
                level: 0,
                settings: session_renderable(&state.live, true, false),
                tiles: None,
            }));
        }
        let recipe: Recipe =
            serde_json::from_str(&engine.get_recipe(row.id.clone()).unwrap()).unwrap();
        assert_eq!(recipe.settings.tone.exposure, 0.7);
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let committed_bytes = std::fs::read(&recipe_path).unwrap();
        let preview_key = edited_preview_key(
            &photo,
            &recipe,
            session.shared.image.metadata().orientation as u8,
            16,
        );
        assert!(
            engine
                .previews
                .get(&preview_key, previews::Level::Full)
                .is_none()
        );

        session.flush().unwrap();
        assert_eq!(std::fs::read(&recipe_path).unwrap(), committed_bytes);
        let xmp = sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo)).unwrap();
        assert_eq!(xmp.to_recipe().unwrap().recipe.settings.tone.exposure, 0.7);
        assert_eq!(
            engine
                .list_images(crate::ImageQuery::default())
                .unwrap()
                .into_iter()
                .find(|image| image.id == row.id)
                .unwrap()
                .recipe_hash,
            recipe.recipe_hash().to_string()
        );
        assert!(
            engine
                .previews
                .get(&preview_key, previews::Level::Full)
                .is_some()
        );
        assert_eq!(
            *saved.0.lock().unwrap(),
            vec![recipe.recipe_hash().to_string()]
        );
    }

    #[test]
    fn failed_develop_sidecar_repair_waits_for_each_explicit_flush() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("repeat.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id).unwrap();
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.shared.path.clone(), 2);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        assert!(session.flush().is_err());
        assert!(session.flush().is_err());
        session.flush().unwrap();
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );
    }

    #[test]
    fn repair_uses_current_disk_recipe_after_foreign_develop_edit() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("foreign.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id.clone()).unwrap();
        let saved = Arc::new(SavedHashes::default());
        session.set_listener(Some(saved.clone()));
        engine
            .request_raw(
                row.id.clone(),
                row.path.clone(),
                16,
                row.recipe_hash.clone(),
            )
            .unwrap();
        assert_eq!(engine.requested_preview_sizes(&row.id), vec![16]);
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        assert!(session.flush().is_err());
        assert!(saved.0.lock().unwrap().is_empty());
        {
            let mut state = session.shared.lock().unwrap();
            state.frame = Some(Arc::new(Frame {
                level: 0,
                settings: session_renderable(&state.live, true, false),
                tiles: None,
            }));
        }

        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        let mut foreign = sidecar::Sidecar::read_recipe(&recipe_path).unwrap();
        foreign
            .recipe
            .edit(EditMeta::default(), |s| s.tone.exposure = 1.2)
            .unwrap();
        sidecar::Sidecar::write_recipe(&recipe_path, &foreign).unwrap();
        let foreign_disk = sidecar::Sidecar::read_recipe(&recipe_path).unwrap().recipe;
        assert_eq!(foreign_disk.settings.tone.exposure, 1.2);
        let foreign_bytes = std::fs::read(&recipe_path).unwrap();

        session.flush().unwrap();
        assert_eq!(std::fs::read(&recipe_path).unwrap(), foreign_bytes);
        let xmp = sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo)).unwrap();
        assert_eq!(xmp.to_recipe().unwrap().recipe.settings.tone.exposure, 1.2);
        assert_eq!(
            engine
                .list_images(crate::ImageQuery::default())
                .unwrap()
                .into_iter()
                .find(|image| image.id == row.id)
                .unwrap()
                .recipe_hash,
            foreign_disk.recipe_hash().to_string()
        );
        assert!(saved.0.lock().unwrap().is_empty());
        let foreign_preview = edited_preview_key(
            &photo,
            &foreign_disk,
            session.shared.image.metadata().orientation as u8,
            16,
        );
        assert!(
            engine
                .previews
                .get(&foreign_preview, previews::Level::Full)
                .is_none()
        );
    }

    #[test]
    fn newer_develop_edit_does_not_hide_previous_save_failure() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("newer.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id.clone()).unwrap();
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session.shared.path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        assert!(session.commit("First".into()).unwrap());
        let mut save = session.shared.save.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while save.retry.is_none() || save.busy || save.due.is_some() {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("first save did not fail");
            save = session
                .shared
                .save_cv
                .wait_timeout(save, remaining)
                .unwrap()
                .0;
        }
        drop(save);

        session
            .set_settings(r#"{"tone":{"exposure":1.2}}"#.into(), false)
            .unwrap();
        assert!(session.commit("Second".into()).unwrap());
        let first_error = session.flush().unwrap_err();
        assert!(
            first_error
                .to_string()
                .contains("injected post-recipe failure")
        );
        let saved: Recipe = serde_json::from_str(&engine.get_recipe(row.id).unwrap()).unwrap();
        assert_eq!(saved.settings.tone.exposure, 1.2);
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            1.2
        );
        session.flush().unwrap();
    }

    #[test]
    fn concurrent_flushes_both_report_the_same_failed_save() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("two-waiters.jpg");
        image::RgbImage::from_pixel(2, 2, image::Rgb([120, 80, 40]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id).unwrap();
        let path = session.shared.path.clone();
        let pause: PostRecipePause = Arc::new((Mutex::new((false, false)), Condvar::new()));
        let _release = ReleasePause(pause.clone());
        PAUSE_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(path.clone(), pause.clone());
        FAIL_AFTER_DEVELOP_RECIPE
            .lock()
            .unwrap()
            .insert(path.clone(), 1);
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        let first = {
            let session = session.clone();
            std::thread::spawn(move || session.flush())
        };
        let (pause_state, pause_cv) = &*pause;
        let mut state = pause_state.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !state.0 {
            if first.is_finished() {
                drop(state);
                panic!(
                    "first flush exited before post-recipe boundary: {:?}",
                    first.join()
                );
            }
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) => remaining,
                None => {
                    drop(state);
                    panic!(
                        "save did not reach post-recipe boundary while first flush remained active"
                    );
                }
            };
            state = pause_cv
                .wait_timeout(state, remaining.min(Duration::from_millis(100)))
                .unwrap()
                .0;
        }
        drop(state);
        let second = {
            let session = session.clone();
            std::thread::spawn(move || session.flush())
        };
        let mut save = session.shared.save.lock().unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while save.flush_waiters != 2 {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("both flush calls did not wait on the save");
            save = session
                .shared
                .save_cv
                .wait_timeout(save, remaining)
                .unwrap()
                .0;
        }
        drop(save);
        let mut state = pause_state.lock().unwrap();
        state.1 = true;
        pause_cv.notify_all();
        drop(state);
        PAUSE_AFTER_DEVELOP_RECIPE.lock().unwrap().remove(&path);
        for result in [first.join().unwrap(), second.join().unwrap()] {
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("injected post-recipe failure")
            );
        }
        session.flush().unwrap();
        assert_eq!(
            sidecar::Sidecar::read_xmp(catalog::xmp_path(&photo))
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
                .tone
                .exposure,
            0.7
        );
    }

    #[test]
    fn jpeg_develop_session_renders_nonblack() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
            .save(dir.path().join("one.jpg"))
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(dir.path().to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.clone().open_develop_session(row.id.clone()).unwrap();
        let source = &session.shared.image;
        let tiles = session
            .shared
            .renderer
            .render_region(
                source,
                &DevelopSettings::default(),
                0,
                PixelRect::full(source.active_extent()),
            )
            .unwrap();
        assert!(
            tiles
                .iter()
                .any(|t| t.samples::<u8>().unwrap().iter().any(|v| *v > 30))
        );
        session
            .set_settings(r#"{"tone":{"exposure":0.7}}"#.into(), false)
            .unwrap();
        session.flush().unwrap();
        let saved: Recipe = serde_json::from_str(&engine.get_recipe(row.id).unwrap()).unwrap();
        assert_eq!(saved.source_kind, engine_api::recipe::SourceKind::Rgb);
        assert_eq!(saved.settings.tone.exposure, 0.7);
    }

    #[test]
    fn unconfigured_session_uses_automatic_noise_without_discarding_saved_denoise() {
        let mut settings = DevelopSettings::default();
        settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
            model: engine_api::id::ModelRef {
                id: "enhance/cfa-unet-fp32".into(),
                version: "a".repeat(64),
            },
            joint_demosaic: false,
        };
        assert!(pipeline_cpu::denoise_active(
            &session_renderable(&settings, true, false).denoise
        ));
        assert_eq!(
            session_renderable(&settings, true, true).denoise,
            settings.denoise
        );
        assert!(pipeline_cpu::denoise_active(&settings.denoise));
        // Shared helpers such as mask guidance use an uninjected base renderer.
        assert!(!pipeline_cpu::denoise_active(
            &renderable(&settings).denoise
        ));
    }

    #[test]
    fn new_denoise_edit_pins_fallback_but_replay_never_reselects() {
        use engine_api::recipe::settings::DenoiseMethod;
        let support = tempfile::tempdir().unwrap();
        let registry = ml_runtime::ModelRegistry::from_support(support.path()).unwrap();
        let mut next = DevelopSettings::default();
        let cfa = registry
            .models()
            .iter()
            .find(|m| m.id == "enhance/cfa-unet-fp32")
            .unwrap();
        next.denoise.method = DenoiseMethod::Neural {
            model: engine_api::id::ModelRef {
                id: cfa.id.as_str().into(),
                version: cfa.version.clone(),
            },
            joint_demosaic: false,
        };
        let saved_cfa = next.clone();
        pin_new_denoise_edit(&registry, &DevelopSettings::default(), &mut next).unwrap();
        let DenoiseMethod::Neural { model, .. } = &next.denoise.method else {
            panic!()
        };
        assert_eq!(model.id.as_str(), "enhance/drunet-color");
        let json = serde_json::to_string(&next).unwrap();
        let mut replay: DevelopSettings = serde_json::from_str(&json).unwrap();
        pin_new_denoise_edit(&registry, &next, &mut replay).unwrap();
        assert_eq!(next, replay);
        let mut replay_cfa = saved_cfa.clone();
        pin_new_denoise_edit(&registry, &saved_cfa, &mut replay_cfa).unwrap();
        assert_eq!(saved_cfa, replay_cfa);
        let mut recipe = engine_api::recipe::Recipe::new(engine_api::id::ImageId(52));
        recipe
            .edit(EditMeta::user("AI Denoise On", 1), |s| *s = next.clone())
            .unwrap();
        let path = support.path().join("recipe.json");
        std::fs::write(&path, serde_json::to_vec(&recipe).unwrap()).unwrap();
        let mut loaded: engine_api::recipe::Recipe =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(loaded.settings, next);
        assert!(loaded.undo().unwrap());
        assert!(matches!(loaded.settings.denoise.method, DenoiseMethod::Off));
        assert!(loaded.redo().unwrap());
        assert_eq!(loaded.settings, next);
    }

    #[test]
    fn renderable_preserves_pinned_cfa_amount_for_configured_session() {
        use engine_api::{id::ModelRef, recipe::settings::DenoiseMethod};
        let mut settings = DevelopSettings::default();
        settings.denoise.method = DenoiseMethod::Neural {
            model: ModelRef {
                id: "enhance/cfa-unet-fp32".into(),
                version: "a".repeat(64),
            },
            joint_demosaic: false,
        };
        settings.denoise.amount = 37.0;
        assert_eq!(
            session_renderable(&settings, true, true).denoise,
            settings.denoise
        );
        settings.denoise.amount = f32::NAN;
        assert_eq!(
            session_renderable(&settings, true, true).denoise.method,
            DenoiseMethod::Off
        );
        settings.denoise.amount = 100.0;
        if let DenoiseMethod::Neural { joint_demosaic, .. } = &mut settings.denoise.method {
            *joint_demosaic = true;
        }
        assert_eq!(
            session_renderable(&settings, true, true).denoise.method,
            DenoiseMethod::Off
        );
    }

    #[test]
    fn surface_frame_materializes_the_captured_recipe_only_on_demand() {
        use std::cell::Cell;
        let calls = Cell::new(0);
        let mut live = DevelopSettings::default();
        live.tone.exposure = 0.25;
        let frame = Frame {
            level: 2,
            settings: live.clone(),
            tiles: None,
        };
        live.tone.exposure = 1.0;
        assert_eq!(calls.get(), 0);
        let pixels = frame
            .pixels(|settings, level| {
                calls.set(calls.get() + 1);
                assert_eq!(level, 2);
                assert_eq!(settings.tone.exposure, 0.25);
                Ok(Vec::new())
            })
            .unwrap();
        assert!(pixels.is_empty());
        assert_eq!(calls.get(), 1);
        let cpu_frame = Frame {
            level: 2,
            settings: live,
            tiles: Some(Vec::new()),
        };
        cpu_frame
            .pixels(|_, _| panic!("CPU frame already has pixels"))
            .unwrap();
    }

    #[test]
    fn cancelled_frames_do_not_advance_the_surface_ring() {
        let mut ring = SurfaceCursor::default();
        assert_eq!(ring.candidate(&[]), None);
        assert_eq!(ring.candidate(&[10, 20]), Some(0));
        ring.published(10);
        // Three jobs cancelled before publication must keep using B, while A
        // remains displayed. The session write lock prevents overlapping jobs.
        for _ in 0..3 {
            assert_eq!(ring.candidate(&[10, 20]), Some(1));
        }
        ring.published(20);
        assert_eq!(ring.candidate(&[10, 20]), Some(0));
        assert_eq!(ring.candidate(&[20, 10]), Some(1));
        assert_eq!(ring.candidate(&[20]), Some(0));
        assert_eq!(ring.candidate(&[20, 30]), Some(1));
    }

    #[test]
    fn merge_patch_follows_rfc7386() {
        let mut v = serde_json::json!({"a": {"b": 1, "c": 2}, "d": 3});
        merge_patch(
            &mut v,
            &serde_json::json!({"a": {"b": 5, "c": null}, "e": [1]}),
        );
        assert_eq!(v, serde_json::json!({"a": {"b": 5}, "d": 3, "e": [1]}));
    }

    #[test]
    fn renderable_keeps_basic_and_drops_unsupported() {
        let mut s = DevelopSettings::default();
        s.tone.exposure = 1.5;
        s.tone.texture = 20.0;
        s.tone.clarity = -30.0;
        s.tone.dehaze = 40.0;
        s.color.vibrance = 50.0;
        s.color.saturation = -60.0;
        s.white_balance.mode = WhiteBalanceMode::Auto;
        assert_eq!(ignored_settings(&s).len(), 1);
        s.white_balance.mode = WhiteBalanceMode::Custom;
        s.white_balance.temperature = 4200.0;
        let r = renderable(&s);
        assert_eq!(r.tone.exposure, 1.5);
        assert_eq!(r.tone.texture, 20.0);
        assert_eq!(r.tone.clarity, -30.0);
        assert_eq!(r.tone.dehaze, 40.0);
        assert_eq!(r.color.vibrance, 50.0);
        assert_eq!(r.color.saturation, -60.0);
        assert_eq!(r.white_balance.temperature, 4200.0);
        pipeline_cpu::validate_settings(&r).unwrap();
        assert!(ignored_settings(&s).is_empty());
    }

    #[test]
    fn renderable_passes_every_panel_and_sanitizes() {
        use engine_api::recipe::settings::{CurvePoint, NormalizedRect, VignetteStyle};
        let mut s = DevelopSettings::default();
        s.tone.curves.parametric.lights = 30.0;
        s.tone.curves.parametric.midtone_split = 60.0;
        let knots = |v: &[(f32, f32)]| Curve(v.iter().map(|&(x, y)| CurvePoint { x, y }).collect());
        s.tone.curves.rgb = knots(&[(0.0, 0.05), (0.5, 0.6), (1.0, 1.0)]);
        s.tone.curves.red = knots(&[(0.0, 0.0), (1.0, 1.0)]);
        s.tone.curves.blue = knots(&[(0.0, 0.5), (0.5, 0.2), (1.0, 1.0)]);
        s.color.hsl.hue.orange = -20.0;
        s.color.hsl.saturation.blue = 250.0;
        s.color.grading.shadows.hue = 400.0;
        s.color.grading.shadows.saturation = 30.0;
        s.color.grading.blending = 70.0;
        s.detail.sharpening.amount = 120.0;
        s.detail.sharpening.radius = 9.0;
        s.detail.noise_reduction.luminance = 35.0;
        s.effects.vignette.amount = -40.0;
        s.effects.vignette.style = VignetteStyle::PaintOverlay;
        s.effects.grain.amount = 25.0;
        s.geometry.crop.rect = NormalizedRect {
            left: 0.1,
            top: 0.2,
            right: 0.8,
            bottom: 0.9,
        };
        s.geometry.crop.angle = 3.5;
        s.geometry.crop.aspect = Some([3, 2]);
        let r = renderable(&s);
        pipeline_cpu::validate_settings(&r).unwrap();
        assert_eq!(r.tone.curves.parametric.lights, 30.0);
        assert_eq!(r.tone.curves.parametric.midtone_split, 60.0);
        assert_eq!(r.tone.curves.rgb, s.tone.curves.rgb);
        assert_eq!(
            r.tone.curves.red,
            Curve::default(),
            "identity renders as empty"
        );
        assert_eq!(
            r.tone.curves.blue,
            Curve::default(),
            "descending knots are refused"
        );
        assert_eq!(r.color.hsl.hue.orange, -20.0);
        assert_eq!(r.color.hsl.saturation.blue, 100.0);
        assert_eq!(r.color.grading.shadows.hue, 40.0);
        assert_eq!(r.color.grading.blending, 70.0);
        assert_eq!(r.detail.sharpening.amount, 120.0);
        assert_eq!(r.detail.sharpening.radius, 3.0);
        assert_eq!(r.detail.noise_reduction.luminance, 35.0);
        assert_eq!(r.effects.vignette, s.effects.vignette);
        assert_eq!(r.effects.grain.amount, 25.0);
        assert_eq!(r.geometry.crop.rect, s.geometry.crop.rect);
        assert_eq!(r.geometry.crop.angle, 3.5);
        assert_eq!(
            r.geometry.crop.aspect, None,
            "the aspect lock draws nothing"
        );
        // Only the refused curves, the clamped values and the lock hint differ.
        let ignored = ignored_settings(&s);
        assert!(
            ignored.iter().all(|p| p.starts_with("/tone/curves/")
                || p.contains("saturation/blue")
                || p.contains("shadows/hue")
                || p.contains("radius")),
            "{ignored:?}"
        );
        // The crop tool renders the whole frame.
        let whole = renderable_with(&s, false);
        assert_eq!(whole.geometry, Default::default());
        assert_eq!(whole.effects, r.effects);
        // Unordered split points fall back to the defaults.
        s.tone.curves.parametric.shadow_split = 70.0;
        let r = renderable(&s);
        assert_eq!(r.tone.curves.parametric.midtone_split, 50.0);
        pipeline_cpu::validate_settings(&r).unwrap();
        // An invalid crop is not drawn.
        s.geometry.crop.angle = 60.0;
        assert_eq!(renderable(&s).geometry, Default::default());
    }

    /// docs/10 §2: the agent group fades as a whole; later manual edits and
    /// step toggles survive; the amount itself is an ordinary undo step.
    #[test]
    fn group_amount_blends_the_agent_group_and_keeps_later_edits() {
        let mut recipe = Recipe::new(ImageId(7));
        let group = recipe.history.add_group("Agent base edit");
        let agent = |label: &str, t: i64| EditMeta {
            author: Author::Agent {
                name: "test".into(),
            },
            group: Some(group),
            rationale: Some(format!("because {label}")),
            ..EditMeta::user(label, t)
        };
        recipe
            .edit(agent("Exposure", 1), |s| s.tone.exposure = 1.0)
            .unwrap();
        recipe
            .edit(agent("Contrast", 2), |s| {
                s.tone.contrast = 20.0;
                s.white_balance.mode = WhiteBalanceMode::Custom;
            })
            .unwrap();
        recipe
            .edit(EditMeta::user("Shadows +10", 3), |s| s.tone.shadows = 10.0)
            .unwrap();
        let groups = history_groups(&recipe).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].amount, 1.0);
        assert_eq!(groups[0].steps, [1, 2]);
        let without: DevelopSettings = serde_json::from_str(&groups[0].without_json).unwrap();
        assert_eq!(
            (
                without.tone.exposure,
                without.tone.contrast,
                without.tone.shadows
            ),
            (0.0, 0.0, 10.0)
        );

        assert!(record_group_amount(&mut recipe, group.0, 0.25, 4).unwrap());
        let s = &recipe.settings;
        assert_eq!(
            (s.tone.exposure, s.tone.contrast, s.tone.shadows),
            (0.25, 5.0, 10.0)
        );
        assert_eq!(
            s.white_balance.mode,
            WhiteBalanceMode::AsShot,
            "non-numeric switches at 50 %"
        );
        assert!(
            !record_group_amount(&mut recipe, group.0, 0.25, 5).unwrap(),
            "same amount: no step"
        );
        let items = history_items(&recipe).unwrap();
        assert_eq!(items[3].group_amount, Some(0.25));
        assert_eq!(items[0].rationale.as_deref(), Some("because Exposure"));
        assert_eq!(items[0].group_id, Some(group.0));
        assert_eq!(history_groups(&recipe).unwrap()[0].amount, 0.25);

        // A later manual edit of a group control wins over the fade.
        recipe
            .edit(EditMeta::user("Exposure +2.00", 6), |s| {
                s.tone.exposure = 2.0
            })
            .unwrap();
        assert!(record_group_amount(&mut recipe, group.0, 1.0, 7).unwrap());
        let s = &recipe.settings;
        assert_eq!(
            (s.tone.exposure, s.tone.contrast, s.tone.shadows),
            (2.0, 20.0, 10.0)
        );

        // Toggling a step off inside a faded group keeps the fade.
        assert!(record_group_amount(&mut recipe, group.0, 0.5, 8).unwrap());
        let lineage: Vec<HistoryEntry> = recipe
            .history
            .lineage(recipe.history.head)
            .unwrap()
            .into_iter()
            .cloned()
            .collect();
        let refs: Vec<&HistoryEntry> = lineage.iter().collect();
        let off = [2u64].into();
        let next = replay_state(&recipe.history.base, &refs, &off, &Default::default()).unwrap();
        assert_eq!((next.tone.exposure, next.tone.contrast), (2.0, 0.0));
        assert!(
            record_group_amount(&mut recipe, 99, 0.5, 9).is_err(),
            "unknown group"
        );
    }

    #[test]
    fn agent_steps_named_after_tools_read_as_their_controls() {
        let mut recipe = Recipe::new(ImageId(7));
        let group = recipe.history.add_group("Agent base edit");
        let meta = EditMeta {
            author: Author::Agent {
                name: "tessera-mcp".into(),
            },
            group: Some(group),
            rationale: Some("warmer".into()),
            ..EditMeta::user("set_tone", 1)
        };
        recipe
            .edit(meta, |s| {
                s.white_balance.temperature = 6000.0;
                s.white_balance.mode = WhiteBalanceMode::Custom;
                s.color.vibrance = 12.0;
            })
            .unwrap();
        recipe
            .edit(EditMeta::user("set_tone", 2), |s| s.tone.exposure = 1.0)
            .unwrap();
        let items = history_items(&recipe).unwrap();
        assert_eq!(
            items[0].label, "Vibrance, Temperature",
            "the mode switch is implied"
        );
        assert_eq!(items[1].label, "set_tone", "only agent steps are renamed");
    }

    #[test]
    fn step_toggles_replay_history_without_the_step() {
        let mut recipe = Recipe::new(ImageId(7));
        recipe
            .edit(EditMeta::user("Exposure +1.00", 1), |s| {
                s.tone.exposure = 1.0
            })
            .unwrap();
        recipe
            .edit(EditMeta::user("Contrast +20", 2), |s| {
                s.tone.contrast = 20.0
            })
            .unwrap();
        let lineage = |r: &Recipe| {
            r.history
                .lineage(r.history.head)
                .unwrap()
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };
        // Turn step 1 off as the session does.
        let l = lineage(&recipe);
        let refs: Vec<&HistoryEntry> = l.iter().collect();
        let off: std::collections::BTreeSet<u64> = [1].into();
        let next = replay_state(&recipe.history.base, &refs, &off, &Default::default()).unwrap();
        assert_eq!(next.tone.exposure, 0.0);
        assert_eq!(next.tone.contrast, 20.0);
        let meta = EditMeta {
            rationale: Some(format!("{TOGGLE_MARKER}:1:off")),
            ..EditMeta::user("Turn Off Exposure +1.00", 3)
        };
        recipe.edit(meta, |s| *s = next).unwrap();
        // A later edit, then turn step 1 back on: both survive.
        recipe
            .edit(EditMeta::user("Shadows +10", 4), |s| s.tone.shadows = 10.0)
            .unwrap();
        let l = lineage(&recipe);
        let refs: Vec<&HistoryEntry> = l.iter().collect();
        assert_eq!(disabled_steps(&refs), [1].into());
        let items = history_items(&recipe).unwrap();
        assert_eq!(items.len(), 4);
        assert!(!items[0].enabled && items[1].enabled);
        assert_eq!(items[2].toggles, Some(1));
        assert!(items[3].is_head && items.iter().all(|i| i.applied));
        let on = replay_state(
            &recipe.history.base,
            &refs,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(
            (on.tone.exposure, on.tone.contrast, on.tone.shadows),
            (1.0, 20.0, 10.0)
        );
        // Undone steps are listed after the applied ones.
        recipe.undo().unwrap();
        let items = history_items(&recipe).unwrap();
        assert_eq!(items.len(), 4);
        assert!(items[2].is_head && !items[3].applied);
        assert_eq!(items[3].label, "Shadows +10");
    }

    #[test]
    fn resident_extended_controls_do_not_start_at_heavy_proxy_level() {
        let mut s = DevelopSettings::default();
        s.color.vibrance = 20.;
        s.effects.vignette.amount = -20.;
        s.tone.curves.parametric.darks = 10.;
        assert_eq!(RenderClass::of(&s, true), RenderClass::Light);
        assert_eq!(RenderClass::of(&s, false), RenderClass::Heavy);
    }

    #[test]
    fn drag_level_prefers_screen_level_within_the_refresh() {
        let mut d = DragLevel::starting_at(0);
        // The first frame at a level refills caches: never evidence.
        d.record(200.0);
        assert_eq!(d.offset, 0, "cold first frame ignored");
        // Over the preferred 12 ms but within one refresh: stay.
        for ms in [13.0, 15.0, 14.5, 15.9] {
            d.record(ms);
            assert_eq!(d.offset, 0, "{ms} ms keeps the screen level");
        }
        // One missed refresh (e.g. exact Dehaze statistics) is a spike.
        d.record(45.0);
        d.record(9.0);
        assert_eq!(d.offset, 0, "single spike");
        // Two consecutive misses drop one level.
        d.record(20.0);
        d.record(20.0);
        assert_eq!(d.offset, 1, "sustained misses drop one level");
        // At the coarser level: the cold frame is ignored, and the level
        // returns only when the finer level is predicted to fit 12 ms.
        d.record(120.0);
        assert_eq!(d.offset, 1);
        d.record(5.0);
        assert_eq!(d.offset, 1, "5 ms x4 = 20 ms predicted: stay coarser");
        for _ in 0..4 {
            d.record(2.5);
        }
        assert_eq!(d.offset, 0, "finer level predicted within 12 ms");
    }

    #[test]
    fn drag_level_adapts_to_the_frame_budget() {
        let mut d = DragLevel::starting_at(1);
        for ms in [40.0, 40.0, 40.0] {
            d.record(ms);
        }
        assert_eq!(d.offset, 2, "over budget: coarser");
        d.record(10.0);
        d.record(10.0);
        d.record(11.0);
        assert_eq!(d.offset, 2, "within budget: stays");
        for _ in 0..6 {
            d.record(2.0);
        }
        assert!(d.offset < 2, "far under budget: finer");
        let mut d = DragLevel::starting_at(MAX_DRAG_OFFSET);
        for _ in 0..4 {
            d.record(100.0);
        }
        assert_eq!(d.offset, MAX_DRAG_OFFSET);
    }

    #[test]
    fn masking_gate_matches_the_operator() {
        assert_eq!(
            MaskSource::gate(0.0, 0.0),
            255,
            "Masking 0 sharpens everywhere"
        );
        assert_eq!(MaskSource::gate(0.01, 50.0), 0, "flat areas are masked out");
        assert_eq!(MaskSource::gate(1.0, 50.0), 255, "edges are sharpened");
        assert!(MaskSource::gate(0.1, 50.0) > MaskSource::gate(0.08, 50.0));
    }
}

#[cfg(test)]
mod m2_49_geometry_tests {
    #[test]
    fn session_keeps_manual_transform_and_lens_controls() {
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.geometry.transform.vertical = 24.0;
        settings.geometry.transform.rotate = 2.0;
        settings.lens.manual_distortion = 12.0;
        let rendered = super::renderable(&settings);
        assert_eq!(rendered.geometry.transform, settings.geometry.transform);
        assert_eq!(rendered.lens, settings.lens);
    }
}

#[cfg(test)]
mod m2_49_blur_tests {
    #[test]
    fn session_keeps_requested_lens_blur_for_depth_hook() {
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.effects.lens_blur = Some(Default::default());
        assert_eq!(
            super::session_renderable(&settings, true, false)
                .effects
                .lens_blur,
            settings.effects.lens_blur
        );
    }
}

#[cfg(test)]
mod m2_49_session_option_tests {
    #[test]
    fn guided_and_depth_options_are_session_only() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
            .save(dir.path().join("one.jpg"))
            .unwrap();
        let engine =
            crate::Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(dir.path().to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.open_develop_session(row.id).unwrap();
        session
            .set_settings(
                r#"{"geometry":{"transform":{"vertical":24}}}"#.into(),
                false,
            )
            .unwrap();
        session.set_render_uncorrected(true).unwrap();
        assert_eq!(
            session.shared.lock().unwrap().drawn().geometry,
            Default::default()
        );
        assert_eq!(
            session
                .shared
                .lock()
                .unwrap()
                .live
                .geometry
                .transform
                .vertical,
            24.0
        );
        session.set_render_uncorrected(false).unwrap();
        assert_eq!(
            session
                .shared
                .lock()
                .unwrap()
                .drawn()
                .geometry
                .transform
                .vertical,
            24.0
        );
        session.set_render_depth_visualisation(true).unwrap();
        assert!(session.shared.lock().unwrap().render_depth_visualisation);
        session.set_render_depth_visualisation(false).unwrap();
    }
}

#[cfg(test)]
mod m2_49_overlay_tests {
    use super::*;
    #[test]
    fn depth_overlay_is_not_saved_as_a_recipe_preview() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
            .save(dir.path().join("one.jpg"))
            .unwrap();
        let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(dir.path().to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.open_develop_session(row.id).unwrap();
        let (generation, settings) = {
            let mut st = session.shared.lock().unwrap();
            st.render_depth_visualisation = true;
            st.histogram.red = vec![7; 256];
            (st.generation, st.drawn())
        };
        let sink = LevelSink {
            output: RenderOutput::Display,
            surface: Default::default(),
            shared: Arc::downgrade(&session.shared),
            generation,
            started: Instant::now(),
            first_level: 0,
            finest_level: 0,
            expected: vec![],
            settings,
            dirty: None,
            current: None,
            screen_level: 0,
            adapt: None,
        };
        sink.publish(
            &session.shared,
            LevelWriter {
                level: 0,
                surface: None,
                remaining: 0,
                tiles: vec![],
            },
            [[0; 256]; 4],
            false,
        );
        let st = session.shared.lock().unwrap();
        assert!(
            st.frame.is_none(),
            "depth overlay must never be cached under a normal recipe hash"
        );
        assert_eq!(
            st.histogram.red,
            vec![7; 256],
            "depth overlay must preserve the image histogram"
        );
    }
}

#[cfg(test)]
mod m2_49_depth_process_tests {
    #[test]
    fn depth_input_does_not_silently_substitute_native_process() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
            .save(dir.path().join("one.jpg"))
            .unwrap();
        let engine =
            crate::Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(dir.path().to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let session = engine.open_develop_session(row.id).unwrap();
        // Even an unsupported imported version must be rejected by depth input,
        // rather than quietly creating a depth cache from native-process pixels.
        session.shared.lock().unwrap().recipe.process_version =
            engine_api::recipe::ProcessVersion {
                family: engine_api::recipe::ProcessFamily::Adobe,
                revision: 99,
            };
        assert!(session.shared.depth_input().is_err());
    }
}

#[cfg(test)]
mod m2_49_depth_histogram_freshness_tests {
    #[test]
    fn cached_histogram_does_not_bypass_current_process_validation() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([180, 90, 40]))
            .save(dir.path().join("one.jpg"))
            .unwrap();
        let engine =
            crate::Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(dir.path().to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let support = engine.db.parent().unwrap();
        let session = engine.clone().open_develop_session(row.id).unwrap();
        let input = session.shared.depth_input().unwrap();
        let rgb = image_core::depth::model_input(&input).unwrap();
        let depth = image_core::ml_depth::DepthMap::from_prediction(
            input.width(),
            input.height(),
            vec![0.; input.width() as usize * input.height() as usize],
        )
        .unwrap();
        let store =
            image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 1 << 20)
                .unwrap();
        depth
            .store(
                &store,
                &image_core::ml_depth::cache_key(&rgb, image_core::ml_depth::MODEL_VERSION),
            )
            .unwrap();
        session.shared.depth_provider.estimate(&input).unwrap();
        assert!(session.shared.depth_provider.histogram().is_ok());
        // A previous render has populated the histogram, but the current
        // recipe is different and has not yet produced a replacement frame.
        session.shared.lock().unwrap().recipe.process_version =
            engine_api::recipe::ProcessVersion {
                family: engine_api::recipe::ProcessFamily::Adobe,
                revision: 99,
            };
        assert!(
            session.depth_histogram().is_err(),
            "cached histogram must not bypass the current recipe's process validation"
        );
    }
}

#[cfg(test)]
mod depth_histogram_read_only_contract_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    fn cache_depth_for_input(
        support: &Path,
        input: &pipeline_cpu::Image,
        values: Vec<f32>,
    ) -> Vec<u64> {
        let rgb = image_core::depth::model_input(input).unwrap();
        let key = image_core::ml_depth::cache_key(&rgb, image_core::ml_depth::MODEL_VERSION);
        let depth =
            image_core::ml_depth::DepthMap::from_prediction(rgb.width(), rgb.height(), values)
                .unwrap();
        let histogram = depth.histogram().to_vec();
        let store =
            image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 1 << 20)
                .unwrap();
        depth.store(&store, &key).unwrap();
        histogram
    }

    fn optional_bytes(path: &Path) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    #[test]
    fn engine_histogram_uses_saved_pixels_without_starting_a_writer_or_publishing() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("readonly.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([55, 92, 131]))
            .save(&photo)
            .unwrap();
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);

        // Keep a legacy Raw source tag on an RGB file. Develop's in-memory
        // normalization must still be used for the histogram input.
        let mut legacy = catalog::document(&photo, parse_id(&row.id).unwrap()).unwrap();
        legacy.recipe.source_kind = engine_api::recipe::SourceKind::Raw;
        let recipe_path = sidecar::Sidecar::paths(&photo).recipe;
        sidecar::Sidecar::write_recipe(&recipe_path, &legacy).unwrap();
        let xmp_path = catalog::xmp_path(&photo);
        let before_recipe = std::fs::read(&recipe_path).unwrap();
        let before_xmp = optional_bytes(&xmp_path);
        let before_change_head = engine.lock().unwrap().index.change_head().unwrap();
        let saved_recipe = catalog::document(&photo, parse_id(&row.id).unwrap())
            .unwrap()
            .recipe;
        let saved_exposure = saved_recipe.settings.tone.exposure;

        let editor = engine.clone().open_develop_session(row.id.clone()).unwrap();
        let saved_input = editor.shared.depth_input().unwrap();
        let pixel_count = (saved_input.width() * saved_input.height()) as usize;
        let saved_histogram = cache_depth_for_input(
            engine.db.parent().unwrap(),
            &saved_input,
            vec![0.0; pixel_count],
        );

        editor
            .set_settings(
                serde_json::json!({"tone":{"exposure": saved_exposure + 4.0}}).to_string(),
                false,
            )
            .unwrap();
        let live_input = editor.shared.depth_input().unwrap();
        let saved_rgb = image_core::depth::model_input(&saved_input).unwrap();
        let live_rgb = image_core::depth::model_input(&live_input).unwrap();
        assert_ne!(
            saved_rgb, live_rgb,
            "fixture must distinguish live from saved pixels"
        );
        let mut live_depth = vec![0.0; pixel_count];
        live_depth[0] = 1.0;
        let live_histogram =
            cache_depth_for_input(engine.db.parent().unwrap(), &live_input, live_depth);
        assert_ne!(saved_histogram, live_histogram);

        let writer_count = engine.develop_writer_constructions.load(Ordering::Relaxed);
        assert_eq!(
            writer_count, 1,
            "only the visible editor should own a writer"
        );
        let actual = engine.clone().depth_histogram(row.id.clone()).unwrap();
        assert_eq!(
            actual, saved_histogram,
            "histogram must use saved recipe pixels"
        );
        assert_ne!(
            actual, live_histogram,
            "unsaved editor controls must not leak into histogram"
        );
        assert_eq!(
            engine.develop_writer_constructions.load(Ordering::Relaxed),
            writer_count,
            "a read-only histogram must not construct a temporary writer session"
        );
        assert_eq!(std::fs::read(&recipe_path).unwrap(), before_recipe);
        assert_eq!(optional_bytes(&xmp_path), before_xmp);
        assert_eq!(
            engine.lock().unwrap().index.change_head().unwrap(),
            before_change_head,
            "histogram must not publish index changes"
        );

        // A close after a live edit is allowed to publish it; restore the
        // original value first so this test isolates histogram side effects.
        editor
            .set_settings(
                serde_json::json!({"tone":{"exposure": saved_exposure}}).to_string(),
                false,
            )
            .unwrap();
        editor.close().unwrap();
    }

    #[test]
    fn engine_histogram_keeps_the_cached_model_miss_error_without_downloading() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([55, 92, 131]))
            .save(photos.join("missing-depth.jpg"))
            .unwrap();
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);

        let error = engine
            .clone()
            .depth_histogram(row.id)
            .expect_err("cache miss without installed model must remain an error");
        assert!(
            error
                .to_string()
                .contains(image_core::ml_depth::MISSING_MODEL_MESSAGE),
            "unexpected depth cache-miss error: {error}"
        );
    }

    #[test]
    fn engine_histogram_rejects_unsupported_saved_process_with_a_populated_cache() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("unsupported-process.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([55, 92, 131]))
            .save(&photo)
            .unwrap();
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let row = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .remove(0);
        let editor = engine.clone().open_develop_session(row.id.clone()).unwrap();
        let valid_input = editor.shared.depth_input().unwrap();
        let pixel_count = (valid_input.width() * valid_input.height()) as usize;
        cache_depth_for_input(
            engine.db.parent().unwrap(),
            &valid_input,
            vec![0.0; pixel_count],
        );

        let mut saved = catalog::document(&photo, parse_id(&row.id).unwrap()).unwrap();
        saved.recipe.process_version = engine_api::recipe::ProcessVersion {
            family: engine_api::recipe::ProcessFamily::Adobe,
            revision: 99,
        };
        sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(&photo).recipe, &saved).unwrap();

        let writer_count = engine.develop_writer_constructions.load(Ordering::Relaxed);
        assert_eq!(
            writer_count, 1,
            "fixture has only its visible editor writer"
        );
        let error = engine
            .clone()
            .depth_histogram(row.id)
            .expect_err("unsupported saved process must fail despite an input cache hit");
        assert!(
            error.to_string().contains("Adobe PV3–6 required"),
            "the error must come from saved process validation, not depth cache miss: {error}"
        );
        assert_eq!(
            engine.develop_writer_constructions.load(Ordering::Relaxed),
            writer_count,
            "process validation must not require a temporary writer session"
        );
        drop(editor);
    }
}
