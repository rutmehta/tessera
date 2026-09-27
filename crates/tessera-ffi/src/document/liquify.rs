//! Filter ▸ Liquify workspace for the app (WP B5-13).
//!
//! The mesh, its freeze plane and the stroke state stay here, engine-side;
//! the app only sends pointer samples and draws what these calls return.
//!
//! - [`DocumentSession::begin_liquify`] snapshots the target: a pixel
//!   layer's raster, or what smart filter `stage_index` of a smart object
//!   receives (its child, unplaced, after the filters below it). Re-editing
//!   an existing Liquify smart filter loads its mesh. A selection on a pixel
//!   layer starts frozen outside (node-sampled) and clips the final write.
//! - `liquify_brush_points` resamples a pointer path into dabs of
//!   `filters::liquify::Mesh::apply_brush` (all ten tools). The mesh holds
//!   **inverse** displacements: output (x, y) samples source (x+dx, y+dy).
//! - `preview_liquify` renders the mesh over a box-downsampled proxy of the
//!   source (at most 2048 px on the long side) into an RGBA8 IOSurface; it
//!   never touches the document or its history.
//! - `commit_liquify` renders at full resolution with `Mesh::render` and
//!   installs it as ONE history node: the layer's pixels, a new layer above,
//!   or a Liquify smart filter (appended, re-edited in place, or — on a pixel
//!   layer — the layer converted to a smart object in the same node). A
//!   changed layer (stale revision) or a cancel never commits.
//! - `cancel_liquify` drops the workspace; a commit still rendering on
//!   another thread sees the cancel before it writes and returns an error.
//!
//! Face-aware Liquify is not wired: there is no landmark source here, and
//! nothing downloads a model.

use super::{DocRect, DocumentSession, DocumentUpdate, Shared, find, raster_from_rgba};
use crate::{Result, failure, surface::Surface};
use compositor::{
    Affine, DocOp, DocState, Layer, LayerId, LayerKind, Raster, Rect, SmartFilter, SmartObject,
    render::smart_filters::FilterBlend,
};
use engine_api::tile::TILE_SIZE;
use filters::liquify::{Brush, BrushTool, Interpolation, Mesh};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

// ─────────────────────────────── records ───────────────────────────────

/// The ten Liquify brush tools (`filters::liquify::BrushTool`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LiquifyTool {
    ForwardWarp,
    Reconstruct,
    Smooth,
    /// Clockwise in image coordinates.
    TwirlClockwise,
    TwirlCounterClockwise,
    Pucker,
    Bloat,
    /// Pushes to the left of the drag direction.
    PushLeft,
    Freeze,
    Thaw,
}

impl LiquifyTool {
    fn engine(self) -> BrushTool {
        match self {
            Self::ForwardWarp => BrushTool::ForwardWarp,
            Self::Reconstruct => BrushTool::Reconstruct,
            Self::Smooth => BrushTool::Smooth,
            Self::TwirlClockwise => BrushTool::Twirl,
            Self::TwirlCounterClockwise => BrushTool::TwirlCounterClockwise,
            Self::Pucker => BrushTool::Pucker,
            Self::Bloat => BrushTool::Bloat,
            Self::PushLeft => BrushTool::PushLeft,
            Self::Freeze => BrushTool::Freeze,
            Self::Thaw => BrushTool::Thaw,
        }
    }

    /// Drag tools act along the path only; the rest also act while the
    /// pointer rests (the app repeats the last point), scaled by Rate.
    fn acts_in_place(self) -> bool {
        !matches!(self, Self::ForwardWarp | Self::PushLeft)
    }
}

/// Brush controls: diameter in canvas pixels; density (hard core), pressure
/// and rate in 0…1. Rate applies to the tools that act in place (Twirl,
/// Pucker, Bloat, Reconstruct, Smooth, Freeze, Thaw); the drag tools
/// (Forward Warp, Push Left) use pressure only, as in Photoshop.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyBrush {
    pub size: f32,
    pub density: f32,
    pub pressure: f32,
    pub rate: f32,
}

/// One pointer sample in level-0 canvas pixels (of the Liquify source:
/// the document canvas for pixel layers, the smart object's own canvas).
/// `pressure` (0…1, 1 for a mouse) scales the brush pressure.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyPoint {
    pub x: f32,
    pub y: f32,
    pub pressure: f32,
}

/// Where `commit_liquify` puts the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum LiquifyDestination {
    /// The pixel layer's own pixels (inside the selection, if any). On a
    /// smart object: the smart filter (as `SmartFilter`).
    CurrentLayer,
    /// A liquified copy of a pixel layer, added above it.
    NewLayer,
    /// A Liquify smart filter: appended, the re-edited one replaced in place,
    /// or on a pixel layer the layer converted to a smart object with it.
    SmartFilter,
}

/// An open Liquify workspace.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LiquifySessionInfo {
    pub token: u64,
    pub layer: u64,
    /// The smart filter being re-edited (`None`: a new Liquify).
    pub stage_index: Option<u32>,
    pub smart_object: bool,
    /// Source (= mesh) size in pixels.
    pub width: u32,
    pub height: u32,
    /// Mesh node spacing in source pixels.
    pub cell_size: u32,
    pub columns: u32,
    pub rows: u32,
    /// Preview proxy size and its source pixels per proxy pixel (1, 2, …).
    pub preview_width: u32,
    pub preview_height: u32,
    pub preview_factor: u32,
    /// Areas outside the selection start frozen (pixel layers).
    pub selection_frozen: bool,
    /// Whether the loaded mesh already deforms (smart filter re-edit).
    pub edited: bool,
}

/// The mesh for the overlay: node (x, y) sits at (x·cell, y·cell) and holds
/// the inverse displacement (dx, dy) and its freeze weight.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyMeshRecord {
    pub columns: u32,
    pub rows: u32,
    pub cell_size: u32,
    /// Row-major interleaved (dx, dy) per node, source pixels.
    pub displacement: Vec<f32>,
    /// Row-major freeze weight per node, 0…1.
    pub freeze: Vec<f32>,
    pub max_displacement: f32,
}

/// A rendered preview (or the untouched source for Before).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyPreview {
    /// RGBA8 IOSurface, straight alpha, layer samples; retained until the
    /// next preview of this workspace (two alternate).
    pub surface_id: u32,
    pub width: u32,
    pub height: u32,
    pub original: bool,
    pub millis: f64,
}

/// What one `liquify_brush_points` call did.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct LiquifyStrokeResult {
    pub dabs: u32,
    /// Source pixels the dabs can have changed.
    pub dirty: Option<DocRect>,
    pub millis: f64,
}

// ─────────────────────────────── state ───────────────────────────────

/// Straight RGBA f32, box-downsampled by `factor`.
struct Proxy {
    w: usize,
    h: usize,
    factor: u32,
    px: Vec<[f32; 4]>,
}

struct Job {
    owner: Weak<Shared>,
    layer: u64,
    stage: Option<usize>,
    smart: bool,
    base: Arc<DocState>,
    revision: u64,
    /// Full-resolution source (level 0, canvas of the source).
    source: Raster,
    /// Pixel layers: the selection the result is written through.
    clip: Option<Raster>,
    mesh: Mesh,
    interpolation: Interpolation,
    proxy: Proxy,
    surfaces: [Option<Arc<Surface>>; 2],
    flip: usize,
    last: Option<[f32; 2]>,
    carry: f32,
    cancel: Arc<AtomicBool>,
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

/// Mesh spacing: the finest power of two (≥ 4) keeping ≤ 250k nodes.
fn cell_size(w: u32, h: u32) -> u32 {
    let mut c = 4u32;
    while ((w.saturating_sub(1)).div_ceil(c) as u64 + 1)
        * ((h.saturating_sub(1)).div_ceil(c) as u64 + 1)
        > 250_000
        && c < 1024
    {
        c *= 2;
    }
    c
}

/// Straight RGBA f32 of every pixel of a canvas-sized 4-channel raster.
fn read_rgba(r: &Raster) -> Result<Vec<[f32; 4]>> {
    let e = r.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut out = vec![[0.0f32; 4]; w * h];
    let (cols, rows) = e.tile_grid(TILE_SIZE);
    let t = TILE_SIZE as usize;
    let ch = r.channels() as usize;
    let mut buf = Vec::new();
    for ty in 0..rows {
        for tx in 0..cols {
            let lay = r.layout(tx, ty);
            r.read_tile(tx, ty, &mut buf)?;
            let n = lay.plane_len();
            for y in 0..lay.extent.height as usize {
                for x in 0..lay.extent.width as usize {
                    let i = y * lay.stride() + x;
                    let p = &mut out[(ty as usize * t + y) * w + tx as usize * t + x];
                    for c in 0..4 {
                        p[c] = if c < ch {
                            buf[c * n + i]
                        } else if c == 3 {
                            1.0
                        } else {
                            buf[i]
                        };
                    }
                }
            }
        }
    }
    Ok(out)
}

fn proxy_of(px: &[[f32; 4]], w: usize, h: usize) -> Proxy {
    let factor = (w.max(h).div_ceil(2048)).max(1);
    if factor == 1 {
        return Proxy {
            w,
            h,
            factor: 1,
            px: px.to_vec(),
        };
    }
    let (pw, ph) = (w.div_ceil(factor), h.div_ceil(factor));
    let mut out = vec![[0.0f32; 4]; pw * ph];
    for py in 0..ph {
        for qx in 0..pw {
            let mut s = [0.0f32; 4];
            let mut n = 0.0f32;
            for y in py * factor..((py + 1) * factor).min(h) {
                for x in qx * factor..((qx + 1) * factor).min(w) {
                    let p = px[y * w + x];
                    for c in 0..3 {
                        s[c] += p[c] * p[3];
                    }
                    s[3] += p[3];
                    n += 1.0;
                }
            }
            out[py * pw + qx] = if s[3] > 0.0 {
                [s[0] / s[3], s[1] / s[3], s[2] / s[3], s[3] / n]
            } else {
                [0.0; 4]
            };
        }
    }
    Proxy {
        w: pw,
        h: ph,
        factor: factor as u32,
        px: out,
    }
}

/// Bilinear sample with clamped edges (as `Mesh::render`).
fn bilinear(p: &Proxy, x: f32, y: f32) -> [f32; 4] {
    let x = x.clamp(0.0, (p.w - 1) as f32);
    let y = y.clamp(0.0, (p.h - 1) as f32);
    let (ix, iy) = (x.floor() as usize, y.floor() as usize);
    let (fx, fy) = (x - ix as f32, y - iy as f32);
    let (jx, jy) = ((ix + 1).min(p.w - 1), (iy + 1).min(p.h - 1));
    let mut out = [0.0f32; 4];
    for (yy, wy) in [(iy, 1.0 - fy), (jy, fy)] {
        for (xx, wx) in [(ix, 1.0 - fx), (jx, fx)] {
            let q = p.px[yy * p.w + xx];
            let k = wx * wy;
            for c in 0..4 {
                out[c] += q[c] * k;
            }
        }
    }
    out
}

/// The mesh over the proxy as RGBA8 rows (`stride` bytes each).
fn render_proxy(mesh: Option<&Mesh>, p: &Proxy, out: &mut [u8], stride: usize) {
    let f = p.factor as f32;
    let workers = std::thread::available_parallelism()
        .map_or(1, usize::from)
        .clamp(1, 8);
    let rows_per = p.h.div_ceil(workers).max(1);
    std::thread::scope(|scope| {
        for (band, chunk) in out.chunks_mut(rows_per * stride).enumerate() {
            scope.spawn(move || {
                for (r, row) in chunk.chunks_mut(stride).enumerate() {
                    let py = band * rows_per + r;
                    if py >= p.h {
                        break;
                    }
                    for px in 0..p.w {
                        let v = match mesh {
                            None => p.px[py * p.w + px],
                            Some(m) => {
                                // Proxy pixel centre in source pixels.
                                let cx = (px as f32 + 0.5) * f - 0.5;
                                let cy = (py as f32 + 0.5) * f - 0.5;
                                let d = m.displacement_at(cx, cy);
                                let sx = (cx + d[0]).clamp(0.0, (m.width - 1) as f32);
                                let sy = (cy + d[1]).clamp(0.0, (m.height - 1) as f32);
                                bilinear(p, (sx + 0.5) / f - 0.5, (sy + 0.5) / f - 0.5)
                            }
                        };
                        for c in 0..4 {
                            row[px * 4 + c] = (v[c].clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
                        }
                    }
                }
            });
        }
    });
}

/// Bounds of the mesh's nonzero displacement, inflated by one cell (the
/// interpolation support), in source pixels.
fn deformed_bounds(m: &Mesh) -> Option<Rect> {
    let (w, h) = m.grid();
    let c = i64::from(m.cell_size);
    let mut r: Option<Rect> = None;
    for y in 0..h {
        for x in 0..w {
            if m.displacement[y * w + x] != [0.0, 0.0] {
                let n = Rect::new(
                    x as i64 * c - c,
                    y as i64 * c - c,
                    x as i64 * c + c + 1,
                    y as i64 * c + c + 1,
                );
                r = Some(r.map_or(n, |a| a.union(&n)));
            }
        }
    }
    r.map(|r| r.intersect(&Rect::new(0, 0, i64::from(m.width), i64::from(m.height))))
}

fn check(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(failure("cancelled"))
    } else {
        Ok(())
    }
}

impl DocumentSession {
    fn liquify_job(&self, token: u64) -> Result<Arc<Mutex<Job>>> {
        let j = jobs().get(&token).cloned().ok_or_else(|| {
            failure("the Liquify workspace is closed (cancelled, applied or replaced)")
        })?;
        let owner = j.lock().map_err(failure)?.owner.clone();
        if !Weak::ptr_eq(&owner, &Arc::downgrade(&self.shared)) {
            return Err(failure(
                "that Liquify workspace belongs to another document",
            ));
        }
        Ok(j)
    }

    /// The pixel layer converted to a smart object holding `filter` (the
    /// construction of `convert_for_smart_filters`), as one Batch.
    pub(crate) fn smart_from_pixels_op(
        s: &DocState,
        layer: u64,
        filter: SmartFilter,
    ) -> Result<DocOp> {
        let l = find(s, layer)?;
        if !matches!(l.kind, LayerKind::Pixel(_)) {
            return Err(failure("not a pixel layer"));
        }
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
        so.filters.push(filter);
        let mut outer = Layer::new(l.props.name.clone(), LayerKind::SmartObject(so));
        outer.id = l.id;
        outer.props = l.props.clone();
        outer.props.background = false;
        outer.mask = l.mask.clone();
        let (parent, index) = s
            .locate(LayerId(layer))
            .ok_or_else(|| failure("layer not found"))?;
        Ok(DocOp::Batch(vec![
            DocOp::RemoveLayer { id: LayerId(layer) },
            DocOp::AddLayer {
                parent,
                index,
                layer: outer,
            },
        ]))
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Opens the Liquify workspace on `layer` (a pixel layer, or a smart
    /// object: a new Liquify smart filter, or with `stage_index` the existing
    /// Liquify smart filter to re-edit). Closes this document's previous
    /// workspace. No history node.
    pub fn begin_liquify(
        &self,
        layer: u64,
        stage_index: Option<u32>,
    ) -> Result<LiquifySessionInfo> {
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
        let (source, clip, smart, mesh, interpolation, stage) = match &l.kind {
            LayerKind::Pixel(r) => {
                if stage_index.is_some() {
                    return Err(failure("only smart objects have smart filters to re-edit"));
                }
                if r.extent() != base.canvas {
                    return Err(failure("the layer's raster is not canvas-sized"));
                }
                let e = r.extent();
                let mut mesh = Mesh::new(e.width, e.height, cell_size(e.width, e.height))?;
                let clip = base.selection.as_deref().cloned();
                if let Some(sel) = &clip {
                    // Outside the selection starts frozen (node-sampled);
                    // the final write is clipped by the selection itself.
                    let (gw, gh) = mesh.grid();
                    for y in 0..gh {
                        for x in 0..gw {
                            let px = ((x as u32) * mesh.cell_size).min(e.width - 1);
                            let py = ((y as u32) * mesh.cell_size).min(e.height - 1);
                            mesh.freeze[y * gw + x] = 1.0 - sel.pixel(px, py)[0].clamp(0.0, 1.0);
                        }
                    }
                }
                (r.clone(), clip, false, mesh, Interpolation::Bilinear, None)
            }
            LayerKind::SmartObject(so) => {
                let stage = match stage_index {
                    Some(i) => {
                        let f = so
                            .filters
                            .get(i as usize)
                            .ok_or_else(|| failure(format!("no smart filter {i}")))?;
                        if f.name != "liquify" {
                            return Err(failure(format!(
                                "smart filter {i} is {}, not Liquify",
                                f.name
                            )));
                        }
                        i as usize
                    }
                    None => so.filters.len(),
                };
                let (extent, px) = Self::smart_stage_source(&base, layer, stage)?;
                let raster = raster_from_rgba(extent, compositor::Depth::F32, &px, false)?;
                let (mesh, interpolation) = match stage_index {
                    Some(i) => {
                        #[derive(serde::Deserialize)]
                        struct P {
                            mesh: Mesh,
                            #[serde(default)]
                            interpolation: Interpolation,
                        }
                        let p: P = serde_json::from_value(so.filters[i as usize].params.clone())
                            .map_err(|e| failure(format!("Liquify smart filter: {e}")))?;
                        if p.mesh.width != extent.width || p.mesh.height != extent.height {
                            return Err(failure(
                                "the Liquify mesh does not match the smart object",
                            ));
                        }
                        (p.mesh, p.interpolation)
                    }
                    None => (
                        Mesh::new(
                            extent.width,
                            extent.height,
                            cell_size(extent.width, extent.height),
                        )?,
                        Interpolation::Bilinear,
                    ),
                };
                (
                    raster,
                    None,
                    true,
                    mesh,
                    interpolation,
                    stage_index.map(|i| i as usize),
                )
            }
            _ => return Err(failure("Liquify works on pixel layers and smart objects")),
        };
        let e = source.extent();
        let px = read_rgba(&source)?;
        let proxy = proxy_of(&px, e.width as usize, e.height as usize);
        drop(px);
        let (gw, gh) = mesh.grid();
        let info = LiquifySessionInfo {
            token: NEXT.fetch_add(1, Ordering::Relaxed),
            layer,
            stage_index,
            smart_object: smart,
            width: e.width,
            height: e.height,
            cell_size: mesh.cell_size,
            columns: gw as u32,
            rows: gh as u32,
            preview_width: proxy.w as u32,
            preview_height: proxy.h as u32,
            preview_factor: proxy.factor,
            selection_frozen: clip.is_some(),
            edited: mesh.displacement.iter().any(|d| *d != [0.0, 0.0]),
        };
        let job = Job {
            owner: Arc::downgrade(&self.shared),
            layer,
            stage,
            smart,
            base,
            revision,
            source,
            clip,
            mesh,
            interpolation,
            proxy,
            surfaces: [None, None],
            flip: 0,
            last: None,
            carry: 0.0,
            cancel: Arc::new(AtomicBool::new(false)),
        };
        let mut m = jobs();
        let me = Arc::downgrade(&self.shared);
        m.retain(|_, j| match j.lock() {
            Ok(j) if Weak::ptr_eq(&j.owner, &me) => {
                j.cancel.store(true, Ordering::Relaxed);
                false
            }
            _ => true,
        });
        m.insert(info.token, Arc::new(Mutex::new(job)));
        Ok(info)
    }

    /// Applies `tool` along `points` (continuing the open stroke, see
    /// `liquify_end_stroke`): dabs every ~size/10 pixels; tools that act in
    /// place also dab on a repeated point. Edits only the workspace mesh.
    pub fn liquify_brush_points(
        &self,
        token: u64,
        tool: LiquifyTool,
        brush: LiquifyBrush,
        points: Vec<LiquifyPoint>,
    ) -> Result<LiquifyStrokeResult> {
        let started = Instant::now();
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        let base = Brush {
            size: brush.size,
            density: brush.density,
            pressure: brush.pressure,
            rate: if tool.acts_in_place() {
                brush.rate
            } else {
                1.0
            },
        };
        base.validate()?;
        if points
            .iter()
            .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.pressure.is_finite())
        {
            return Err(failure("nonfinite brush point"));
        }
        let spacing = (brush.size * 0.1).max(1.0);
        let mut mesh = j.mesh.clone();
        let (mut last, mut carry) = (j.last, j.carry);
        let mut dabs = 0u32;
        let mut dirty: Option<Rect> = None;
        let r = (brush.size * 0.5).ceil() as i64 + 1;
        for p in &points {
            let mut b = base;
            b.pressure = (base.pressure * p.pressure.clamp(0.0, 1.0)).clamp(0.0, 1.0);
            let mut dab = |c: [f32; 2], delta: [f32; 2], mesh: &mut Mesh| -> Result<()> {
                mesh.apply_brush(tool.engine(), c, delta, &b)?;
                dabs += 1;
                let (x, y) = (c[0].round() as i64, c[1].round() as i64);
                let n = Rect::new(x - r, y - r, x + r + 1, y + r + 1);
                dirty = Some(dirty.map_or(n, |d| d.union(&n)));
                Ok(())
            };
            match last {
                None => {
                    if tool.acts_in_place() {
                        dab([p.x, p.y], [0.0, 0.0], &mut mesh)?;
                    }
                    carry = 0.0;
                    last = Some([p.x, p.y]);
                }
                Some(l) => {
                    let (dx, dy) = (p.x - l[0], p.y - l[1]);
                    let len = dx.hypot(dy);
                    if len == 0.0 {
                        if tool.acts_in_place() {
                            dab([p.x, p.y], [0.0, 0.0], &mut mesh)?;
                        }
                        continue;
                    }
                    // Dabs at every `spacing` along the segment; each drag
                    // dab carries the motion since the previous dab.
                    let mut t = spacing - carry;
                    let mut prev = l;
                    while t <= len {
                        let c = [l[0] + dx * t / len, l[1] + dy * t / len];
                        dab(c, [c[0] - prev[0], c[1] - prev[1]], &mut mesh)?;
                        prev = c;
                        t += spacing;
                    }
                    carry = len - (t - spacing);
                    last = Some(if tool.acts_in_place() {
                        [p.x, p.y]
                    } else {
                        // Unspent motion stays with the next segment.
                        prev
                    });
                    if !tool.acts_in_place() {
                        carry = 0.0;
                    }
                }
            }
        }
        j.mesh = mesh;
        j.last = last;
        j.carry = carry;
        let full = Rect::new(0, 0, i64::from(j.mesh.width), i64::from(j.mesh.height));
        Ok(LiquifyStrokeResult {
            dabs,
            dirty: dirty.and_then(|d| DocRect::of(d.intersect(&full))),
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Ends the open stroke (the next point starts a new one).
    pub fn liquify_end_stroke(&self, token: u64) -> Result<()> {
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        j.last = None;
        j.carry = 0.0;
        Ok(())
    }

    /// The workspace mesh (overlay data).
    pub fn liquify_mesh(&self, token: u64) -> Result<LiquifyMeshRecord> {
        let job = self.liquify_job(token)?;
        let j = job.lock().map_err(failure)?;
        let (w, h) = j.mesh.grid();
        Ok(LiquifyMeshRecord {
            columns: w as u32,
            rows: h as u32,
            cell_size: j.mesh.cell_size,
            displacement: j.mesh.displacement.iter().flatten().copied().collect(),
            freeze: j.mesh.freeze.clone(),
            max_displacement: j
                .mesh
                .displacement
                .iter()
                .map(|d| d[0].hypot(d[1]))
                .fold(0.0, f32::max),
        })
    }

    /// Reconstruct (whole mesh): every unfrozen displacement scaled by
    /// `1 − amount·(1 − freeze)`; amount 1 restores unfrozen areas.
    pub fn liquify_reconstruct_all(&self, token: u64, amount: f32) -> Result<()> {
        if !amount.is_finite() || !(0.0..=1.0).contains(&amount) {
            return Err(failure("amount must be 0…1"));
        }
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        let mut m = j.mesh.clone();
        for (d, f) in m.displacement.iter_mut().zip(&j.mesh.freeze) {
            let k = 1.0 - amount * (1.0 - f);
            *d = [d[0] * k, d[1] * k];
        }
        m.validate()?;
        j.mesh = m;
        Ok(())
    }

    /// Restore All: no displacement. `keep_freeze` keeps the freeze plane.
    pub fn liquify_reset(&self, token: u64, keep_freeze: bool) -> Result<()> {
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        j.mesh.displacement.fill([0.0, 0.0]);
        if !keep_freeze {
            j.mesh.freeze.fill(0.0);
        }
        j.last = None;
        Ok(())
    }

    /// Freeze everything (`true`) or thaw everything (`false`).
    pub fn liquify_freeze_all(&self, token: u64, frozen: bool) -> Result<()> {
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        j.mesh.freeze.fill(if frozen { 1.0 } else { 0.0 });
        Ok(())
    }

    /// Renders the workspace (or with `original` the untouched source) at
    /// proxy resolution into an IOSurface. Never touches the document.
    pub fn preview_liquify(&self, token: u64, original: bool) -> Result<LiquifyPreview> {
        let started = Instant::now();
        let job = self.liquify_job(token)?;
        let mut j = job.lock().map_err(failure)?;
        let (w, h) = (j.proxy.w as u32, j.proxy.h as u32);
        j.flip ^= 1;
        let slot = j.flip;
        let surface = match &j.surfaces[slot] {
            Some(s) => s.clone(),
            None => {
                let s = Arc::new(Surface::create_rgba8(w, h).map_err(failure)?);
                j.surfaces[slot] = Some(s.clone());
                s
            }
        };
        let mesh = (!original).then_some(&j.mesh);
        surface
            .with_pixels(|px, stride| render_proxy(mesh, &j.proxy, px, stride))
            .map_err(failure)?;
        Ok(LiquifyPreview {
            surface_id: surface.id(),
            width: w,
            height: h,
            original,
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Renders the mesh at full resolution (`Mesh::render`) and installs it
    /// as one history node at `destination`. Blocking: call off the main
    /// thread; `cancel_liquify` stops it (nothing is written). The workspace
    /// closes on success; on an error it stays open.
    pub fn commit_liquify(
        &self,
        token: u64,
        destination: LiquifyDestination,
    ) -> Result<DocumentUpdate> {
        let job = self.liquify_job(token)?;
        let (layer, stage, smart, base, revision, source, clip, mesh, interpolation, cancel) = {
            let j = job.lock().map_err(failure)?;
            (
                j.layer,
                j.stage,
                j.smart,
                j.base.clone(),
                j.revision,
                j.source.clone(),
                j.clip.clone(),
                j.mesh.clone(),
                j.interpolation,
                j.cancel.clone(),
            )
        };
        let still_open = || -> Result<()> {
            check(&cancel)?;
            if !jobs().contains_key(&token) {
                return Err(failure("cancelled"));
            }
            Ok(())
        };
        let params = serde_json::json!({"mesh": mesh, "interpolation": interpolation});
        let update = if smart {
            if destination == LiquifyDestination::NewLayer {
                return Err(failure(
                    "New layer output needs a pixel layer; smart objects take a smart filter",
                ));
            }
            // Validated by rendering the whole stack before the node is added.
            still_open()?;
            self.set_adapter_smart_filter(layer, "Liquify", revision, stage, "liquify", params)?
        } else if destination == LiquifyDestination::SmartFilter {
            if clip.is_some() {
                return Err(failure(
                    "Smart filter output works without a selection (the selection only froze the mesh); deselect first or apply to the layer",
                ));
            }
            mesh.render(&source, interpolation, &cancel)?;
            still_open()?;
            let op = Self::smart_from_pixels_op(
                &base,
                layer,
                SmartFilter {
                    name: "liquify".into(),
                    enabled: true,
                    blend: FilterBlend::default(),
                    params,
                },
            )?;
            let mut u = self.edit_layer_checked(layer, revision, op, "Liquify")?;
            u.created.clear();
            u
        } else {
            let l = find(&base, layer)?;
            let LayerKind::Pixel(raster) = &l.kind else {
                return Err(failure("not a pixel layer"));
            };
            let rendered = mesh.render(&source, interpolation, &cancel)?;
            still_open()?;
            if destination == LiquifyDestination::CurrentLayer {
                let region = deformed_bounds(&mesh).unwrap_or(Rect::new(0, 0, 0, 0));
                let tiles = super::filtering::blended_tiles(
                    raster,
                    &rendered,
                    region,
                    clip.as_ref(),
                    l.props.locks.transparency,
                    base.depth,
                )?;
                let op = DocOp::PaintTiles {
                    id: LayerId(layer),
                    target: compositor::PaintTarget::Content,
                    tiles,
                    dirty: region,
                };
                self.edit_layer_checked(layer, revision, op, "Liquify")?
            } else {
                let mut copy = raster.clone();
                let tiles = super::filtering::blended_tiles(
                    raster,
                    &rendered,
                    Rect::of_extent(base.canvas),
                    clip.as_ref(),
                    false,
                    base.depth,
                )?;
                for t in tiles {
                    copy.set_slot(t.tx, t.ty, t.tile, 0)?;
                }
                let mut nl = Layer::new(
                    format!("{} (Liquify)", l.props.name),
                    LayerKind::Pixel(copy),
                );
                nl.props.visible = true;
                let (parent, index) = base
                    .locate(LayerId(layer))
                    .ok_or_else(|| failure("layer not found"))?;
                self.edit_layer_checked(
                    layer,
                    revision,
                    DocOp::AddLayer {
                        parent,
                        index: index + 1,
                        layer: nl,
                    },
                    "Liquify",
                )?
            }
        };
        jobs().remove(&token);
        Ok(update)
    }

    /// Closes the workspace without changing the document; a commit still
    /// rendering stops before it writes.
    pub fn cancel_liquify(&self, token: u64) {
        let mut m = jobs();
        let mine = m.get(&token).is_some_and(|j| {
            j.lock().is_ok_and(|j| {
                let mine = Weak::ptr_eq(&j.owner, &Arc::downgrade(&self.shared));
                if mine {
                    j.cancel.store(true, Ordering::Relaxed);
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
    /// The workspace mesh as the engine type.
    #[doc(hidden)]
    pub fn liquify_engine_mesh(&self, token: u64) -> Option<Mesh> {
        self.liquify_job(token)
            .ok()?
            .lock()
            .ok()
            .map(|j| j.mesh.clone())
    }

    /// Reads back the last preview surface of the workspace as RGBA8 rows.
    #[doc(hidden)]
    pub fn liquify_preview_pixels(&self, token: u64) -> Option<Vec<u8>> {
        let job = self.liquify_job(token).ok()?;
        let j = job.lock().ok()?;
        let s = j.surfaces[j.flip].as_ref()?;
        let (w, h) = (j.proxy.w, j.proxy.h);
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
