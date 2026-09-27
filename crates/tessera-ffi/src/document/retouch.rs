//! Remove tool, Content-Aware Fill and neural filters for the app (WP B5-09).
//!
//! M5-29 exposes these operations through `apply_raster_filter`, whose
//! parameters carry masks as one float per canvas pixel. The app should not
//! build those, so this module builds the requests engine-side:
//!
//! - [`DocumentSession::remove_with_selection`] and
//!   [`DocumentSession::content_aware_fill_selection`] take the mask from the
//!   document's selection.
//! - `begin_remove_stroke` / `remove_stroke_points` / `end_remove_stroke`
//!   accumulate a Remove stroke engine-side (hard round dabs from the brush
//!   crate's coverage maths, thresholded at half coverage, so a single dab
//!   equals a non-anti-aliased elliptical marquee of the same circle).
//! - `detect_distractions` runs the geometric distraction detector and keeps
//!   its suggestions here for review; `remove_distraction_suggestions` removes
//!   only the accepted ones.
//! - [`DocumentSession::neural_filter`] applies a neural filter to the current
//!   layer, to a new layer above it, or as a smart filter, and fills in face
//!   boxes when the caller omits them.
//!
//! Each apply is one history node and reports which backend actually ran.
//! `cancel_retouch` (and, for the paths that go through the M5-29 adapter,
//! `cancel_filter`) cancels a running apply. Model weights are only ever read
//! from the local cache: a missing model is an error that names the model and
//! where its file would come from. Nothing here downloads.

use super::{DocRect, DocumentSession, DocumentUpdate, Shared, ToolPoint, find, raster_from_rgba};
use crate::{Result, failure};
use compositor::{
    Affine, DocOp, DocState, Layer, LayerId, LayerKind, Raster, Rect, SmartFilter, SmartObject,
    render::smart_filters::{FilterBlend, FilterContext, SmartFilterEvaluator},
};
use engine_api::tile::{Extent, TILE_SIZE};
use std::{
    collections::HashMap,
    fmt::Write as _,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

// ─────────────────────────────── records ───────────────────────────────

/// Which inpainting backend Remove asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RemoveBackend {
    /// LaMa when its weights are installed, PatchMatch otherwise.
    Auto,
    /// CPU PatchMatch (no weights).
    PatchMatch,
    /// LaMa only; an error when its weights are not installed.
    Lama,
}

/// Neural filters M5-29 exposes (adapter ids `neural/skin_smoothing`, …).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NeuralFilterKind {
    SkinSmoothing,
    Colorize,
    JpegArtifactRemoval,
}

/// Where a neural filter's result goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum NeuralDestination {
    /// The layer's own pixels (inside the selection); on a smart object, a
    /// smart filter (the adapter's behaviour).
    CurrentLayer,
    /// A new pixel layer above a pixel layer (the layer is unchanged).
    NewLayer,
    /// A smart filter; a pixel layer becomes a smart object in the same step.
    SmartFilter,
}

/// What one retouch apply did.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RetouchResult {
    pub update: DocumentUpdate,
    /// The backend that actually ran: `PatchMatch`, `LaMa`, `Content-Aware
    /// Fill`, `Skin Smoothing (CPU)`, `DDColor`, `DRUNet`, or `none` (an
    /// empty mask).
    pub backend: String,
    /// Why the backend differs from the request (Auto without LaMa), how
    /// face boxes were found, …
    pub note: Option<String>,
    /// Engine time, milliseconds.
    pub millis: f64,
}

/// One control of a neural filter.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NeuralParam {
    /// JSON key (`strength`).
    pub key: String,
    /// Label (`Strength`).
    pub label: String,
    pub min: f32,
    pub max: f32,
    pub default_value: f32,
}

/// A neural filter of the Neural Filters panel.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct NeuralFilterInfo {
    pub kind: NeuralFilterKind,
    /// Adapter id (`neural/colorize`).
    pub filter_id: String,
    pub name: String,
    pub params: Vec<NeuralParam>,
    pub requires_weights: bool,
    pub limitation: Option<String>,
}

/// A model file retouching can use, and whether it is installed locally.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct RetouchModel {
    /// Registry id (`remove/lama`).
    pub model_id: String,
    /// Pinned registry version (what `ModelDownloads::request` takes).
    pub version: String,
    /// What needs it (`Remove (LaMa)`).
    pub used_by: String,
    pub installed: bool,
    /// Where the verified file is (or would be) cached.
    pub cache_path: String,
    /// Where the file comes from (never fetched implicitly).
    pub source_url: String,
}

/// Kind of distraction suggestion.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DistractionKind {
    /// A thin straight ridge or valley (geometric wire proxy).
    Wire,
    /// A face box dilated by half its size on each side (not a person mask).
    FaceBox,
}

/// One suggestion of `detect_distractions`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DistractionSuggestion {
    pub id: u32,
    pub kind: DistractionKind,
    /// Bounds in level-0 canvas pixels.
    pub bounds: DocRect,
    /// Pixels the suggestion covers.
    pub pixels: u64,
}

/// Result of `detect_distractions`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct DistractionScan {
    pub suggestions: Vec<DistractionSuggestion>,
    /// Where face boxes came from (`caller`, `face detector`, or why none).
    pub faces: String,
    /// What the detector is (and is not).
    pub limitation: String,
}

/// The neural filters M5-29 registers, with their controls.
#[uniffi::export]
pub fn neural_filters() -> Vec<NeuralFilterInfo> {
    filters::neural_catalog()
        .into_iter()
        .zip([
            NeuralFilterKind::SkinSmoothing,
            NeuralFilterKind::Colorize,
            NeuralFilterKind::JpegArtifactRemoval,
        ])
        .map(|(f, kind)| NeuralFilterInfo {
            kind,
            filter_id: neural_id(kind).into(),
            name: f.name.into(),
            params: f
                .params
                .iter()
                .map(|p| NeuralParam {
                    key: p.name.to_lowercase().replace(' ', "_"),
                    label: p.name.into(),
                    min: p.min,
                    max: p.max,
                    default_value: p.default,
                })
                .collect(),
            requires_weights: f.requires_weights,
            limitation: f.limitation.map(str::to_owned),
        })
        .collect()
}

fn neural_id(kind: NeuralFilterKind) -> &'static str {
    match kind {
        NeuralFilterKind::SkinSmoothing => "neural/skin_smoothing",
        NeuralFilterKind::Colorize => "neural/colorize",
        NeuralFilterKind::JpegArtifactRemoval => "neural/jpeg_artifact_removal",
    }
}

fn neural_name(kind: NeuralFilterKind) -> &'static str {
    match kind {
        NeuralFilterKind::SkinSmoothing => "Skin Smoothing",
        NeuralFilterKind::Colorize => "Colorize",
        NeuralFilterKind::JpegArtifactRemoval => "JPEG Artifact Removal",
    }
}

// ─────────────────────────────── models ───────────────────────────────

const LAMA: &str = "remove/lama";
const DDCOLOR: &str = "filters/ddcolor";
const DRUNET: &str = "enhance/drunet-color";
const YUNET: &str = "opencv/yunet";
const SFACE: &str = "opencv/sface";

/// Set once this process installed LaMa into the adapter (from the cache).
static LAMA_LOADED: AtomicBool = AtomicBool::new(false);

/// The app's model folder (`<support>/models`), its manifest refreshed from
/// the build's pinned `models.toml`.
fn models_dir(shared: &Shared) -> Result<std::path::PathBuf> {
    let engine = shared
        .engine
        .upgrade()
        .ok_or_else(|| failure("the engine is shut down"))?;
    let dir = engine.support_dir()?.join("models");
    std::fs::create_dir_all(&dir).map_err(failure)?;
    let manifest = dir.join("models.toml");
    let text = include_str!("../../../ml-runtime/models.toml");
    if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
        std::fs::write(&manifest, text).map_err(failure)?;
    }
    Ok(dir)
}

/// The shared app cache `<support>/models/cache`: where the app's model
/// downloads (`ModelDownloads`, Settings ▸ AI "Allow model downloads") put
/// verified files, and so where retouching looks for them.
fn app_cache(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("cache")
}

/// A hand-installed extra cache (`TESSERA_RETOUCH_MODEL_CACHE`), looked at
/// before the app cache. It never replaces the app cache (B5-09b: downloads
/// land in the app cache and must be found there).
fn extra_cache() -> Option<std::path::PathBuf> {
    std::env::var_os("TESSERA_RETOUCH_MODEL_CACHE")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
}

/// The registry over the app cache. Opening it never downloads.
fn registry(shared: &Shared) -> Result<ml_runtime::ModelRegistry> {
    let dir = models_dir(shared)?;
    ml_runtime::ModelRegistry::open(dir.join("models.toml"), app_cache(&dir)).map_err(failure)
}

/// The registry whose cache holds model `id` (the extra cache first, then
/// the app cache); the app cache's when neither does, so a missing model is
/// reported where a download would put it.
fn registry_for(shared: &Shared, id: &str) -> Result<ml_runtime::ModelRegistry> {
    let dir = models_dir(shared)?;
    if let Some(extra) = extra_cache()
        && extra != app_cache(&dir)
    {
        let reg =
            ml_runtime::ModelRegistry::open(dir.join("models.toml"), &extra).map_err(failure)?;
        if cached(&reg, id).unwrap_or(false) {
            return Ok(reg);
        }
    }
    registry(shared)
}

/// Whether model `id` is cached and hash-verified (never downloads).
fn cached(reg: &ml_runtime::ModelRegistry, id: &str) -> Result<bool> {
    let spec = reg
        .models()
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| failure(format!("{id} is not in the model manifest")))?;
    let r = engine_api::id::ModelRef {
        id: spec.id.as_str().into(),
        version: spec.version.clone(),
    };
    Ok(reg.resolve_cached_ref(&r).map_err(failure)?.is_some())
}

impl DocumentSession {
    /// Where a missing model is expected: the app cache, where downloads go.
    fn cache_dir(&self) -> Option<std::path::PathBuf> {
        models_dir(&self.shared).ok().map(|d| app_cache(&d))
    }

    /// `Ok` when model `id` is installed; otherwise the documented error:
    /// what needs it, the model id, where the file comes from and where it
    /// is expected.
    fn require(&self, reg: &ml_runtime::ModelRegistry, id: &str, what: &str) -> Result<()> {
        if cached(reg, id)? {
            return Ok(());
        }
        let spec = reg.models().iter().find(|m| m.id == id);
        let url = spec.map_or("", |s| s.download_url.as_str());
        let path = match (self.cache_dir(), spec) {
            (Some(d), Some(s)) => d.join(format!("{}.onnx", s.sha256)).display().to_string(),
            _ => "the app's model cache".into(),
        };
        Err(failure(format!(
            "{what} needs the {id} model weights, which are not installed. The engine never \
             downloads weights on its own (the app asks first, per Settings ▸ AI ▸ Allow model \
             downloads): the file comes from {url} and is expected, hash-verified, at {path}"
        )))
    }

    fn installed(reg: &ml_runtime::ModelRegistry, id: &str) -> bool {
        cached(reg, id).unwrap_or(false)
    }

    /// Installs a cached model into the M5-29 adapter (no download: the
    /// file is already cached and verified).
    fn load_cached(
        &self,
        reg: &ml_runtime::ModelRegistry,
        id: &str,
        adapter: &str,
        what: &str,
    ) -> Result<()> {
        self.require(reg, id, what)?;
        filters::CompositorFilters::load_model(adapter, Some(reg))?;
        if adapter == "remove" {
            LAMA_LOADED.store(true, Ordering::Relaxed);
        }
        Ok(())
    }
}

// ─────────────────────────────── session state ───────────────────────────────

struct Stroke {
    layer: u64,
    size: f32,
    backend: RemoveBackend,
    canvas: Extent,
    /// Canvas-sized, 1 = remove.
    mask: Vec<u8>,
    last: Option<(f32, f32)>,
    /// Distance walked since the last dab.
    carry: f32,
    bounds: Option<Rect>,
}

struct Suggestions {
    layer: u64,
    revision: u64,
    canvas: Extent,
    /// Pixel indices per suggestion id.
    pixels: Vec<Vec<u32>>,
}

#[derive(Default)]
struct Retouch {
    stroke: Option<Stroke>,
    suggestions: Option<Suggestions>,
    cancel: Arc<AtomicBool>,
}

struct Entry {
    owner: Weak<Shared>,
    state: Retouch,
}

/// Per-session retouch state, keyed by the session's `Shared` address
/// (session ids are only unique per engine).
static STATE: LazyLock<Mutex<HashMap<usize, Entry>>> = LazyLock::new(Default::default);

impl DocumentSession {
    fn retouch<T>(&self, f: impl FnOnce(&mut Retouch) -> T) -> T {
        let mut m = STATE.lock().unwrap_or_else(|e| e.into_inner());
        m.retain(|_, e| e.owner.strong_count() > 0);
        let key = Arc::as_ptr(&self.shared) as usize;
        let e = m.entry(key).or_insert_with(|| Entry {
            owner: Arc::downgrade(&self.shared),
            state: Retouch::default(),
        });
        if !Weak::ptr_eq(&e.owner, &Arc::downgrade(&self.shared)) {
            *e = Entry {
                owner: Arc::downgrade(&self.shared),
                state: Retouch::default(),
            };
        }
        f(&mut e.state)
    }

    /// A fresh cancel flag for one apply.
    fn arm_cancel(&self) -> Arc<AtomicBool> {
        self.retouch(|r| {
            r.cancel = Arc::new(AtomicBool::new(false));
            r.cancel.clone()
        })
    }
}

fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(failure("cancelled"))
    } else {
        Ok(())
    }
}

// ─────────────────────────────── pixels ───────────────────────────────

/// Interleaved f32 samples (`channels` per pixel) of a canvas-sized raster.
fn read_all(r: &Raster, canvas: Extent, channels: usize) -> Result<Vec<f32>> {
    let (w, h) = (canvas.width as usize, canvas.height as usize);
    let mut out = vec![0.0f32; w * h * channels];
    let (cols, rows) = canvas.tile_grid(TILE_SIZE);
    let mut buf = Vec::new();
    let t = TILE_SIZE as usize;
    for ty in 0..rows {
        for tx in 0..cols {
            let lay = r.layout(tx, ty);
            r.read_tile(tx, ty, &mut buf)?;
            let n = lay.plane_len();
            let stride = lay.stride();
            for y in 0..lay.extent.height as usize {
                let gy = ty as usize * t + y;
                if gy >= h {
                    break;
                }
                for x in 0..lay.extent.width as usize {
                    let gx = tx as usize * t + x;
                    if gx >= w {
                        break;
                    }
                    let i = y * stride + x;
                    let o = (gy * w + gx) * channels;
                    for c in 0..channels {
                        out[o + c] = buf[c * n + i];
                    }
                }
            }
        }
    }
    Ok(out)
}

/// A mask as JSON (`0`, `1`, or three decimals), without a `serde_json::Value`.
fn mask_json(mask: impl Iterator<Item = f32>, len: usize) -> String {
    let mut s = String::with_capacity(len * 2 + 2);
    s.push('[');
    for (i, v) in mask.enumerate() {
        if i > 0 {
            s.push(',');
        }
        let v = v.clamp(0.0, 1.0);
        if v == 0.0 {
            s.push('0');
        } else if v == 1.0 {
            s.push('1');
        } else {
            let _ = write!(s, "{v:.3}");
        }
    }
    s.push(']');
    s
}

/// A params object from the caller (`{}` when empty).
fn params_object(json: &str) -> Result<serde_json::Map<String, serde_json::Value>> {
    let json = if json.trim().is_empty() { "{}" } else { json };
    match serde_json::from_str::<serde_json::Value>(json)
        .map_err(|e| failure(format!("params JSON: {e}")))?
    {
        serde_json::Value::Object(m) => Ok(m),
        _ => Err(failure("params must be a JSON object")),
    }
}

// ─────────────────────────────── session calls ───────────────────────────────

impl DocumentSession {
    /// State, canvas, the target layer and the selection, checked for retouching.
    fn retouch_target(&self, layer: u64) -> Result<(Arc<DocState>, Option<Arc<Raster>>)> {
        let st = self.shared.lock()?;
        st.open()?;
        let s = st.live().state().clone();
        let l = find(&s, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_) | LayerKind::SmartObject(_)) {
            return Err(failure(
                "retouching applies to pixel layers and smart objects",
            ));
        }
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let sel = s.selection.clone();
        Ok((s, sel))
    }

    /// The selection as a canvas mask, or an error when there is none.
    fn selection_mask(s: &DocState, sel: Option<&Raster>) -> Result<Vec<f32>> {
        let sel = sel.ok_or_else(|| failure("make a selection first"))?;
        let m = read_all(sel, s.canvas, 1)?;
        if m.iter().all(|&v| v <= 0.0) {
            return Err(failure("the selection is empty"));
        }
        Ok(m)
    }

    /// Resolves the backend request: the adapter's `backend` value, what will
    /// run, and a note when it differs from the request.
    fn remove_backend(
        &self,
        backend: RemoveBackend,
    ) -> Result<(&'static str, &'static str, Option<String>)> {
        match backend {
            RemoveBackend::PatchMatch => Ok(("cpu", "PatchMatch", None)),
            RemoveBackend::Lama => {
                let reg = registry_for(&self.shared, LAMA)?;
                self.load_cached(&reg, LAMA, "remove", "Remove with LaMa")?;
                Ok(("onnx", "LaMa", None))
            }
            RemoveBackend::Auto => {
                if LAMA_LOADED.load(Ordering::Relaxed) {
                    return Ok(("auto", "LaMa", None));
                }
                let reg = registry_for(&self.shared, LAMA)?;
                if Self::installed(&reg, LAMA) {
                    self.load_cached(&reg, LAMA, "remove", "Remove with LaMa")?;
                    Ok(("auto", "LaMa", None))
                } else {
                    Ok((
                        "cpu",
                        "PatchMatch",
                        Some(format!(
                            "LaMa ({LAMA}) is not installed, so Auto used PatchMatch"
                        )),
                    ))
                }
            }
        }
    }

    /// `apply_raster_filter` with a mask, relabelled; one history node.
    #[allow(clippy::too_many_arguments)]
    fn apply_masked(
        &self,
        layer: u64,
        operation: super::RasterFilterOperation,
        mask: &[f32],
        key: &str,
        options: serde_json::Value,
        label: &str,
        backend: &str,
        note: Option<String>,
        started: Instant,
    ) -> Result<RetouchResult> {
        let empty = mask.iter().all(|&v| v <= 0.0);
        let params = format!(
            "{{\"mask\":{},\"{key}\":{}}}",
            mask_json(mask.iter().copied(), mask.len()),
            options
        );
        let mut update = self.apply_raster_filter(
            layer,
            super::RasterFilterRequest {
                operation,
                params_json: params,
            },
        )?;
        self.relabel(&mut update, label)?;
        Ok(RetouchResult {
            update,
            backend: if empty { "none".into() } else { backend.into() },
            note,
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Names the head history node (the adapter labels it with its id).
    fn relabel(&self, update: &mut DocumentUpdate, label: &str) -> Result<()> {
        let mut st = self.shared.lock()?;
        st.labels.insert(update.history_head, label.to_owned());
        Ok(())
    }

    fn remove_options(backend: &str, params_json: &str) -> Result<serde_json::Value> {
        let mut p = params_object(params_json)?;
        p.insert("backend".into(), backend.into());
        Ok(serde_json::Value::Object(p))
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Model files retouching can use, and whether each is installed.
    pub fn retouch_models(&self) -> Result<Vec<RetouchModel>> {
        let reg = registry(&self.shared)?;
        let dir = self.cache_dir();
        let extra = extra_cache();
        Ok([
            (LAMA, "Remove (LaMa)"),
            (DDCOLOR, "Colorize (DDColor)"),
            (DRUNET, "JPEG Artifact Removal (DRUNet)"),
            (YUNET, "Face boxes (YuNet)"),
            (SFACE, "Face boxes (SFace)"),
        ]
        .into_iter()
        .filter_map(|(id, used_by)| {
            let s = reg.models().iter().find(|m| m.id == id)?;
            let file = format!("{}.onnx", s.sha256);
            let in_app = Self::installed(&reg, id);
            // A hand-installed extra cache counts too (and is where it is).
            let in_extra = !in_app
                && registry_for(&self.shared, id)
                    .map(|r| Self::installed(&r, id))
                    .unwrap_or(false);
            let at = if in_extra { extra.clone() } else { dir.clone() };
            Some(RetouchModel {
                model_id: id.into(),
                version: s.version.clone(),
                used_by: used_by.into(),
                installed: in_app || in_extra,
                cache_path: at
                    .map(|d| d.join(&file).display().to_string())
                    .unwrap_or_default(),
                source_url: s.download_url.clone(),
            })
        })
        .collect())
    }

    /// Edit ▸ Content-Aware Fill: fills the selection of `layer` (one node).
    /// `params_json` is the adapter's `fill` object (`{}` for defaults).
    pub fn content_aware_fill_selection(
        &self,
        layer: u64,
        params_json: String,
    ) -> Result<RetouchResult> {
        let started = Instant::now();
        let (s, sel) = self.retouch_target(layer)?;
        let mask = Self::selection_mask(&s, sel.as_deref())?;
        let fill = serde_json::Value::Object(params_object(&params_json)?);
        self.apply_masked(
            layer,
            super::RasterFilterOperation::ContentAwareFill,
            &mask,
            "fill",
            fill,
            "Content-Aware Fill",
            "Content-Aware Fill",
            None,
            started,
        )
    }

    /// Remove inside the selection (one node). `params_json`: the adapter's
    /// Remove options (`dilation`, `fill`); `backend` picks the inpainter.
    pub fn remove_with_selection(
        &self,
        layer: u64,
        backend: RemoveBackend,
        params_json: String,
    ) -> Result<RetouchResult> {
        let started = Instant::now();
        let (s, sel) = self.retouch_target(layer)?;
        let mask = Self::selection_mask(&s, sel.as_deref())?;
        let (b, used, note) = self.remove_backend(backend)?;
        self.apply_masked(
            layer,
            super::RasterFilterOperation::Remove,
            &mask,
            "remove",
            Self::remove_options(b, &params_json)?,
            "Remove",
            used,
            note,
            started,
        )
    }

    /// Starts a Remove stroke on `layer` with a hard round brush of
    /// diameter `size` (canvas pixels). A stroke already open is dropped.
    pub fn begin_remove_stroke(&self, layer: u64, size: f32, backend: RemoveBackend) -> Result<()> {
        if !size.is_finite() || !(1.0..=5000.0).contains(&size) {
            return Err(failure("brush size must be 1…5000 pixels"));
        }
        let (s, _) = self.retouch_target(layer)?;
        let canvas = s.canvas;
        self.retouch(|r| {
            r.stroke = Some(Stroke {
                layer,
                size,
                backend,
                canvas,
                mask: vec![0; canvas.area() as usize],
                last: None,
                carry: 0.0,
                bounds: None,
            })
        });
        Ok(())
    }

    /// Adds points (level-0 canvas pixels) to the Remove stroke; returns the
    /// canvas rectangle the new dabs covered.
    pub fn remove_stroke_points(&self, points: Vec<ToolPoint>) -> Result<Option<DocRect>> {
        self.retouch(|r| {
            let s = r
                .stroke
                .as_mut()
                .ok_or_else(|| failure("no Remove stroke is open"))?;
            let mut dirty = Rect::default();
            for p in points {
                if !p.x.is_finite() || !p.y.is_finite() {
                    return Err(failure("stroke points must be finite"));
                }
                let spacing = (s.size * 0.15).max(0.5);
                match s.last {
                    None => {
                        dirty = dirty.union(&stamp(s, p.x, p.y));
                    }
                    Some((lx, ly)) => {
                        let (dx, dy) = (p.x - lx, p.y - ly);
                        let len = dx.hypot(dy);
                        let mut t = spacing - s.carry;
                        while t <= len {
                            let k = t / len;
                            dirty = dirty.union(&stamp(s, lx + dx * k, ly + dy * k));
                            t += spacing;
                        }
                        s.carry = (s.carry + len) % spacing;
                    }
                }
                s.last = Some((p.x, p.y));
            }
            if !dirty.is_empty() {
                s.bounds = Some(s.bounds.map_or(dirty, |b| b.union(&dirty)));
            }
            Ok(DocRect::of(dirty))
        })
    }

    /// Bounds of the open Remove stroke's mask (canvas pixels).
    pub fn remove_stroke_bounds(&self) -> Option<DocRect> {
        self.retouch(|r| {
            r.stroke
                .as_ref()
                .and_then(|s| s.bounds)
                .and_then(DocRect::of)
        })
    }

    /// Drops the open Remove stroke (no history).
    pub fn cancel_remove_stroke(&self) {
        self.retouch(|r| r.stroke = None);
    }

    /// Removes what the stroke covered (inside the selection, if any): one
    /// node. `params_json` as for `remove_with_selection`. Blocking.
    pub fn end_remove_stroke(&self, params_json: String) -> Result<RetouchResult> {
        let started = Instant::now();
        let stroke = self
            .retouch(|r| r.stroke.take())
            .ok_or_else(|| failure("no Remove stroke is open"))?;
        if stroke.bounds.is_none() {
            return Err(failure("the Remove stroke covered nothing"));
        }
        let (s, sel) = self.retouch_target(stroke.layer)?;
        if s.canvas != stroke.canvas {
            return Err(failure("the canvas changed during the stroke"));
        }
        let mut mask: Vec<f32> = stroke.mask.iter().map(|&m| f32::from(m)).collect();
        if let Some(sel) = sel.as_deref() {
            let sm = read_all(sel, s.canvas, 1)?;
            for (m, k) in mask.iter_mut().zip(sm) {
                *m *= k.clamp(0.0, 1.0);
            }
        }
        let (b, used, note) = self.remove_backend(stroke.backend)?;
        self.apply_masked(
            stroke.layer,
            super::RasterFilterOperation::Remove,
            &mask,
            "remove",
            Self::remove_options(b, &params_json)?,
            "Remove",
            used,
            note,
            started,
        )
    }

    /// Runs the geometric distraction detector on `layer` and keeps its
    /// suggestions for review (no history). `params_json`: `{"wires":bool,
    /// "faces_as_people":bool,"faces":[[x,y,w,h]]}`, all optional; without
    /// `faces`, the face detector's boxes when its weights are installed.
    pub fn detect_distractions(&self, layer: u64, params_json: String) -> Result<DistractionScan> {
        use filters::distraction::DistractionDetector;
        let p = params_object(&params_json)?;
        for k in p.keys() {
            if !matches!(k.as_str(), "wires" | "faces_as_people" | "faces") {
                return Err(failure(format!("distraction params: unknown field {k}")));
            }
        }
        let wires = p.get("wires").and_then(|v| v.as_bool()).unwrap_or(true);
        let people = p
            .get("faces_as_people")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let (s, _) = self.retouch_target(layer)?;
        let l = find(&s, layer)?;
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure(
                "Remove Distractions scans pixel layers; rasterize the smart object first",
            ));
        };
        let revision = super::layer_revision(l);
        let px = read_all(raster, s.canvas, 4)?;
        let (faces, source) = if !people {
            (Vec::new(), "not used".to_owned())
        } else {
            match p.get("faces") {
                Some(v) => (
                    serde_json::from_value::<Vec<[f32; 4]>>(v.clone())
                        .map_err(|e| failure(format!("faces: {e}")))?,
                    "supplied".to_owned(),
                ),
                None => match self.detect_faces(&px, s.canvas) {
                    Ok(f) => {
                        let n = f.len();
                        (f, format!("face detector ({n} found)"))
                    }
                    Err(e) => (Vec::new(), format!("none: {e}")),
                },
            }
        };
        let input = raster_from_rgba(s.canvas, compositor::Depth::F32, &px, false)?;
        let faces: Vec<ml_faces::Face> = faces
            .into_iter()
            .map(|bbox| ml_faces::Face {
                bbox,
                score: 1.0,
                landmarks5: [[0.0; 2]; 5],
            })
            .collect();
        let masks = filters::distraction::CpuDistractionDetector.detect(
            &input,
            &faces,
            &AtomicBool::new(false),
        )?;
        let w = s.canvas.width as usize;
        let mut pixels = Vec::new();
        let mut suggestions = Vec::new();
        let groups = [
            (
                DistractionKind::Wire,
                if wires { &masks.wires } else { &Vec::new() },
            ),
            (DistractionKind::FaceBox, &masks.people),
        ];
        for (kind, m) in groups {
            if m.is_empty() {
                continue;
            }
            for comp in components(m, w) {
                let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
                for &i in &comp {
                    let (x, y) = (i as usize % w, i as usize / w);
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x + 1);
                    y1 = y1.max(y + 1);
                }
                let id = pixels.len() as u32;
                suggestions.push(DistractionSuggestion {
                    id,
                    kind,
                    bounds: DocRect {
                        x: x0 as i64,
                        y: y0 as i64,
                        width: (x1 - x0) as i64,
                        height: (y1 - y0) as i64,
                    },
                    pixels: comp.len() as u64,
                });
                pixels.push(comp);
            }
        }
        let canvas = s.canvas;
        self.retouch(|r| {
            r.suggestions = Some(Suggestions {
                layer,
                revision,
                canvas,
                pixels,
            })
        });
        Ok(DistractionScan {
            suggestions,
            faces: source,
            limitation: "Geometric proxies: thin straight ridges for wires and dilated face \
                         boxes for people. Not semantic or person segmentation."
                .into(),
        })
    }

    /// Removes the accepted suggestions of the last scan (one node).
    pub fn remove_distraction_suggestions(
        &self,
        layer: u64,
        accepted: Vec<u32>,
        backend: RemoveBackend,
        params_json: String,
    ) -> Result<RetouchResult> {
        let started = Instant::now();
        if accepted.is_empty() {
            return Err(failure("no suggestion was accepted"));
        }
        let (s, _) = self.retouch_target(layer)?;
        let revision = super::layer_revision(find(&s, layer)?);
        let mask = self.retouch(|r| {
            let sg = r
                .suggestions
                .as_ref()
                .ok_or_else(|| failure("scan for distractions first"))?;
            if sg.layer != layer || sg.revision != revision || sg.canvas != s.canvas {
                return Err(failure(
                    "the layer changed since the scan; scan for distractions again",
                ));
            }
            let mut mask = vec![0.0f32; s.canvas.area() as usize];
            for id in &accepted {
                let px = sg
                    .pixels
                    .get(*id as usize)
                    .ok_or_else(|| failure(format!("no suggestion {id}")))?;
                for &i in px {
                    mask[i as usize] = 1.0;
                }
            }
            Ok(mask)
        })?;
        if s.selection.is_some() {
            return Err(failure(
                "Remove Distractions works without a selection; deselect first",
            ));
        }
        let (b, used, note) = self.remove_backend(backend)?;
        let r = self.apply_masked(
            layer,
            super::RasterFilterOperation::Remove,
            &mask,
            "remove",
            Self::remove_options(b, &params_json)?,
            "Remove Distractions",
            used,
            note,
            started,
        )?;
        self.retouch(|r| r.suggestions = None);
        Ok(r)
    }

    /// Forgets the suggestions of the last scan.
    pub fn clear_distraction_suggestions(&self) {
        self.retouch(|r| r.suggestions = None);
    }

    /// Applies a neural filter (one node). `params_json` uses the keys of
    /// `neural_filters()`; Skin Smoothing's `faces` default to the face
    /// detector's boxes (installed weights) or the selection's bounds.
    pub fn neural_filter(
        &self,
        layer: u64,
        kind: NeuralFilterKind,
        params_json: String,
        destination: NeuralDestination,
    ) -> Result<RetouchResult> {
        let started = Instant::now();
        let cancel = self.arm_cancel();
        let (s, sel) = self.retouch_target(layer)?;
        let l = find(&s, layer)?;
        let mut params = params_object(&params_json)?;
        let mut note = None;
        let backend = match kind {
            NeuralFilterKind::SkinSmoothing => "Skin Smoothing (CPU)",
            NeuralFilterKind::Colorize => "DDColor",
            NeuralFilterKind::JpegArtifactRemoval => "DRUNet",
        };
        // Weights first: a missing model is the clear error, before any work.
        match kind {
            NeuralFilterKind::Colorize => {
                let reg = registry_for(&self.shared, DDCOLOR)?;
                self.load_cached(&reg, DDCOLOR, "neural/colorize", "Colorize")?;
            }
            NeuralFilterKind::JpegArtifactRemoval => {
                let reg = registry_for(&self.shared, DRUNET)?;
                self.load_cached(
                    &reg,
                    DRUNET,
                    "neural/jpeg_artifact_removal",
                    "JPEG Artifact Removal",
                )?;
            }
            NeuralFilterKind::SkinSmoothing => {
                if !params.contains_key("faces") {
                    let (faces, how) = self.default_faces(&s, l, sel.as_deref())?;
                    params.insert(
                        "faces".into(),
                        serde_json::to_value(faces).map_err(failure)?,
                    );
                    note = Some(how);
                }
            }
        }
        check(&cancel)?;
        let id = neural_id(kind);
        let name = neural_name(kind);
        let params = serde_json::Value::Object(params);
        let is_smart = matches!(l.kind, LayerKind::SmartObject(_));
        let mut update = match (destination, is_smart) {
            (NeuralDestination::CurrentLayer, _) | (NeuralDestination::SmartFilter, true) => {
                let op = match kind {
                    NeuralFilterKind::SkinSmoothing => super::RasterFilterOperation::SkinSmoothing,
                    NeuralFilterKind::Colorize => super::RasterFilterOperation::Colorize,
                    NeuralFilterKind::JpegArtifactRemoval => {
                        super::RasterFilterOperation::JpegArtifactRemoval
                    }
                };
                self.apply_raster_filter(
                    layer,
                    super::RasterFilterRequest {
                        operation: op,
                        params_json: params.to_string(),
                    },
                )?
            }
            (NeuralDestination::NewLayer, true) => {
                return Err(failure(
                    "New layer output needs a pixel layer; use Smart filter for smart objects",
                ));
            }
            (NeuralDestination::NewLayer, false) => {
                self.neural_new_layer(&s, layer, id, name, &params, sel.as_deref(), &cancel)?
            }
            (NeuralDestination::SmartFilter, false) => {
                if sel.is_some() {
                    return Err(failure(
                        "Smart filter output works without a selection; deselect first",
                    ));
                }
                self.neural_smart_from_pixels(&s, layer, id, name, &params, &cancel)?
            }
        };
        self.relabel(&mut update, name)?;
        Ok(RetouchResult {
            update,
            backend: backend.into(),
            note,
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Cancels a running retouch or neural apply (and any filter apply).
    pub fn cancel_retouch(&self) {
        self.retouch(|r| r.cancel.store(true, Ordering::Relaxed));
        self.cancel_filter();
    }
}

/// Hard round dab: pixels whose centre lies within the radius (the brush's
/// `round_coverage` at hardness 1 is ≥ ½ exactly there).
fn stamp(s: &mut Stroke, cx: f32, cy: f32) -> Rect {
    let r = s.size * 0.5;
    let (w, h) = (s.canvas.width as i64, s.canvas.height as i64);
    let x0 = ((cx - r - 1.0).floor() as i64).clamp(0, w);
    let x1 = ((cx + r + 1.0).ceil() as i64).clamp(0, w);
    let y0 = ((cy - r - 1.0).floor() as i64).clamp(0, h);
    let y1 = ((cy + r + 1.0).ceil() as i64).clamp(0, h);
    let mut out = Rect::default();
    for y in y0..y1 {
        for x in x0..x1 {
            let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy);
            if brush::tip::round_coverage(d, r, 1.0) >= 0.5 {
                s.mask[(y * w + x) as usize] = 1;
                out = out.union(&Rect::new(x, y, x + 1, y + 1));
            }
        }
    }
    out
}

/// 8-connected components of the nonzero pixels of `mask` (row width `w`).
fn components(mask: &[f32], w: usize) -> Vec<Vec<u32>> {
    let h = mask.len() / w.max(1);
    let mut seen = vec![false; mask.len()];
    let mut out = Vec::new();
    for seed in 0..mask.len() {
        if seen[seed] || mask[seed] <= 0.0 {
            continue;
        }
        seen[seed] = true;
        let mut comp = vec![seed as u32];
        let mut next = 0;
        while next < comp.len() {
            let i = comp[next] as usize;
            next += 1;
            let (x, y) = (i % w, i / w);
            for yy in y.saturating_sub(1)..=(y + 1).min(h - 1) {
                for xx in x.saturating_sub(1)..=(x + 1).min(w - 1) {
                    let j = yy * w + xx;
                    if !seen[j] && mask[j] > 0.0 {
                        seen[j] = true;
                        comp.push(j as u32);
                    }
                }
            }
        }
        out.push(comp);
    }
    out
}

impl DocumentSession {
    /// Face boxes from the face detector (cached weights only).
    fn detect_faces(&self, rgba: &[f32], canvas: Extent) -> Result<Vec<[f32; 4]>> {
        let reg = registry_for(&self.shared, YUNET)?;
        self.require(&reg, YUNET, "Finding faces")?;
        self.require(&reg, SFACE, "Finding faces")?;
        let mut models = ml_faces::FaceModels::load(&reg, ml_runtime::SessionOptions::default())
            .map_err(|e| failure(format!("face detector: {e}")))?;
        let img = image::RgbImage::from_fn(canvas.width, canvas.height, |x, y| {
            let i = (y as usize * canvas.width as usize + x as usize) * 4;
            let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            image::Rgb([q(rgba[i]), q(rgba[i + 1]), q(rgba[i + 2])])
        });
        Ok(models
            .detect(&img)
            .map_err(|e| failure(format!("face detector: {e}")))?
            .into_iter()
            .map(|f| f.bbox)
            .collect())
    }

    /// Skin Smoothing without explicit faces: the detector, else the selection.
    fn default_faces(
        &self,
        s: &DocState,
        l: &Layer,
        sel: Option<&Raster>,
    ) -> Result<(Vec<[f32; 4]>, String)> {
        let detector = match &l.kind {
            LayerKind::Pixel(r) => {
                read_all(r, s.canvas, 4).and_then(|px| self.detect_faces(&px, s.canvas))
            }
            _ => Err(failure("face detection reads pixel layers")),
        };
        match detector {
            Ok(f) if !f.is_empty() => {
                let n = f.len();
                Ok((
                    f,
                    format!(
                        "{n} face{} found by the face detector",
                        if n == 1 { "" } else { "s" }
                    ),
                ))
            }
            other => {
                if let Some(b) = sel.and_then(super::io::selection_bounds) {
                    return Ok((
                        vec![[
                            b.x0 as f32,
                            b.y0 as f32,
                            b.width() as f32,
                            b.height() as f32,
                        ]],
                        "the selection's bounds used as the face box".into(),
                    ));
                }
                let why = match other {
                    Ok(_) => "the face detector found no face".to_owned(),
                    Err(e) => e.to_string(),
                };
                Err(failure(format!(
                    "Skin Smoothing needs face boxes. {why}. Select a face (any selection tool) \
                     to use its bounds instead"
                )))
            }
        }
    }

    /// The adapter's output for the layer's pixels (full canvas, F32).
    fn evaluate_neural(
        &self,
        s: &DocState,
        raster: &Raster,
        id: &str,
        params: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<Vec<f32>> {
        let px = read_all(raster, s.canvas, 4)?;
        check(cancel)?;
        let input = raster_from_rgba(s.canvas, compositor::Depth::F32, &px, false)?;
        let out = filters::CompositorFilters.evaluate(
            &input,
            &SmartFilter {
                name: id.into(),
                enabled: true,
                blend: FilterBlend::default(),
                params: params.clone(),
            },
            &FilterContext {
                profile: s.profile.clone(),
                canvas: s.canvas,
                level: 0,
            },
        )?;
        check(cancel)?;
        read_all(&out, s.canvas, 4)
    }

    /// A new pixel layer above `layer` holding the filtered pixels (inside
    /// the selection, if any): one node.
    #[allow(clippy::too_many_arguments)]
    fn neural_new_layer(
        &self,
        s: &DocState,
        layer: u64,
        id: &str,
        name: &str,
        params: &serde_json::Value,
        sel: Option<&Raster>,
        cancel: &AtomicBool,
    ) -> Result<DocumentUpdate> {
        let l = find(s, layer)?;
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("not a pixel layer"));
        };
        let mut px = self.evaluate_neural(s, raster, id, params, cancel)?;
        if let Some(sel) = sel {
            let m = read_all(sel, s.canvas, 1)?;
            for (p, k) in px.as_chunks_mut::<4>().0.iter_mut().zip(m) {
                p[3] *= k.clamp(0.0, 1.0);
            }
        }
        let out = raster_from_rgba(s.canvas, s.depth, &px, true)?;
        let mut nl = Layer::new(format!("{} ({name})", l.props.name), LayerKind::Pixel(out));
        nl.props.visible = true;
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        self.commit_op(
            s,
            DocOp::AddLayer {
                parent,
                index: index + 1,
                layer: nl,
            },
            name,
            false,
        )
    }

    /// The pixel layer as a smart object with the neural filter (validated
    /// by evaluating it first): one node.
    fn neural_smart_from_pixels(
        &self,
        s: &DocState,
        layer: u64,
        id: &str,
        name: &str,
        params: &serde_json::Value,
        cancel: &AtomicBool,
    ) -> Result<DocumentUpdate> {
        let l = find(s, layer)?;
        let LayerKind::Pixel(raster) = &l.kind else {
            return Err(failure("not a pixel layer"));
        };
        self.evaluate_neural(s, raster, id, params, cancel)?;
        // Filter ▸ Convert for Smart Filters (same construction), plus the filter.
        let mut child = DocState::new(s.canvas, s.depth);
        child.profile = s.profile.clone();
        let mut inner = l.clone();
        inner.props = compositor::LayerProps {
            name: l.props.name.clone(),
            ..Default::default()
        };
        inner.mask = None;
        inner.id = LayerId(1);
        child.next_id = 2;
        child.root = vec![Arc::new(inner)];
        let mut so = SmartObject::new(child, Affine::IDENTITY);
        so.filters.push(SmartFilter {
            name: id.into(),
            enabled: true,
            blend: FilterBlend::default(),
            params: params.clone(),
        });
        let mut outer = Layer::new(l.props.name.clone(), LayerKind::SmartObject(so));
        outer.id = l.id;
        outer.props = l.props.clone();
        outer.props.background = false;
        outer.mask = l.mask.clone();
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        self.commit_op(
            s,
            DocOp::Batch(vec![
                DocOp::RemoveLayer { id: LayerId(layer) },
                DocOp::AddLayer {
                    parent,
                    index,
                    layer: outer,
                },
            ]),
            name,
            true,
        )
    }

    /// One history node, provided the document is still `base`.
    fn commit_op(
        &self,
        base: &DocState,
        op: DocOp,
        label: &str,
        replaced: bool,
    ) -> Result<DocumentUpdate> {
        let _ = self.clear_preview();
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        self.commit_pending(&mut st, None)?;
        if st.doc.state().rev != base.rev {
            return Err(failure(
                "the document changed while the filter ran; apply it again",
            ));
        }
        let applied = st.doc.apply(op)?;
        st.labels.insert(applied.node, label.to_owned());
        let mut u = self.update(&mut st, &before, Some(&applied), true);
        if replaced {
            u.created.clear();
        }
        Ok(u)
    }
}

/// Test support (not exported over UniFFI).
impl DocumentSession {
    /// The open Remove stroke's mask (1 = remove), canvas-sized.
    #[doc(hidden)]
    pub fn remove_stroke_mask(&self) -> Option<Vec<f32>> {
        self.retouch(|r| {
            r.stroke
                .as_ref()
                .map(|s| s.mask.iter().map(|&m| f32::from(m)).collect())
        })
    }
}
