//! `DevelopSession` over UniFFI: live develop settings, progressive viewport
//! rendering into host IOSurfaces, histogram, history and persistence.
//!
//! # Rendering
//!
//! The session owns a [`RawImage`] and shares the engine's [`Renderer`]
//! (CPU operators by default, Metal with `TESSERA_RENDER_BACKEND=gpu`; see
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
//! Output on the memoized WhiteBalance tiles of that level. During an
//! interactive drag on a screen level above [`DRAG_BUDGET_PX`] the session
//! renders one level coarser; `commit` (mouse-up) refines to the screen level.
//!
//! # History and persistence
//!
//! `set_settings` changes only the live state. `commit` records one
//! [`Recipe::edit`] history entry for everything since the last commit, so a
//! whole drag is one undo step. Commits, undo/redo, reset and snapshots
//! schedule a debounced save ([`SAVE_DEBOUNCE`]) on the session's writer
//! thread: recipe JSON + XMP through `sidecar`, index refresh, and edited
//! previews keyed by the new recipe hash.

use crate::{Engine, Result, catalog, failure, now_ms, parse_id, surface::Surface};
use engine_api::{
    color::ColorMatrix3,
    jobs::{Job, JobContext, JobHandle, Priority, Scheduler},
    recipe::{
        DevelopSettings, EditMeta, Recipe,
        settings::{DemosaicMethod, DisplayTransform, HighlightReconstruction, WhiteBalanceMode},
    },
    stage::StageId,
    tile::{TILE_SIZE, Tile},
};
use image_core::{
    PixelRect, ProgressiveRenderJob, RawImage, RenderOutput, Renderer, Viewport, render::MAX_LEVEL,
};
use serde_json::Value;
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex, MutexGuard, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Interactive drags on screen levels larger than this render one level
/// coarser until the drag is committed.
pub const DRAG_BUDGET_PX: u64 = 4_200_000;
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
    tiles: Vec<Tile>,
}

struct State {
    recipe: Recipe,
    live: DevelopSettings,
    /// Renderable settings of the last submitted render.
    rendered: Option<DevelopSettings>,
    rendered_level: Option<u8>,
    surfaces: Vec<Arc<Surface>>,
    next_surface: usize,
    screen_level: u8,
    job: Option<JobHandle>,
    generation: u64,
    histogram: Histogram,
    frame: Option<Arc<Frame>>,
    closed: bool,
}

#[derive(Default)]
struct SaveState {
    due: Option<Instant>,
    flush: bool,
    busy: bool,
    shutdown: bool,
    error: Option<String>,
}

pub(crate) struct Shared {
    engine: std::sync::Weak<Engine>,
    image_id: String,
    path: PathBuf,
    image: RawImage,
    renderer: Arc<Renderer>,
    backend: String,
    state: Mutex<State>,
    generation: AtomicU64,
    listener: Mutex<Option<Arc<dyn DevelopListener>>>,
    save: Mutex<SaveState>,
    save_cv: Condvar,
    file_hash: OnceLock<std::result::Result<blake3::Hasher, String>>,
}

#[derive(uniffi::Object)]
pub struct DevelopSession {
    shared: Arc<Shared>,
    writer: Mutex<Option<JoinHandle<()>>>,
}

// ─────────────────────────── settings helpers ───────────────────────────

/// The supported Basic controls; every other field keeps its native defaults.
/// Unsupported settings stay in the recipe and XMP
/// but are not drawn; see [`ignored_settings`].
pub fn renderable(s: &DevelopSettings) -> DevelopSettings {
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
        *dst = finite_or(src, 0.0).clamp(-100.0, 100.0);
    }
    r.tone.display_transform = match t.display_transform {
        d @ (DisplayTransform::Native | DisplayTransform::Sigmoid) => d,
        _ => r.tone.display_transform,
    };
    r.output.gamut_mapping = s.output.gamut_mapping;
    r
}

fn finite_or(v: f32, default: f32) -> f32 {
    if v.is_finite() { v } else { default }
}

/// JSON pointers of settings present in `s` that the viewport does not render.
pub fn ignored_settings(s: &DevelopSettings) -> Vec<String> {
    let (Ok(a), Ok(b)) = (serde_json::to_value(s), serde_json::to_value(renderable(s))) else {
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

#[uniffi::export]
impl Engine {
    /// Opens a develop session on an indexed RAW image. Blocking (decodes the
    /// raw): call off the main thread. One session per visible image.
    pub fn open_develop_session(self: Arc<Self>, image_id: String) -> Result<Arc<DevelopSession>> {
        let id = parse_id(&image_id)?;
        let path = {
            let c = self.lock()?;
            Self::path(&c, &image_id)?
        };
        let path = PathBuf::from(path);
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "tif" | "tiff" | "png" | "heic"
        ) {
            return Err(failure("develop needs a RAW file"));
        }
        let recipe = catalog::document(&path, id)?.recipe;
        let image = RawImage::open(id, &path)?;
        let (renderer, backend) = self.develop_renderer();
        let screen_level = default_level(&image);
        let shared = Arc::new(Shared {
            engine: Arc::downgrade(&self),
            image_id,
            path,
            image,
            renderer,
            backend,
            state: Mutex::new(State {
                live: recipe.settings.clone(),
                recipe,
                rendered: None,
                rendered_level: None,
                surfaces: Vec::new(),
                next_surface: 0,
                screen_level,
                job: None,
                generation: 0,
                histogram: Histogram::default(),
                frame: None,
                closed: false,
            }),
            generation: AtomicU64::new(0),
            listener: Mutex::new(None),
            save: Mutex::new(SaveState::default()),
            save_cv: Condvar::new(),
            file_hash: OnceLock::new(),
        });
        let writer = {
            let shared = shared.clone();
            std::thread::Builder::new()
                .name("develop-save".into())
                .spawn(move || shared.writer_loop())?
        };
        Ok(Arc::new(DevelopSession {
            shared,
            writer: Mutex::new(Some(writer)),
        }))
    }
}

impl Engine {
    /// Writes develop state into the image's recipe document without touching
    /// fields other writers own (selection, unknown members), then the XMP
    /// (crs: develop values + selection) and the index. Returns the new
    /// recipe hash.
    fn save_develop(&self, image_id: &str, path: &Path, recipe: &Recipe) -> Result<String> {
        let id = parse_id(image_id)?;
        let mut c = self.lock()?;
        let mut doc = catalog::document(path, id)?;
        doc.recipe.process_version = recipe.process_version;
        doc.recipe.settings = recipe.settings.clone();
        doc.recipe.history = recipe.history.clone();
        doc.recipe.ids.next_mask = doc.recipe.ids.next_mask.max(recipe.ids.next_mask);
        doc.recipe.ids.next_retouch = doc.recipe.ids.next_retouch.max(recipe.ids.next_retouch);
        doc.record_write("tessera-mac", now_ms())?;
        let packet = catalog::selection_packet(path, &doc)?.with_recipe(&doc.recipe)?;
        sidecar::Sidecar::write_recipe(sidecar::Sidecar::paths(path).recipe, &doc)?;
        sidecar::Sidecar::write_xmp(catalog::xmp_path(path), &packet)?;
        c.index.scan(
            path.parent()
                .ok_or_else(|| failure("image has no folder"))?,
            &catalog::Sidecars,
            &catalog::EmbeddedMetadata,
        )?;
        Ok(doc.recipe.recipe_hash().to_string())
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

// ─────────────────────────────── session ───────────────────────────────

impl Shared {
    fn lock(&self) -> Result<MutexGuard<'_, State>> {
        self.state.lock().map_err(failure)
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
        st.generation += 1;
        let generation = st.generation;
        self.generation.store(generation, Ordering::SeqCst);
        if let Some(job) = st.job.take() {
            job.cancel();
        }
        let settings = renderable(&st.live);
        let dirty = match &st.rendered {
            Some(prev) => prev.first_dirty_stage(&settings),
            None => Some(StageId::Decode),
        };
        st.rendered = Some(settings.clone());
        let target = st.screen_level;
        let area = self.image.level_extent(target).area();
        let first = if interactive && area > DRAG_BUDGET_PX {
            (target + 1).min(MAX_LEVEL)
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
                let n = Renderer::tiles_for(&self.image, l, viewport.rect.at_level(l)).len();
                (l, n)
            })
            .collect();
        let sink = LevelSink {
            shared: Arc::downgrade(self),
            generation,
            started: Instant::now(),
            first_level: first,
            finest_level: finest,
            expected,
            settings: settings.clone(),
            dirty: dirty.map(stage_name),
            current: None,
        };
        let job = DevelopJob {
            inner: ProgressiveRenderJob {
                renderer: self.renderer.clone(),
                image: self.image.clone(),
                settings,
                viewport,
                output: RenderOutput::Display,
                priority: Priority::Viewport,
                sink: Box::new(sink.into_fn()),
            },
            shared: Arc::downgrade(self),
            generation,
        };
        if let Some(engine) = self.engine.upgrade() {
            st.job = Some(engine.jobs.submit(Box::new(job), None));
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
            s.due = None;
            s.flush = false;
            s.busy = true;
            drop(s);
            let result = self.save_now();
            let mut s = self.save.lock().unwrap_or_else(|e| e.into_inner());
            s.busy = false;
            s.error = result.err().map(|e| e.to_string());
            self.save_cv.notify_all();
        }
    }

    fn save_now(&self) -> Result<()> {
        let (recipe, frame) = {
            let st = self.lock()?;
            (st.recipe.clone(), st.frame.clone())
        };
        let engine = self
            .engine
            .upgrade()
            .ok_or_else(|| failure("engine closed"))?;
        let hash = engine.save_develop(&self.image_id, &self.path, &recipe)?;
        if let Some(frame) = frame.filter(|f| f.settings == renderable(&recipe.settings))
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
        let e = self.image.level_extent(frame.level);
        let mut rgb = image::RgbImage::new(e.width, e.height);
        for t in &frame.tiles {
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
    shared: std::sync::Weak<Shared>,
    generation: u64,
    started: Instant,
    first_level: u8,
    finest_level: u8,
    expected: Vec<(u8, usize)>,
    settings: DevelopSettings,
    dirty: Option<String>,
    current: Option<LevelWriter>,
}

struct LevelWriter {
    level: u8,
    surface: Option<Arc<Surface>>,
    remaining: usize,
    tiles: Vec<Tile>,
}

impl LevelSink {
    fn into_fn(mut self) -> impl FnMut(Tile) + Send {
        move |tile| self.accept(tile)
    }

    fn accept(&mut self, tile: Tile) {
        let Some(shared) = self.shared.upgrade() else {
            return;
        };
        if shared.generation.load(Ordering::SeqCst) != self.generation {
            return;
        }
        let level = tile.coord().level;
        if self.current.as_ref().map(|c| c.level) != Some(level) {
            let surface = {
                let Ok(mut st) = shared.state.lock() else {
                    return;
                };
                if st.surfaces.is_empty() {
                    None
                } else {
                    let i = st.next_surface % st.surfaces.len();
                    st.next_surface = st.next_surface.wrapping_add(1);
                    Some(st.surfaces[i].clone())
                }
            };
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
        let render_ms = self.started.elapsed().as_secs_f64() * 1000.0;
        let extent = shared.image.level_extent(done.level);
        let (width, height) = match &done.surface {
            Some(s) => (extent.width.min(s.width()), extent.height.min(s.height())),
            None => (extent.width, extent.height),
        };
        let [r, g, b, y] = hist;
        {
            let Ok(mut st) = shared.state.lock() else {
                return;
            };
            if st.generation != self.generation {
                return;
            }
            st.histogram = Histogram {
                red: r.to_vec(),
                green: g.to_vec(),
                blue: b.to_vec(),
                luminance: y.to_vec(),
                level: done.level,
                generation: self.generation,
            };
            st.rendered_level = Some(done.level);
            st.frame = Some(Arc::new(Frame {
                level: done.level,
                settings: self.settings.clone(),
                tiles: done.tiles,
            }));
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
            });
        }
    }
}

type Hist = [[u32; 256]; 4];

/// Writes a complete level into `surface` (if any) and returns its
/// histograms. Each thread owns one band of 256 surface rows (one tile row),
/// so the bands are disjoint slices of the locked surface.
fn write_level(surface: Option<&Surface>, tiles: &[Tile]) -> std::result::Result<Hist, String> {
    let mut rows: std::collections::BTreeMap<u32, Vec<&Tile>> = Default::default();
    for t in tiles {
        rows.entry(t.coord().y).or_default().push(t);
    }
    let band = |pixels: Option<(&mut [u8], usize, u32, u32)>, tiles: &[&Tile]| {
        let mut hist: Hist = [[0; 256]; 4];
        let mut error = None;
        let mut pixels = pixels;
        for t in tiles {
            if let Some((px, stride, first_row, width)) = pixels.as_mut()
                && let Err(e) = crate::surface::write_rgba8(px, *stride, *first_row, *width, t)
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

fn accumulate(hist: &mut Hist, tile: &Tile) {
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
        let result = Box::new(self.inner).run(ctx);
        if let Err(e) = &result
            && !matches!(e, engine_api::EngineError::Cancelled)
            && let Some(s) = shared.upgrade()
            && s.generation.load(Ordering::SeqCst) == generation
            && let Some(l) = s.listener()
        {
            l.render_failed(e.to_string());
        }
        result
    }
}

impl Drop for DevelopSession {
    fn drop(&mut self) {
        self.shared.close();
        {
            let mut s = self.shared.save.lock().unwrap_or_else(|e| e.into_inner());
            s.shutdown = true;
            self.shared.save_cv.notify_all();
        }
        if let Some(writer) = self.writer.lock().ok().and_then(|mut w| w.take()) {
            let _ = writer.join();
        }
    }
}

#[uniffi::export]
impl DevelopSession {
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

    /// Adds an RGBA8 IOSurface (see `surface.rs`) to the frame ring; call two
    /// or three times with surfaces of one `plan_surface` size so the host
    /// never samples the surface being written. A different size replaces
    /// the ring. The first surface of a ring starts a render at the new
    /// screen level.
    pub fn attach_surface(&self, iosurface_id: u32, width: u32, height: u32) -> Result<()> {
        let s = &self.shared;
        let level = (0..=MAX_LEVEL)
            .find(|&l| {
                let e = s.image.level_extent(l);
                e.width == width && e.height == height
            })
            .ok_or_else(|| failure("surface size must come from plan_surface"))?;
        let surface = Arc::new(Surface::lookup(iosurface_id, width, height).map_err(failure)?);
        let mut st = s.lock()?;
        if st
            .surfaces
            .first()
            .is_some_and(|f| f.width() != width || f.height() != height)
        {
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

    /// Releases every surface. Renders continue (histogram only) until a new
    /// surface is attached.
    pub fn detach_surfaces(&self) {
        if let Ok(mut st) = self.shared.lock() {
            st.surfaces.clear();
            st.next_surface = 0;
        }
    }

    /// Re-renders the live settings at the screen level (first paint).
    pub fn refresh(&self) -> Result<()> {
        let mut st = self.shared.lock()?;
        self.shared.render(&mut st, false);
        Ok(())
    }

    /// Merges an RFC 7386 JSON patch into the live settings and renders.
    /// Setting `white_balance.temperature`/`tint` without a mode switches to
    /// `custom`. Not recorded in history until `commit`.
    pub fn set_settings(&self, json_patch: String, interactive: bool) -> Result<()> {
        let patch: Value = serde_json::from_str(&json_patch).map_err(failure)?;
        let mut st = self.shared.lock()?;
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
        let next: DevelopSettings = serde_json::from_value(value).map_err(failure)?;
        if next == st.live && st.rendered.is_some() && !interactive {
            return Ok(());
        }
        st.live = next;
        self.shared.render(&mut st, interactive);
        Ok(())
    }

    /// Records the live changes since the last commit as one undo step
    /// labelled `label`, refines the viewport and schedules a save.
    pub fn commit(&self, label: String) -> Result<bool> {
        let recorded = {
            let mut st = self.shared.lock()?;
            let recorded = self.shared.commit_pending(&mut st, &label)?;
            if st.rendered_level != Some(st.screen_level)
                || st.rendered.as_ref() != Some(&renderable(&st.live))
            {
                self.shared.render(&mut st, false);
            }
            recorded
        };
        if recorded {
            self.shared.schedule_save();
        }
        Ok(recorded)
    }

    pub fn get_settings_json(&self) -> Result<String> {
        serde_json::to_string(&self.shared.lock()?.live).map_err(failure)
    }

    /// Settings kept in the recipe but not rendered by this pipeline version.
    pub fn ignored_settings(&self) -> Result<Vec<String>> {
        Ok(ignored_settings(&self.shared.lock()?.live))
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
        let changed = {
            let mut st = self.shared.lock()?;
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
            }
            changed
        };
        if changed {
            self.shared.schedule_save();
        }
        Ok(changed)
    }

    /// Names the current state (after committing pending changes).
    pub fn snapshot(&self, name: String) -> Result<()> {
        {
            let mut st = self.shared.lock()?;
            self.shared.commit_pending(&mut st, "Edit")?;
            st.recipe.create_snapshot(name, now_ms())?;
        }
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
        {
            let mut st = self.shared.lock()?;
            self.shared.commit_pending(&mut st, "Edit")?;
            st.recipe.restore_snapshot(&name)?;
            st.live = st.recipe.settings.clone();
            self.shared.render(&mut st, false);
        }
        self.shared.schedule_save();
        Ok(())
    }

    /// Writes pending changes now and waits for the save to finish.
    pub fn flush(&self) -> Result<()> {
        {
            let mut st = self.shared.lock()?;
            if self.shared.commit_pending(&mut st, "Edit")? {
                drop(st);
                self.shared.schedule_save();
            }
        }
        let mut s = self.shared.save.lock().map_err(failure)?;
        if s.due.is_some() {
            s.flush = true;
            self.shared.save_cv.notify_all();
        }
        while s.due.is_some() || s.busy {
            s = self.shared.save_cv.wait(s).map_err(failure)?;
        }
        match s.error.take() {
            Some(e) => Err(failure(e)),
            None => Ok(()),
        }
    }

    /// Stops rendering and writes pending changes. The session is unusable
    /// for rendering afterwards.
    pub fn close(&self) -> Result<()> {
        let result = self.flush();
        self.shared.close();
        result
    }
}

impl DevelopSession {
    fn history_move(
        &self,
        f: impl FnOnce(&mut Recipe) -> engine_api::EngineResult<bool>,
    ) -> Result<bool> {
        let moved = {
            let mut st = self.shared.lock()?;
            self.shared.commit_pending(&mut st, "Edit")?;
            let moved = f(&mut st.recipe)?;
            if moved {
                st.live = st.recipe.settings.clone();
                self.shared.render(&mut st, false);
            }
            moved
        };
        if moved {
            self.shared.schedule_save();
        }
        Ok(moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
