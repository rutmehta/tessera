//! Layered-editor tools on a [`DocumentSession`] (WP M5-11): painting
//! (brush, eraser, clone stamp, healing brush) on pixels or layer masks,
//! pixel selections (marquee, lasso, magic wand, quick selection, colour
//! range, subject / sky / object, modify, refine edge, alpha channels,
//! marching-ants outlines), Free Transform, fill / delete in a selection and
//! the eyedropper. The brush engine is the `brush` crate and the selection
//! algorithms the `selection` crate (M5-05).
//!
//! # Strokes
//!
//! [`DocumentSession::begin_stroke`] snapshots the target raster (a layer's
//! pixels or its mask) and opens a live stroke on a scratch copy of the
//! document, which the viewport shows. The host calls
//! [`DocumentSession::stroke_points`] once per display frame with the pointer
//! samples of that frame: new dabs are rasterized, exactly their dirty rect
//! is composited into a live copy of the raster (recomputed from the
//! stroke-start snapshot, so nothing is double-applied), the replaced tiles
//! go to the scratch document as a `DocOp::PaintTiles`, and one frame is
//! requested. The resident renderer uploads only those tiles and composites
//! only the damaged blocks. [`DocumentSession::end_stroke`] applies the whole
//! stroke to the real document as **one** history node.
//!
//! # Selections
//!
//! Every selection call is one history node labelled like Photoshop's
//! (`Magic Wand`, `Lasso`, …) and combines with the current selection by
//! [`SelectionOp`] (⇧ add, ⌥ subtract, ⇧⌥ intersect); without a selection,
//! add and replace give the new shape and subtract / intersect give nothing.
//! Image-driven tools (wand, quick selection, colour range, subject, sky,
//! object, magnetic lasso, refine edge) analyse the image at the finest
//! pyramid level of at most [`ANALYSIS_PIXELS`] pixels and upsample the
//! result bilinearly to the canvas.
//!
//! # Transform
//!
//! [`DocumentSession::begin_transform`] captures layers, `set_transform`
//! shows the resampled layers live on the scratch (bilinear, all cores),
//! `commit_transform` resamples with the chosen interpolation into one
//! `Free Transform` node and `cancel_transform` drops the preview.

use super::{
    DocRect, DocumentSession, DocumentUpdate, find, io, parse_blend, raster_from_rgba,
    tile_from_f32,
};
use crate::{MaskSegmenter, Result, SegmentRequest, failure};
use brush::{
    Brush, CloneSource, InputPoint, PaintMode, SampledTip, Smoothing, Stroke, Symmetry, Tip,
    dynamics::Control,
};
use compositor::{
    Affine, BlendMode, DocOp, DocState, Document, LayerId, LayerKind, Mask as LayerMask,
    PaintTarget, Raster, Rect, TileDelta,
    document::selection::{self as docsel, Combine},
};
use engine_api::{
    EngineError, EngineResult,
    tile::{Extent, TILE_SIZE, TileCoord},
};
use selection::{Image, Mask};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

/// Largest image (pixels) the image-driven selection tools analyse.
pub const ANALYSIS_PIXELS: u64 = 6_000_000;

// ─────────────────────────────── records ───────────────────────────────

/// A straight, display-encoded RGB colour, each channel `0…1`.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct PaintColor {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

impl PaintColor {
    fn rgb(self) -> [f32; 3] {
        [self.r, self.g, self.b]
    }
}

/// A point in level-0 canvas pixels.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct ToolPoint {
    pub x: f32,
    pub y: f32,
}

/// What a stroke paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StrokeTarget {
    /// The layer's pixels (pixel layers).
    Pixels,
    /// The layer mask (created revealing all when the layer has none).
    Mask,
}

/// The painting tool.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum StrokeTool {
    Brush,
    /// Removes alpha (on a mask: paints black, hiding).
    Eraser,
    /// Clone Stamp from the clone source (`set_clone_source`).
    Clone,
    /// Healing Brush: clone, then Poisson-blend each dab into its surroundings.
    Heal,
}

/// Paint symmetry (spec 02 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PaintSymmetry {
    None,
    /// Mirror across the vertical line `x = symmetry_x`.
    Vertical,
    /// Mirror across the horizontal line `y = symmetry_y`.
    Horizontal,
    /// Both axes (4 copies).
    Dual,
    /// Mirror across the diagonal through the centre.
    Diagonal,
    /// `symmetry_count` rotated copies about the centre.
    Radial,
    /// Radial with a mirror in every segment.
    Mandala,
}

/// Brush options (the options bar and the Brushes panel).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PaintBrush {
    /// Diameter, pixels (0.1…10000).
    pub size: f32,
    /// 0 soft … 1 hard (computed round tips).
    pub hardness: f32,
    /// Stroke opacity cap 0…1.
    pub opacity: f32,
    /// Per-dab flow 0…1.
    pub flow: f32,
    /// Dab spacing as a fraction of the diameter (0.01…10).
    pub spacing: f32,
    /// Tip angle, degrees.
    pub angle: f32,
    /// Tip roundness 0.01…1.
    pub roundness: f32,
    /// A blend mode name (`normal`, `multiply`, … as in `LayerNode`).
    pub blend_mode: String,
    /// Pen pressure controls the size / opacity / flow.
    pub pressure_size: bool,
    pub pressure_opacity: bool,
    pub pressure_flow: bool,
    /// Pulled-string smoothing length, pixels (0 = off).
    pub smoothing: f32,
    pub symmetry: PaintSymmetry,
    /// Symmetry axis / centre, canvas pixels.
    pub symmetry_x: f32,
    pub symmetry_y: f32,
    /// Radial / mandala segments.
    pub symmetry_count: u32,
    /// A sampled or imported tip (`brush_tips`); `None` = computed round.
    pub tip_id: Option<String>,
    /// Clone / heal sample the visible composite instead of one layer.
    pub sample_all_layers: bool,
}

/// One pointer sample.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StrokeSample {
    /// Level-0 canvas pixels.
    pub x: f32,
    pub y: f32,
    /// 0…1 (1 without a tablet), after the host's pressure curve.
    pub pressure: f32,
    /// Pen tilt, each −1…1.
    pub tilt_x: f32,
    pub tilt_y: f32,
    /// Seconds (any epoch; used for velocity and airbrush).
    pub timestamp: f64,
}

/// What one `stroke_points` call did.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StrokeFrame {
    /// Level-0 pixels that changed (`None`: no new dabs, e.g. under smoothing).
    pub dirty_rect: Option<DocRect>,
    /// Dabs placed by this call.
    pub dabs: u32,
    /// Dabs so far in the stroke.
    pub total_dabs: u32,
    /// Engine time of this call (dab rasterization and tile composite), ms.
    pub raster_ms: f64,
    /// Document epoch after the call (its frame carries it).
    pub epoch: u64,
}

/// How a new selection combines with the current one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionOp {
    Replace,
    /// ⇧
    Add,
    /// ⌥
    Subtract,
    /// ⇧⌥
    Intersect,
}

/// Marquee shapes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum MarqueeShape {
    Rect,
    Ellipse,
    /// Single row at `y` (height ignored).
    Row,
    /// Single column at `x` (width ignored).
    Column,
}

/// Lasso kinds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LassoKind {
    Free,
    Polygon,
    /// Live-wire between the anchor points (edge-snapping).
    Magnetic,
}

/// Select ▸ Modify.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionModify {
    Border,
    Smooth,
    Expand,
    Contract,
    Feather,
}

/// Select and Mask global refinements (pixels at level 0).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct RefineEdgeParams {
    /// Edge detection radius.
    pub radius: f32,
    pub smart_radius: bool,
    /// Outline smoothing σ.
    pub smooth: f32,
    /// Feather σ.
    pub feather: f32,
    /// 0…1 (1 = hard edge).
    pub contrast: f32,
    /// Positive grows.
    pub shift_edge: f32,
}

/// One marching-ants outline, level-0 canvas pixels.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OutlinePolyline {
    pub points: Vec<ToolPoint>,
    pub closed: bool,
}

/// A 2-D affine map `x' = a·x + b·y + c`, `y' = d·x + e·y + f` (canvas pixels).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct TransformMatrix {
    pub a: f64,
    pub b: f64,
    pub c: f64,
    pub d: f64,
    pub e: f64,
    pub f: f64,
}

impl TransformMatrix {
    fn affine(self) -> Affine {
        Affine {
            m: [self.a, self.b, self.c, self.d, self.e, self.f],
        }
    }
}

/// Resampling of a transform.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TransformInterpolation {
    Nearest,
    Bilinear,
    Bicubic,
}

/// What `begin_transform` captured.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TransformInfo {
    pub layers: Vec<u64>,
    /// Pixel-exact bounds of the layers' content (`None`: empty layers).
    pub bounds: Option<DocRect>,
}

/// Edit ▸ Fill contents.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum SelectionFill {
    Color {
        color: PaintColor,
    },
    /// Placeholder for Content-Aware Fill: a smooth membrane (harmonic)
    /// interpolation of the selection's surroundings (≤ 0.5 MP).
    ContentAware,
}

/// A brush tip in the tip library (built-in or imported from `.abr`).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrushTipInfo {
    pub id: String,
    pub name: String,
    /// Sample size (1 × 1 for computed tips).
    pub width: u32,
    pub height: u32,
    /// Natural diameter, pixels.
    pub diameter: f32,
    /// Spacing the file suggests (fraction of the diameter).
    pub spacing: Option<f32>,
    pub sampled: bool,
}

/// A grey tip preview (`0` = no paint, `255` = full paint), row-major.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct BrushTipImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

// ─────────────────────────────── state ───────────────────────────────

/// Tool state kept with the session state (`State::tools`).
#[derive(Default)]
pub(crate) struct ToolState {
    stroke: Option<ActiveStroke>,
    transform: Option<ActiveTransform>,
    /// Clone source: `(source layer, dx, dy)`; the pixel painted at `p` is
    /// sampled at `p + (dx, dy)`.
    clone_source: Option<(u64, f32, f32)>,
    /// Save / Load Selection channels (session state).
    channels: BTreeMap<String, Arc<Raster>>,
    /// Selection before an interactive Refine Edge.
    refine_base: Option<Arc<Raster>>,
    /// Outline of the last selection asked for: `(selection, level, outline)`.
    outline: Option<(Weak<Raster>, u8, Arc<Vec<OutlinePolyline>>)>,
}

struct ActiveStroke {
    layer: u64,
    target: PaintTarget,
    tool: StrokeTool,
    stroke: Stroke,
    /// The target raster with the stroke so far.
    live: Raster,
    /// Ops before the paint (a new mask for a mask stroke without one).
    prelude: Vec<DocOp>,
    lock_alpha: bool,
    dabs: u32,
}

struct TransformLayer {
    id: u64,
    content: Raster,
    mask: Option<Raster>,
}

struct ActiveTransform {
    layers: Vec<TransformLayer>,
    matrix: Affine,
    interpolation: TransformInterpolation,
}

fn engine_err(e: EngineError) -> crate::BridgeError {
    failure(e)
}

fn history_label(tool: StrokeTool) -> &'static str {
    match tool {
        StrokeTool::Brush => "Brush Tool",
        StrokeTool::Eraser => "Eraser",
        StrokeTool::Clone => "Clone Stamp",
        StrokeTool::Heal => "Healing Brush",
    }
}

// ─────────────────────────────── brush tips ───────────────────────────────

struct TipEntry {
    info: BrushTipInfo,
    tip: Tip,
}

fn tip_library() -> &'static Mutex<Vec<TipEntry>> {
    static TIPS: OnceLock<Mutex<Vec<TipEntry>>> = OnceLock::new();
    TIPS.get_or_init(|| Mutex::new(builtin_tips()))
}

/// Built-in sampled tips: a chalk-like grain and a square.
fn builtin_tips() -> Vec<TipEntry> {
    let n = 64u32;
    let mut rng = brush::Rng::new(7);
    let chalk: Vec<f32> = (0..n * n)
        .map(|i| {
            let (x, y) = ((i % n) as f32 + 0.5 - 32.0, (i / n) as f32 + 0.5 - 32.0);
            let d = (x * x + y * y).sqrt() / 32.0;
            let grain = rng.unit();
            if d > 1.0 {
                0.0
            } else {
                ((1.0 - d).min(0.35) / 0.35) * (0.35 + 0.65 * grain)
            }
        })
        .collect();
    let square = vec![1.0; 32 * 32];
    let mk = |id: &str, name: &str, w: u32, h: u32, data: Vec<f32>, spacing: f32| TipEntry {
        info: BrushTipInfo {
            id: id.into(),
            name: name.into(),
            width: w,
            height: h,
            diameter: w.max(h) as f32,
            spacing: Some(spacing),
            sampled: true,
        },
        tip: Tip::sampled(SampledTip::new(name, w, h, data).expect("valid tip")),
    };
    vec![
        mk("builtin:chalk", "Chalk", n, n, chalk, 0.15),
        mk("builtin:square", "Square", 32, 32, square, 0.1),
    ]
}

fn lookup_tip(id: &str) -> Result<Tip> {
    tip_library()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .find(|t| t.info.id == id)
        .map(|t| t.tip.clone())
        .ok_or_else(|| failure(format!("unknown brush tip {id:?}")))
}

/// Imports a Photoshop `.abr` file into the tip library (for this run);
/// returns the tips it added. Damaged records are skipped.
#[uniffi::export]
pub fn import_abr(path: String) -> Result<Vec<BrushTipInfo>> {
    let bytes = std::fs::read(&path).map_err(|e| failure(format!("{path}: {e}")))?;
    let file = brush::abr::parse(&bytes).map_err(engine_err)?;
    let stem = std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "abr".into());
    let mut lib = tip_library().lock().unwrap_or_else(|e| e.into_inner());
    let mut added = Vec::new();
    for (i, b) in file.brushes.iter().enumerate() {
        let (tip, diameter) = b.to_tip();
        let (width, height, sampled) = match &b.tip {
            brush::abr::AbrTip::Sampled(s) => (s.width, s.height, true),
            brush::abr::AbrTip::Computed { .. } => (1, 1, false),
        };
        let id = format!("abr:{stem}:{i}");
        let info = BrushTipInfo {
            id: id.clone(),
            name: if b.name.is_empty() {
                format!("{stem} {}", i + 1)
            } else {
                b.name.clone()
            },
            width,
            height,
            diameter,
            spacing: b.spacing,
            sampled,
        };
        lib.retain(|t| t.info.id != id);
        lib.push(TipEntry {
            info: info.clone(),
            tip,
        });
        added.push(info);
    }
    if added.is_empty() {
        return Err(failure(format!("{path}: no brushes could be read")));
    }
    Ok(added)
}

/// Every tip in the library (built-ins first, then imports in order).
#[uniffi::export]
pub fn brush_tips() -> Vec<BrushTipInfo> {
    tip_library()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .map(|t| t.info.clone())
        .collect()
}

/// A preview of tip `id` (or the computed round tip of `hardness` for
/// `round:<hardness>`) at most `max_px` on the long edge.
#[uniffi::export]
pub fn brush_tip_preview(id: String, max_px: u32) -> Result<BrushTipImage> {
    let max_px = max_px.clamp(4, 256);
    let tip = match id.strip_prefix("round:") {
        Some(h) => Tip::round(h.parse::<f32>().unwrap_or(1.0).clamp(0.0, 1.0)),
        None => lookup_tip(&id)?,
    };
    let (w, h) = match &tip.shape {
        brush::TipShape::Sampled(s) => {
            let k = max_px as f32 / s.width.max(s.height) as f32;
            (
                ((s.width as f32 * k).round() as u32).max(1),
                ((s.height as f32 * k).round() as u32).max(1),
            )
        }
        brush::TipShape::Round { .. } => (max_px, max_px),
    };
    let size = w.max(h) as f32 - 2.0;
    let dab = brush::Dab {
        x: w as f32 * 0.5,
        y: h as f32 * 0.5,
        size: size.max(1.0),
        angle: tip.angle,
        roundness: tip.roundness,
        flow: 1.0,
        opacity: 1.0,
        flip_x: false,
        flip_y: false,
        dual: Vec::new(),
        index: 0,
    };
    let mut pixels = Vec::with_capacity((w * h) as usize);
    for y in 0..h {
        for x in 0..w {
            let (c, _) = tip.coverage(&dab, x as f32 + 0.5 - dab.x, y as f32 + 0.5 - dab.y);
            pixels.push((c.clamp(0.0, 1.0) * 255.0 + 0.5) as u8);
        }
    }
    Ok(BrushTipImage {
        width: w,
        height: h,
        pixels,
    })
}

fn make_brush(b: &PaintBrush, tool: StrokeTool, color: [f32; 3], mode: PaintMode) -> Result<Brush> {
    let mut tip = match &b.tip_id {
        Some(id) => lookup_tip(id)?,
        None => Tip::round(b.hardness.clamp(0.0, 1.0)),
    };
    if !b.angle.is_finite() || !b.roundness.is_finite() {
        return Err(failure("brush angle and roundness must be finite"));
    }
    tip.angle += b.angle.to_radians();
    tip.roundness = (tip.roundness * b.roundness).clamp(0.01, 1.0);
    let symmetry = {
        let (x, y, n) = (b.symmetry_x, b.symmetry_y, b.symmetry_count.max(1));
        match b.symmetry {
            PaintSymmetry::None => Symmetry::None,
            PaintSymmetry::Vertical => Symmetry::Vertical { x },
            PaintSymmetry::Horizontal => Symmetry::Horizontal { y },
            PaintSymmetry::Dual => Symmetry::Dual { x, y },
            PaintSymmetry::Diagonal => Symmetry::Diagonal { cx: x, cy: y },
            PaintSymmetry::Radial => Symmetry::Radial {
                cx: x,
                cy: y,
                count: n,
            },
            PaintSymmetry::Mandala => Symmetry::Mandala {
                cx: x,
                cy: y,
                count: n,
            },
        }
    };
    let mut brush = Brush {
        tip,
        size: b.size,
        spacing: b.spacing,
        flow: b.flow,
        opacity: b.opacity,
        color,
        blend: if tool == StrokeTool::Brush {
            parse_blend(&b.blend_mode)?
        } else {
            BlendMode::Normal
        },
        mode,
        smoothing: Smoothing {
            string_length: if b.smoothing.is_finite() {
                b.smoothing.max(0.0)
            } else {
                0.0
            },
            catch_up: true,
        },
        symmetry,
        ..Brush::default()
    };
    if b.pressure_size {
        brush.dynamics.size.control = Control::Pressure;
    }
    if b.pressure_opacity {
        brush.dynamics.opacity.control = Control::Pressure;
    }
    if b.pressure_flow {
        brush.dynamics.flow.control = Control::Pressure;
    }
    brush.validate().map_err(engine_err)?;
    Ok(brush)
}

// ─────────────────────────────── raster helpers ───────────────────────────────

/// Dense straight RGBA copy of `r` box-downsampled to `level` (missing
/// channels: gray expands to RGB, alpha reads 1 for 1/3-channel rasters).
fn dense_level(r: &Raster, level: u8) -> Result<(u32, u32, Vec<[f32; 4]>)> {
    let e = r.extent().at_level(level);
    let (w, h) = (e.width as usize, e.height as usize);
    let k = 1usize << level;
    let mut sum = vec![[0.0f32; 4]; w * h];
    let mut cnt = vec![0u32; w * h];
    let ch = usize::from(r.channels());
    let (cols, rows) = r.grid();
    let mut buf = Vec::new();
    for ty in 0..rows {
        for tx in 0..cols {
            r.read_tile(tx, ty, &mut buf).map_err(engine_err)?;
            let l = r.layout(tx, ty);
            let (stride, plane) = (l.stride(), l.plane_len());
            let (ox, oy) = ((tx * TILE_SIZE) as usize, (ty * TILE_SIZE) as usize);
            for y in 0..l.extent.height as usize {
                let row = ((oy + y) / k) * w;
                for x in 0..l.extent.width as usize {
                    let i = row + (ox + x) / k;
                    let s = &mut sum[i];
                    let j = y * stride + x;
                    let px = match ch {
                        1 => {
                            let v = buf[j];
                            [v, v, v, 1.0]
                        }
                        3 => [buf[j], buf[plane + j], buf[2 * plane + j], 1.0],
                        _ => [
                            buf[j],
                            buf[plane + j],
                            buf[2 * plane + j],
                            buf[3 * plane + j],
                        ],
                    };
                    for c in 0..4 {
                        s[c] += px[c];
                    }
                    cnt[i] += 1;
                }
            }
        }
    }
    for (s, n) in sum.iter_mut().zip(&cnt) {
        let n = (*n).max(1) as f32;
        for v in s.iter_mut() {
            *v /= n;
        }
    }
    Ok((e.width, e.height, sum))
}

/// The finest level whose extent has at most `limit` pixels.
fn analysis_level(canvas: Extent, limit: u64) -> u8 {
    (0..compositor::render::MAX_LEVEL)
        .find(|&l| {
            let e = canvas.at_level(l);
            u64::from(e.width) * u64::from(e.height) <= limit
        })
        .unwrap_or(compositor::render::MAX_LEVEL - 1)
}

/// A dense selection mask of `sel` at `level` (`None` = nothing).
fn selection_mask(sel: Option<&Raster>, canvas: Extent, level: u8) -> Result<Mask> {
    match sel {
        None => {
            let e = canvas.at_level(level);
            Ok(Mask::new(e.width, e.height))
        }
        Some(r) => {
            let (w, h, px) = dense_level(r, level)?;
            Mask::from_vec(w, h, px.into_iter().map(|p| p[0]).collect()).map_err(engine_err)
        }
    }
}

/// The level-0 selection raster of a mask computed at `level` (bilinear
/// upsampling limited to where the mask is non-zero); `None` when empty.
fn mask_to_raster(m: &Mask, level: u8, canvas: Extent) -> Result<Option<Raster>> {
    let Some(b) = m.bounds(0.0) else {
        return Ok(None);
    };
    let s = (1i64 << level) as f32;
    let full = Rect::of_extent(canvas);
    let region = Rect::new(
        (b.x0 - 1) * (1 << level),
        (b.y0 - 1) * (1 << level),
        (b.x1 + 1) * (1 << level),
        (b.y1 + 1) * (1 << level),
    )
    .intersect(&full);
    let mut r = Raster::new(canvas, 1, compositor::Depth::F32, 0.0);
    let (mw, mh) = (m.width() as i64, m.height() as i64);
    r.edit_region(region, 0, |x, y, p| {
        if level == 0 {
            p[0] = m.get(i64::from(x), i64::from(y)).clamp(0.0, 1.0);
            return;
        }
        let fx = ((x as f32 + 0.5) / s - 0.5).clamp(0.0, (mw - 1) as f32);
        let fy = ((y as f32 + 0.5) / s - 0.5).clamp(0.0, (mh - 1) as f32);
        let (x0, y0) = (fx.floor() as i64, fy.floor() as i64);
        let (ax, ay) = (fx - x0 as f32, fy - y0 as f32);
        let (x1, y1) = ((x0 + 1).min(mw - 1), (y0 + 1).min(mh - 1));
        let a = m.get(x0, y0) + (m.get(x1, y0) - m.get(x0, y0)) * ax;
        let c = m.get(x0, y1) + (m.get(x1, y1) - m.get(x0, y1)) * ax;
        p[0] = (a + (c - a) * ay).clamp(0.0, 1.0);
    })
    .map_err(engine_err)?;
    Ok(Some(r))
}

/// A canvas-sized selection raster holding `m` at `origin` (level 0).
fn submask_to_raster(m: &Mask, origin: (i64, i64), canvas: Extent) -> Result<Option<Raster>> {
    let Some(b) = m.bounds(0.0) else {
        return Ok(None);
    };
    let region = Rect::new(
        b.x0 + origin.0,
        b.y0 + origin.1,
        b.x1 + origin.0,
        b.y1 + origin.1,
    )
    .intersect(&Rect::of_extent(canvas));
    if region.is_empty() {
        return Ok(None);
    }
    let mut r = Raster::new(canvas, 1, compositor::Depth::F32, 0.0);
    r.edit_region(region, 0, |x, y, p| {
        p[0] = m
            .get(i64::from(x) - origin.0, i64::from(y) - origin.1)
            .clamp(0.0, 1.0);
    })
    .map_err(engine_err)?;
    Ok(Some(r))
}

/// The selection over `region` (level 0) as a dense mask.
fn read_submask(sel: &Raster, region: Rect) -> Result<Mask> {
    let (w, h) = (region.width().max(0) as u32, region.height().max(0) as u32);
    let mut data = vec![sel.default_value(); w as usize * h as usize];
    let mut buf = Vec::new();
    let ts = i64::from(TILE_SIZE);
    if w == 0 || h == 0 {
        return Mask::from_vec(w, h, data).map_err(engine_err);
    }
    for ty in (region.y0 / ts)..=((region.y1 - 1) / ts) {
        for tx in (region.x0 / ts)..=((region.x1 - 1) / ts) {
            if sel.tile(tx as u32, ty as u32).is_none() {
                continue;
            }
            sel.read_tile(tx as u32, ty as u32, &mut buf)
                .map_err(engine_err)?;
            let l = sel.layout(tx as u32, ty as u32);
            let (ox, oy) = (tx * ts, ty * ts);
            let tr = region.intersect(&Rect::new(
                ox,
                oy,
                ox + i64::from(l.extent.width),
                oy + i64::from(l.extent.height),
            ));
            for y in tr.y0..tr.y1 {
                for x in tr.x0..tr.x1 {
                    data[((y - region.y0) as usize) * w as usize + (x - region.x0) as usize] =
                        buf[(y - oy) as usize * l.stride() + (x - ox) as usize];
                }
            }
        }
    }
    Mask::from_vec(w, h, data).map_err(engine_err)
}

/// `cur op new` with "no selection" as empty; an empty result is `None`.
fn combine_selection(
    cur: Option<&Raster>,
    new: Option<Raster>,
    op: SelectionOp,
) -> Result<Option<Raster>> {
    let out = match (op, cur, new) {
        (SelectionOp::Replace, _, n) => n,
        (SelectionOp::Add, None, n) => n,
        (SelectionOp::Add, Some(c), None) => Some(c.clone()),
        (SelectionOp::Add, Some(c), Some(n)) => {
            Some(docsel::combine(c, &n, Combine::Add).map_err(engine_err)?)
        }
        (SelectionOp::Subtract, None, _) => None,
        (SelectionOp::Subtract, Some(c), None) => Some(c.clone()),
        (SelectionOp::Subtract, Some(c), Some(n)) => {
            Some(docsel::combine(c, &n, Combine::Subtract).map_err(engine_err)?)
        }
        (SelectionOp::Intersect, Some(c), Some(n)) => {
            Some(docsel::combine(c, &n, Combine::Intersect).map_err(engine_err)?)
        }
        (SelectionOp::Intersect, _, _) => None,
    };
    Ok(out.filter(|r| io::selection_bounds(r).is_some()))
}

fn image_from(w: u32, h: u32, px: Vec<[f32; 4]>) -> Image {
    Image {
        width: w,
        height: h,
        data: px,
    }
}

fn scale_points(points: &[ToolPoint], level: u8) -> Vec<[f32; 2]> {
    let s = (1u32 << level) as f32;
    points.iter().map(|p| [p.x / s, p.y / s]).collect()
}

fn check_points(points: &[ToolPoint]) -> Result<()> {
    if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return Err(failure("points must be finite"));
    }
    Ok(())
}

/// Pixel-exact bounds of a layer raster's content (alpha > 0 for RGBA).
fn content_bounds(r: &Raster) -> Option<Rect> {
    if r.channels() != 4 {
        return r.bounds();
    }
    let mut out = Rect::default();
    let mut buf = Vec::new();
    for ((tx, ty), slot) in r.slots() {
        if slot.tile.is_none() || r.read_tile(tx, ty, &mut buf).is_err() {
            continue;
        }
        let l = r.layout(tx, ty);
        let (stride, plane) = (l.stride(), l.plane_len());
        let (ox, oy) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
        for y in 0..l.extent.height as usize {
            let row =
                &buf[3 * plane + y * stride..3 * plane + y * stride + l.extent.width as usize];
            if let Some(first) = row.iter().position(|v| *v > 0.0) {
                let last = row.iter().rposition(|v| *v > 0.0).unwrap_or(first);
                out = out.union(&Rect::new(
                    ox + first as i64,
                    oy + y as i64,
                    ox + last as i64 + 1,
                    oy + y as i64 + 1,
                ));
            }
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Tile deltas of `live` over the tiles touched by `rect`.
fn live_tiles(live: &Raster, rect: Rect) -> Vec<TileDelta> {
    let r = rect.intersect(&Rect::of_extent(live.extent()));
    if r.is_empty() {
        return Vec::new();
    }
    let ts = i64::from(TILE_SIZE);
    let mut out = Vec::new();
    for ty in (r.y0 / ts)..=((r.y1 - 1) / ts) {
        for tx in (r.x0 / ts)..=((r.x1 - 1) / ts) {
            out.push(TileDelta {
                tx: tx as u32,
                ty: ty as u32,
                tile: live.tile(tx as u32, ty as u32).cloned(),
            });
        }
    }
    out
}

// ─────────────────────────────── resampling ───────────────────────────────

/// Decoded source tiles for random access (premultiplied RGBA).
struct Grid {
    cols: u32,
    rows: u32,
    channels: usize,
    extent: Extent,
    outside: [f32; 4],
    tiles: Vec<Option<Vec<f32>>>,
}

impl Grid {
    fn new(r: &Raster) -> Result<Self> {
        let (cols, rows) = r.grid();
        let channels = usize::from(r.channels());
        let mut tiles = vec![None; (cols * rows) as usize];
        for ((tx, ty), slot) in r.slots() {
            if slot.tile.is_none() {
                continue;
            }
            let mut buf = Vec::new();
            r.read_tile(tx, ty, &mut buf).map_err(engine_err)?;
            if channels == 4 {
                let n = r.layout(tx, ty).plane_len();
                for i in 0..n {
                    let a = buf[3 * n + i];
                    for c in 0..3 {
                        buf[c * n + i] *= a;
                    }
                }
            }
            tiles[(ty * cols + tx) as usize] = Some(buf);
        }
        let d = r.default_value();
        let mut outside = [0.0; 4];
        for v in outside.iter_mut().take(channels) {
            *v = d;
        }
        if channels == 4 {
            for c in 0..3 {
                outside[c] *= outside[3];
            }
        }
        Ok(Self {
            cols,
            rows,
            channels,
            extent: r.extent(),
            outside,
            tiles,
        })
    }

    #[inline]
    fn get(&self, x: i64, y: i64) -> [f32; 4] {
        if x < 0 || y < 0 || x >= i64::from(self.extent.width) || y >= i64::from(self.extent.height)
        {
            return self.outside;
        }
        let (tx, ty) = (x as u32 / TILE_SIZE, y as u32 / TILE_SIZE);
        let Some(t) = &self.tiles[(ty * self.cols + tx) as usize] else {
            // A default-filled tile: interior reads the default.
            let _ = self.rows;
            return self.outside;
        };
        let w = (self.extent.width - tx * TILE_SIZE).min(TILE_SIZE) as usize;
        let h = (self.extent.height - ty * TILE_SIZE).min(TILE_SIZE) as usize;
        let n = w * h;
        let i = (y as u32 % TILE_SIZE) as usize * w + (x as u32 % TILE_SIZE) as usize;
        let mut p = [0.0; 4];
        for (c, v) in p.iter_mut().enumerate().take(self.channels) {
            *v = t[c * n + i];
        }
        p
    }

    fn sample(&self, x: f32, y: f32, interp: TransformInterpolation) -> [f32; 4] {
        match interp {
            TransformInterpolation::Nearest => self.get(x.floor() as i64, y.floor() as i64),
            TransformInterpolation::Bilinear => {
                let (fx, fy) = (x - 0.5, y - 0.5);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (ax, ay) = (fx - ix, fy - iy);
                let (ix, iy) = (ix as i64, iy as i64);
                let (p00, p10, p01, p11) = (
                    self.get(ix, iy),
                    self.get(ix + 1, iy),
                    self.get(ix, iy + 1),
                    self.get(ix + 1, iy + 1),
                );
                std::array::from_fn(|c| {
                    let a = p00[c] + (p10[c] - p00[c]) * ax;
                    let b = p01[c] + (p11[c] - p01[c]) * ax;
                    a + (b - a) * ay
                })
            }
            TransformInterpolation::Bicubic => {
                let (fx, fy) = (x - 0.5, y - 0.5);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (wx, wy) = (cubic(fx - ix), cubic(fy - iy));
                let (ix, iy) = (ix as i64, iy as i64);
                let mut o = [0.0; 4];
                for (j, wyj) in wy.iter().enumerate() {
                    for (i, wxi) in wx.iter().enumerate() {
                        let p = self.get(ix + i as i64 - 1, iy + j as i64 - 1);
                        for c in 0..4 {
                            o[c] += p[c] * wxi * wyj;
                        }
                    }
                }
                o
            }
        }
    }
}

fn cubic(t: f32) -> [f32; 4] {
    // Catmull-Rom (a = −0.5).
    let a = -0.5f32;
    let w = |x: f32| {
        let x = x.abs();
        if x < 1.0 {
            (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
        } else if x < 2.0 {
            a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
        } else {
            0.0
        }
    };
    [w(1.0 + t), w(t), w(1.0 - t), w(2.0 - t)]
}

/// Tile deltas replacing `src` by its image under `t` (current → new canvas
/// coordinates), computed on all cores. Uncovered pixels read the raster
/// default (transparent content, reveal-all masks).
fn resample(src: &Raster, t: &Affine, interp: TransformInterpolation) -> Result<Vec<TileDelta>> {
    let canvas = Rect::of_extent(src.extent());
    let src_bounds = match src.bounds() {
        Some(b) => b,
        None => return Ok(Vec::new()),
    };
    let inv = t
        .inverse()
        .ok_or_else(|| failure("the transform is singular"))?;
    let grid = Grid::new(src)?;
    let dst_bounds = t.map_rect(&src_bounds).inflate(2).intersect(&canvas);
    let region = src_bounds.union(&dst_bounds).intersect(&canvas);
    if region.is_empty() {
        return Ok(Vec::new());
    }
    let ts = i64::from(TILE_SIZE);
    let mut jobs = Vec::new();
    for ty in (region.y0 / ts)..=((region.y1 - 1) / ts) {
        for tx in (region.x0 / ts)..=((region.x1 - 1) / ts) {
            jobs.push((tx as u32, ty as u32));
        }
    }
    let next = AtomicUsize::new(0);
    let out = Mutex::new(Vec::with_capacity(jobs.len()));
    let failed = Mutex::new(None);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(jobs.len())
        .max(1);
    let channels = grid.channels;
    let default = src.default_value();
    let depth = src.depth();
    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                loop {
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&(tx, ty)) = jobs.get(k) else { break };
                    let layout = src.layout(tx, ty);
                    let (ox, oy) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
                    let tr = Rect::new(
                        ox,
                        oy,
                        ox + i64::from(layout.extent.width),
                        oy + i64::from(layout.extent.height),
                    );
                    let cover = tr.intersect(&dst_bounds);
                    let had = src.tile(tx, ty).is_some();
                    if cover.is_empty() {
                        if had {
                            out.lock()
                                .expect("out")
                                .push(TileDelta { tx, ty, tile: None });
                        }
                        continue;
                    }
                    let n = layout.plane_len();
                    let stride = layout.stride();
                    let mut buf = vec![default; n * channels];
                    let mut any = false;
                    for y in cover.y0..cover.y1 {
                        for x in cover.x0..cover.x1 {
                            let (sx, sy) = inv.apply(x as f64 + 0.5, y as f64 + 0.5);
                            let mut p = grid.sample(sx as f32, sy as f32, interp);
                            if channels == 4 {
                                let a = p[3].clamp(0.0, 1.0);
                                if a <= 1e-7 {
                                    p = [0.0; 4];
                                } else {
                                    for v in p.iter_mut().take(3) {
                                        *v = (*v / a).clamp(0.0, 1.0);
                                    }
                                    p[3] = a;
                                }
                            }
                            let i = (y - oy) as usize * stride + (x - ox) as usize;
                            for c in 0..channels {
                                let v = p[c].clamp(0.0, 1.0);
                                any |= v != default;
                                buf[c * n + i] = v;
                            }
                        }
                    }
                    if !any {
                        if had {
                            out.lock()
                                .expect("out")
                                .push(TileDelta { tx, ty, tile: None });
                        }
                        continue;
                    }
                    match tile_from_f32(TileCoord::new(0, tx, ty), layout, depth, buf) {
                        Ok(tile) => out.lock().expect("out").push(TileDelta {
                            tx,
                            ty,
                            tile: Some(tile),
                        }),
                        Err(e) => *failed.lock().expect("failed") = Some(e),
                    }
                }
            });
        }
    });
    if let Some(e) = failed.into_inner().expect("failed") {
        return Err(engine_err(e));
    }
    Ok(out.into_inner().expect("out"))
}

// ─────────────────────────────── segmentation ───────────────────────────────

/// The engine's segmenter as a `selection` segmentation model.
struct Segment<'a>(&'a mut dyn MaskSegmenter);

impl Segment<'_> {
    fn run(&mut self, img: &Image, req: SegmentRequest) -> EngineResult<Mask> {
        let raw = img
            .data
            .iter()
            .flat_map(|p| [p[0], p[1], p[2]].map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8))
            .collect();
        let rgb = image::RgbImage::from_raw(img.width, img.height, raw)
            .ok_or_else(|| EngineError::invalid("image", "size"))?;
        let v = self
            .0
            .segment(&rgb, &req)
            .map_err(|e| EngineError::internal(format!("segmentation: {e:#}")))?;
        Mask::from_vec(img.width, img.height, v)
    }
}

impl selection::ml::SegmentModel for Segment<'_> {
    fn subject(&mut self, img: &Image) -> EngineResult<Mask> {
        self.run(img, SegmentRequest::Subject)
    }
    fn sky(&mut self, img: &Image) -> EngineResult<Mask> {
        self.run(img, SegmentRequest::Sky)
    }
    fn object(&mut self, img: &Image, prompt: &selection::ml::ObjectPrompt) -> EngineResult<Mask> {
        let (w, h) = (img.width as f32, img.height as f32);
        let n = |p: [f32; 2]| [(p[0] / w).clamp(0.0, 1.0), (p[1] / h).clamp(0.0, 1.0)];
        let req = match prompt {
            selection::ml::ObjectPrompt::Box(b) => {
                let (a, c) = (n([b[0], b[1]]), n([b[2], b[3]]));
                SegmentRequest::Prompts {
                    clicks: vec![],
                    boxes: vec![[a[0], a[1], c[0], c[1]]],
                }
            }
            selection::ml::ObjectPrompt::Points(ps) => SegmentRequest::Prompts {
                clicks: ps
                    .iter()
                    .filter(|(_, pos)| *pos)
                    .map(|(p, _)| n(*p))
                    .collect(),
                boxes: vec![],
            },
            selection::ml::ObjectPrompt::Lasso(_) => {
                return Err(EngineError::invalid("prompt", "lasso is reduced to a box"));
            }
        };
        self.run(img, req)
    }
}

/// Which ml selection to run.
enum MlKind {
    Subject,
    Sky,
    Object(f32, f32),
}

// ─────────────────────────────── session calls ───────────────────────────────

impl DocumentSession {
    /// A copy of the live document (cheap: states are shared) for work
    /// done without the session lock.
    fn live_copy(&self) -> Result<Document> {
        let st = self.shared.lock()?;
        st.open()?;
        Ok(st.live().clone())
    }

    /// The image the image-driven tools analyse at `level`: the visible
    /// composite (`sample_all`) or the primary selected layer's pixels.
    fn analysis_image(&self, doc: &Document, level: u8, sample_all: bool) -> Result<Image> {
        let layer = if sample_all {
            None
        } else {
            let st = self.shared.lock()?;
            let s = doc.state();
            st.selected
                .iter()
                .rev()
                .filter_map(|id| s.find(LayerId(*id)))
                .find_map(|l| match &l.kind {
                    LayerKind::Pixel(r) => Some(r.clone()),
                    _ => None,
                })
        };
        match layer {
            Some(r) => {
                let (w, h, px) = dense_level(&r, level)?;
                Ok(image_from(w, h, px))
            }
            None => {
                let (w, h, v) = self.shared.render.read_level(doc, level)?;
                let px = v.as_chunks::<4>().0.to_vec();
                Ok(image_from(w, h, px))
            }
        }
    }

    /// Combines `new` into the current selection and records it.
    fn apply_selection(
        &self,
        new: Option<Raster>,
        op: SelectionOp,
        label: &str,
    ) -> Result<DocumentUpdate> {
        let cur = {
            let st = self.shared.lock()?;
            st.open()?;
            st.live().state().selection.clone()
        };
        let out = combine_selection(cur.as_deref(), new, op)?;
        self.edit(DocOp::SetSelection { selection: out }, Some(label))
    }

    fn canvas(&self) -> Result<Extent> {
        let st = self.shared.lock()?;
        st.open()?;
        Ok(st.live().state().canvas)
    }

    fn run_ml(&self, kind: MlKind, op: SelectionOp, label: &str) -> Result<DocumentUpdate> {
        let doc = self.live_copy()?;
        let canvas = doc.state().canvas;
        // Models see a few hundred pixels; the guided-filter refinement runs
        // at the analysis level.
        let level = analysis_level(canvas, ANALYSIS_PIXELS / 2);
        let img = self.analysis_image(&doc, level, true)?;
        let engine = self
            .shared
            .engine
            .upgrade()
            .ok_or_else(|| failure("engine closed"))?;
        let mask = engine
            .with_segmenter(|seg| {
                let mut model = Segment(seg);
                let s = (1u32 << level) as f32;
                let m = match kind {
                    MlKind::Subject => selection::ml::select_subject(&mut model, &img, 2),
                    MlKind::Sky => selection::ml::select_sky(&mut model, &img, 2),
                    MlKind::Object(x, y) => selection::ml::select_object(
                        &mut model,
                        &img,
                        &selection::ml::ObjectPrompt::Points(vec![([x / s, y / s], true)]),
                        2,
                    ),
                };
                m.map_err(|e| anyhow::anyhow!("{e}"))
            })
            .map_err(failure)?;
        // Soft model output: keep it soft, drop the faint tail.
        let mut mask = mask;
        mask.data_mut().iter_mut().for_each(|v| {
            if *v < 0.02 {
                *v = 0.0
            }
        });
        self.apply_selection(mask_to_raster(&mask, level, canvas)?, op, label)
    }
}

#[uniffi::export]
impl DocumentSession {
    // ─────────────────────────── painting ───────────────────────────

    /// Starts a stroke of `tool` on `layer`'s pixels or mask with `brush` and
    /// `color` (the foreground colour; a mask takes its luminance). Commits a
    /// pending drag first; a stroke already open is committed. The selection
    /// limits the paint; a transparency-locked layer keeps its alpha.
    pub fn begin_stroke(
        &self,
        layer: u64,
        target: StrokeTarget,
        tool: StrokeTool,
        brush: PaintBrush,
        color: PaintColor,
    ) -> Result<()> {
        if self.shared.lock()?.tools.stroke.is_some() {
            self.end_stroke()?;
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        if self.commit_pending(&mut st, None)?.is_some() {
            self.update(&mut st, &before, None, true);
        }
        let state = st.doc.state().clone();
        let l = find(&state, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let mut prelude = Vec::new();
        let (base, paint_target, lock_alpha) = match target {
            StrokeTarget::Pixels => match &l.kind {
                LayerKind::Pixel(r) => {
                    (r.clone(), PaintTarget::Content, l.props.locks.transparency)
                }
                _ => {
                    return Err(failure(
                        "only pixel layers can be painted; paint its mask or add a pixel layer",
                    ));
                }
            },
            StrokeTarget::Mask => match &l.mask {
                Some(m) => (m.raster.clone(), PaintTarget::Mask, false),
                None => {
                    let m = LayerMask::reveal_all(state.canvas, state.depth);
                    let r = m.raster.clone();
                    prelude.push(DocOp::SetMask {
                        id: LayerId(layer),
                        mask: Some(m),
                    });
                    (r, PaintTarget::Mask, false)
                }
            },
        };
        let clone_source = |sample_all: bool| -> Result<CloneSource> {
            let (src, dx, dy) = st
                .tools
                .clone_source
                .ok_or_else(|| failure("set a clone source first: ⌥-click where to sample"))?;
            let source = if sample_all {
                let (w, h, rgba) = self.shared.render.read_level(st.live(), 0)?;
                Some(raster_from_rgba(
                    Extent::new(w, h),
                    state.depth,
                    &rgba,
                    true,
                )?)
            } else if src == layer {
                None
            } else {
                match &find(&state, src)?.kind {
                    LayerKind::Pixel(r) if paint_target == PaintTarget::Content => Some(r.clone()),
                    _ => None,
                }
            };
            Ok(CloneSource {
                offset: [dx, dy],
                source,
            })
        };
        let mode = match tool {
            StrokeTool::Brush => PaintMode::Paint,
            StrokeTool::Eraser => PaintMode::Erase,
            StrokeTool::Clone => PaintMode::Clone(clone_source(brush.sample_all_layers)?),
            StrokeTool::Heal => PaintMode::Heal(clone_source(brush.sample_all_layers)?),
        };
        if !color.rgb().iter().all(|c| c.is_finite()) {
            return Err(failure("colour must be finite"));
        }
        let b = make_brush(&brush, tool, color.rgb().map(|c| c.clamp(0.0, 1.0)), mode)?;
        let seed = state.rev ^ layer.rotate_left(17);
        let mut stroke = Stroke::new(b, &base, seed).map_err(engine_err)?;
        if let Some(sel) = &state.selection {
            stroke = stroke.with_selection(sel).map_err(engine_err)?;
        }
        let mut scratch = st.doc.clone();
        scratch.set_max_states(2);
        for op in prelude.iter().cloned() {
            scratch.apply(op).map_err(engine_err)?;
        }
        st.scratch = Some(scratch);
        st.tools.stroke = Some(ActiveStroke {
            layer,
            target: paint_target,
            tool,
            stroke,
            live: base,
            prelude,
            lock_alpha,
            dabs: 0,
        });
        Ok(())
    }

    /// Adds the pointer samples of one display frame to the open stroke and
    /// shows the result (one frame). Returns the pixels that changed.
    pub fn stroke_points(&self, points: Vec<StrokeSample>) -> Result<StrokeFrame> {
        let t0 = Instant::now();
        let mut st = self.shared.lock()?;
        st.open()?;
        let st = &mut *st;
        let s = st
            .tools
            .stroke
            .as_mut()
            .ok_or_else(|| failure("no stroke is open"))?;
        let before = s.stroke.dabs().len();
        let mut dirty = Rect::default();
        for p in &points {
            if !(p.x.is_finite() && p.y.is_finite() && p.pressure.is_finite()) {
                return Err(failure("stroke samples must be finite"));
            }
            let ip = InputPoint {
                x: p.x,
                y: p.y,
                pressure: p.pressure.clamp(0.0, 1.0),
                tilt: [p.tilt_x.clamp(-1.0, 1.0), p.tilt_y.clamp(-1.0, 1.0)],
                rotation: 0.0,
                time: p.timestamp,
            };
            if let Some(r) = s.stroke.add_point(ip).map_err(engine_err)? {
                dirty = dirty.union(&r);
            }
        }
        let dabs = (s.stroke.dabs().len() - before) as u32;
        s.dabs += dabs;
        let total = s.dabs;
        let dirty = (!dirty.is_empty()).then_some(dirty);
        if let Some(d) = dirty {
            paint_live(s, d)?;
            let op = DocOp::PaintTiles {
                id: LayerId(s.layer),
                target: s.target,
                tiles: live_tiles(&s.live, d),
                dirty: d,
            };
            if st.scratch.is_none() {
                // Another edit dropped the preview: show the stroke again.
                let mut scratch = st.doc.clone();
                scratch.set_max_states(2);
                for op in s.prelude.iter().cloned() {
                    scratch.apply(op).map_err(engine_err)?;
                }
                st.scratch = Some(scratch);
            }
            st.scratch
                .as_mut()
                .expect("scratch")
                .apply(op)
                .map_err(engine_err)?;
            st.epoch += 1;
            // A frame only: rows and thumbnails refresh when the stroke ends.
            self.shared.render.request(Vec::new(), false, st.epoch);
        }
        Ok(StrokeFrame {
            dirty_rect: dirty.and_then(DocRect::of),
            dabs,
            total_dabs: total,
            raster_ms: t0.elapsed().as_secs_f64() * 1000.0,
            epoch: st.epoch,
        })
    }

    /// Ends the open stroke: one history node ("Brush Tool", "Eraser",
    /// "Clone Stamp", "Healing Brush"). A stroke that placed no dab records
    /// nothing.
    pub fn end_stroke(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let mut s = st
            .tools
            .stroke
            .take()
            .ok_or_else(|| failure("no stroke is open"))?;
        if let Some(d) = s.stroke.finish().map_err(engine_err)? {
            paint_live(&mut s, d)?;
        }
        st.scratch = None;
        let Some(total) = s.stroke.dirty() else {
            return Ok(self.update(&mut st, &before, None, false));
        };
        let mut ops = std::mem::take(&mut s.prelude);
        ops.push(DocOp::PaintTiles {
            id: LayerId(s.layer),
            target: s.target,
            tiles: live_tiles(&s.live, total),
            dirty: total,
        });
        let op = if ops.len() == 1 {
            ops.pop().expect("one op")
        } else {
            DocOp::Batch(ops)
        };
        let applied = st.doc.apply(op).map_err(engine_err)?;
        st.labels
            .insert(applied.node, history_label(s.tool).to_owned());
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    /// Drops the open stroke without recording anything.
    pub fn cancel_stroke(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        st.tools.stroke = None;
        st.scratch = None;
        Ok(self.update(&mut st, &before, None, false))
    }

    /// Where the clone stamp and healing brush sample: the pixel painted at
    /// `p` comes from `p + (dx, dy)` of `layer` (the painted layer when it is
    /// the same). Session state; strokes keep the offset (aligned).
    pub fn set_clone_source(&self, layer: u64, dx: f32, dy: f32) -> Result<()> {
        if !dx.is_finite() || !dy.is_finite() {
            return Err(failure("offset must be finite"));
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        find(st.live().state(), layer)?;
        st.tools.clone_source = Some((layer, dx, dy));
        Ok(())
    }

    // ─────────────────────────── selections ───────────────────────────

    /// A marquee selection of `x, y, width × height` (level 0), feathered
    /// by `feather` pixels, anti-aliased when `antialias` (ellipses).
    #[allow(clippy::too_many_arguments)]
    pub fn select_marquee(
        &self,
        shape: MarqueeShape,
        x: f64,
        y: f64,
        width: f64,
        height: f64,
        feather: f32,
        antialias: bool,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        if ![x, y, width, height].iter().all(|v| v.is_finite()) || !feather.is_finite() {
            return Err(failure("marquee must be finite"));
        }
        let feather = feather.clamp(0.0, 1000.0);
        let canvas = self.canvas()?;
        let full = Rect::of_extent(canvas);
        let (label, raster) = match shape {
            MarqueeShape::Rect => {
                let r = Rect::new(
                    x.round() as i64,
                    y.round() as i64,
                    (x + width).round() as i64,
                    (y + height).round() as i64,
                );
                if r.intersect(&full).is_empty() {
                    return Err(failure("the marquee is outside the canvas"));
                }
                (
                    "Rectangular Marquee",
                    Some(io::rect_selection(canvas, r, feather)?),
                )
            }
            MarqueeShape::Ellipse => {
                if width < 1.0 || height < 1.0 {
                    return Err(failure("the ellipse is empty"));
                }
                let pad = (2.0 * feather).ceil() as i64 + 2;
                let origin = (x.floor() as i64 - pad, y.floor() as i64 - pad);
                let (w, h) = (
                    (width.ceil() as i64 + 2 * pad + 1) as u32,
                    (height.ceil() as i64 + 2 * pad + 1) as u32,
                );
                let m = selection::marquee::ellipse(
                    w,
                    h,
                    [
                        (x - origin.0 as f64) as f32,
                        (y - origin.1 as f64) as f32,
                        (x + width - origin.0 as f64) as f32,
                        (y + height - origin.1 as f64) as f32,
                    ],
                    antialias,
                );
                let m = selection::ops::feather(&m, feather);
                ("Elliptical Marquee", submask_to_raster(&m, origin, canvas)?)
            }
            MarqueeShape::Row => {
                let yy = y.floor() as i64;
                if !(0..i64::from(canvas.height)).contains(&yy) {
                    return Err(failure("the row is outside the canvas"));
                }
                (
                    "Single Row Marquee",
                    Some(
                        docsel::rect(canvas, Rect::new(0, yy, full.x1, yy + 1))
                            .map_err(engine_err)?,
                    ),
                )
            }
            MarqueeShape::Column => {
                let xx = x.floor() as i64;
                if !(0..i64::from(canvas.width)).contains(&xx) {
                    return Err(failure("the column is outside the canvas"));
                }
                (
                    "Single Column Marquee",
                    Some(
                        docsel::rect(canvas, Rect::new(xx, 0, xx + 1, full.y1))
                            .map_err(engine_err)?,
                    ),
                )
            }
        };
        self.apply_selection(raster, op, label)
    }

    /// A lasso selection through `points` (level 0): freehand path, polygon
    /// vertices, or magnetic anchors (snapped to edges between anchors).
    pub fn select_lasso(
        &self,
        points: Vec<ToolPoint>,
        kind: LassoKind,
        feather: f32,
        antialias: bool,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        check_points(&points)?;
        if points.len() < 3 && kind != LassoKind::Magnetic || points.len() < 2 {
            return Err(failure("a lasso needs at least 3 points"));
        }
        let feather = if feather.is_finite() {
            feather.clamp(0.0, 1000.0)
        } else {
            0.0
        };
        let canvas = self.canvas()?;
        let (label, raster) = match kind {
            LassoKind::Free | LassoKind::Polygon => {
                let pad = (2.0 * feather).ceil() + 2.0;
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for p in &points {
                    x0 = x0.min(p.x);
                    y0 = y0.min(p.y);
                    x1 = x1.max(p.x);
                    y1 = y1.max(p.y);
                }
                let (x0, y0) = (
                    (x0 - pad).floor().max(-1.0) as i64,
                    (y0 - pad).floor().max(-1.0) as i64,
                );
                let (x1, y1) = (
                    ((x1 + pad).ceil() as i64).min(i64::from(canvas.width) + 1),
                    ((y1 + pad).ceil() as i64).min(i64::from(canvas.height) + 1),
                );
                if x1 <= x0 || y1 <= y0 {
                    return Err(failure("the lasso is outside the canvas"));
                }
                let local: Vec<[f32; 2]> = points
                    .iter()
                    .map(|p| [p.x - x0 as f32, p.y - y0 as f32])
                    .collect();
                let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
                let m = if kind == LassoKind::Free {
                    selection::lasso::freehand(w, h, &local, antialias)
                } else {
                    selection::lasso::polygonal(w, h, &local, antialias)
                };
                let m = selection::ops::feather(&m, feather);
                (
                    if kind == LassoKind::Free {
                        "Lasso"
                    } else {
                        "Polygonal Lasso"
                    },
                    submask_to_raster(&m, (x0, y0), canvas)?,
                )
            }
            LassoKind::Magnetic => {
                let doc = self.live_copy()?;
                let level = analysis_level(canvas, ANALYSIS_PIXELS);
                let img = self.analysis_image(&doc, level, true)?;
                let s = (1u32 << level) as f32;
                let m = selection::lasso::magnetic(
                    &img,
                    &scale_points(&points, level),
                    (10.0 / s).max(2.0),
                    0.1,
                    antialias,
                );
                let m = selection::ops::feather(&m, feather / s);
                ("Magnetic Lasso", mask_to_raster(&m, level, canvas)?)
            }
        };
        self.apply_selection(raster, op, label)
    }

    /// The live-wire path a magnetic lasso would take from `from` to `to`
    /// (level 0), for drawing while the pointer moves.
    pub fn magnetic_path(&self, from: ToolPoint, to: ToolPoint) -> Result<Vec<ToolPoint>> {
        check_points(&[from, to])?;
        let doc = self.live_copy()?;
        let canvas = doc.state().canvas;
        let level = analysis_level(canvas, ANALYSIS_PIXELS);
        let img = self.analysis_image(&doc, level, true)?;
        let s = (1u32 << level) as f32;
        let lasso = selection::lasso::MagneticLasso::new(&img, 0.1);
        let w = (10.0 / s).max(2.0);
        let a = lasso.snap([from.x / s, from.y / s], w);
        let b = lasso.snap([to.x / s, to.y / s], w);
        Ok(lasso
            .path(a, b, 32)
            .into_iter()
            .map(|p| ToolPoint {
                x: p[0] * s,
                y: p[1] * s,
            })
            .collect())
    }

    /// Magic wand at `(x, y)`: pixels within `tolerance` (0–255 levels per
    /// channel) of the clicked colour, connected to it when `contiguous`,
    /// from the composite when `sample_all`, else the selected layer.
    #[allow(clippy::too_many_arguments)]
    pub fn select_wand(
        &self,
        x: f32,
        y: f32,
        tolerance: f32,
        contiguous: bool,
        sample_all: bool,
        antialias: bool,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        let doc = self.live_copy()?;
        let canvas = doc.state().canvas;
        if !(x.is_finite() && y.is_finite())
            || x < 0.0
            || y < 0.0
            || x >= canvas.width as f32
            || y >= canvas.height as f32
        {
            return Err(failure("click inside the canvas"));
        }
        let level = analysis_level(canvas, ANALYSIS_PIXELS);
        let img = self.analysis_image(&doc, level, sample_all)?;
        let s = (1u32 << level) as f32;
        let seed = (
            ((x / s) as u32).min(img.width - 1),
            ((y / s) as u32).min(img.height - 1),
        );
        let m = selection::wand::magic_wand(
            &img,
            seed,
            &selection::wand::WandOptions {
                tolerance: if tolerance.is_finite() {
                    tolerance.clamp(0.0, 255.0)
                } else {
                    32.0
                },
                contiguous,
                antialias,
                sample_size: 1,
            },
        );
        self.apply_selection(mask_to_raster(&m, level, canvas)?, op, "Magic Wand")
    }

    /// Quick Selection: grows a region from a brush stroke of `radius`
    /// pixels through similar colour and texture; `op` is `Add` (default
    /// in Photoshop), `Subtract` or `Replace`.
    pub fn select_quick(
        &self,
        stroke: Vec<ToolPoint>,
        radius: f32,
        sample_all: bool,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        check_points(&stroke)?;
        if stroke.is_empty() {
            return Err(failure("quick selection needs a stroke"));
        }
        let doc = self.live_copy()?;
        let canvas = doc.state().canvas;
        let level = analysis_level(canvas, ANALYSIS_PIXELS);
        let img = self.analysis_image(&doc, level, sample_all)?;
        let s = (1u32 << level) as f32;
        let m = selection::quick::quick_select(
            &img,
            &scale_points(&stroke, level),
            &selection::quick::QuickOptions {
                radius: (radius.max(1.0) / s).max(1.0),
                ..Default::default()
            },
            None,
            false,
        );
        self.apply_selection(mask_to_raster(&m, level, canvas)?, op, "Quick Selection")
    }

    /// Select ▸ Color Range: pixels near `color` in Lab, softened by
    /// `fuzziness` (0–200), from the composite.
    pub fn select_color_range(
        &self,
        color: PaintColor,
        fuzziness: f32,
        op: SelectionOp,
    ) -> Result<DocumentUpdate> {
        if !color.rgb().iter().all(|c| c.is_finite()) || !fuzziness.is_finite() {
            return Err(failure("colour and fuzziness must be finite"));
        }
        let doc = self.live_copy()?;
        let canvas = doc.state().canvas;
        let level = analysis_level(canvas, ANALYSIS_PIXELS);
        let img = self.analysis_image(&doc, level, true)?;
        let m = selection::range::color_range(&img, &[color.rgb()], fuzziness.clamp(0.0, 200.0));
        self.apply_selection(mask_to_raster(&m, level, canvas)?, op, "Color Range")
    }

    /// Select ▸ Subject (on-device segmentation of the composite).
    pub fn select_subject(&self, op: SelectionOp) -> Result<DocumentUpdate> {
        self.run_ml(MlKind::Subject, op, "Select Subject")
    }

    /// Select ▸ Sky.
    pub fn select_sky(&self, op: SelectionOp) -> Result<DocumentUpdate> {
        self.run_ml(MlKind::Sky, op, "Select Sky")
    }

    /// Object Selection: the object under `(x, y)` (level 0).
    pub fn select_object(&self, x: f32, y: f32, op: SelectionOp) -> Result<DocumentUpdate> {
        if !x.is_finite() || !y.is_finite() {
            return Err(failure("point must be finite"));
        }
        self.run_ml(MlKind::Object(x, y), op, "Object Selection")
    }

    /// Select ▸ All.
    pub fn select_all(&self) -> Result<DocumentUpdate> {
        let canvas = self.canvas()?;
        self.edit(
            DocOp::SetSelection {
                selection: Some(Raster::new(canvas, 1, compositor::Depth::F32, 1.0)),
            },
            Some("Select All"),
        )
    }

    /// Select ▸ Deselect (`clear_selection`).
    pub fn select_none(&self) -> Result<DocumentUpdate> {
        self.clear_selection()
    }

    /// Select ▸ Inverse.
    pub fn select_inverse(&self) -> Result<DocumentUpdate> {
        let (canvas, cur) = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state();
            (s.canvas, s.selection.clone())
        };
        let cur = cur.ok_or_else(|| failure("there is no selection to invert"))?;
        let full = Raster::new(canvas, 1, compositor::Depth::F32, 1.0);
        let out = docsel::combine(&full, &cur, Combine::Subtract).map_err(engine_err)?;
        let out = io::selection_bounds(&out).is_some().then_some(out);
        self.edit(DocOp::SetSelection { selection: out }, Some("Inverse"))
    }

    /// Select ▸ Modify ▸ Border / Smooth / Expand / Contract / Feather by
    /// `px` pixels.
    pub fn modify_selection(&self, kind: SelectionModify, px: f32) -> Result<DocumentUpdate> {
        if !px.is_finite() || px < 0.0 {
            return Err(failure("amount must be ≥ 0"));
        }
        let px = px.min(500.0);
        let (canvas, cur) = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state();
            (s.canvas, s.selection.clone())
        };
        let cur = cur.ok_or_else(|| failure("there is no selection"))?;
        let bounds = io::selection_bounds(&cur).unwrap_or_default();
        let grow = match kind {
            SelectionModify::Contract => 2,
            SelectionModify::Feather => (2.0 * px).ceil() as i64 + 2,
            _ => px.ceil() as i64 + 2,
        };
        let region = bounds.inflate(grow).intersect(&Rect::of_extent(canvas));
        let m = read_submask(&cur, region)?;
        let out = match kind {
            SelectionModify::Border => selection::ops::border(&m, px),
            SelectionModify::Smooth => selection::ops::smooth(&m, px),
            SelectionModify::Expand => selection::ops::grow(&m, px),
            SelectionModify::Contract => selection::ops::contract(&m, px),
            SelectionModify::Feather => selection::ops::feather(&m, px),
        };
        let label = match kind {
            SelectionModify::Border => "Border",
            SelectionModify::Smooth => "Smooth",
            SelectionModify::Expand => "Expand",
            SelectionModify::Contract => "Contract",
            SelectionModify::Feather => "Feather",
        };
        let raster = submask_to_raster(&out, (region.x0, region.y0), canvas)?;
        self.edit(DocOp::SetSelection { selection: raster }, Some(label))
    }

    /// Select and Mask: refines the selection edge against the composite.
    /// `interactive`: shown live (no history) until a non-interactive call
    /// records one "Refine Edge" node or `cancel_refine_edge` restores it;
    /// every interactive call refines the selection as it was before the
    /// first one.
    pub fn refine_edge(
        &self,
        params: RefineEdgeParams,
        interactive: bool,
    ) -> Result<DocumentUpdate> {
        let p = params;
        if ![p.radius, p.smooth, p.feather, p.contrast, p.shift_edge]
            .iter()
            .all(|v| v.is_finite())
        {
            return Err(failure("refine parameters must be finite"));
        }
        let (doc, base) = {
            let mut st = self.shared.lock()?;
            st.open()?;
            let base = match st.tools.refine_base.clone() {
                Some(b) => b,
                None => st
                    .doc
                    .state()
                    .selection
                    .clone()
                    .ok_or_else(|| failure("there is no selection to refine"))?,
            };
            if interactive {
                st.tools.refine_base = Some(base.clone());
            }
            (st.doc.clone(), base)
        };
        let canvas = doc.state().canvas;
        let level = analysis_level(canvas, ANALYSIS_PIXELS);
        let img = self.analysis_image(&doc, level, true)?;
        let m = selection_mask(Some(&base), canvas, level)?;
        let s = (1u32 << level) as f32;
        let refined = selection::refine::refine_edge(
            &m,
            &img,
            &selection::refine::RefineParams {
                radius: p.radius.max(0.0) / s,
                smart_radius: p.smart_radius,
                smooth: p.smooth.max(0.0) / s,
                feather: p.feather.max(0.0) / s,
                contrast: p.contrast.clamp(0.0, 1.0),
                shift_edge: p.shift_edge / s,
                ..Default::default()
            },
        )
        .map_err(engine_err)?;
        let raster = mask_to_raster(&refined, level, canvas)?;
        let op = DocOp::SetSelection { selection: raster };
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        if interactive {
            let mut scratch = st.doc.clone();
            scratch.set_max_states(2);
            let applied = scratch.apply(op).map_err(engine_err)?;
            st.scratch = Some(scratch);
            return Ok(self.update(&mut st, &before, Some(&applied), false));
        }
        st.tools.refine_base = None;
        st.scratch = None;
        self.commit_pending(&mut st, None)?;
        let applied = st.doc.apply(op).map_err(engine_err)?;
        st.labels.insert(applied.node, "Refine Edge".into());
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    /// Ends an interactive Refine Edge without changing the selection.
    pub fn cancel_refine_edge(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        st.tools.refine_base = None;
        st.scratch = None;
        Ok(self.update(&mut st, &before, None, false))
    }

    /// Marching ants: the 0.5 iso-contours of the (live) selection traced at
    /// pyramid `level` (coarser is faster; the host passes its viewport
    /// level), in level-0 canvas pixels. Empty without a selection. Cached
    /// per selection and level.
    pub fn selection_outline(&self, level: u8) -> Result<Vec<OutlinePolyline>> {
        let (sel, canvas) = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state();
            (s.selection.clone(), s.canvas)
        };
        let Some(sel) = sel else {
            return Ok(Vec::new());
        };
        // Never trace more than ~8 MP.
        let level = level
            .min(compositor::render::MAX_LEVEL - 1)
            .max(analysis_level(canvas, 8_000_000));
        {
            let st = self.shared.lock()?;
            if let Some((w, l, o)) = &st.tools.outline
                && *l == level
                && w.upgrade().is_some_and(|w| Arc::ptr_eq(&w, &sel))
            {
                return Ok((**o).clone());
            }
        }
        let m = selection_mask(Some(&sel), canvas, level)?;
        let s = (1u32 << level) as f32;
        let out: Vec<OutlinePolyline> = selection::contour::trace(&m, 0.5, 0.35)
            .into_iter()
            .map(|p| OutlinePolyline {
                points: p
                    .points
                    .iter()
                    .map(|q| ToolPoint {
                        x: q[0] * s,
                        y: q[1] * s,
                    })
                    .collect(),
                closed: p.closed,
            })
            .collect();
        let mut st = self.shared.lock()?;
        st.tools.outline = Some((Arc::downgrade(&sel), level, Arc::new(out.clone())));
        Ok(out)
    }

    /// Select ▸ Save Selection as channel `name` (replacing one of that
    /// name). Session state: channels are not written to files yet.
    pub fn save_selection(&self, name: String) -> Result<()> {
        if name.trim().is_empty() {
            return Err(failure("channel name is empty"));
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        let sel = st
            .live()
            .state()
            .selection
            .clone()
            .ok_or_else(|| failure("there is no selection to save"))?;
        st.tools.channels.insert(name, sel);
        Ok(())
    }

    /// Select ▸ Load Selection from channel `name`, combined by `op`.
    pub fn load_selection(&self, name: String, op: SelectionOp) -> Result<DocumentUpdate> {
        let ch = {
            let st = self.shared.lock()?;
            st.open()?;
            st.tools
                .channels
                .get(&name)
                .cloned()
                .ok_or_else(|| failure(format!("no channel named {name:?}")))?
        };
        self.apply_selection(Some((*ch).clone()), op, "Load Selection")
    }

    /// Saved selection channels, by name.
    pub fn selection_channels(&self) -> Result<Vec<String>> {
        Ok(self.shared.lock()?.tools.channels.keys().cloned().collect())
    }

    // ─────────────────────────── transform ───────────────────────────

    /// Free Transform of pixel `layers` (with their linked masks). Commits a
    /// pending drag or stroke first.
    pub fn begin_transform(&self, layers: Vec<u64>) -> Result<TransformInfo> {
        if layers.is_empty() {
            return Err(failure("select a layer to transform"));
        }
        if self.shared.lock()?.tools.stroke.is_some() {
            self.end_stroke()?;
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        if self.commit_pending(&mut st, None)?.is_some() {
            self.update(&mut st, &before, None, true);
        }
        let state = st.doc.state().clone();
        let mut captured = Vec::new();
        let mut bounds = Rect::default();
        for &id in &layers {
            let l = find(&state, id)?;
            if l.props.locks.position || l.props.locks.all || l.props.locks.pixels {
                return Err(failure(format!("layer {:?} is locked", l.props.name)));
            }
            let LayerKind::Pixel(r) = &l.kind else {
                return Err(failure(format!(
                    "{:?} is not a pixel layer (transforming adjustments, groups and smart objects is not supported yet)",
                    l.props.name
                )));
            };
            if let Some(b) = content_bounds(r) {
                bounds = bounds.union(&b);
            }
            let linked = !st.unlinked_masks.contains(&id);
            captured.push(TransformLayer {
                id,
                content: r.clone(),
                mask: l.mask.as_ref().filter(|_| linked).map(|m| m.raster.clone()),
            });
        }
        st.tools.transform = Some(ActiveTransform {
            layers: captured,
            matrix: Affine::IDENTITY,
            interpolation: TransformInterpolation::Bicubic,
        });
        Ok(TransformInfo {
            layers,
            bounds: DocRect::of(bounds),
        })
    }

    /// Shows the layers under `matrix` (current → new canvas pixels), live
    /// and without history (bilinear preview; `interpolation` is used by
    /// `commit_transform`).
    pub fn set_transform(
        &self,
        matrix: TransformMatrix,
        interpolation: TransformInterpolation,
    ) -> Result<DocumentUpdate> {
        let t = matrix.affine();
        if t.inverse().is_none() {
            return Err(failure("the transform must be finite and invertible"));
        }
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let op = {
            let tr = st
                .tools
                .transform
                .as_mut()
                .ok_or_else(|| failure("no transform is open"))?;
            tr.matrix = t;
            tr.interpolation = interpolation;
            transform_op(tr, &t, TransformInterpolation::Bilinear)?
        };
        let mut scratch = st.doc.clone();
        scratch.set_max_states(2);
        let applied = scratch.apply(op).map_err(engine_err)?;
        st.scratch = Some(scratch);
        Ok(self.update(&mut st, &before, Some(&applied), false))
    }

    /// Applies the transform set last as one "Free Transform" node.
    pub fn commit_transform(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        let tr = st
            .tools
            .transform
            .take()
            .ok_or_else(|| failure("no transform is open"))?;
        st.scratch = None;
        if tr.matrix == Affine::IDENTITY {
            return Ok(self.update(&mut st, &before, None, false));
        }
        let op = transform_op(&tr, &tr.matrix, tr.interpolation)?;
        let applied = st.doc.apply(op).map_err(engine_err)?;
        st.labels.insert(applied.node, "Free Transform".into());
        Ok(self.update(&mut st, &before, Some(&applied), true))
    }

    /// Drops the transform preview.
    pub fn cancel_transform(&self) -> Result<DocumentUpdate> {
        let mut st = self.shared.lock()?;
        st.open()?;
        let before = st.live().state().clone();
        st.tools.transform = None;
        st.scratch = None;
        Ok(self.update(&mut st, &before, None, false))
    }

    // ─────────────────────────── fill / delete / sample ───────────────────────────

    /// Edit ▸ Fill the selection (everything without one) of pixel layer
    /// `layer` at `opacity` (one history node).
    pub fn fill_selection(
        &self,
        layer: u64,
        fill: SelectionFill,
        opacity: f32,
    ) -> Result<DocumentUpdate> {
        let opacity = if opacity.is_finite() {
            opacity.clamp(0.0, 1.0)
        } else {
            return Err(failure("opacity must be finite"));
        };
        let op = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state().clone();
            drop(st);
            let l = find(&s, layer)?;
            let LayerKind::Pixel(r) = &l.kind else {
                return Err(failure("fill needs a pixel layer"));
            };
            let sel = s.selection.clone();
            let region = match &sel {
                Some(sel) => {
                    io::selection_bounds(sel).ok_or_else(|| failure("the selection is empty"))?
                }
                None => Rect::of_extent(s.canvas),
            };
            match fill {
                SelectionFill::Color { color } => {
                    let c = color.rgb().map(|v| v.clamp(0.0, 1.0));
                    let ch = r.channels();
                    let mut selpx = sel.as_deref().map(brush::pixels::Pixels::new);
                    compositor::paint_op(
                        &s,
                        LayerId(layer),
                        PaintTarget::Content,
                        region,
                        |x, y, p| {
                            let a = opacity
                                * selpx.as_mut().map_or(1.0, |m| {
                                    m.get(i64::from(x), i64::from(y))[0].clamp(0.0, 1.0)
                                });
                            *p = brush::composite(*p, c, a, BlendMode::Normal, ch, x, y, 0);
                        },
                    )
                    .map_err(engine_err)?
                }
                SelectionFill::ContentAware => {
                    let sel = sel.ok_or_else(|| failure("Content-Aware Fill needs a selection"))?;
                    content_aware_op(&s, layer, r, &sel, region, opacity)?
                }
            }
        };
        self.edit(op, Some("Fill"))
    }

    /// Edit ▸ Clear / ⌫: erases the selection on pixel layer `layer` to
    /// transparency; a Background layer (or an opaque 3-channel one) is
    /// filled with `background` instead.
    pub fn delete_selection(&self, layer: u64, background: PaintColor) -> Result<DocumentUpdate> {
        let op = {
            let st = self.shared.lock()?;
            st.open()?;
            let s = st.live().state().clone();
            drop(st);
            let l = find(&s, layer)?;
            let LayerKind::Pixel(r) = &l.kind else {
                return Err(failure("clearing needs a pixel layer"));
            };
            let sel = s
                .selection
                .clone()
                .ok_or_else(|| failure("there is no selection to clear"))?;
            let region =
                io::selection_bounds(&sel).ok_or_else(|| failure("the selection is empty"))?;
            let ch = r.channels();
            let fill_bg = l.props.background || ch != 4;
            let bg = background.rgb().map(|v| v.clamp(0.0, 1.0));
            let mut selpx = brush::pixels::Pixels::new(&sel);
            compositor::paint_op(
                &s,
                LayerId(layer),
                PaintTarget::Content,
                region,
                |x, y, p| {
                    let a = selpx.get(i64::from(x), i64::from(y))[0].clamp(0.0, 1.0);
                    if a <= 0.0 {
                        return;
                    }
                    if fill_bg {
                        *p = brush::composite(*p, bg, a, BlendMode::Normal, ch, x, y, 0);
                    } else {
                        p[3] *= 1.0 - a;
                    }
                },
            )
            .map_err(engine_err)?
        };
        self.edit(op, Some("Clear"))
    }

    /// The eyedropper: the mean colour of a `(2·radius + 1)²` square at
    /// `(x, y)` of the composite (`sample_all`) or of `layer`.
    pub fn sample_color(
        &self,
        x: f32,
        y: f32,
        sample_all: bool,
        layer: Option<u64>,
        radius: u32,
    ) -> Result<PaintColor> {
        let doc = self.live_copy()?;
        let s = doc.state().clone();
        if !(x.is_finite() && y.is_finite())
            || x < 0.0
            || y < 0.0
            || x >= s.canvas.width as f32
            || y >= s.canvas.height as f32
        {
            return Err(failure("sample inside the canvas"));
        }
        let (cx, cy) = (x as i64, y as i64);
        let r = i64::from(radius.min(50));
        let win =
            Rect::new(cx - r, cy - r, cx + r + 1, cy + r + 1).intersect(&Rect::of_extent(s.canvas));
        let mut sum = [0.0f64; 3];
        let mut n = 0.0f64;
        let raster = match (sample_all, layer) {
            (false, Some(id)) => match &find(&s, id)?.kind {
                LayerKind::Pixel(r) => Some(r.clone()),
                _ => None,
            },
            _ => None,
        };
        match raster {
            Some(ras) => {
                for yy in win.y0..win.y1 {
                    for xx in win.x0..win.x1 {
                        let p = ras.pixel(xx as u32, yy as u32);
                        let (p, a) = match ras.channels() {
                            1 => ([p[0]; 3], 1.0),
                            3 => ([p[0], p[1], p[2]], 1.0),
                            _ => ([p[0], p[1], p[2]], p[3]),
                        };
                        for c in 0..3 {
                            sum[c] += f64::from(p[c] * a);
                        }
                        n += f64::from(a);
                    }
                }
            }
            None => {
                let comp = compositor::Compositor::new(16 << 20);
                let ts = i64::from(TILE_SIZE);
                for ty in (win.y0 / ts)..=((win.y1 - 1) / ts) {
                    for tx in (win.x0 / ts)..=((win.x1 - 1) / ts) {
                        let tile = comp
                            .render_tile(&doc, TileCoord::new(0, tx as u32, ty as u32))
                            .map_err(engine_err)?;
                        let l = tile.layout();
                        let px = tile.samples::<f32>().map_err(engine_err)?;
                        let plane = l.plane_len();
                        let (ox, oy) = (tx * ts, ty * ts);
                        let tr = win.intersect(&Rect::new(
                            ox,
                            oy,
                            ox + i64::from(l.extent.width),
                            oy + i64::from(l.extent.height),
                        ));
                        for yy in tr.y0..tr.y1 {
                            for xx in tr.x0..tr.x1 {
                                let i = (yy - oy) as usize * l.stride() + (xx - ox) as usize;
                                let a = if l.channels >= 4 {
                                    px[3 * plane + i]
                                } else {
                                    1.0
                                };
                                for c in 0..3 {
                                    sum[c] += f64::from(px[c * plane + i] * a);
                                }
                                n += f64::from(a);
                            }
                        }
                    }
                }
            }
        }
        if n <= 1e-9 {
            return Err(failure("the sampled pixels are transparent"));
        }
        Ok(PaintColor {
            r: (sum[0] / n) as f32,
            g: (sum[1] / n) as f32,
            b: (sum[2] / n) as f32,
        })
    }
}

/// Composites the stroke over `rect` into its live raster.
fn paint_live(s: &mut ActiveStroke, rect: Rect) -> Result<()> {
    let ActiveStroke {
        stroke,
        live,
        lock_alpha,
        ..
    } = s;
    let lock = *lock_alpha;
    live.edit_region(rect, 0, |x, y, p| {
        let a = p[3];
        stroke.shade(x, y, p);
        if lock {
            p[3] = a;
        }
    })
    .map_err(engine_err)
}

/// The op replacing each captured layer by its image under `t`.
fn transform_op(tr: &ActiveTransform, t: &Affine, interp: TransformInterpolation) -> Result<DocOp> {
    let mut ops = Vec::new();
    for l in &tr.layers {
        let canvas = Rect::of_extent(l.content.extent());
        ops.push(DocOp::PaintTiles {
            id: LayerId(l.id),
            target: PaintTarget::Content,
            tiles: resample(&l.content, t, interp)?,
            dirty: canvas,
        });
        if let Some(m) = &l.mask {
            ops.push(DocOp::PaintTiles {
                id: LayerId(l.id),
                target: PaintTarget::Mask,
                tiles: resample(m, t, interp)?,
                dirty: canvas,
            });
        }
    }
    Ok(DocOp::Batch(ops))
}

/// Content-Aware Fill placeholder: the selection (> 0.5) replaced by the
/// harmonic (membrane) interpolation of its boundary, blended by the soft
/// selection and `opacity`.
fn content_aware_op(
    s: &DocState,
    layer: u64,
    r: &Raster,
    sel: &Raster,
    region: Rect,
    opacity: f32,
) -> Result<DocOp> {
    let rr = region.inflate(1).intersect(&Rect::of_extent(s.canvas));
    let (w, h) = (rr.width() as usize, rr.height() as usize);
    if (w * h) as u64 > 512 * 1024 {
        return Err(failure(
            "Content-Aware Fill (placeholder) handles selections up to 0.5 MP",
        ));
    }
    let mut src = brush::pixels::Pixels::new(r);
    let mut m = brush::pixels::Pixels::new(sel);
    let ch = r.channels();
    let mut dest = vec![[0.0f32; 4]; w * h];
    let mut alpha = vec![0.0f32; w * h];
    let mut omega = vec![false; w * h];
    for y in rr.y0..rr.y1 {
        for x in rr.x0..rr.x1 {
            let i = (y - rr.y0) as usize * w + (x - rr.x0) as usize;
            let a = m.get(x, y)[0].clamp(0.0, 1.0);
            alpha[i] = a;
            omega[i] = a > 0.5;
            let p = src.get(x, y);
            dest[i] = if ch == 4 {
                [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
            } else {
                p
            };
        }
    }
    let orig = dest.clone();
    let guide = vec![[0.0f32; 4]; w * h];
    let iters = brush::heal::default_iterations(w, h).min(1500);
    brush::heal::poisson_blend(
        &mut dest,
        &guide,
        &omega,
        w,
        h,
        usize::from(ch.min(4)),
        iters,
    );
    compositor::paint_op(s, LayerId(layer), PaintTarget::Content, rr, |x, y, p| {
        let i = (i64::from(y) - rr.y0) as usize * w + (i64::from(x) - rr.x0) as usize;
        let a = alpha[i] * opacity;
        if a <= 0.0 || !omega[i] && alpha[i] < 1.0 && a <= 0.0 {
            return;
        }
        let f = if omega[i] { dest[i] } else { orig[i] };
        let mut o = [0.0f32; 4];
        for c in 0..4 {
            o[c] = orig[i][c] + (f[c] - orig[i][c]) * a;
        }
        if ch == 4 {
            let al = o[3].clamp(0.0, 1.0);
            if al > 1e-6 {
                *p = [o[0] / al, o[1] / al, o[2] / al, al];
            }
        } else {
            *p = o;
        }
    })
    .map_err(engine_err)
}

#[doc(hidden)]
impl DocumentSession {
    /// Whether a stroke is open (tests).
    pub fn stroke_open(&self) -> bool {
        self.shared
            .lock()
            .map(|st| st.tools.stroke.is_some())
            .unwrap_or(false)
    }
}
