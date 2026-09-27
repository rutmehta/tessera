//! Content-Aware Move / Extend for the app (WP B5-13).
//!
//! `apply_raster_filter` is NOT used here: it crops its result to the
//! selection and `write_pixels` clips by the selection again, which would
//! lose the moved subject wherever the destination lies outside the original
//! selection. Instead:
//!
//! - [`DocumentSession::begin_content_aware_move`] freezes the selection as
//!   a canvas mask and snapshots the layer (a pixel layer's raster, or what
//!   a smart filter appended to a smart object receives). The live selection
//!   is never cleared or changed.
//! - `preview_content_aware_move` runs `filters::caf::move_or_extend` on
//!   that immutable snapshot at full resolution (it can take seconds: the
//!   app throttles and shows busy / Cancel) and shows the result in the
//!   viewport through the filter preview slot. No history node.
//! - `commit_content_aware_move` installs the previewed result as ONE
//!   revision-checked history node covering the source AND destination
//!   (bounds of the frozen mask and of the mask moved by the offset), with
//!   the operation's own coverage applied once — no second selection clip.
//!   On a smart object it appends a `content_aware_move` / `_extend` smart
//!   filter storing the frozen mask explicitly (never the shared filter mask).
//! - `cancel_content_aware_move` leaves layer, selection and history as they
//!   were. A running preview stops at the engine's next cancellation
//!   checkpoint (per PatchMatch iteration / seam relaxation step); its late
//!   result is discarded either way.

use super::{DocRect, DocumentSession, DocumentUpdate, Shared, find, raster_from_rgba};
use crate::{Result, failure};
use compositor::{Affine, DocOp, DocState, LayerId, LayerKind, Raster, Rect};
use engine_api::tile::TILE_SIZE;
use filters::caf::{ColourAdaptation, FillParams, MoveMode};
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

/// Move cuts the selection and heals the hole; Extend keeps the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ContentAwareMode {
    Move,
    Extend,
}

/// How the pasted subject's colours adapt at the seam (`caf::ColourAdaptation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ContentAwareSeam {
    None,
    Default,
    High,
    VeryHigh,
}

impl ContentAwareSeam {
    fn engine(self) -> ColourAdaptation {
        match self {
            Self::None => ColourAdaptation::None,
            Self::Default => ColourAdaptation::Default,
            Self::High => ColourAdaptation::High,
            Self::VeryHigh => ColourAdaptation::VeryHigh,
        }
    }
}

/// An open Content-Aware Move.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ContentAwareMoveInfo {
    pub token: u64,
    pub layer: u64,
    pub mode: ContentAwareMode,
    pub smart_object: bool,
    /// Bounds of the frozen selection, level-0 canvas pixels.
    pub selection_bounds: DocRect,
    pub width: u32,
    pub height: u32,
}

/// A computed preview (shown in the viewport).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ContentAwarePreviewResult {
    pub dx: i32,
    pub dy: i32,
    /// Source ∪ destination bounds the result changes.
    pub affected: Option<DocRect>,
    pub millis: f64,
}

#[derive(Clone, PartialEq)]
struct Params {
    offset: [i32; 2],
    fill_json: String,
    seam: ContentAwareSeam,
}

struct Job {
    owner: Weak<Shared>,
    layer: u64,
    mode: ContentAwareMode,
    smart: bool,
    base: Arc<DocState>,
    revision: u64,
    input: Raster,
    mask: Arc<Vec<f32>>,
    bounds: Rect,
    generation: u64,
    running: Arc<AtomicBool>,
    closed: Arc<AtomicBool>,
    /// The last finished preview and what produced it.
    result: Option<(Params, Raster, Rect)>,
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

fn read_mask(sel: &Raster) -> Result<Vec<f32>> {
    let e = sel.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut out = vec![0.0f32; w * h];
    let (cols, rows) = e.tile_grid(TILE_SIZE);
    let t = TILE_SIZE as usize;
    let mut buf = Vec::new();
    for ty in 0..rows {
        for tx in 0..cols {
            let lay = sel.layout(tx, ty);
            sel.read_tile(tx, ty, &mut buf)?;
            for y in 0..lay.extent.height as usize {
                for x in 0..lay.extent.width as usize {
                    out[(ty as usize * t + y) * w + tx as usize * t + x] =
                        buf[y * lay.stride() + x].clamp(0.0, 1.0);
                }
            }
        }
    }
    Ok(out)
}

fn mask_bounds(mask: &[f32], w: usize) -> Option<Rect> {
    let mut r: Option<Rect> = None;
    for (i, &m) in mask.iter().enumerate() {
        if m > 0.0 {
            let (x, y) = ((i % w) as i64, (i / w) as i64);
            let n = Rect::new(x, y, x + 1, y + 1);
            r = Some(r.map_or(n, |a| a.union(&n)));
        }
    }
    r
}

fn fill_params(json: &str) -> Result<FillParams> {
    let json = if json.trim().is_empty() { "{}" } else { json };
    let p: FillParams =
        serde_json::from_str(json).map_err(|e| failure(format!("fill JSON: {e}")))?;
    if p.output_new_layer {
        return Err(failure(
            "output_new_layer is not supported by Content-Aware Move; duplicate the layer first",
        ));
    }
    Ok(p)
}

impl DocumentSession {
    fn cam_job(&self, token: u64) -> Result<Arc<Mutex<Job>>> {
        let j = jobs().get(&token).cloned().ok_or_else(|| {
            failure("the Content-Aware Move is closed (cancelled, applied or replaced)")
        })?;
        let owner = j.lock().map_err(failure)?.owner.clone();
        if !Weak::ptr_eq(&owner, &Arc::downgrade(&self.shared)) {
            return Err(failure(
                "that Content-Aware Move belongs to another document",
            ));
        }
        Ok(j)
    }

    fn cam_label(mode: ContentAwareMode) -> &'static str {
        match mode {
            ContentAwareMode::Move => "Content-Aware Move",
            ContentAwareMode::Extend => "Content-Aware Extend",
        }
    }
}

#[uniffi::export]
impl DocumentSession {
    /// Starts a Content-Aware Move on `layer` with the current selection,
    /// frozen now. Closes this document's previous one. No history node.
    pub fn begin_content_aware_move(
        &self,
        layer: u64,
        mode: ContentAwareMode,
    ) -> Result<ContentAwareMoveInfo> {
        let base = {
            let st = self.shared.lock()?;
            st.open()?;
            st.live().state().clone()
        };
        let l = find(&base, layer)?;
        if l.props.locks.pixels || l.props.locks.all {
            return Err(failure("the layer's pixels are locked"));
        }
        let sel = base
            .selection
            .clone()
            .ok_or_else(|| failure("make a selection first"))?;
        let mask = read_mask(&sel)?;
        let w = base.canvas.width as usize;
        let bounds = mask_bounds(&mask, w).ok_or_else(|| failure("the selection is empty"))?;
        let (input, smart) = match &l.kind {
            LayerKind::Pixel(r) => {
                if r.extent() != base.canvas {
                    return Err(failure("the layer's raster is not canvas-sized"));
                }
                (r.clone(), false)
            }
            LayerKind::SmartObject(so) => {
                if so.transform != Affine::IDENTITY || so.state.canvas != base.canvas {
                    return Err(failure(
                        "Content-Aware Move on a smart object needs it unscaled and untransformed at the document size (its mask lives in the object's own pixels)",
                    ));
                }
                let (extent, px) = Self::smart_stage_source(&base, layer, so.filters.len())?;
                (
                    raster_from_rgba(extent, compositor::Depth::F32, &px, false)?,
                    true,
                )
            }
            _ => {
                return Err(failure(
                    "Content-Aware Move works on pixel layers and smart objects",
                ));
            }
        };
        let info = ContentAwareMoveInfo {
            token: NEXT.fetch_add(1, Ordering::Relaxed),
            layer,
            mode,
            smart_object: smart,
            selection_bounds: DocRect::of(bounds)
                .ok_or_else(|| failure("the selection is empty"))?,
            width: base.canvas.width,
            height: base.canvas.height,
        };
        let job = Job {
            owner: Arc::downgrade(&self.shared),
            layer,
            mode,
            smart,
            revision: super::layer_revision(l),
            base,
            input,
            mask: Arc::new(mask),
            bounds,
            generation: 0,
            running: Arc::new(AtomicBool::new(false)),
            closed: Arc::new(AtomicBool::new(false)),
            result: None,
        };
        let mut m = jobs();
        let me = Arc::downgrade(&self.shared);
        m.retain(|_, j| match j.lock() {
            Ok(j) if Weak::ptr_eq(&j.owner, &me) => {
                j.running.store(true, Ordering::Relaxed);
                j.closed.store(true, Ordering::Relaxed);
                false
            }
            _ => true,
        });
        m.insert(info.token, Arc::new(Mutex::new(job)));
        Ok(info)
    }

    /// Computes the move by the integer level-0 offset (`dx`, `dy`) with the
    /// adapter's `fill` options (`{}`: defaults; `seed` makes it repeatable)
    /// and `seam` adaptation, and shows it in the viewport. Blocking (full
    /// resolution): call off the main thread. A newer preview or a cancel
    /// stops this one, which then returns an error and shows nothing.
    pub fn preview_content_aware_move(
        &self,
        token: u64,
        dx: i32,
        dy: i32,
        fill_json: String,
        seam: ContentAwareSeam,
    ) -> Result<ContentAwarePreviewResult> {
        let started = Instant::now();
        let fill = fill_params(&fill_json)?;
        let job = self.cam_job(token)?;
        let params = Params {
            offset: [dx, dy],
            fill_json,
            seam,
        };
        let (generation, cancel, input, mask, mode, layer, canvas, bounds) = {
            let mut j = job.lock().map_err(failure)?;
            if dx.unsigned_abs() >= j.base.canvas.width || dy.unsigned_abs() >= j.base.canvas.height
            {
                return Err(failure("the offset moves the selection off the canvas"));
            }
            j.running.store(true, Ordering::Relaxed);
            j.running = Arc::new(AtomicBool::new(false));
            j.generation += 1;
            (
                j.generation,
                j.running.clone(),
                j.input.clone(),
                j.mask.clone(),
                j.mode,
                j.layer,
                j.base.canvas,
                j.bounds,
            )
        };
        let engine_mode = match mode {
            ContentAwareMode::Move => MoveMode::Move,
            ContentAwareMode::Extend => MoveMode::Extend,
        };
        let out = filters::caf::move_or_extend(
            &input,
            &mask,
            [dx, dy],
            engine_mode,
            &fill,
            seam.engine(),
            &cancel,
        )
        .map_err(|e| match e {
            engine_api::EngineError::Cancelled => failure("cancelled"),
            e => failure(e),
        })?;
        let full = Rect::of_extent(canvas);
        let moved = Rect::new(
            bounds.x0 + i64::from(dx),
            bounds.y0 + i64::from(dy),
            bounds.x1 + i64::from(dx),
            bounds.y1 + i64::from(dy),
        );
        let affected = if [dx, dy] == [0, 0] {
            Rect::new(0, 0, 0, 0)
        } else {
            bounds.union(&moved).intersect(&full)
        };
        {
            let mut j = job.lock().map_err(failure)?;
            if j.generation != generation
                || cancel.load(Ordering::Relaxed)
                || j.closed.load(Ordering::Relaxed)
            {
                return Err(failure("cancelled"));
            }
            j.result = Some((params, out.composite.clone(), affected));
        }
        self.show_layer_preview(layer, out.composite)?;
        Ok(ContentAwarePreviewResult {
            dx,
            dy,
            affected: DocRect::of(affected),
            millis: started.elapsed().as_secs_f64() * 1000.0,
        })
    }

    /// Installs the last preview as one history node (computing it first
    /// when none finished). Fails, changing nothing, when the layer changed
    /// since `begin_content_aware_move` or the move was cancelled.
    pub fn commit_content_aware_move(&self, token: u64) -> Result<DocumentUpdate> {
        let job = self.cam_job(token)?;
        let (layer, mode, smart, base, revision, mask, result, closed) = {
            let j = job.lock().map_err(failure)?;
            (
                j.layer,
                j.mode,
                j.smart,
                j.base.clone(),
                j.revision,
                j.mask.clone(),
                j.result.clone(),
                j.closed.clone(),
            )
        };
        let (params, composite, affected) =
            result.ok_or_else(|| failure("move the selection first (no preview to apply)"))?;
        if closed.load(Ordering::Relaxed) {
            return Err(failure("cancelled"));
        }
        let label = Self::cam_label(mode);
        let update = if smart {
            let id = match mode {
                ContentAwareMode::Move => "content_aware_move",
                ContentAwareMode::Extend => "content_aware_extend",
            };
            let fill: serde_json::Value =
                serde_json::from_str(if params.fill_json.trim().is_empty() {
                    "{}"
                } else {
                    &params.fill_json
                })
                .map_err(failure)?;
            let seam = serde_json::to_value(params.seam.engine()).map_err(failure)?;
            let p = serde_json::json!({
                "mask": mask.as_slice(),
                "offset": params.offset,
                "fill": fill,
                "seam": seam,
            });
            self.set_adapter_smart_filter(layer, label, revision, None, id, p)?
        } else {
            let l = find(&base, layer)?;
            let LayerKind::Pixel(raster) = &l.kind else {
                return Err(failure("not a pixel layer"));
            };
            // The operation's coverage was applied once by the engine; the
            // selection does NOT clip again (the destination may lie outside it).
            let tiles = super::filtering::blended_tiles(
                raster,
                &composite,
                affected,
                None,
                l.props.locks.transparency,
                base.depth,
            )?;
            let op = DocOp::PaintTiles {
                id: LayerId(layer),
                target: compositor::PaintTarget::Content,
                tiles,
                dirty: affected,
            };
            self.edit_layer_checked(layer, revision, op, label)?
        };
        if let Ok(j) = job.lock() {
            j.closed.store(true, Ordering::Relaxed);
        }
        jobs().remove(&token);
        Ok(update)
    }

    /// Stops a running preview, ends the preview and closes the move.
    /// Layer, selection and history are unchanged.
    pub fn cancel_content_aware_move(&self, token: u64) {
        let mut m = jobs();
        let mine = m.get(&token).and_then(|j| {
            let j = j.lock().ok()?;
            Weak::ptr_eq(&j.owner, &Arc::downgrade(&self.shared)).then(|| {
                j.running.store(true, Ordering::Relaxed);
                j.closed.store(true, Ordering::Relaxed);
            })
        });
        if mine.is_some() {
            m.remove(&token);
            drop(m);
            let _ = self.clear_preview();
        }
    }
}

/// Test support (not exported over UniFFI).
impl DocumentSession {
    /// The frozen operation mask of an open move.
    #[doc(hidden)]
    pub fn content_aware_mask(&self, token: u64) -> Option<Vec<f32>> {
        let j = self.cam_job(token).ok()?;
        let j = j.lock().ok()?;
        Some(j.mask.as_ref().clone())
    }
}
