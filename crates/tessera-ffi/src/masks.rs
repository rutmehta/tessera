//! Masking (M2-14): local-adjustment groups on the develop session.
//!
//! # Model
//!
//! Masks are the recipe's procedural `settings.locals.adjustments`
//! ([`LocalAdjustment`] groups of [`MaskComponent`]s). Every edit here changes
//! the session's *live* settings and renders like a slider does; the host
//! calls `commit(label)` to make it one undo step (a whole brush stroke or a
//! gradient drag is one step). Coordinates in this API are normalized to the
//! full, uncropped image in **sensor orientation** (the recipe's mask space);
//! the host maps loupe points through the crop and EXIF orientation.
//!
//! # Rendering
//!
//! Procedural components (linear, radial, brush, luminance and colour range)
//! rasterize in `pipeline-cpu`. AI components (subject, sky, background,
//! object/person prompts) are computed by `ml-segment` in a background job
//! ([`AiMaskJob`]) on a display-oriented sRGB render of the as-shot image and
//! kept per session in sensor orientation. The session's renderer owns its
//! mask raster cache, and [`Hooks`] (image-core [`MaskHooks`]) composes AI
//! rasters with the procedural components inside it, so AI groups are cached
//! and blended exactly like procedural ones. Until a raster is ready the
//! component selects nothing.
//!
//! The hooks also *observe* every group raster the renderer uses, so the loupe
//! overlay and the panel thumbnails come from the render itself (no second
//! rasterization). The overlay is written as an 8-bit alpha plane into
//! host-owned R8 IOSurfaces after each frame, through the frame's crop and
//! straighten, anchored top-left like the frame (see `MaskListener`).
//!
//! Local adjustments render on the whole-level M2 chain (Heavy render class),
//! so local slider drags adapt their level like the heavy panels; mask rasters
//! are cached independently of the local sliders.

use super::*;
use crate::surface::Surface;
use engine_api::{
    id::{MaskId, ModelRef},
    recipe::{
        mask::{BrushStroke, LocalAdjustment, LocalParams, MaskCombine, MaskComponent, MaskKind},
        settings::{LocalsSettings, NormalizedRect},
    },
};
use image::RgbImage;
use image_core::mask_cache::MaskHooks;
use std::collections::{HashMap, VecDeque};

// ─────────────────────────────── records ───────────────────────────────

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaskCombineMode {
    Add,
    Subtract,
    Intersect,
}

impl From<MaskCombineMode> for MaskCombine {
    fn from(m: MaskCombineMode) -> Self {
        match m {
            MaskCombineMode::Add => Self::Add,
            MaskCombineMode::Subtract => Self::Subtract,
            MaskCombineMode::Intersect => Self::Intersect,
        }
    }
}

impl From<MaskCombine> for MaskCombineMode {
    fn from(m: MaskCombine) -> Self {
        match m {
            MaskCombine::Add => Self::Add,
            MaskCombine::Subtract => Self::Subtract,
            MaskCombine::Intersect => Self::Intersect,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MaskComponentType {
    Subject,
    Sky,
    Background,
    Person,
    Object,
    Landscape,
    Depth,
    Linear,
    Radial,
    Brush,
    LuminanceRange,
    ColorRange,
}

/// State of an AI component's raster in this session.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AiMaskState {
    /// Procedural component.
    NotAi,
    /// Queued or computing.
    Pending {
        fraction: f32,
        message: String,
    },
    Ready,
    Failed {
        message: String,
    },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MaskComponentInfo {
    pub kind: MaskComponentType,
    pub combine: MaskCombineMode,
    pub invert: bool,
    /// "Subject", "Brush (3 strokes)", "Linear Gradient", …
    pub title: String,
    /// The component's `MaskKind` as recipe JSON (geometry for the loupe handles).
    pub definition_json: String,
    pub ai: AiMaskState,
    /// Identity of the AI raster (matches `MaskJobUpdate::key`), if AI.
    pub ai_key: Option<String>,
    /// False when this pipeline version keeps but does not draw the component.
    pub rendered: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LocalParamValue {
    pub name: String,
    pub value: f32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MaskGroupInfo {
    pub id: u32,
    pub name: String,
    pub enabled: bool,
    /// Scales every local slider, `0..=200` %.
    pub amount: f32,
    pub invert: bool,
    pub components: Vec<MaskComponentInfo>,
    /// The editable local sliders ([`LOCAL_PARAMS`] order).
    pub params: Vec<LocalParamValue>,
}

/// Group fields to change; `None` leaves a field as it is.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct MaskGroupPatch {
    pub name: Option<String>,
    pub enabled: Option<bool>,
    pub amount: Option<f32>,
    pub invert: Option<bool>,
}

/// A brush sample in mask space (sensor-oriented, normalized), pressure 0..=1.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct BrushPoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct BrushSettings {
    /// Radius normalized to the image width (sensor orientation).
    pub radius: f32,
    /// 0..=100.
    pub feather: f32,
    /// 0..=100.
    pub flow: f32,
    pub erase: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct MaskPoint {
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct MaskRect {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RangeKind {
    Color,
    Luminance,
}

/// What an AI mask selects. Boxes and points are in mask space.
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum AiMaskRequest {
    Subject,
    Sky,
    Background,
    /// A person from a face box: expanded to a whole-person box and segmented
    /// with the promptable model (not part parsing or identity).
    Person {
        face: MaskRect,
    },
    /// An object from positive clicks and/or a box (promptable segmentation).
    Object {
        points: Vec<MaskPoint>,
        region: Option<MaskRect>,
    },
}

/// A group's mask for the panel: 8-bit alpha, display orientation, uncropped.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MaskThumbnail {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
}

/// An overlay plane written into a host R8 surface for the frame at `level`.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MaskOverlayFrame {
    pub surface_id: u32,
    pub group_id: u32,
    pub level: u8,
    /// Valid region, anchored top-left (the frame's cropped extent).
    pub width: u32,
    pub height: u32,
    /// Generation of the frame it belongs to.
    pub generation: u64,
    /// Mean alpha, 0..=1 (diagnostics, tests).
    pub coverage: f32,
}

/// Progress of an AI mask computation.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MaskJobUpdate {
    pub key: String,
    pub title: String,
    pub fraction: f32,
    pub message: String,
    pub done: bool,
    pub error: Option<String>,
}

/// Mask callbacks, on engine worker threads.
#[uniffi::export(with_foreign)]
pub trait MaskListener: Send + Sync {
    fn overlay_ready(&self, frame: MaskOverlayFrame);
    fn ai_progress(&self, update: MaskJobUpdate);
}

/// The local sliders, in panel order (`LocalParams` JSON member names).
pub const LOCAL_PARAMS: [&str; 16] = [
    "temperature",
    "tint",
    "exposure",
    "contrast",
    "highlights",
    "shadows",
    "whites",
    "blacks",
    "texture",
    "clarity",
    "dehaze",
    "hue",
    "saturation",
    "sharpness",
    "noise",
    "moire",
];

// ─────────────────────────────── sanitizing ───────────────────────────────

/// Local adjustments this pipeline draws, sanitized so a recipe cannot fail a
/// render. Person, landscape and depth components (no model or depth plane
/// here) and defringe / colour overlay are kept in the recipe but not drawn;
/// retouch operations are not drawn.
pub(crate) fn renderable_locals(s: &LocalsSettings) -> LocalsSettings {
    LocalsSettings {
        retouch: Vec::new(),
        adjustments: s.adjustments.iter().map(renderable_group).collect(),
    }
}

fn renderable_group(g: &LocalAdjustment) -> LocalAdjustment {
    let mut r = g.clone();
    r.amount = finite_or(g.amount, 100.0).clamp(0.0, 200.0);
    let p = &mut r.params;
    p.exposure = finite_or(p.exposure, 0.0).clamp(-5.0, 5.0);
    for v in [
        &mut p.contrast,
        &mut p.highlights,
        &mut p.shadows,
        &mut p.whites,
        &mut p.blacks,
        &mut p.temperature,
        &mut p.tint,
        &mut p.saturation,
        &mut p.texture,
        &mut p.clarity,
        &mut p.dehaze,
        &mut p.sharpness,
        &mut p.noise,
        &mut p.moire,
    ] {
        *v = finite_or(*v, 0.0).clamp(-100.0, 100.0);
    }
    p.hue = finite_or(p.hue, 0.0).clamp(-180.0, 180.0);
    p.defringe = 0.0;
    p.color_overlay = None;
    r.components = g
        .components
        .iter()
        .filter_map(renderable_component)
        .collect();
    r
}

fn coord(v: f32) -> f32 {
    finite_or(v, 0.0).clamp(-16.0, 16.0)
}

fn renderable_component(c: &MaskComponent) -> Option<MaskComponent> {
    let kind = match &c.kind {
        k @ (MaskKind::Subject { .. } | MaskKind::Sky { .. } | MaskKind::Background { .. }) => {
            k.clone()
        }
        MaskKind::Object { region, points, .. } => {
            let prompts = object_prompts(region.as_ref(), points)?;
            if prompts.0.is_empty() && prompts.1.is_empty() {
                return None;
            }
            c.kind.clone()
        }
        MaskKind::Person { .. } | MaskKind::Landscape { .. } | MaskKind::Depth { .. } => {
            return None;
        }
        MaskKind::Linear { start, end } => {
            let (start, end) = (start.map(coord), end.map(coord));
            if (end[0] - start[0]).hypot(end[1] - start[1]) < 1e-4 {
                return None;
            }
            MaskKind::Linear { start, end }
        }
        MaskKind::Radial {
            center,
            radii,
            angle,
            feather,
        } => MaskKind::Radial {
            center: center.map(coord),
            radii: radii.map(|r| finite_or(r, 0.1).clamp(1e-4, 16.0)),
            angle: finite_or(*angle, 0.0).rem_euclid(360.0),
            feather: unit(*feather, 50.0),
        },
        MaskKind::Brush { strokes } => {
            let strokes: Vec<BrushStroke> = strokes
                .iter()
                .filter_map(|s| {
                    let points: Vec<[f32; 3]> = s
                        .points
                        .iter()
                        .filter(|p| p.iter().all(|v| v.is_finite()))
                        .map(|p| [coord(p[0]), coord(p[1]), p[2].clamp(0.0, 1.0)])
                        .collect();
                    (!points.is_empty()).then(|| BrushStroke {
                        points,
                        radius: finite_or(s.radius, 0.02).clamp(1e-4, 1.0),
                        feather: unit(s.feather, 50.0),
                        flow: unit(s.flow, 100.0),
                        erase: s.erase,
                    })
                })
                .collect();
            if strokes.is_empty() {
                return None;
            }
            MaskKind::Brush { strokes }
        }
        MaskKind::LuminanceRange { range, smoothness } => {
            let lo = finite_or(range[0], 0.0).clamp(0.0, 1.0);
            let hi = finite_or(range[1], 1.0).clamp(lo, 1.0);
            MaskKind::LuminanceRange {
                range: [lo, hi],
                smoothness: unit(*smoothness, 50.0),
            }
        }
        MaskKind::ColorRange { samples, amount } => {
            let samples: Vec<[f32; 3]> = samples
                .iter()
                .filter(|s| s.iter().all(|v| v.is_finite()))
                .copied()
                .collect();
            if samples.is_empty() {
                return None;
            }
            MaskKind::ColorRange {
                samples,
                amount: unit(*amount, 20.0),
            }
        }
    };
    Some(MaskComponent {
        kind,
        combine: c.combine,
        invert: c.invert,
    })
}

/// Valid clicks and boxes of an object component, `None` if any is invalid.
#[allow(clippy::type_complexity)]
fn object_prompts(
    region: Option<&NormalizedRect>,
    points: &[[f32; 2]],
) -> Option<(Vec<[f32; 2]>, Vec<[f32; 4]>)> {
    let unit = |v: f32| v.is_finite() && (0.0..=1.0).contains(&v);
    if !points.iter().flatten().all(|&v| unit(v)) || points.len() > 256 {
        return None;
    }
    let boxes = match region {
        Some(r) => {
            let b = [r.left, r.top, r.right, r.bottom];
            if !b.iter().all(|&v| unit(v)) || b[0] >= b[2] || b[1] >= b[3] {
                return None;
            }
            vec![b]
        }
        None => Vec::new(),
    };
    Some((points.to_vec(), boxes))
}

/// `locals` for a 1:1 window `(x, y, w, h)` of the level-0 `extent`: mask
/// coordinates re-normalized to the window (exact for gradients, brushes and
/// ranges; a rotated radial is approximated when the window's aspect differs).
/// Groups with AI components are left out: their rasters cover the whole frame.
pub(crate) fn window_locals(
    locals: &LocalsSettings,
    extent: engine_api::tile::Extent,
    (wx, wy, ww, wh): (u32, u32, u32, u32),
) -> LocalsSettings {
    let (sx, sy) = (
        extent.width as f32 / ww.max(1) as f32,
        extent.height as f32 / wh.max(1) as f32,
    );
    let (ox, oy) = (wx as f32 / ww.max(1) as f32, wy as f32 / wh.max(1) as f32);
    let map = |p: [f32; 2]| [p[0] * sx - ox, p[1] * sy - oy];
    let mut out = locals.clone();
    out.adjustments
        .retain(|g| !g.components.iter().any(|c| c.kind.is_ai()));
    for g in &mut out.adjustments {
        for c in &mut g.components {
            match &mut c.kind {
                MaskKind::Linear { start, end } => {
                    let d = [end[0] - start[0], end[1] - start[1]];
                    // Keep (p - s)·d / |d|² invariant under the anisotropic map.
                    let u = [d[0] / sx, d[1] / sy];
                    let k = (d[0] * d[0] + d[1] * d[1]) / (u[0] * u[0] + u[1] * u[1]).max(1e-12);
                    let s = map(*start);
                    *start = s;
                    *end = [s[0] + k * u[0], s[1] + k * u[1]];
                }
                MaskKind::Radial { center, radii, .. } => {
                    *center = map(*center);
                    *radii = [radii[0] * sx, radii[1] * sy];
                }
                MaskKind::Brush { strokes } => {
                    for s in strokes {
                        s.radius *= sx;
                        for p in &mut s.points {
                            let m = map([p[0], p[1]]);
                            (p[0], p[1]) = (m[0], m[1]);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    out
}

// ─────────────────────────────── descriptions ───────────────────────────────

fn component_type(k: &MaskKind) -> MaskComponentType {
    match k {
        MaskKind::Subject { .. } => MaskComponentType::Subject,
        MaskKind::Sky { .. } => MaskComponentType::Sky,
        MaskKind::Background { .. } => MaskComponentType::Background,
        MaskKind::Person { .. } => MaskComponentType::Person,
        MaskKind::Object { .. } => MaskComponentType::Object,
        MaskKind::Landscape { .. } => MaskComponentType::Landscape,
        MaskKind::Depth { .. } => MaskComponentType::Depth,
        MaskKind::Linear { .. } => MaskComponentType::Linear,
        MaskKind::Radial { .. } => MaskComponentType::Radial,
        MaskKind::Brush { .. } => MaskComponentType::Brush,
        MaskKind::LuminanceRange { .. } => MaskComponentType::LuminanceRange,
        MaskKind::ColorRange { .. } => MaskComponentType::ColorRange,
    }
}

fn component_title(k: &MaskKind) -> String {
    match k {
        MaskKind::Subject { .. } => "Subject".into(),
        MaskKind::Sky { .. } => "Sky".into(),
        MaskKind::Background { .. } => "Background".into(),
        MaskKind::Person { .. } => "Person".into(),
        MaskKind::Object { prompt, .. } if prompt.as_deref() == Some(PERSON_PROMPT) => {
            "Person".into()
        }
        MaskKind::Object { .. } => "Object".into(),
        MaskKind::Landscape { class, .. } => format!("Landscape ({class:?})"),
        MaskKind::Depth { .. } => "Depth Range".into(),
        MaskKind::Linear { .. } => "Linear Gradient".into(),
        MaskKind::Radial { .. } => "Radial Gradient".into(),
        MaskKind::Brush { strokes } => match strokes.len() {
            1 => "Brush (1 stroke)".into(),
            n => format!("Brush ({n} strokes)"),
        },
        MaskKind::LuminanceRange { .. } => "Luminance Range".into(),
        MaskKind::ColorRange { samples, .. } => match samples.len() {
            1 => "Color Range".into(),
            n => format!("Color Range ({n} samples)"),
        },
    }
}

/// `prompt` of an object component created from a face box.
const PERSON_PROMPT: &str = "person";

/// Identity of an AI component's raster: the kind without combine/invert.
fn ai_key(k: &MaskKind) -> Option<String> {
    k.is_ai()
        .then(|| serde_json::to_string(k).unwrap_or_default())
}

fn group_info(g: &LocalAdjustment, ai: &HashMap<String, AiEntry>) -> MaskGroupInfo {
    let params = serde_json::to_value(&g.params).unwrap_or(Value::Null);
    MaskGroupInfo {
        id: g.id.0,
        name: g.name.clone(),
        enabled: g.enabled,
        amount: g.amount,
        invert: g.invert,
        components: g
            .components
            .iter()
            .map(|c| {
                let key = ai_key(&c.kind);
                let state = match &key {
                    None => AiMaskState::NotAi,
                    Some(k) => match ai.get(k) {
                        Some(AiEntry::Ready(_)) => AiMaskState::Ready,
                        Some(AiEntry::Failed(m)) => AiMaskState::Failed { message: m.clone() },
                        Some(AiEntry::Pending { fraction, message }) => AiMaskState::Pending {
                            fraction: *fraction,
                            message: message.clone(),
                        },
                        None => AiMaskState::Pending {
                            fraction: 0.0,
                            message: "Queued".into(),
                        },
                    },
                };
                MaskComponentInfo {
                    kind: component_type(&c.kind),
                    combine: c.combine.into(),
                    invert: c.invert,
                    title: component_title(&c.kind),
                    definition_json: serde_json::to_string(&c.kind).unwrap_or_default(),
                    ai: state,
                    ai_key: key,
                    rendered: renderable_component(c).is_some(),
                }
            })
            .collect(),
        params: LOCAL_PARAMS
            .iter()
            .map(|&name| LocalParamValue {
                name: name.into(),
                value: params[name].as_f64().unwrap_or(0.0) as f32,
            })
            .collect(),
    }
}

fn parse_kind(json: &str) -> Result<MaskKind> {
    serde_json::from_str(json).map_err(|e| failure(format!("mask definition: {e}")))
}

fn default_model(kind: &mut MaskKind) {
    let u2net = || ModelRef {
        id: "segment/u2net".into(),
        version: ml_segment::SUBJECT_VERSION.into(),
    };
    match kind {
        MaskKind::Subject { model } | MaskKind::Sky { model } | MaskKind::Background { model }
            if model.is_none() =>
        {
            *model = Some(u2net());
        }
        MaskKind::Object { model, .. } if model.is_none() => {
            *model = Some(ModelRef {
                id: "segment/sam-decoder".into(),
                version: ml_segment::SAM_VERSION.into(),
            });
        }
        _ => {}
    }
}

// ─────────────────────────────── shared state ───────────────────────────────

pub(crate) enum AiEntry {
    Pending { fraction: f32, message: String },
    Ready(Arc<AlphaPlane>),
    Failed(String),
}

/// A single-channel alpha raster.
pub(crate) struct AlphaPlane {
    width: u32,
    height: u32,
    data: Vec<f32>,
}

struct Observed {
    level: u8,
    width: u32,
    height: u32,
    signature: [u8; 32],
    raster: Arc<[f32]>,
}

#[derive(Default)]
struct Overlay {
    surfaces: Vec<Arc<Surface>>,
    next: usize,
    group: Option<u32>,
}

/// Scene-linear pre-local RGB at one level (range sampling).
struct PreLocal {
    level: u8,
    settings: DevelopSettings,
    image: pipeline_cpu::Image,
}

/// An AI raster resampled to one extent: (key, width, height, plane).
type Resampled = (String, u32, u32, Arc<[f32]>);

/// Session mask state reachable from worker threads and the renderer hooks.
pub(crate) struct MaskShared {
    ai: Mutex<HashMap<String, AiEntry>>,
    revision: AtomicU64,
    resampled: Mutex<VecDeque<Resampled>>,
    observed: Mutex<HashMap<u32, Observed>>,
    /// Level extents of the image (the observer ignores detail windows).
    extents: Vec<(u32, u32)>,
    overlay: Mutex<Overlay>,
    listener: Mutex<Option<Arc<dyn MaskListener>>>,
    prelocal: Mutex<Option<Arc<PreLocal>>>,
    segment_input: Mutex<Option<Arc<RgbImage>>>,
}

impl MaskShared {
    pub(crate) fn new(image: &RawImage) -> Arc<Self> {
        Self::with_extents(
            (0..=MAX_LEVEL)
                .map(|l| {
                    let e = image.level_extent(l);
                    (e.width, e.height)
                })
                .collect(),
        )
    }

    fn with_extents(extents: Vec<(u32, u32)>) -> Arc<Self> {
        Arc::new(Self {
            ai: Mutex::default(),
            revision: AtomicU64::new(0),
            resampled: Mutex::default(),
            observed: Mutex::default(),
            extents,
            overlay: Mutex::default(),
            listener: Mutex::default(),
            prelocal: Mutex::default(),
            segment_input: Mutex::default(),
        })
    }

    fn listener(&self) -> Option<Arc<dyn MaskListener>> {
        self.listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// AI raster of `key` resampled to `width × height`, zeros until ready.
    fn ai_plane(&self, key: &str, width: u32, height: u32) -> Arc<[f32]> {
        let n = width as usize * height as usize;
        {
            let cache = self.resampled.lock().unwrap_or_else(|e| e.into_inner());
            if let Some((_, _, _, p)) = cache
                .iter()
                .find(|(k, w, h, _)| k == key && *w == width && *h == height)
            {
                return p.clone();
            }
        }
        let source = match self.ai.lock().unwrap_or_else(|e| e.into_inner()).get(key) {
            Some(AiEntry::Ready(p)) => p.clone(),
            _ => return vec![0.0; n].into(),
        };
        let plane: Arc<[f32]> = resample(&source, width, height).into();
        let mut cache = self.resampled.lock().unwrap_or_else(|e| e.into_inner());
        cache.push_back((key.into(), width, height, plane.clone()));
        while cache.len() > 8 {
            cache.pop_front();
        }
        plane
    }

    fn set_ai(&self, key: &str, entry: AiEntry) {
        let ready = matches!(entry, AiEntry::Ready(_));
        self.ai
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(key.into(), entry);
        if ready {
            self.resampled
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|(k, ..)| k != key);
            self.revision.fetch_add(1, Ordering::SeqCst);
        }
    }
}

fn signature(group: &LocalAdjustment) -> [u8; 32] {
    let bytes = serde_json::to_vec(&(&group.components, group.invert)).unwrap_or_default();
    *blake3::hash(&bytes).as_bytes()
}

/// Bilinear resampling with pixel-centre alignment.
fn resample(src: &AlphaPlane, width: u32, height: u32) -> Vec<f32> {
    let (sw, sh) = (src.width as usize, src.height as usize);
    let (w, h) = (width as usize, height as usize);
    if (sw, sh) == (w, h) {
        return src.data.clone();
    }
    let axis = |i: usize, n: usize, sn: usize| {
        let s = ((i as f32 + 0.5) * sn as f32 / n as f32 - 0.5).clamp(0.0, (sn - 1) as f32);
        let i0 = s.floor() as usize;
        (i0, (i0 + 1).min(sn - 1), s - i0 as f32)
    };
    let xs: Vec<_> = (0..w).map(|x| axis(x, w, sw)).collect();
    let mut out = vec![0f32; w * h];
    for (y, row) in out.chunks_mut(w).enumerate() {
        let (y0, y1, fy) = axis(y, h, sh);
        let (r0, r1) = (&src.data[y0 * sw..][..sw], &src.data[y1 * sw..][..sw]);
        for (o, &(x0, x1, fx)) in row.iter_mut().zip(&xs) {
            let top = r0[x0] + (r0[x1] - r0[x0]) * fx;
            let bottom = r1[x0] + (r1[x1] - r1[x0]) * fx;
            *o = (top + (bottom - top) * fy).clamp(0.0, 1.0);
        }
    }
    out
}

/// image-core mask hooks of one session.
pub(crate) struct Hooks(pub(crate) Arc<MaskShared>);

impl MaskHooks for Hooks {
    fn revision(&self) -> u64 {
        self.0.revision.load(Ordering::SeqCst)
    }

    fn rasterize(
        &self,
        input: &pipeline_cpu::Image,
        group: &LocalAdjustment,
        _level: u8,
    ) -> engine_api::EngineResult<Vec<f32>> {
        let (w, h) = (input.width(), input.height());
        let mut out = vec![0f32; w as usize * h as usize];
        for (index, c) in group.components.iter().enumerate() {
            let plane: Arc<[f32]> = match ai_key(&c.kind) {
                Some(key) => self.0.ai_plane(&key, w, h),
                None => {
                    let single = LocalAdjustment {
                        components: vec![MaskComponent::new(c.kind.clone())],
                        ..Default::default()
                    };
                    pipeline_cpu::masks::rasterize(
                        input,
                        &single,
                        pipeline_cpu::masks::MaskOptions::default(),
                    )?
                    .into()
                }
            };
            // Composition exactly as `pipeline_cpu::masks::rasterize`.
            for (a, &b) in out.iter_mut().zip(plane.iter()) {
                let b = if c.invert { 1.0 - b } else { b };
                *a = if index == 0 {
                    b
                } else {
                    match c.combine {
                        MaskCombine::Add => a.max(b),
                        MaskCombine::Subtract => *a * (1.0 - b),
                        MaskCombine::Intersect => *a * b,
                    }
                };
            }
        }
        if group.invert && !group.components.is_empty() {
            for v in &mut out {
                *v = 1.0 - *v;
            }
        }
        Ok(out)
    }

    fn observe(&self, group: &LocalAdjustment, level: u8, width: u32, raster: &Arc<[f32]>) {
        let Some(&(lw, lh)) = self.0.extents.get(level as usize) else {
            return;
        };
        if width != lw || raster.len() != lw as usize * lh as usize {
            return; // a 1:1 detail window, not the viewport
        }
        self.0
            .observed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(
                group.id.0,
                Observed {
                    level,
                    width: lw,
                    height: lh,
                    signature: signature(group),
                    raster: raster.clone(),
                },
            );
    }
}

// ─────────────────────────────── segmentation ───────────────────────────────

/// A segmentation request on a display-oriented image; boxes and clicks are
/// normalized to it.
#[derive(Clone, Debug, PartialEq)]
pub enum SegmentRequest {
    Subject,
    Sky,
    Background,
    Prompts {
        clicks: Vec<[f32; 2]>,
        boxes: Vec<[f32; 4]>,
    },
}

/// The segmentation backend: `ml_segment::Segmenter`, or a stand-in in tests.
pub trait MaskSegmenter: Send {
    /// A mask of `image`'s size, values in `0..=1`.
    fn segment(&mut self, image: &RgbImage, request: &SegmentRequest) -> anyhow::Result<Vec<f32>>;
}

impl MaskSegmenter for ml_segment::Segmenter {
    fn segment(&mut self, image: &RgbImage, request: &SegmentRequest) -> anyhow::Result<Vec<f32>> {
        let raster = match request {
            SegmentRequest::Subject => self.subject(image, 0)?,
            SegmentRequest::Sky => self.sky(image, 0)?,
            SegmentRequest::Background => self.background(image, 0)?,
            SegmentRequest::Prompts { clicks, boxes } => self.promptable(
                image,
                &ml_segment::Prompts {
                    clicks: clicks
                        .iter()
                        .map(|&point| ml_segment::Click {
                            point,
                            positive: true,
                        })
                        .collect(),
                    boxes: boxes.clone(),
                },
                0,
            )?,
        };
        anyhow::ensure!(
            (raster.width(), raster.height()) == image.dimensions(),
            "segmentation returned a different size"
        );
        Ok(raster.data().to_vec())
    }
}

/// The engine's segmenter, loaded on first use (loading may download the
/// pinned weights into the app's model cache).
#[derive(Default)]
pub(crate) struct SegmenterSlot(Mutex<Option<Box<dyn MaskSegmenter>>>);

impl Engine {
    /// Replaces the segmentation backend (tests, alternative models).
    #[doc(hidden)]
    pub fn install_mask_segmenter(&self, segmenter: Box<dyn MaskSegmenter>) {
        *self.segmenter.0.lock().unwrap_or_else(|e| e.into_inner()) = Some(segmenter);
    }

    fn load_segmenter(&self) -> anyhow::Result<Box<dyn MaskSegmenter>> {
        use anyhow::Context;
        let support = self.db.parent().context("support directory")?;
        let dir = support.join("models");
        std::fs::create_dir_all(&dir)?;
        let manifest = dir.join("models.toml");
        let text = include_str!("../../ml-runtime/models.toml");
        if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
            std::fs::write(&manifest, text)?;
        }
        let cache = std::env::var_os("TESSERA_SEGMENT_MODELS")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("cache"));
        let registry = ml_runtime::ModelRegistry::open(&manifest, &cache)?;
        let store = ml_segment::MaskStore::new(support.join("mask-cache"), 256 << 20)?;
        Ok(Box::new(ml_segment::Segmenter::load(
            &registry,
            ml_runtime::SessionOptions::default(),
            store,
        )?))
    }
}

/// EXIF orientation: displayed normalized point → stored (sensor) point, as
/// the loupe shader samples.
fn orient(d: [f32; 2], o: u16) -> [f32; 2] {
    let [x, y] = d;
    match o {
        2 => [1.0 - x, y],
        3 => [1.0 - x, 1.0 - y],
        4 => [x, 1.0 - y],
        5 => [y, x],
        6 => [y, 1.0 - x],
        7 => [1.0 - y, 1.0 - x],
        8 => [1.0 - y, x],
        _ => [x, y],
    }
}

/// Stored → displayed (the inverse of [`orient`]).
fn unorient(s: [f32; 2], o: u16) -> [f32; 2] {
    orient(
        s,
        match o {
            6 => 8,
            8 => 6,
            o => o,
        },
    )
}

/// `f(displayed x, y)` for every stored pixel of a `w × h` (sensor) plane, or
/// the other way round: maps pixel centres through the orientation.
fn reorient<T: Copy>(
    src: &[T],
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    map: impl Fn([f32; 2]) -> [f32; 2],
) -> Vec<T> {
    let mut out = Vec::with_capacity(dw as usize * dh as usize);
    for y in 0..dh {
        for x in 0..dw {
            let p = map([(x as f32 + 0.5) / dw as f32, (y as f32 + 0.5) / dh as f32]);
            let sx = ((p[0] * sw as f32) as u32).min(sw - 1);
            let sy = ((p[1] * sh as f32) as u32).min(sh - 1);
            out.push(src[(sy * sw + sx) as usize]);
        }
    }
    out
}

/// Runs one AI component on the session's segmentation input.
struct AiMaskJob {
    shared: std::sync::Weak<Shared>,
    key: String,
    kind: MaskKind,
}

impl AiMaskJob {
    fn progress(&self, masks: &MaskShared, fraction: f32, message: &str) {
        masks.set_ai(
            &self.key,
            AiEntry::Pending {
                fraction,
                message: message.into(),
            },
        );
        if let Some(l) = masks.listener() {
            l.ai_progress(MaskJobUpdate {
                key: self.key.clone(),
                title: component_title(&self.kind),
                fraction,
                message: message.into(),
                done: false,
                error: None,
            });
        }
    }

    fn compute(&self, shared: &Arc<Shared>, ctx: &JobContext) -> anyhow::Result<AlphaPlane> {
        let engine = shared
            .engine
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("engine closed"))?;
        let masks = &shared.masks;
        let orientation = shared.image.metadata().orientation;
        // Display-oriented sRGB of the as-shot rendering (stable across edits).
        let input = {
            let cached = masks
                .segment_input
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            match cached {
                Some(i) => i,
                None => {
                    self.progress(masks, 0.05, "Preparing image");
                    let level = default_level(&shared.image);
                    let e = shared.image.level_extent(level);
                    let settings = renderable(&DevelopSettings::default());
                    let tiles = shared.renderer.render_region(
                        &shared.image,
                        &settings,
                        level,
                        PixelRect::full(e),
                    )?;
                    let mut rgb = vec![[0u8; 3]; e.area() as usize];
                    for t in &tiles {
                        let l = t.layout();
                        let n = l.plane_len();
                        let d = t.samples::<u8>()?;
                        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
                        for y in 0..l.extent.height {
                            for x in 0..l.extent.width {
                                let i = (y * l.extent.width + x) as usize;
                                rgb[((oy + y) * e.width + ox + x) as usize] =
                                    [d[i], d[n + i], d[2 * n + i]];
                            }
                        }
                    }
                    let (dw, dh) = if orientation >= 5 {
                        (e.height, e.width)
                    } else {
                        (e.width, e.height)
                    };
                    let shown =
                        reorient(&rgb, e.width, e.height, dw, dh, |d| orient(d, orientation));
                    let image = Arc::new(
                        RgbImage::from_raw(dw, dh, shown.into_iter().flatten().collect())
                            .ok_or_else(|| anyhow::anyhow!("segmentation input"))?,
                    );
                    *masks
                        .segment_input
                        .lock()
                        .unwrap_or_else(|e| e.into_inner()) = Some(image.clone());
                    image
                }
            }
        };
        ctx.check_cancelled()?;
        let request = match &self.kind {
            MaskKind::Subject { .. } => SegmentRequest::Subject,
            MaskKind::Sky { .. } => SegmentRequest::Sky,
            MaskKind::Background { .. } => SegmentRequest::Background,
            MaskKind::Object { region, points, .. } => {
                let (clicks, boxes) = object_prompts(region.as_ref(), points)
                    .ok_or_else(|| anyhow::anyhow!("invalid object prompt"))?;
                let to_shown = |p: [f32; 2]| unorient(p, orientation);
                SegmentRequest::Prompts {
                    clicks: clicks.into_iter().map(to_shown).collect(),
                    boxes: boxes
                        .into_iter()
                        .map(|b| {
                            let a = to_shown([b[0], b[1]]);
                            let c = to_shown([b[2], b[3]]);
                            [
                                a[0].min(c[0]),
                                a[1].min(c[1]),
                                a[0].max(c[0]),
                                a[1].max(c[1]),
                            ]
                        })
                        .collect(),
                }
            }
            _ => anyhow::bail!("unsupported AI mask kind"),
        };
        let mut slot = engine.segmenter.0.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            self.progress(masks, 0.15, "Loading segmentation models");
            *slot = Some(engine.load_segmenter()?);
        }
        ctx.check_cancelled()?;
        self.progress(masks, 0.4, "Segmenting");
        let alpha = slot.as_mut().expect("loaded").segment(&input, &request)?;
        drop(slot);
        let (dw, dh) = input.dimensions();
        anyhow::ensure!(
            alpha.len() == dw as usize * dh as usize
                && alpha
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
            "invalid segmentation raster"
        );
        let (sw, sh) = if orientation >= 5 { (dh, dw) } else { (dw, dh) };
        let data = reorient(&alpha, dw, dh, sw, sh, |s| unorient(s, orientation));
        Ok(AlphaPlane {
            width: sw,
            height: sh,
            data,
        })
    }
}

impl Job for AiMaskJob {
    fn label(&self) -> &str {
        "AI mask"
    }
    fn priority(&self) -> Priority {
        Priority::Prefetch
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        let Some(shared) = self.shared.upgrade() else {
            return Ok(());
        };
        let result = self.compute(&shared, ctx);
        let masks = &shared.masks;
        let error = match result {
            Ok(plane) => {
                masks.set_ai(&self.key, AiEntry::Ready(Arc::new(plane)));
                None
            }
            Err(e) => {
                let message = format!("{e:#}");
                masks.set_ai(&self.key, AiEntry::Failed(message.clone()));
                Some(message)
            }
        };
        if let Some(l) = masks.listener() {
            l.ai_progress(MaskJobUpdate {
                key: self.key.clone(),
                title: component_title(&self.kind),
                fraction: 1.0,
                message: if error.is_some() { "Failed" } else { "Ready" }.into(),
                done: true,
                error,
            });
        }
        if let Ok(mut st) = shared.state.lock() {
            let uses = st
                .live
                .locals
                .adjustments
                .iter()
                .flat_map(|g| &g.components)
                .any(|c| ai_key(&c.kind).as_deref() == Some(&self.key));
            if uses {
                // Rasters are part of the mask cache key: re-render.
                st.rendered = None;
                shared.render(&mut st, false);
            }
        }
        Ok(())
    }
}

/// Queues segmentation for AI components of `settings` without a raster.
pub(crate) fn ensure_ai_jobs(shared: &Arc<Shared>, settings: &DevelopSettings) {
    let wanted: Vec<(String, MaskKind)> = settings
        .locals
        .adjustments
        .iter()
        .flat_map(|g| &g.components)
        .filter_map(|c| ai_key(&c.kind).map(|k| (k, c.kind.clone())))
        .collect();
    if wanted.is_empty() {
        return;
    }
    let Some(engine) = shared.engine.upgrade() else {
        return;
    };
    let mut ai = shared.masks.ai.lock().unwrap_or_else(|e| e.into_inner());
    for (key, kind) in wanted {
        if ai.contains_key(&key) {
            continue;
        }
        ai.insert(
            key.clone(),
            AiEntry::Pending {
                fraction: 0.0,
                message: "Queued".into(),
            },
        );
        let job = AiMaskJob {
            shared: Arc::downgrade(shared),
            key,
            kind,
        };
        // Fire-and-forget: the handle is not needed (jobs finish or fail).
        drop(engine.jobs.submit(Box::new(job), None));
    }
}

// ─────────────────────────────── overlay ───────────────────────────────

/// Writes the overlay group's observed raster for a frame at `level` into the
/// next overlay surface and notifies the host.
pub(crate) fn publish_overlay(
    shared: &Shared,
    settings: &DevelopSettings,
    level: u8,
    generation: u64,
) {
    let masks = &shared.masks;
    let (surface, group_id) = {
        let overlay = masks.overlay.lock().unwrap_or_else(|e| e.into_inner());
        let (Some(group), false) = (overlay.group, overlay.surfaces.is_empty()) else {
            return;
        };
        let surface = overlay.surfaces[overlay.next % overlay.surfaces.len()].clone();
        (surface, group)
    };
    let Some(group) = settings
        .locals
        .adjustments
        .iter()
        .find(|g| g.id.0 == group_id)
    else {
        return;
    };
    let (lw, lh) = masks.extents[level as usize];
    let raster: Arc<[f32]> = {
        let observed = masks.observed.lock().unwrap_or_else(|e| e.into_inner());
        match observed.get(&group_id) {
            Some(o) if o.level == level && o.signature == signature(group) => o.raster.clone(),
            // Disabled, zero-amount or not yet rendered: nothing to show.
            _ => vec![0f32; lw as usize * lh as usize].into(),
        }
    };
    let plane = if settings.geometry == Default::default() {
        (lw, lh, raster.to_vec())
    } else {
        crop_plane(&raster, lw, lh, &settings.geometry.crop)
    };
    let (pw, ph, data) = plane;
    let (w, h) = (pw.min(surface.width()), ph.min(surface.height()));
    let coverage = data.iter().sum::<f32>() / data.len().max(1) as f32;
    let written = surface.with_pixels(|px, stride| {
        for (y, row) in px.chunks_mut(stride).take(h as usize).enumerate() {
            let src = &data[y * pw as usize..][..w as usize];
            for (o, &v) in row.iter_mut().zip(src) {
                *o = (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            }
        }
    });
    if let Err(e) = written {
        eprintln!("develop: mask overlay: {e}");
        return;
    }
    {
        let mut overlay = masks.overlay.lock().unwrap_or_else(|e| e.into_inner());
        overlay.next = overlay.next.wrapping_add(1);
    }
    if let Some(l) = masks.listener() {
        l.overlay_ready(MaskOverlayFrame {
            surface_id: surface.id(),
            group_id,
            level,
            width: w,
            height: h,
            generation,
            coverage,
        });
    }
}

/// `plane` (`w × h`) through the drawn crop and straighten, bilinear: the
/// operator's inverse map (`pipeline_cpu::geometry` without its Lanczos
/// filter, which an overlay does not need) at a fraction of the cost.
fn crop_plane(
    plane: &[f32],
    w: u32,
    h: u32,
    crop: &engine_api::recipe::settings::Crop,
) -> (u32, u32, Vec<f32>) {
    let r = crop.rect;
    let (iw, ih) = (w as f32, h as f32);
    let (cw, ch) = ((r.right - r.left) * iw, (r.bottom - r.top) * ih);
    let (ow, oh) = (cw.round().max(1.0) as u32, ch.round().max(1.0) as u32);
    let (cx, cy) = ((r.left + r.right) * iw / 2.0, (r.top + r.bottom) * ih / 2.0);
    let (sin, cos) = crop.angle.to_radians().sin_cos();
    let at = |x: i64, y: i64| -> f32 {
        if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
            0.0
        } else {
            plane[(y as u32 * w + x as u32) as usize]
        }
    };
    let mut out = Vec::with_capacity(ow as usize * oh as usize);
    for y in 0..oh {
        for x in 0..ow {
            let dx = (x as f32 + 0.5) * cw / ow as f32 - cw / 2.0;
            let dy = (y as f32 + 0.5) * ch / oh as f32 - ch / 2.0;
            let sx = cx + cos * dx + sin * dy - 0.5;
            let sy = cy - sin * dx + cos * dy - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            let top = at(x0, y0) + (at(x0 + 1, y0) - at(x0, y0)) * fx;
            let bottom = at(x0, y0 + 1) + (at(x0 + 1, y0 + 1) - at(x0, y0 + 1)) * fx;
            out.push(top + (bottom - top) * fy);
        }
    }
    (ow, oh, out)
}

// ─────────────────────────────── helpers ───────────────────────────────

/// OkLab of scene-linear Rec.2020 (the colour-range operator's space).
fn oklab(rgb: [f32; 3]) -> [f32; 3] {
    let m = |m: [[f32; 3]; 3], v: [f32; 3]| {
        std::array::from_fn::<f32, 3, _>(|r| m[r][0] * v[0] + m[r][1] * v[1] + m[r][2] * v[2])
    };
    let lms = m(
        [
            [0.6167558, 0.3601984, 0.0230458],
            [0.265133, 0.6358394, 0.0990276],
            [0.1001026, 0.2039065, 0.6959909],
        ],
        rgb,
    )
    .map(f32::cbrt);
    m(
        [
            [0.21045426, 0.7936178, -0.004072047],
            [1.9779985, -2.4285922, 0.4505937],
            [0.025904037, 0.78277177, -0.80867577],
        ],
        lms,
    )
}

fn find_group(list: &mut [LocalAdjustment], id: u32) -> Result<&mut LocalAdjustment> {
    list.iter_mut()
        .find(|g| g.id.0 == id)
        .ok_or_else(|| failure(format!("no mask {id}")))
}

fn component_at(g: &mut LocalAdjustment, index: u32) -> Result<&mut MaskComponent> {
    g.components
        .get_mut(index as usize)
        .ok_or_else(|| failure(format!("mask has no component {index}")))
}

impl Shared {
    fn next_mask_id(&self, st: &mut State) -> MaskId {
        let live = st
            .live
            .locals
            .adjustments
            .iter()
            .map(|a| a.id.0 + 1)
            .max()
            .unwrap_or(0);
        let id = st.recipe.allocate_mask_id().0.max(live);
        st.recipe.ids.next_mask = id + 1;
        MaskId(id)
    }

    fn new_group(&self, st: &mut State, components: Vec<MaskComponent>) -> u32 {
        let id = self.next_mask_id(st);
        let n = st
            .live
            .locals
            .adjustments
            .iter()
            .filter_map(|g| g.name.strip_prefix("Mask ")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            .max(st.live.locals.adjustments.len() as u32);
        st.live.locals.adjustments.push(LocalAdjustment {
            id,
            name: format!("Mask {}", n + 1),
            components,
            ..Default::default()
        });
        id.0
    }

    /// Renders after a mask edit (or only refreshes the overlay when the
    /// drawn settings did not change, e.g. a rename).
    fn masks_changed(self: &Arc<Self>, st: &mut State, interactive: bool) {
        let drawn = st.drawn();
        if !interactive
            && st.rendered.as_ref() == Some(&drawn)
            && st.rendered_level == Some(st.screen_level)
        {
            let (level, generation) = (st.screen_level, st.generation);
            publish_overlay(self, &drawn, level, generation);
            return;
        }
        self.render(st, interactive);
    }

    /// Pre-local scene-linear RGB at the screen level (cached per settings).
    fn prelocal(&self, st: &State) -> Result<Arc<PreLocal>> {
        let mut settings = renderable_with(&st.live, false);
        settings.locals = Default::default();
        settings.effects = Default::default();
        let level = st.screen_level;
        let mut cached = self.masks.prelocal.lock().map_err(failure)?;
        if let Some(p) = cached
            .as_ref()
            .filter(|p| p.level == level && p.settings == settings)
        {
            return Ok(p.clone());
        }
        let e = self.image.level_extent(level);
        let tiles = self.renderer.render_region_as(
            &self.image,
            &settings,
            level,
            PixelRect::full(e),
            image_core::RenderOutput::SceneLinear,
        )?;
        let (w, h) = (e.width as usize, e.height as usize);
        let mut planes = vec![vec![0f32; w * h]; 3];
        for t in &tiles {
            let l = t.layout();
            let n = l.plane_len();
            let d = t.samples::<f32>()?;
            let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
            let tw = l.extent.width as usize;
            for row in 0..l.extent.height as usize {
                for col in 0..tw {
                    let i = row * tw + col;
                    let o = (oy as usize + row) * w + ox as usize + col;
                    for c in 0..3 {
                        planes[c][o] = if d[c * n + i].is_finite() {
                            d[c * n + i]
                        } else {
                            0.0
                        };
                    }
                }
            }
        }
        let p = Arc::new(PreLocal {
            level,
            settings,
            image: pipeline_cpu::Image::new(e.width, e.height, planes)?,
        });
        *cached = Some(p.clone());
        Ok(p)
    }
}

// ─────────────────────────────── session API ───────────────────────────────

#[uniffi::export]
impl DevelopSession {
    pub fn set_mask_listener(&self, listener: Option<Arc<dyn MaskListener>>) {
        *self
            .shared
            .masks
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = listener;
    }

    /// The live mask groups, in render order.
    pub fn mask_groups(&self) -> Result<Vec<MaskGroupInfo>> {
        let st = self.shared.lock()?;
        let ai = self.shared.masks.ai.lock().map_err(failure)?;
        Ok(st
            .live
            .locals
            .adjustments
            .iter()
            .map(|g| group_info(g, &ai))
            .collect())
    }

    /// Adds a group with one component (`MaskKind` JSON, e.g.
    /// `{"kind":"linear","start":[0.5,0],"end":[0.5,0.6]}`); returns its id.
    pub fn add_mask(&self, definition_json: String, interactive: bool) -> Result<u32> {
        let mut kind = parse_kind(&definition_json)?;
        default_model(&mut kind);
        let mut st = self.shared.lock()?;
        let id = self
            .shared
            .new_group(&mut st, vec![MaskComponent::new(kind)]);
        self.shared.masks_changed(&mut st, interactive);
        Ok(id)
    }

    /// Adds a component to a group; returns its index.
    pub fn add_mask_component(
        &self,
        group_id: u32,
        definition_json: String,
        combine: MaskCombineMode,
        interactive: bool,
    ) -> Result<u32> {
        let mut kind = parse_kind(&definition_json)?;
        default_model(&mut kind);
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        g.components.push(MaskComponent {
            kind,
            combine: combine.into(),
            invert: false,
        });
        let index = g.components.len() as u32 - 1;
        self.shared.masks_changed(&mut st, interactive);
        Ok(index)
    }

    /// Replaces a component's definition (gradient handle drags, range tuning).
    pub fn set_mask_component(
        &self,
        group_id: u32,
        index: u32,
        definition_json: String,
        interactive: bool,
    ) -> Result<()> {
        let mut kind = parse_kind(&definition_json)?;
        default_model(&mut kind);
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        component_at(g, index)?.kind = kind;
        self.shared.masks_changed(&mut st, interactive);
        Ok(())
    }

    pub fn set_mask_component_mode(
        &self,
        group_id: u32,
        index: u32,
        combine: MaskCombineMode,
        invert: bool,
    ) -> Result<()> {
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        let c = component_at(g, index)?;
        c.combine = combine.into();
        c.invert = invert;
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    /// Removes a component; removing the last one deletes the group.
    pub fn remove_mask_component(&self, group_id: u32, index: u32) -> Result<()> {
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        component_at(g, index)?;
        g.components.remove(index as usize);
        if g.components.is_empty() {
            st.live.locals.adjustments.retain(|g| g.id.0 != group_id);
        }
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    pub fn update_mask_group(
        &self,
        group_id: u32,
        patch: MaskGroupPatch,
        interactive: bool,
    ) -> Result<()> {
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        if let Some(name) = patch.name {
            g.name = name;
        }
        if let Some(v) = patch.enabled {
            g.enabled = v;
        }
        if let Some(v) = patch.amount {
            g.amount = finite_or(v, 100.0).clamp(0.0, 200.0);
        }
        if let Some(v) = patch.invert {
            g.invert = v;
        }
        self.shared.masks_changed(&mut st, interactive);
        Ok(())
    }

    /// Sets one local slider ([`LOCAL_PARAMS`] name) of a group.
    pub fn set_mask_param(
        &self,
        group_id: u32,
        name: String,
        value: f32,
        interactive: bool,
    ) -> Result<()> {
        if !LOCAL_PARAMS.contains(&name.as_str()) || !value.is_finite() {
            return Err(failure(format!("unknown local parameter {name}")));
        }
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        let mut params = serde_json::to_value(&g.params).map_err(failure)?;
        params[name.as_str()] = serde_json::json!(value);
        g.params = serde_json::from_value::<LocalParams>(params).map_err(failure)?;
        self.shared.masks_changed(&mut st, interactive);
        Ok(())
    }

    /// Every local slider of a group back to neutral.
    pub fn reset_mask_params(&self, group_id: u32) -> Result<()> {
        let mut st = self.shared.lock()?;
        find_group(&mut st.live.locals.adjustments, group_id)?.params = LocalParams::default();
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    pub fn delete_mask(&self, group_id: u32) -> Result<()> {
        let mut st = self.shared.lock()?;
        find_group(&mut st.live.locals.adjustments, group_id)?;
        st.live.locals.adjustments.retain(|g| g.id.0 != group_id);
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    /// Copies a group (components and sliders) under a new id; returns it.
    pub fn duplicate_mask(&self, group_id: u32) -> Result<u32> {
        let mut st = self.shared.lock()?;
        let mut copy = find_group(&mut st.live.locals.adjustments, group_id)?.clone();
        copy.id = self.shared.next_mask_id(&mut st);
        copy.name = format!("{} Copy", copy.name);
        let id = copy.id.0;
        let at = st
            .live
            .locals
            .adjustments
            .iter()
            .position(|g| g.id.0 == group_id)
            .map_or(st.live.locals.adjustments.len(), |i| i + 1);
        st.live.locals.adjustments.insert(at, copy);
        self.shared.masks_changed(&mut st, false);
        Ok(id)
    }

    /// Starts a brush stroke in `group_id` (a new group when `None`): paint
    /// strokes and erase strokes share the group's brush component. Returns
    /// the group id. Follow with `add_brush_points` (one call per display
    /// frame) and `end_brush_stroke`, then `commit`.
    pub fn begin_brush_stroke(&self, group_id: Option<u32>, brush: BrushSettings) -> Result<u32> {
        let stroke = BrushStroke {
            points: Vec::new(),
            radius: finite_or(brush.radius, 0.02).clamp(1e-4, 1.0),
            feather: unit(brush.feather, 50.0),
            flow: unit(brush.flow, 100.0),
            erase: brush.erase,
        };
        let mut st = self.shared.lock()?;
        let id = match group_id {
            Some(id) => {
                find_group(&mut st.live.locals.adjustments, id)?;
                id
            }
            None => {
                if brush.erase {
                    return Err(failure("erasing needs a mask"));
                }
                self.shared.new_group(
                    &mut st,
                    vec![MaskComponent::new(MaskKind::Brush { strokes: vec![] })],
                )
            }
        };
        let g = find_group(&mut st.live.locals.adjustments, id)?;
        let index = match g.components.iter().rposition(|c| {
            matches!(c.kind, MaskKind::Brush { .. }) && c.combine == MaskCombine::Add && !c.invert
        }) {
            Some(i) => i,
            None if brush.erase => return Err(failure("this mask has no brush to erase from")),
            None => {
                g.components
                    .push(MaskComponent::new(MaskKind::Brush { strokes: vec![] }));
                g.components.len() - 1
            }
        };
        if let MaskKind::Brush { strokes } = &mut g.components[index].kind {
            strokes.push(stroke);
        }
        st.masks.stroke = Some((id, index));
        Ok(id)
    }

    /// Appends samples to the stroke in progress and renders interactively.
    pub fn add_brush_points(&self, points: Vec<BrushPoint>) -> Result<()> {
        let mut st = self.shared.lock()?;
        let (id, index) = st
            .masks
            .stroke
            .ok_or_else(|| failure("no brush stroke in progress"))?;
        let g = find_group(&mut st.live.locals.adjustments, id)?;
        let Some(MaskComponent {
            kind: MaskKind::Brush { strokes },
            ..
        }) = g.components.get_mut(index)
        else {
            return Err(failure("brush component changed"));
        };
        let stroke = strokes.last_mut().ok_or_else(|| failure("no stroke"))?;
        stroke.points.extend(
            points
                .iter()
                .filter(|p| p.x.is_finite() && p.y.is_finite())
                .map(|p| {
                    [
                        coord(p.x),
                        coord(p.y),
                        finite_or(p.pressure, 1.0).clamp(0.0, 1.0),
                    ]
                }),
        );
        self.shared.masks_changed(&mut st, true);
        Ok(())
    }

    /// Ends the stroke: refines the viewport (commit makes it an undo step).
    pub fn end_brush_stroke(&self) -> Result<()> {
        let mut st = self.shared.lock()?;
        if let Some((id, index)) = st.masks.stroke.take()
            && let Ok(g) = find_group(&mut st.live.locals.adjustments, id)
        {
            // A click without samples leaves no empty stroke, brush or group.
            if let Some(MaskComponent {
                kind: MaskKind::Brush { strokes },
                ..
            }) = g.components.get_mut(index)
            {
                strokes.retain(|s| !s.points.is_empty());
                if strokes.is_empty() {
                    g.components.remove(index);
                }
            }
            if g.components.is_empty() {
                st.live.locals.adjustments.retain(|g| g.id.0 != id);
            }
        }
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    /// Samples the pre-local image around (`x`, `y`) (mask space) and adds a
    /// colour or luminance range there: to `group_id` combined with
    /// `combine`, or as a new group. Returns the group id. Blocking.
    pub fn add_range_mask(
        &self,
        group_id: Option<u32>,
        kind: RangeKind,
        x: f32,
        y: f32,
        combine: MaskCombineMode,
    ) -> Result<u32> {
        let rgb = self.sample_prelocal(x, y)?;
        let component = match kind {
            RangeKind::Color => MaskKind::ColorRange {
                samples: vec![oklab(rgb)],
                amount: 12.0,
            },
            RangeKind::Luminance => {
                let l = (0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]).max(0.0);
                MaskKind::LuminanceRange {
                    range: [(l * 0.5).min(1.0), (l * 2.0).min(1.0)],
                    smoothness: 50.0,
                }
            }
        };
        let mut st = self.shared.lock()?;
        let id = match group_id {
            Some(id) => {
                find_group(&mut st.live.locals.adjustments, id)?
                    .components
                    .push(MaskComponent {
                        kind: component,
                        combine: combine.into(),
                        invert: false,
                    });
                id
            }
            None => self
                .shared
                .new_group(&mut st, vec![MaskComponent::new(component)]),
        };
        self.shared.masks_changed(&mut st, false);
        Ok(id)
    }

    /// Adds another sampled colour to a colour range component. Blocking.
    pub fn add_color_range_sample(&self, group_id: u32, index: u32, x: f32, y: f32) -> Result<()> {
        let lab = oklab(self.sample_prelocal(x, y)?);
        let mut st = self.shared.lock()?;
        let g = find_group(&mut st.live.locals.adjustments, group_id)?;
        match &mut component_at(g, index)?.kind {
            MaskKind::ColorRange { samples, .. } if samples.len() < 8 => samples.push(lab),
            MaskKind::ColorRange { .. } => return Err(failure("at most 8 colour samples")),
            _ => return Err(failure("not a colour range")),
        }
        self.shared.masks_changed(&mut st, false);
        Ok(())
    }

    /// Adds an AI component (to `group_id`, or as a new group) and queues its
    /// segmentation; progress arrives through `MaskListener::ai_progress` and
    /// the viewport re-renders when the raster is ready. Returns the group id.
    pub fn add_ai_mask(
        &self,
        group_id: Option<u32>,
        request: AiMaskRequest,
        combine: MaskCombineMode,
    ) -> Result<u32> {
        let rect = |r: MaskRect| -> Result<NormalizedRect> {
            let (l, t) = (r.left.min(r.right), r.top.min(r.bottom));
            let (rr, b) = (r.left.max(r.right), r.top.max(r.bottom));
            let rect = NormalizedRect {
                left: l.clamp(0.0, 1.0),
                top: t.clamp(0.0, 1.0),
                right: rr.clamp(0.0, 1.0),
                bottom: b.clamp(0.0, 1.0),
            };
            if !(rect.right - rect.left > 1e-4 && rect.bottom - rect.top > 1e-4) {
                return Err(failure("empty box"));
            }
            Ok(rect)
        };
        let orientation = self.shared.image.metadata().orientation;
        let mut kind = match request {
            AiMaskRequest::Subject => MaskKind::Subject { model: None },
            AiMaskRequest::Sky => MaskKind::Sky { model: None },
            AiMaskRequest::Background => MaskKind::Background { model: None },
            AiMaskRequest::Person { face } => {
                // ml_segment::person_box in display orientation: 3 face widths
                // wide, from half a face above to 7 faces below.
                let f = rect(face)?;
                let a = unorient([f.left, f.top], orientation);
                let b = unorient([f.right, f.bottom], orientation);
                let (x0, y0, x1, y1) = (
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[0].max(b[0]),
                    a[1].max(b[1]),
                );
                let (w, h) = (x1 - x0, y1 - y0);
                let p0 = orient(
                    [(x0 - w).clamp(0.0, 1.0), (y0 - 0.5 * h).clamp(0.0, 1.0)],
                    orientation,
                );
                let p1 = orient(
                    [
                        (x0 + 2.0 * w).clamp(0.0, 1.0),
                        (y0 + 7.0 * h).clamp(0.0, 1.0),
                    ],
                    orientation,
                );
                MaskKind::Object {
                    prompt: Some(PERSON_PROMPT.into()),
                    region: Some(rect(MaskRect {
                        left: p0[0],
                        top: p0[1],
                        right: p1[0],
                        bottom: p1[1],
                    })?),
                    points: vec![],
                    model: None,
                }
            }
            AiMaskRequest::Object { points, region } => {
                let points: Vec<[f32; 2]> = points
                    .iter()
                    .map(|p| [p.x.clamp(0.0, 1.0), p.y.clamp(0.0, 1.0)])
                    .collect();
                let region = region.map(rect).transpose()?;
                if points.is_empty() && region.is_none() {
                    return Err(failure("click or drag a box around the object"));
                }
                MaskKind::Object {
                    prompt: None,
                    region,
                    points,
                    model: None,
                }
            }
        };
        default_model(&mut kind);
        let mut st = self.shared.lock()?;
        let id = match group_id {
            Some(id) => {
                find_group(&mut st.live.locals.adjustments, id)?
                    .components
                    .push(MaskComponent {
                        kind,
                        combine: combine.into(),
                        invert: false,
                    });
                id
            }
            None => self
                .shared
                .new_group(&mut st, vec![MaskComponent::new(kind)]),
        };
        let live = st.live.clone();
        ensure_ai_jobs(&self.shared, &live);
        self.shared.masks_changed(&mut st, false);
        Ok(id)
    }

    /// Re-runs segmentation for a failed (or stale) AI component.
    pub fn retry_ai_mask(&self, key: String) -> Result<()> {
        self.shared.masks.ai.lock().map_err(failure)?.remove(&key);
        let live = self.shared.lock()?.live.clone();
        ensure_ai_jobs(&self.shared, &live);
        Ok(())
    }

    /// Adds an R8 (one byte per pixel, `'L008'`) surface of the current
    /// `plan_surface` size to the overlay ring (two surfaces). A different
    /// size replaces the ring.
    pub fn attach_mask_overlay_surface(
        &self,
        iosurface_id: u32,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let surface = Arc::new(Surface::lookup_r8(iosurface_id, width, height).map_err(failure)?);
        let mut o = self.shared.masks.overlay.lock().map_err(failure)?;
        if o.surfaces
            .first()
            .is_some_and(|f| (f.width(), f.height()) != (width, height))
        {
            o.surfaces.clear();
        }
        o.surfaces.retain(|s| s.id() != surface.id());
        if o.surfaces.len() >= 2 {
            o.surfaces.remove(0);
        }
        o.surfaces.push(surface);
        Ok(())
    }

    pub fn detach_mask_overlay_surfaces(&self) {
        if let Ok(mut o) = self.shared.masks.overlay.lock() {
            o.surfaces.clear();
        }
    }

    /// Shows `group_id`'s mask in the overlay surfaces after every frame
    /// (`None`: off). Writes the current frame's overlay at once.
    pub fn set_mask_overlay(&self, group_id: Option<u32>) -> Result<()> {
        self.shared.masks.overlay.lock().map_err(failure)?.group = group_id;
        if group_id.is_some() {
            let st = self.shared.lock()?;
            if let (Some(frame), Some(level)) = (&st.frame, st.rendered_level) {
                let (settings, generation) = (frame.settings.clone(), st.generation);
                drop(st);
                publish_overlay(&self.shared, &settings, level, generation);
            }
        }
        Ok(())
    }

    /// The group's mask as last rendered, fitted into `max_px` (display
    /// orientation, uncropped); `None` before it has been rendered.
    pub fn mask_thumbnail(&self, group_id: u32, max_px: u32) -> Result<Option<MaskThumbnail>> {
        let (raster, w, h) = {
            let observed = self.shared.masks.observed.lock().map_err(failure)?;
            match observed.get(&group_id) {
                Some(o) => (o.raster.clone(), o.width, o.height),
                None => return Ok(None),
            }
        };
        let o = self.shared.image.metadata().orientation;
        let (dw, dh) = if o >= 5 { (h, w) } else { (w, h) };
        let scale = (max_px.max(1) as f32 / dw.max(dh) as f32).min(1.0);
        let (tw, th) = (
            ((dw as f32 * scale).round() as u32).max(1),
            ((dh as f32 * scale).round() as u32).max(1),
        );
        // Box-filtered: mean of the stored pixels under each thumbnail pixel.
        let mut alpha = Vec::with_capacity((tw * th) as usize);
        for y in 0..th {
            for x in 0..tw {
                let d0 = [x as f32 / tw as f32, y as f32 / th as f32];
                let d1 = [(x + 1) as f32 / tw as f32, (y + 1) as f32 / th as f32];
                let (a, b) = (orient(d0, o), orient(d1, o));
                let (x0, x1) = (
                    (a[0].min(b[0]) * w as f32) as u32,
                    (a[0].max(b[0]) * w as f32).ceil() as u32,
                );
                let (y0, y1) = (
                    (a[1].min(b[1]) * h as f32) as u32,
                    (a[1].max(b[1]) * h as f32).ceil() as u32,
                );
                let (x1, y1) = (x1.clamp(x0 + 1, w), y1.clamp(y0 + 1, h));
                let (x0, y0) = (x0.min(w - 1), y0.min(h - 1));
                let mut sum = 0.0;
                for yy in y0..y1 {
                    for xx in x0..x1 {
                        sum += raster[(yy * w + xx) as usize];
                    }
                }
                let n = ((x1 - x0) * (y1 - y0)).max(1) as f32;
                alpha.push((sum / n * 255.0).round().clamp(0.0, 255.0) as u8);
            }
        }
        Ok(Some(MaskThumbnail {
            width: tw,
            height: th,
            alpha,
        }))
    }
}

impl DevelopSession {
    /// Mean scene-linear pre-local RGB in a 5×5 window at (`x`, `y`).
    fn sample_prelocal(&self, x: f32, y: f32) -> Result<[f32; 3]> {
        if !(x.is_finite() && y.is_finite() && (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y))
        {
            return Err(failure("sample point outside the image"));
        }
        let p = {
            let st = self.shared.lock()?;
            self.shared.prelocal(&st)?
        };
        let (w, h) = (p.image.width() as i64, p.image.height() as i64);
        let (cx, cy) = ((x * w as f32) as i64, (y * h as f32) as i64);
        let mut sum = [0f32; 3];
        let mut n = 0f32;
        for yy in (cy - 2).max(0)..=(cy + 2).min(h - 1) {
            for xx in (cx - 2).max(0)..=(cx + 2).min(w - 1) {
                let i = (yy * w + xx) as usize;
                for (c, s) in sum.iter_mut().enumerate() {
                    *s += p.image.planes()[c][i];
                }
                n += 1.0;
            }
        }
        Ok(sum.map(|s| s / n.max(1.0)))
    }
}

/// Session-side mask state guarded by the session lock.
#[derive(Default)]
pub(crate) struct MaskState {
    /// Brush stroke in progress: (group id, brush component index).
    pub(crate) stroke: Option<(u32, usize)>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_maps_round_trip() {
        for o in 1..=8u16 {
            for p in [[0.1f32, 0.7], [0.9, 0.2], [0.5, 0.5]] {
                let s = orient(p, o);
                let back = unorient(s, o);
                assert!(
                    (back[0] - p[0]).abs() < 1e-6 && (back[1] - p[1]).abs() < 1e-6,
                    "o={o}"
                );
            }
        }
        // Orientation 6 (rotated 90° CW for display): displayed top-left is stored bottom-left.
        assert_eq!(orient([0.0, 0.0], 6), [0.0, 1.0]);
    }

    #[test]
    fn oklab_matches_the_colour_range_operator() {
        let rgb = [0.3f32, 0.12, 0.05];
        let image = pipeline_cpu::Image::new(
            2,
            1,
            vec![vec![0.3, 0.05], vec![0.12, 0.3], vec![0.05, 0.1]],
        )
        .unwrap();
        let group = LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::ColorRange {
                samples: vec![oklab(rgb)],
                amount: 0.0,
            })],
            ..Default::default()
        };
        let mask = pipeline_cpu::masks::rasterize(
            &image,
            &group,
            pipeline_cpu::masks::MaskOptions::default(),
        )
        .unwrap();
        assert_eq!(mask, vec![1.0, 0.0]);
    }

    #[test]
    fn sanitizing_drops_what_cannot_render_and_clamps_the_rest() {
        let comps = vec![
            MaskComponent::new(MaskKind::Linear {
                start: [0.5, 0.5],
                end: [0.5, 0.5],
            }),
            MaskComponent::new(MaskKind::Radial {
                center: [0.5, f32::NAN],
                radii: [0.0, 40.0],
                angle: 400.0,
                feather: 300.0,
            }),
            MaskComponent::new(MaskKind::Depth {
                range: [0.0, 1.0],
                feather: 0.0,
                model: None,
            }),
            MaskComponent::new(MaskKind::Subject { model: None }),
            MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.2, 0.2, 2.0], [f32::NAN, 0.0, 1.0]],
                    ..Default::default()
                }],
            }),
            MaskComponent::new(MaskKind::ColorRange {
                samples: vec![],
                amount: 10.0,
            }),
        ];
        let g = LocalAdjustment {
            components: comps,
            amount: 500.0,
            params: LocalParams {
                exposure: 40.0,
                defringe: 20.0,
                color_overlay: Some([10.0, 20.0]),
                ..Default::default()
            },
            ..Default::default()
        };
        let r = renderable_group(&g);
        assert_eq!(r.amount, 200.0);
        assert_eq!(r.params.exposure, 5.0);
        assert_eq!((r.params.defringe, r.params.color_overlay), (0.0, None));
        let kinds: Vec<_> = r
            .components
            .iter()
            .map(|c| component_type(&c.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                MaskComponentType::Radial,
                MaskComponentType::Subject,
                MaskComponentType::Brush
            ]
        );
        match &r.components[0].kind {
            MaskKind::Radial {
                center,
                radii,
                angle,
                feather,
            } => {
                assert_eq!(center, &[0.5, 0.0]);
                assert_eq!(radii, &[1e-4, 16.0]);
                assert_eq!((*angle, *feather), (40.0, 100.0));
            }
            _ => unreachable!(),
        }
        match &r.components[2].kind {
            MaskKind::Brush { strokes } => assert_eq!(strokes[0].points, vec![[0.2, 0.2, 1.0]]),
            _ => unreachable!(),
        }
        // The procedural subset is accepted by the operators.
        let procedural = LocalAdjustment {
            components: r
                .components
                .iter()
                .filter(|c| !c.kind.is_ai())
                .cloned()
                .collect(),
            ..r.clone()
        };
        let image = pipeline_cpu::Image::new(8, 6, vec![vec![0.2; 48]; 3]).unwrap();
        pipeline_cpu::masks::rasterize(&image, &procedural, Default::default()).unwrap();
        pipeline_cpu::adjust_local(&image, &r.params, r.amount).unwrap();
    }

    #[test]
    fn hooks_compose_ai_rasters_like_the_operator() {
        let image = pipeline_cpu::Image::new(4, 2, vec![vec![0.2; 8]; 3]).unwrap();
        let shared = MaskShared::with_extents(vec![(4, 2)]);
        let subject = MaskKind::Subject { model: None };
        let key = ai_key(&subject).unwrap();
        // Left half selected at half resolution.
        shared.set_ai(
            &key,
            AiEntry::Ready(Arc::new(AlphaPlane {
                width: 2,
                height: 1,
                data: vec![1.0, 0.0],
            })),
        );
        let hooks = Hooks(shared.clone());
        let group = LocalAdjustment {
            components: vec![
                MaskComponent::new(subject.clone()),
                MaskComponent {
                    kind: MaskKind::Linear {
                        start: [0.0, 0.0],
                        end: [0.0, 1.0],
                    },
                    combine: MaskCombine::Subtract,
                    invert: false,
                },
            ],
            ..Default::default()
        };
        let m = hooks.rasterize(&image, &group, 0).unwrap();
        // Linear: 0.75 on the top row, 0.25 on the bottom; subtract from subject.
        assert!((m[0] - 0.25).abs() < 1e-5, "{m:?}");
        assert!((m[4] - 0.75).abs() < 1e-5, "{m:?}");
        assert_eq!(m[3], 0.0);
        let rev = hooks.revision();
        shared.set_ai(
            &key,
            AiEntry::Ready(Arc::new(AlphaPlane {
                width: 1,
                height: 1,
                data: vec![1.0],
            })),
        );
        assert!(hooks.revision() > rev, "a new raster changes the cache key");
        // Through the renderer's raster cache (the path the render takes).
        let cache = image_core::MaskRasterCache::new(1 << 20);
        cache.set_hooks(Some(Arc::new(Hooks(shared.clone()))));
        let cached = cache
            .rasterize(&image, &group, 0, Default::default(), Default::default())
            .unwrap();
        // The whole frame is subject now: only the subtracted gradient remains.
        assert!((cached[3] - 0.25).abs() < 1e-5, "{cached:?}");
        let observed = shared.observed.lock().unwrap();
        assert_eq!(observed.get(&0).map(|o| o.width), Some(4));
    }

    #[test]
    fn overlay_crop_follows_the_geometry_operator() {
        use engine_api::recipe::settings::{GeometrySettings, NormalizedRect};
        let (w, h) = (64u32, 48u32);
        let plane: Vec<f32> = (0..w * h)
            .map(|i| ((i % w) as f32 / w as f32) * 0.8 + (i / w) as f32 / h as f32 * 0.2)
            .collect();
        let mut g = GeometrySettings::default();
        g.crop.rect = NormalizedRect {
            left: 0.2,
            top: 0.1,
            right: 0.8,
            bottom: 0.7,
        };
        g.crop.angle = 5.0;
        let (ow, oh, ours) = crop_plane(&plane, w, h, &g.crop);
        let reference = pipeline_cpu::geometry(
            &pipeline_cpu::Image::new(w, h, vec![plane.clone()]).unwrap(),
            &g,
        )
        .unwrap();
        assert_eq!((ow, oh), (reference.width(), reference.height()));
        let worst = ours
            .iter()
            .zip(&reference.planes()[0])
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(worst < 0.02, "{worst}");
    }

    #[test]
    fn resampling_is_centre_aligned() {
        let src = AlphaPlane {
            width: 2,
            height: 1,
            data: vec![0.0, 1.0],
        };
        let out = resample(&src, 4, 1);
        assert_eq!(out, vec![0.0, 0.25, 0.75, 1.0]);
    }
}
