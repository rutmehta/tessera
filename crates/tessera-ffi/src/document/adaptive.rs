//! Filter ▸ Adaptive Wide Angle for the app (WP B5-20).
//!
//! The filter is the `adaptive_wide_angle` smart filter (crates/filters,
//! `CompositorFilters`): its params are a `transform::adaptive::Adaptive`
//! recipe stored verbatim, so a smart object keeps it editable across save
//! and reopen with no format change. Rendering is CPU only (the resident
//! displacement renderer takes `transform` stages; this is not one).
//!
//! - [`DocumentSession::begin_adaptive_wide_angle`] snapshots the target: a
//!   pixel layer's raster, or what smart filter `stage_index` of a smart
//!   object receives. It returns the recipe to edit (a default Manual
//!   rectilinear camera, or the stored recipe when re-editing) and a
//!   box-downsampled proxy size. Layers over 100 megapixels are refused here
//!   with a clear message (B5-20b: layers over the dense 16,777,216-vertex
//!   lattice render through `filters::adaptive_lattice`'s coarse lattice).
//! - `preview_adaptive_wide_angle` solves a uniformly scaled copy of the
//!   recipe on the proxy and renders it into an RGBA8 IOSurface (straight
//!   alpha, layer samples). It never touches the document or its history.
//!   Conflicting or degenerate constraints are errors here too.
//! - `commit_adaptive_wide_angle` renders at full resolution and installs
//!   ONE history node: a pixel layer's pixels (destructive, through the
//!   selection as Filter menu filters), or on a smart object the smart
//!   filter appended or the re-edited one replaced in place. Errors (solve
//!   failures, a changed layer, a cancel) never reach history. A cancel
//!   stops a pixel layer's render between output tiles. A smart object
//!   whose stack would exceed the compositor's CPU smart-filter pass limit
//!   (`FilterPassLimits::retained_bytes`, ≈ 33.5 MP) is refused before any
//!   render with a plain message (B5-20c; pixel layers are unaffected).
//! - `cancel_adaptive_wide_angle` drops the workspace.
//!
//! Camera models: Manual rectilinear or equidistant (fisheye) with a focal
//! length. No lens profiles. The 35 mm-equivalent focal length of a library
//! image (`FocalLengthIn35mmFilm` in the catalog) is offered as a suggestion.

use super::{DocumentSession, DocumentUpdate, Shared, find, raster_from_rgba};
use crate::{Result, failure, surface::Surface};
use compositor::{
    DocOp, LayerId, LayerKind, Raster, Rect, SmartFilter,
    render::smart_filters::{FilterContext, FilterPassLimits, SmartFilterEvaluator},
};
use engine_api::{jobs::CancellationToken, tile::Extent};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};
use transform::adaptive::{Adaptive, CameraModel, Projection};

/// The smart filter id.
pub const ADAPTIVE_WIDE_ANGLE_ID: &str = "adaptive_wide_angle";
const LABEL: &str = "Adaptive Wide Angle";
/// Longest proxy side for previews.
const PROXY: u32 = 768;
/// Default focal length when the image has none, 35 mm equivalent.
const DEFAULT_FOCAL_35MM: f64 = 24.0;

/// An open Adaptive Wide Angle workspace.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdaptiveWideAngleInfo {
    pub token: u64,
    pub layer: u64,
    /// The smart filter being re-edited (`None`: a new filter).
    pub stage_index: Option<u32>,
    pub smart_object: bool,
    /// Source (= recipe source and output) size in pixels.
    pub width: u32,
    pub height: u32,
    /// Preview proxy size and source pixels per proxy pixel.
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_factor: u32,
    /// The recipe to edit (`transform::adaptive::Adaptive` JSON, level-0
    /// source pixels): the stored one when re-editing.
    pub recipe_json: String,
    /// `FocalLengthIn35mmFilm` of the library image the document came from.
    pub exif_focal_35mm: Option<f64>,
}

/// A rendered preview (or the untouched source).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdaptiveWideAnglePreview {
    /// RGBA8 IOSurface, straight alpha, layer samples; retained until the
    /// next preview of this workspace (two alternate).
    pub surface_id: u32,
    pub width: u32,
    pub height: u32,
    pub original: bool,
    pub millis: f64,
}

struct Job {
    owner: Weak<Shared>,
    layer: u64,
    stage: Option<usize>,
    smart: bool,
    revision: u64,
    extent: Extent,
    /// Pixel layers: the layer raster and the selection it is written through.
    pixels: Option<(Raster, Option<Raster>, bool, compositor::Depth)>,
    proxy: Raster,
    factor: u32,
    surfaces: [Option<Arc<Surface>>; 2],
    flip: usize,
    cancel: Arc<AtomicBool>,
    /// The same cancel for the full-resolution render (checked per tile).
    stop: CancellationToken,
}

static NEXT: AtomicU64 = AtomicU64::new(1);
static JOBS: LazyLock<Mutex<HashMap<u64, Arc<Mutex<Job>>>>> = LazyLock::new(Default::default);

fn jobs() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<Mutex<Job>>>> {
    let mut m = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    m.retain(|_, j| {
        j.lock()
            .map(|j| j.owner.strong_count() > 0)
            .unwrap_or(false)
    });
    m
}

/// The engine's size limit as a user-facing refusal (`None`: supported).
pub(crate) fn size_refusal(width: u32, height: u32) -> Option<String> {
    filters::adaptive_lattice::size_refusal(width as usize, height as usize)
}

/// B5-20c: the refusal for a smart object over the compositor's CPU
/// smart-filter pass limit (A's decision: keep the limit, say it plainly).
pub(crate) const SMART_OBJECT_LIMIT_MESSAGE: &str = "Adaptive Wide Angle on a Smart Object is limited to \
about 33 MP. Rasterize the layer, or apply to a pixel layer.";
/// What the compositor's full-level CPU pass retains per smart-object pixel
/// while it renders the stack: the unmasked source and result, RGBA F32 each
/// (compositor `smart_filters::entry_bytes`, `canvas.area() * 32`).
const PASS_BYTES_PER_PIXEL: u64 = 32;
/// The compositor's error for that limit (`FilterPass::reserve`). Only a
/// fallback: the bridge carries it as text, and the up-front check below
/// catches the smart object's own entry; this also covers nested smart
/// objects whose entries add up past the limit.
const PASS_LIMIT_ERROR: &str = "CPU smart-filter pass retained results exceed configured limit";

/// Largest smart object (pixels) whose stack the compositor's CPU pass can
/// retain, from the real limit (`FilterPassLimits::default()`, 1 GiB):
/// 33,554,432.
pub(crate) fn smart_object_max_pixels() -> u64 {
    FilterPassLimits::default().retained_bytes as u64 / PASS_BYTES_PER_PIXEL
}

/// `Some(message)` when the compositor would refuse a smart object of
/// `extent` (its child canvas) for the pass limit.
pub(crate) fn smart_object_refusal(extent: Extent) -> Option<&'static str> {
    (extent.area() > smart_object_max_pixels()).then_some(SMART_OBJECT_LIMIT_MESSAGE)
}

/// Largest layer (pixels) Adaptive Wide Angle renders, for the app's copy.
#[uniffi::export]
pub fn adaptive_wide_angle_max_pixels() -> u64 {
    filters::adaptive_lattice::MAX_PIXELS as u64
}

/// Box-downsample straight RGBA by an integer factor (premultiplied average).
fn proxy_of(source: &Raster, factor: u32) -> Result<Raster> {
    let e = source.extent();
    let (pw, ph) = (e.width.div_ceil(factor), e.height.div_ceil(factor));
    let mut px = vec![0.0f32; pw as usize * ph as usize * 4];
    for py in 0..ph {
        for qx in 0..pw {
            let mut sum = [0.0f64; 4];
            let mut n = 0.0f64;
            for y in py * factor..((py + 1) * factor).min(e.height) {
                for x in qx * factor..((qx + 1) * factor).min(e.width) {
                    let p = source.pixel(x, y);
                    let a = f64::from(p[3]);
                    for c in 0..3 {
                        sum[c] += f64::from(p[c]) * a;
                    }
                    sum[3] += a;
                    n += 1.0;
                }
            }
            let o = (py as usize * pw as usize + qx as usize) * 4;
            let a = sum[3] / n;
            for c in 0..3 {
                px[o + c] = if sum[3] > 0.0 {
                    (sum[c] / sum[3]) as f32
                } else {
                    0.0
                };
            }
            px[o + 3] = a as f32;
        }
    }
    raster_from_rgba(Extent::new(pw, ph), compositor::Depth::F32, &px, false)
}

fn parse_recipe(json: &str) -> Result<(serde_json::Value, Adaptive)> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| failure(format!("{LABEL} recipe: {e}")))?;
    let recipe: Adaptive = serde_json::from_value(value.clone())
        .map_err(|e| failure(format!("{LABEL} recipe: {e}")))?;
    Ok((value, recipe))
}

/// The recipe for a source downsampled by `factor` to `extent`: sizes, focal
/// lengths, centre, crop, samples and the line tolerance scale together
/// (B5-20b: the coarse lattice's scaling, which also keeps the solver's
/// full-resolution sample count on short segments).
fn scaled(recipe: &Adaptive, factor: u32, extent: Extent) -> Adaptive {
    let (w, h) = (extent.width as usize, extent.height as usize);
    filters::adaptive_lattice::scale_recipe(recipe, 1.0 / f64::from(factor), [w, h, w, h])
}

fn evaluate(
    input: &Raster,
    params: serde_json::Value,
    cancel: &CancellationToken,
) -> Result<Raster> {
    Ok(filters::CompositorFilters.evaluate_with_cancel(
        input,
        &SmartFilter {
            name: ADAPTIVE_WIDE_ANGLE_ID.into(),
            enabled: true,
            params,
            ..Default::default()
        },
        &FilterContext {
            profile: None,
            canvas: input.extent(),
            level: 0,
        },
        cancel,
    )?)
}

/// Leading number of an EXIF display value ("24", "24 mm", "24.0").
fn leading_number(v: &str) -> Option<f64> {
    let t = v.trim();
    let end = t
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(t.len());
    t[..end]
        .parse::<f64>()
        .ok()
        .filter(|f| f.is_finite() && *f > 0.0 && *f <= 5000.0)
}

/// Inverts the camera-only reprojection `Adaptive::project` near `guess`
/// (damped Newton, finite-difference Jacobian).
fn unproject(recipe: &Adaptive, q: [f64; 2], guess: [f64; 2]) -> Option<[f64; 2]> {
    let (w, h) = (recipe.source_width as f64, recipe.source_height as f64);
    let clamp = |p: [f64; 2]| [p[0].clamp(0.0, w), p[1].clamp(0.0, h)];
    let mut p = clamp(guess);
    for _ in 0..60 {
        let f = recipe.project(p)?;
        let r = [f[0] - q[0], f[1] - q[1]];
        if r[0].hypot(r[1]) < 1e-7 {
            return Some(p);
        }
        let d = 1e-3;
        let fx = recipe.project([p[0] + d, p[1]])?;
        let fy = recipe.project([p[0], p[1] + d])?;
        let j = [
            [(fx[0] - f[0]) / d, (fy[0] - f[0]) / d],
            [(fx[1] - f[1]) / d, (fy[1] - f[1]) / d],
        ];
        let det = j[0][0] * j[1][1] - j[0][1] * j[1][0];
        if !det.is_finite() || det.abs() < 1e-12 {
            return None;
        }
        let step = [
            (j[1][1] * r[0] - j[0][1] * r[1]) / det,
            (j[0][0] * r[1] - j[1][0] * r[0]) / det,
        ];
        let len = step[0].hypot(step[1]);
        let k = if len > w.max(h) / 8.0 {
            w.max(h) / 8.0 / len
        } else {
            1.0
        };
        p = clamp([p[0] - k * step[0], p[1] - k * step[1]]);
    }
    let f = recipe.project(p)?;
    ((f[0] - q[0]).hypot(f[1] - q[1]) < 1e-3).then_some(p)
}

/// Output-pixel error allowed for one traced segment: the largest distance
/// between the projected midpoint of a straight source segment and the
/// straight projected edge. A fifth of the solver's default 0.25 px
/// constraint tolerance, leaving the rest to the mesh fit.
const CURVE_SAGITTA_PX: f64 = 0.05;
/// Most segments per traced line (1,025 input samples; the solver still
/// densifies every four source pixels, so this does not raise its sample
/// count).
const CURVE_MAX_SEGMENTS: usize = 1024;

/// The source-pixel image of the straight scene edge from `from` to `to`
/// under `recipe_json`'s camera: the curve a constraint line follows, as
/// interleaved x, y samples including both ends. Photoshop's Constraint
/// tool bends lines the same way; the constraint then only has to absorb
/// the camera model's error, not the lens curvature.
///
/// Segment count: the solver joins samples with straight source segments,
/// and a chord of length `L` on a curve of curvature `k` misses it by the
/// sagitta `k L² / 8`. Curvature in pixels falls as 1 / image size for a
/// given field of view, so a fixed count (the B5-20 cap of 64) grows the
/// error linearly with the photo (≈ 0.46 px on a full-width 24 MP line,
/// ≈ 1.9 px at 100 MP, against 0.25 px). The start is one segment per
/// 24 source px (`clamp(ceil(chord / 24), 4, 256)`); the count then
/// doubles, up to 1024, while any segment's measured output-space sagitta
/// exceeds `CURVE_SAGITTA_PX`. Halving `L` quarters the sagitta, so one or
/// two doublings cover 100 MP and strong fisheyes (a full-width 24 MP
/// horizon at f = 0.4 w takes one: 470 segments).
#[uniffi::export]
pub fn adaptive_wide_angle_curve(
    recipe_json: String,
    from: Vec<f64>,
    to: Vec<f64>,
) -> Result<Vec<f64>> {
    let (_, recipe) = parse_recipe(&recipe_json)?;
    let (a, b) = match (from.as_slice(), to.as_slice()) {
        ([ax, ay], [bx, by]) => ([*ax, *ay], [*bx, *by]),
        _ => return Err(failure("line ends are (x, y) pairs")),
    };
    let outside = || failure("that line reaches beyond the camera's field of view");
    let (qa, qb) = (
        recipe.project(a).ok_or_else(outside)?,
        recipe.project(b).ok_or_else(outside)?,
    );
    let chord = (b[0] - a[0]).hypot(b[1] - a[1]);
    if !chord.is_finite() || chord < 1.0 {
        return Err(failure("a constraint line needs two distinct ends"));
    }
    let trace = |n: usize| -> Result<Vec<[f64; 2]>> {
        let mut out = Vec::with_capacity(n + 1);
        let mut prev = a;
        for i in 0..=n {
            let t = i as f64 / n as f64;
            let p = if i == 0 {
                a
            } else if i == n {
                b
            } else {
                let q = [qa[0] + (qb[0] - qa[0]) * t, qa[1] + (qb[1] - qa[1]) * t];
                unproject(&recipe, q, prev).ok_or_else(outside)?
            };
            out.push(p);
            prev = p;
        }
        Ok(out)
    };
    // Distance of each segment's projected midpoint from the projected edge.
    let (dx, dy) = (qb[0] - qa[0], qb[1] - qa[1]);
    let span = dx.hypot(dy);
    let sagitta = |points: &[[f64; 2]]| -> f64 {
        if span.is_nan() || span <= 1e-9 {
            return 0.0;
        }
        points
            .windows(2)
            .map(|s| {
                let m = [(s[0][0] + s[1][0]) / 2., (s[0][1] + s[1][1]) / 2.];
                recipe.project(m).map_or(f64::INFINITY, |q| {
                    ((q[0] - qa[0]) * dy - (q[1] - qa[1]) * dx).abs() / span
                })
            })
            .fold(0.0, f64::max)
    };
    let mut n = ((chord / 24.0).ceil() as usize).clamp(4, 256);
    let mut points = trace(n)?;
    while n < CURVE_MAX_SEGMENTS && sagitta(&points) > CURVE_SAGITTA_PX {
        n *= 2;
        points = trace(n)?;
    }
    Ok(points.into_iter().flatten().collect())
}

impl DocumentSession {
    fn adaptive_job(&self, token: u64) -> Result<Arc<Mutex<Job>>> {
        let j = jobs().get(&token).cloned().ok_or_else(|| {
            failure("the Adaptive Wide Angle workspace is closed (cancelled, applied or replaced)")
        })?;
        let owner = j.lock().map_err(failure)?.owner.clone();
        if !Weak::ptr_eq(&owner, &Arc::downgrade(&self.shared)) {
            return Err(failure(
                "that Adaptive Wide Angle workspace belongs to another document",
            ));
        }
        Ok(j)
    }

    /// `FocalLengthIn35mmFilm` of the library image this document was made
    /// from, when the catalog has it.
    fn exif_focal_35mm(&self) -> Option<f64> {
        let id = self.shared.lock().ok()?.source_image_id.clone()?;
        let image = crate::parse_id(&id).ok()?;
        let engine = self.shared.engine.upgrade()?;
        let c = engine.lock().ok()?;
        let value: String = c
            .reader
            .query_row(
                "SELECT value FROM metadata WHERE image_id=? AND (key='FocalLengthIn35mmFilm' OR key LIKE '%:FocalLengthIn35mmFilm') LIMIT 1",
                [image.to_string()],
                |r| r.get(0),
            )
            .ok()?;
        leading_number(&value)
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Opens the Adaptive Wide Angle workspace on `layer` (a pixel layer, or
    /// a smart object: a new smart filter, or with `stage_index` the existing
    /// Adaptive Wide Angle smart filter to re-edit). Closes this document's
    /// previous workspace. No history node.
    pub fn begin_adaptive_wide_angle(
        &self,
        layer: u64,
        stage_index: Option<u32>,
    ) -> Result<AdaptiveWideAngleInfo> {
        let base = {
            let st = self.shared.lock()?;
            st.open()?;
            st.live().state().clone()
        };
        let l = find(&base, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let revision = super::layer_revision(l);
        let (source, pixels, stored, stage) = match &l.kind {
            LayerKind::Pixel(r) => {
                if stage_index.is_some() {
                    return Err(failure("only smart objects have smart filters to re-edit"));
                }
                if r.extent() != base.canvas {
                    return Err(failure("the layer's raster is not canvas-sized"));
                }
                if let Some(why) = size_refusal(r.extent().width, r.extent().height) {
                    return Err(failure(why));
                }
                let clip = base.selection.as_deref().cloned();
                let keep_alpha = l.props.locks.transparency;
                (
                    r.clone(),
                    Some((r.clone(), clip, keep_alpha, base.depth)),
                    None,
                    None,
                )
            }
            LayerKind::SmartObject(so) => {
                let (stage, stored) = match stage_index {
                    Some(i) => {
                        let f = so
                            .filters
                            .get(i as usize)
                            .ok_or_else(|| failure(format!("no smart filter {i}")))?;
                        if f.name != ADAPTIVE_WIDE_ANGLE_ID {
                            return Err(failure(format!(
                                "smart filter {i} is {}, not Adaptive Wide Angle",
                                f.name
                            )));
                        }
                        (i as usize, Some(f.params.clone()))
                    }
                    None => (so.filters.len(), None),
                };
                let child = so.state.canvas;
                if let Some(why) = size_refusal(child.width, child.height) {
                    return Err(failure(why));
                }
                let (extent, px) = Self::smart_stage_source(&base, layer, stage)?;
                let raster = raster_from_rgba(extent, compositor::Depth::F32, &px, false)?;
                (raster, None, stored, stage_index.map(|i| i as usize))
            }
            _ => {
                return Err(failure(
                    "Adaptive Wide Angle works on pixel layers and smart objects",
                ));
            }
        };
        let e = source.extent();
        let exif = self.exif_focal_35mm();
        let recipe_json = match stored {
            Some(params) => {
                let (_, recipe) = parse_recipe(&params.to_string())?;
                if [recipe.source_width, recipe.source_height]
                    != [e.width as usize, e.height as usize]
                {
                    return Err(failure(
                        "the Adaptive Wide Angle recipe does not match the smart object",
                    ));
                }
                params.to_string()
            }
            None => {
                let long = f64::from(e.width.max(e.height));
                let focal_px = exif.unwrap_or(DEFAULT_FOCAL_35MM) / 36.0 * long;
                let recipe = Adaptive::new(
                    e.width as usize,
                    e.height as usize,
                    CameraModel::Manual {
                        focal_px,
                        center: [f64::from(e.width) / 2.0, f64::from(e.height) / 2.0],
                        projection: Projection::Rectilinear,
                    },
                );
                serde_json::to_string(&recipe).map_err(failure)?
            }
        };
        let factor = e.width.max(e.height).div_ceil(PROXY).max(1);
        let proxy = proxy_of(&source, factor)?;
        let pe = proxy.extent();
        let info = AdaptiveWideAngleInfo {
            token: NEXT.fetch_add(1, Ordering::Relaxed),
            layer,
            stage_index,
            smart_object: pixels.is_none(),
            width: e.width,
            height: e.height,
            preview_width: pe.width,
            preview_height: pe.height,
            preview_factor: factor,
            recipe_json,
            exif_focal_35mm: exif,
        };
        let job = Job {
            owner: Arc::downgrade(&self.shared),
            layer,
            stage,
            smart: pixels.is_none(),
            revision,
            extent: e,
            pixels,
            proxy,
            factor,
            surfaces: [None, None],
            flip: 0,
            cancel: Arc::new(AtomicBool::new(false)),
            stop: CancellationToken::new(),
        };
        let mut m = jobs();
        let me = Arc::downgrade(&self.shared);
        m.retain(|_, j| match j.lock() {
            Ok(j) if Weak::ptr_eq(&j.owner, &me) => {
                j.cancel.store(true, Ordering::Relaxed);
                j.stop.cancel();
                false
            }
            _ => true,
        });
        m.insert(info.token, Arc::new(Mutex::new(job)));
        Ok(info)
    }

    /// Renders `recipe_json` (level-0 recipe; `None`: the untouched source)
    /// on the workspace proxy into an IOSurface. Blocking (a proxy solve):
    /// call off the main thread, latest wins. Never touches the document.
    pub fn preview_adaptive_wide_angle(
        &self,
        token: u64,
        recipe_json: Option<String>,
    ) -> Result<AdaptiveWideAnglePreview> {
        let started = Instant::now();
        let job = self.adaptive_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        let rendered = match &recipe_json {
            None => j.proxy.clone(),
            Some(json) => {
                let (_, recipe) = parse_recipe(json)?;
                if [recipe.source_width, recipe.source_height]
                    != [j.extent.width as usize, j.extent.height as usize]
                {
                    return Err(failure(
                        "the Adaptive Wide Angle recipe does not match the layer",
                    ));
                }
                let small = scaled(&recipe, j.factor, j.proxy.extent());
                evaluate(
                    &j.proxy,
                    serde_json::to_value(small).map_err(failure)?,
                    &CancellationToken::new(),
                )?
            }
        };
        let e = rendered.extent();
        j.flip ^= 1;
        let slot = j.flip;
        let surface = match &j.surfaces[slot] {
            Some(s) => s.clone(),
            None => {
                let s = Arc::new(Surface::create_rgba8(e.width, e.height).map_err(failure)?);
                j.surfaces[slot] = Some(s.clone());
                s
            }
        };
        surface
            .with_pixels(|px, stride| {
                for y in 0..e.height {
                    for x in 0..e.width {
                        let p = rendered.pixel(x, y);
                        let o = y as usize * stride + x as usize * 4;
                        for c in 0..4 {
                            px[o + c] = (p[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                }
            })
            .map_err(failure)?;
        Ok(AdaptiveWideAnglePreview {
            surface_id: surface.id(),
            width: e.width,
            height: e.height,
            original: recipe_json.is_none(),
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Applies `recipe_json` at full resolution as one history node (see the
    /// module docs). Blocking: call off the main thread;
    /// `cancel_adaptive_wide_angle` stops it before it writes. The workspace
    /// closes on success; on an error it stays open and history is unchanged.
    pub fn commit_adaptive_wide_angle(
        &self,
        token: u64,
        recipe_json: String,
    ) -> Result<DocumentUpdate> {
        let job = self.adaptive_job(token)?;
        let (layer, stage, smart, revision, extent, pixels, cancel, stop) = {
            let j = job.lock().map_err(failure)?;
            (
                j.layer,
                j.stage,
                j.smart,
                j.revision,
                j.extent,
                j.pixels.clone(),
                j.cancel.clone(),
                j.stop.clone(),
            )
        };
        let (params, recipe) = parse_recipe(&recipe_json)?;
        if [recipe.source_width, recipe.source_height]
            != [extent.width as usize, extent.height as usize]
        {
            return Err(failure(
                "the Adaptive Wide Angle recipe does not match the layer",
            ));
        }
        let update = if smart {
            // Refused before rendering: the stack cannot fit the compositor's
            // CPU smart-filter pass (`extent` is the child canvas).
            if let Some(why) = smart_object_refusal(extent) {
                return Err(failure(why));
            }
            // Validated by rendering the whole stack before the node is added.
            self.set_adapter_smart_filter(
                layer,
                LABEL,
                revision,
                stage,
                ADAPTIVE_WIDE_ANGLE_ID,
                params,
                &cancel,
            )
            .map_err(|e| match e {
                crate::BridgeError::Failure { message } if message.contains(PASS_LIMIT_ERROR) => {
                    failure(SMART_OBJECT_LIMIT_MESSAGE)
                }
                e => e,
            })?
        } else {
            let (raster, clip, keep_alpha, depth) =
                pixels.ok_or_else(|| failure("not a pixel layer"))?;
            let rendered = match evaluate(&raster, params, &stop) {
                Err(_) if stop.is_cancelled() => return Err(failure("cancelled")),
                r => r?,
            };
            if cancel.load(Ordering::Relaxed) || !jobs().contains_key(&token) {
                return Err(failure("cancelled"));
            }
            let full = Rect::of_extent(extent);
            let tiles = super::filtering::blended_tiles(
                &raster,
                &rendered,
                full,
                clip.as_ref(),
                keep_alpha,
                depth,
            )?;
            let op = DocOp::PaintTiles {
                id: LayerId(layer),
                target: compositor::PaintTarget::Content,
                tiles,
                dirty: full,
            };
            self.edit_layer_checked(layer, revision, op, LABEL, &cancel)?
        };
        jobs().remove(&token);
        Ok(update)
    }

    /// Closes the workspace without changing the document; a commit still
    /// rendering stops before it writes.
    pub fn cancel_adaptive_wide_angle(&self, token: u64) {
        let mut m = jobs();
        let mine = m.get(&token).is_some_and(|j| {
            j.lock().is_ok_and(|j| {
                let mine = Weak::ptr_eq(&j.owner, &Arc::downgrade(&self.shared));
                if mine {
                    j.cancel.store(true, Ordering::Relaxed);
                    j.stop.cancel();
                }
                mine
            })
        });
        if mine {
            m.remove(&token);
        }
    }
}

/// Test support (not exported over UniFFI).
impl DocumentSession {
    /// Reads back the last preview surface of the workspace as RGBA8 rows.
    #[doc(hidden)]
    pub fn adaptive_wide_angle_preview_pixels(&self, token: u64) -> Option<Vec<u8>> {
        let job = self.adaptive_job(token).ok()?;
        let j = job.lock().ok()?;
        let s = j.surfaces[j.flip].as_ref()?;
        let e = j.proxy.extent();
        let (w, h) = (e.width as usize, e.height as usize);
        s.with_pixels(|px, stride| {
            let mut out = Vec::with_capacity(w * h * 4);
            for y in 0..h {
                out.extend_from_slice(&px[y * stride..y * stride + w * 4]);
            }
            out
        })
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_values_parse_leading_numbers() {
        assert_eq!(leading_number("24"), Some(24.0));
        assert_eq!(leading_number(" 15 mm"), Some(15.0));
        assert_eq!(leading_number("10.5mm"), Some(10.5));
        assert_eq!(leading_number("n/a"), None);
        assert_eq!(leading_number("0"), None);
    }

    #[test]
    fn curves_follow_the_camera_and_are_straight_after_projection() {
        let fish = Adaptive::new(
            800,
            600,
            CameraModel::Manual {
                focal_px: 350.,
                center: [400., 300.],
                projection: Projection::Equidistant,
            },
        );
        let json = serde_json::to_string(&fish).unwrap();
        let c =
            adaptive_wide_angle_curve(json.clone(), vec![150., 100.], vec![150., 500.]).unwrap();
        let pts: Vec<[f64; 2]> = c.chunks(2).map(|p| [p[0], p[1]]).collect();
        // 400 px chord: 17 segments to start (one per 24 px), doubled while a
        // segment's output-space sagitta exceeds CURVE_SAGITTA_PX.
        let segments = pts.len() - 1;
        assert!(
            segments.is_multiple_of(17) && (segments / 17).is_power_of_two(),
            "{segments}"
        );
        for s in pts.windows(2) {
            let m = [(s[0][0] + s[1][0]) / 2., (s[0][1] + s[1][1]) / 2.];
            let q = fish.project(m).unwrap();
            let (a, b) = (
                fish.project([150., 100.]).unwrap(),
                fish.project([150., 500.]).unwrap(),
            );
            let err = ((q[0] - a[0]) * (b[1] - a[1]) - (q[1] - a[1]) * (b[0] - a[0])).abs()
                / (b[0] - a[0]).hypot(b[1] - a[1]);
            assert!(err <= CURVE_SAGITTA_PX, "segment sagitta {err}");
        }
        assert_eq!((pts[0], pts[pts.len() - 1]), ([150., 100.], [150., 500.]));
        // Barrel: the middle of a left-side vertical bows away from the centre.
        assert!(pts[pts.len() / 2][0] < 149.0, "{:?}", pts[pts.len() / 2]);
        let q: Vec<_> = pts.iter().map(|p| fish.project(*p).unwrap()).collect();
        let (a, b) = (q[0], q[q.len() - 1]);
        for p in &q {
            let err = ((p[0] - a[0]) * (b[1] - a[1]) - (p[1] - a[1]) * (b[0] - a[0])).abs()
                / (b[0] - a[0]).hypot(b[1] - a[1]);
            assert!(err < 1e-4, "{err}");
        }
        // Rectilinear: the chord itself.
        let rect = Adaptive::new(
            800,
            600,
            CameraModel::Manual {
                focal_px: 350.,
                center: [400., 300.],
                projection: Projection::Rectilinear,
            },
        );
        let c = adaptive_wide_angle_curve(
            serde_json::to_string(&rect).unwrap(),
            vec![100., 100.],
            vec![700., 100.],
        )
        .unwrap();
        assert!(c.chunks(2).all(|p| (p[1] - 100.).abs() < 1e-6));
        assert!(adaptive_wide_angle_curve(json.clone(), vec![1., 1.], vec![1., 1.]).is_err());
        assert!(adaptive_wide_angle_curve(json, vec![1.], vec![1., 1.]).is_err());
    }

    #[test]
    fn the_smart_object_limit_comes_from_the_compositor_pass_limit() {
        assert_eq!(smart_object_max_pixels(), 33_554_432);
        let e = |w, h| Extent {
            width: w,
            height: h,
        };
        assert_eq!(smart_object_refusal(e(6000, 5500)), None);
        assert_eq!(
            smart_object_refusal(e(8192, 4096)),
            None,
            "exactly the limit"
        );
        assert_eq!(
            smart_object_refusal(e(6000, 6000)),
            Some(SMART_OBJECT_LIMIT_MESSAGE)
        );
    }

    #[test]
    fn size_refusal_is_the_absolute_pixel_limit() {
        // B5-20b: real photo sizes render through the coarse lattice.
        for (w, h) in [
            (4095, 4095),
            (4096, 4096),
            (5212, 3468),
            (6000, 4000),
            (10000, 10000),
        ] {
            assert!(size_refusal(w, h).is_none(), "{w} × {h}");
        }
        let why = size_refusal(12000, 9000).unwrap();
        assert!(
            why.contains("12000 × 9000") && why.contains("100 megapixels"),
            "{why}"
        );
        assert!(size_refusal(10001, 10000).is_some());
    }

    /// B5-20b: the 1/8 proxy of a 24 MP recipe solves (the B5-20 proxy scaling
    /// under-sampled long traced lines and failed the scaled tolerance).
    #[test]
    fn a_24_megapixel_recipe_solves_on_its_preview_proxy() {
        let (w, h) = (6000.0, 4000.0);
        let mut a = Adaptive::new(
            6000,
            4000,
            CameraModel::Manual {
                focal_px: 0.4 * w,
                center: [w / 2., h / 2.],
                projection: Projection::Equidistant,
            },
        );
        a.output_focal_px = 0.4 * w;
        let c = adaptive_wide_angle_curve(
            serde_json::to_string(&a).unwrap(),
            vec![0.25 * w, 0.2 * h],
            vec![0.26 * w, 0.8 * h],
        )
        .unwrap();
        a.lines.push(transform::adaptive::LineConstraint {
            points: c.chunks(2).map(|p| [p[0], p[1]]).collect(),
            orientation: transform::adaptive::LineOrientation::Vertical,
            weight: 1.,
        });
        scaled(&a, 8, Extent::new(750, 500)).solve().unwrap();
    }

    #[test]
    fn scaled_recipe_divides_geometry_uniformly() {
        let mut a = Adaptive::new(
            800,
            600,
            CameraModel::Manual {
                focal_px: 400.,
                center: [400., 300.],
                projection: Projection::Equidistant,
            },
        );
        a.crop = [8., 4.];
        a.lines.push(transform::adaptive::LineConstraint {
            points: vec![[100., 50.], [800., 600.]],
            orientation: transform::adaptive::LineOrientation::Straight,
            weight: 1.,
        });
        let s = scaled(&a, 4, Extent::new(200, 150));
        assert_eq!([s.source_width, s.output_height], [200, 150]);
        assert_eq!(
            s.camera,
            CameraModel::Manual {
                focal_px: 100.,
                center: [100., 75.],
                projection: Projection::Equidistant
            }
        );
        assert_eq!(s.output_focal_px, 100.);
        assert_eq!(s.crop, [2., 1.]);
        // 890 px densify to 64 samples at full size; the 222 px proxy chord
        // would get 56, so the full-resolution samples are inserted.
        assert_eq!(s.lines[0].points.len(), 65);
        assert_eq!(s.lines[0].points[0], [25., 12.5]);
        assert_eq!(s.lines[0].points[64], [200., 150.]);
        assert_eq!(s.line_tolerance, 0.0625);
        s.validate().unwrap();
    }
}
