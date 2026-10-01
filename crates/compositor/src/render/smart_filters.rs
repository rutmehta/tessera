//! Ordered smart-filter evaluation before smart-object resampling. The filter
//! crate installs an evaluator to avoid a compositor -> filters dependency cycle.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    tile::{TILE_SIZE, Tile, TileCoord, TileFormat},
};
use serde::{Deserialize, Serialize};

use super::{Compositor, DocRef};
use crate::blend::{BlendMode, blend_pixel, dissolve_threshold};
use crate::document::{DocState, Layer, LayerKind, Mask, SmartFilter, SmartObject};
use crate::geom::{Rect, next_doc_key};
use crate::raster::{Depth, Raster};

/// Blending options of one filter's result against its input.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FilterBlend {
    /// Blend function, evaluated on straight RGB.
    pub mode: BlendMode,
    /// Interpolation amount, finite and in [0,1].
    pub opacity: f32,
}
impl Default for FilterBlend {
    fn default() -> Self {
        Self {
            mode: BlendMode::Normal,
            opacity: 1.0,
        }
    }
}

/// Shared ICC profile metadata and bytes, using the document's existing profile
/// representation. Cloning retains the embedded bytes through their `Arc`.
/// `engine_api` provides `IccProfileHandle`, but no byte-bearing profile reference.
pub type ColorProfileRef = crate::document::ColorProfile;

/// Interpretation and geometry of the straight-RGBA filter input.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterContext {
    /// Source document profile; `None` means untagged sRGB.
    pub profile: Option<ColorProfileRef>,
    /// Actual input mip level, not the eventual presentation level.
    pub level: u32,
    /// Full-resolution source document canvas.
    pub canvas: engine_api::tile::Extent,
}

impl FilterContext {
    pub(crate) fn native(state: &DocState) -> Self {
        Self {
            profile: state.profile.clone(),
            level: 0,
            canvas: state.canvas,
        }
    }

    pub(crate) fn cache_digest(&self) -> [u8; 32] {
        let mut h = blake3::Hasher::new();
        h.update(&self.level.to_le_bytes());
        h.update(&self.canvas.width.to_le_bytes());
        h.update(&self.canvas.height.to_le_bytes());
        h.update(&[u8::from(self.profile.is_some())]);
        if let Some(profile) = &self.profile {
            // Include both handle and embedded content: unresolved profiles must
            // not reuse results produced when bytes were available.
            h.update(profile.handle.0.as_bytes());
            h.update(blake3::hash(profile.name.as_bytes()).as_bytes());
            h.update(&[u8::from(profile.icc.is_some())]);
            if let Some(icc) = &profile.icc {
                h.update(blake3::hash(icc).as_bytes());
            }
        }
        *h.finalize().as_bytes()
    }
}

/// Adapter supplied by the filters crate (or a host extension). Implementations
/// must return same-size straight F32 RGBA, preserve input, and be deterministic.
pub trait SmartFilterEvaluator: Send + Sync {
    /// Evaluate a whole nested composite; neighbourhood operators gather their
    /// halos internally, never from separately filtered compositor tiles.
    fn evaluate(
        &self,
        input: &Raster,
        filter: &SmartFilter,
        context: &FilterContext,
    ) -> EngineResult<Raster>;

    /// Evaluate with a caller token. Existing evaluators receive boundary
    /// checks; implementations may override this to check inside their work.
    fn evaluate_with_cancel(
        &self,
        input: &Raster,
        filter: &SmartFilter,
        context: &FilterContext,
        cancel: &CancellationToken,
    ) -> EngineResult<Raster> {
        cancel.check()?;
        let result = self.evaluate(input, filter, context)?;
        cancel.check()?;
        Ok(result)
    }
}

pub(super) fn check_render_cancel(cancel: Option<&CancellationToken>) -> EngineResult<()> {
    if let Some(cancel) = cancel {
        cancel.check()?;
    }
    Ok(())
}

/// Shared-device smart-filter bridge. Buffers are tightly interleaved straight
/// f32 RGBA. Implementations submit on the supplied queue without pixel readback.
pub trait ResidentFilterEvaluator: SmartFilterEvaluator {
    /// Capability preflight, before any stage is executed.
    fn supports(&self, filter: &SmartFilter) -> EngineResult<bool>;
    /// Return a same-extent STORAGE | COPY_SRC buffer on `device`.
    fn evaluate_resident(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        filter: &SmartFilter,
        context: &FilterContext,
    ) -> EngineResult<wgpu::Buffer>;
}

struct BasicFilters;
impl SmartFilterEvaluator for BasicFilters {
    fn evaluate(
        &self,
        input: &Raster,
        filter: &SmartFilter,
        _context: &FilterContext,
    ) -> EngineResult<Raster> {
        if matches!(filter.name.as_str(), "gaussian" | "gaussian_blur") {
            return gaussian(input, &filter.params);
        }
        if filter.name != "invert" {
            return Err(EngineError::Unsupported {
                what: format!(
                    "smart filter {}: install filters::CompositorFilters",
                    filter.name
                ),
            });
        }
        let mut out = input.clone();
        out.edit_region(Rect::of_extent(input.extent()), 1, |_, _, p| {
            for c in &mut p[..3] {
                *c = 1.0 - *c;
            }
        })?;
        Ok(out)
    }
}

// Minimal dependency-free fallback for native documents. The filters adapter
// provides the complete inventory and accelerated/large-radius implementations.
fn gaussian(input: &Raster, params: &serde_json::Value) -> EngineResult<Raster> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Gaussian {
        radius: f32,
    }
    let p: Gaussian = serde_json::from_value(params.clone())
        .map_err(|e| EngineError::invalid("gaussian", e.to_string()))?;
    if !p.radius.is_finite() || !(0.0..=250.0).contains(&p.radius) {
        return Err(EngineError::invalid(
            "gaussian",
            "radius must be finite [0,250]",
        ));
    }
    if p.radius == 0.0 {
        return Ok(input.clone());
    }
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut a: Vec<[f32; 4]> = (0..e.height)
        .flat_map(|y| (0..e.width).map(move |x| input.pixel(x, y)))
        .collect();
    let mut b = a.clone();
    let radius = (3.0 * p.radius).ceil() as i32;
    let mut k: Vec<f32> = (-radius..=radius)
        .map(|i| (-0.5 * (i as f32 / p.radius).powi(2)).exp())
        .collect();
    let sum: f32 = k.iter().sum();
    for v in &mut k {
        *v /= sum;
    }
    for vertical in [false, true] {
        for y in 0..h {
            for x in 0..w {
                let mut value = [0.0; 4];
                for (j, weight) in k.iter().enumerate() {
                    let d = j as i32 - radius;
                    let xx =
                        (x as i32 + if vertical { 0 } else { d }).clamp(0, w as i32 - 1) as usize;
                    let yy =
                        (y as i32 + if vertical { d } else { 0 }).clamp(0, h as i32 - 1) as usize;
                    for (c, v) in value.iter_mut().enumerate() {
                        *v += a[yy * w + xx][c] * weight;
                    }
                }
                b[y * w + x] = value;
            }
        }
        std::mem::swap(&mut a, &mut b);
    }
    let mut out = input.clone();
    out.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        *p = a[y as usize * w + x as usize]
    })?;
    Ok(out)
}

// Transform kernels require premultiplied planes. Evaluate at native child
// resolution; the existing smart-object resampler selects output mip levels.
fn map_transform_result<T>(result: transform::Result<T>) -> EngineResult<T> {
    result.map_err(|error| match error {
        transform::Error::Cancelled => EngineError::Cancelled,
        transform::Error::Invalid(_) => EngineError::invalid("transform", error.to_string()),
    })
}

fn evaluate_transform(
    input: &Raster,
    op: &transform::TransformOp,
    cancel: Option<&CancellationToken>,
) -> EngineResult<Raster> {
    check_render_cancel(cancel)?;
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::with_capacity(w * h));
    for y in 0..e.height {
        check_render_cancel(cancel)?;
        for x in 0..e.width {
            if x & 1023 == 0 {
                check_render_cancel(cancel)?;
            }
            let p = input.pixel(x, y);
            for c in 0..4 {
                planes[c].push(if c == 3 { p[3] } else { p[c] * p[3] });
            }
        }
    }
    let image = map_transform_result(transform::Image::new(w, h, planes))?;
    check_render_cancel(cancel)?;
    // A content-aware resize changes content bounds, not the child canvas.
    // Keep stack masks/blends in the original canvas, clipping or padding at origin.
    let (rw, rh) = match &op.operation {
        transform::Operation::ContentAwareScale(p) => (p.target_width, p.target_height),
        _ => (w, h),
    };
    let result = map_transform_result(if let Some(cancel) = cancel {
        op.apply_with_cancel(&image, rw, rh, 0, cancel)
    } else {
        op.apply(&image, rw, rh, 0)
    })?;
    check_render_cancel(cancel)?;
    let mut out = Raster::new(e, 4, Depth::F32, 0.0);
    let (cols, rows) = e.tile_grid(TILE_SIZE);
    for ty in 0..rows {
        for tx in 0..cols {
            check_render_cancel(cancel)?;
            out.edit_region(Rect::of_tile(TileCoord::new(0, tx, ty), e), 1, |x, y, p| {
                if x as usize >= rw || y as usize >= rh {
                    *p = [0.; 4];
                    return;
                }
                let i = y as usize * rw + x as usize;
                let a = result.planes[3][i];
                for (c, channel) in p.iter_mut().enumerate().take(3) {
                    *channel = if a > 0.0 {
                        result.planes[c][i] / a
                    } else {
                        0.0
                    };
                }
                p[3] = a;
            })?;
        }
    }
    check_render_cancel(cancel)?;
    Ok(out)
}

type CacheKey = (u64, u64, [u8; 32], [u8; 32]);
type MaskedKey = (CacheKey, [u8; 32]);

#[cfg(test)]
mod cancellation_mapping_tests {
    use engine_api::{EngineError, jobs::CancellationToken, tile::Extent};

    use crate::document::Mask;
    use crate::raster::{Depth, Raster};

    #[test]
    fn transform_cancelled_is_not_reported_as_invalid_input() {
        assert!(matches!(
            super::map_transform_result::<()>(Err(transform::Error::Cancelled)),
            Err(EngineError::Cancelled)
        ));
    }

    #[test]
    fn partial_mask_cancellation_counts_only_completed_tiles() {
        let extent = Extent::new(257, 1);
        let source = Raster::new(extent, 4, Depth::F32, 0.0);
        let mut result = source.clone();
        let mask = Mask::hide_all(extent, Depth::F32);
        let runtime = super::FilterRuntime::new(0);
        let cancel = CancellationToken::new();
        let mut completed = 0;

        assert!(matches!(
            super::blend_mask_tiles(
                &mut result,
                &source,
                &mask,
                &runtime,
                Some(&cancel),
                |_, _| {
                    completed += 1;
                    cancel.cancel();
                },
            ),
            Err(EngineError::Cancelled)
        ));
        assert_eq!(completed, 1);
        let stats = runtime.stats();
        assert_eq!(stats.mask_pixels_visited, 256);
        assert_eq!(stats.mask_tile_bytes_produced, 256 * 4 * 4);
    }
}

struct Cached {
    source: Raster,
    result: Raster,
    bytes: usize,
}

/// Retained filter results allowed during one full-level CPU render.
/// This excludes temporary rasters, evaluator allocations, tile caches and GPU memory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FilterPassLimits {
    /// Maximum combined unmasked source/result and masked-output tile charge
    /// of live pass entries.
    /// Zero rejects every filtered source reached by the frame.
    pub retained_bytes: usize,
    /// Maximum number of live pass entries, including in-progress reservations.
    /// Zero rejects every filtered source reached by the frame.
    pub entries: usize,
}

impl Default for FilterPassLimits {
    fn default() -> Self {
        Self {
            retained_bytes: 1 << 30,
            entries: 256,
        }
    }
}

#[derive(Default)]
struct PassState {
    entries: HashMap<CacheKey, Arc<Cached>>,
    masked: HashMap<MaskedKey, Arc<Raster>>,
    reserved_bytes: usize,
    reserved_entries: usize,
}

/// One caller's full-level results; never shared with direct-tile or other frame calls.
pub(crate) struct FilterPass {
    limits: FilterPassLimits,
    state: Mutex<PassState>,
}

struct PassReservation<'a> {
    pass: &'a FilterPass,
    bytes: usize,
    active: bool,
}

impl FilterPass {
    pub(super) fn new(limits: FilterPassLimits) -> Self {
        Self {
            limits,
            state: Mutex::new(PassState::default()),
        }
    }

    fn get(&self, key: &CacheKey) -> Option<Arc<Cached>> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entries
            .get(key)
            .cloned()
    }

    fn get_masked(&self, key: &MaskedKey) -> Option<Arc<Raster>> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .masked
            .get(key)
            .cloned()
    }

    fn reserve(&self, bytes: usize) -> EngineResult<PassReservation<'_>> {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let next_bytes = state.reserved_bytes.checked_add(bytes);
        let next_entries = state.reserved_entries.checked_add(1);
        if next_bytes.is_none_or(|n| n > self.limits.retained_bytes)
            || next_entries.is_none_or(|n| n > self.limits.entries)
        {
            return Err(EngineError::ResourceExhausted {
                resource: "CPU smart-filter pass retained results exceed configured limit".into(),
            });
        }
        state.reserved_bytes = next_bytes.unwrap();
        state.reserved_entries = next_entries.unwrap();
        Ok(PassReservation {
            pass: self,
            bytes,
            active: true,
        })
    }
}

impl PassReservation<'_> {
    fn commit(mut self, key: CacheKey, entry: Arc<Cached>) -> Arc<Cached> {
        let mut state = self.pass.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = state.entries.get(&key).cloned() {
            state.reserved_bytes -= self.bytes;
            state.reserved_entries -= 1;
            self.active = false;
            return existing;
        }
        state.entries.insert(key, entry.clone());
        self.active = false;
        entry
    }

    fn commit_masked(mut self, key: MaskedKey, raster: Arc<Raster>) -> Arc<Raster> {
        let mut state = self.pass.state.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(existing) = state.masked.get(&key).cloned() {
            state.reserved_bytes -= self.bytes;
            state.reserved_entries -= 1;
            self.active = false;
            return existing;
        }
        state.masked.insert(key, raster.clone());
        self.active = false;
        raster
    }
}

impl Drop for PassReservation<'_> {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.pass.state.lock().unwrap_or_else(|e| e.into_inner());
            state.reserved_bytes -= self.bytes;
            state.reserved_entries -= 1;
        }
    }
}

#[cfg(test)]
mod pass_reservation_tests {
    use super::{FilterPass, FilterPassLimits};

    #[test]
    fn failed_nested_admission_releases_outer_charge_in_same_pass() {
        let pass = FilterPass::new(FilterPassLimits {
            retained_bytes: 16,
            entries: 1,
        });
        {
            let outer = pass.reserve(16).unwrap();
            assert!(pass.reserve(1).is_err());
            drop(outer); // Models source/evaluator failure after a nested admission error.
        }
        let next = pass.reserve(16).unwrap();
        let state = pass.state.lock().unwrap();
        assert_eq!(state.reserved_bytes, 16);
        assert_eq!(state.reserved_entries, 1);
        drop(state);
        drop(next);
        let state = pass.state.lock().unwrap();
        assert_eq!(state.reserved_bytes, 0);
        assert_eq!(state.reserved_entries, 0);
    }
}

fn entry_bytes(so: &SmartObject) -> EngineResult<usize> {
    usize::try_from(so.state.canvas.area())
        .ok()
        .and_then(|area| area.checked_mul(32))
        .ok_or_else(|| EngineError::ResourceExhausted {
            resource: "CPU smart-filter source and result size overflow".into(),
        })
}

fn masked_entry_bytes(raster: &Raster) -> EngineResult<usize> {
    usize::try_from(raster.extent().area())
        .ok()
        .and_then(|area| area.checked_mul(16))
        .ok_or_else(|| EngineError::ResourceExhausted {
            resource: "CPU smart-filter masked result size overflow".into(),
        })
}

// Hash semantic mask data rather than layer IDs, revisions or temporary style
// object addresses. Dense masks still require a full stored-byte scan on each
// lookup; the counter makes that residual work visible.
fn mask_digest(
    mask: &Mask,
    cancel: Option<&CancellationToken>,
    bytes_visited: &AtomicU64,
) -> EngineResult<[u8; 32]> {
    check_render_cancel(cancel)?;
    let mut hash = blake3::Hasher::new();
    hash.update(b"tessera-smart-filter-mask-v1");
    hash.update(&[u8::from(mask.enabled)]);
    hash.update(&mask.density.to_bits().to_le_bytes());
    hash.update(&mask.feather.to_bits().to_le_bytes());
    let raster = &mask.raster;
    let extent = raster.extent();
    hash.update(&extent.width.to_le_bytes());
    hash.update(&extent.height.to_le_bytes());
    hash.update(&[raster.channels()]);
    hash.update(&[match raster.depth() {
        Depth::U8 => 1,
        Depth::U16 => 2,
        Depth::F32 => 4,
    }]);
    hash.update(&raster.default_value().to_bits().to_le_bytes());
    for ((tx, ty), slot) in raster.slots() {
        check_render_cancel(cancel)?;
        hash.update(&tx.to_le_bytes());
        hash.update(&ty.to_le_bytes());
        let Some(tile) = &slot.tile else {
            hash.update(&[0]);
            continue;
        };
        hash.update(&[1]);
        let layout = tile.layout();
        hash.update(&layout.extent.width.to_le_bytes());
        hash.update(&layout.extent.height.to_le_bytes());
        hash.update(&layout.halo.to_le_bytes());
        hash.update(&[layout.channels]);
        hash.update(&[match tile.format() {
            TileFormat::U8 => 1,
            TileFormat::U16 => 2,
            TileFormat::F16Planar => 3,
            TileFormat::F32Planar => 4,
        }]);
        match tile.format() {
            TileFormat::U8 => {
                hash.update(tile.samples::<u8>()?);
            }
            TileFormat::U16 => {
                for sample in tile.samples::<u16>()? {
                    hash.update(&sample.to_le_bytes());
                }
            }
            TileFormat::F16Planar => {
                for sample in tile.samples::<half::f16>()? {
                    hash.update(&sample.to_bits().to_le_bytes());
                }
            }
            TileFormat::F32Planar => {
                for sample in tile.samples::<f32>()? {
                    hash.update(&sample.to_bits().to_le_bytes());
                }
            }
        }
        bytes_visited.fetch_add(
            u64::try_from(tile.byte_len()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }
    check_render_cancel(cancel)?;
    Ok(*hash.finalize().as_bytes())
}

/// Whole-image CPU filter work, including source work discarded after a cold race.
/// Relaxed atomics give a non-transactional snapshot; active counts refer to
/// stack attempts, not threads. These are not process-memory or GPU counters.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FilterEvaluationStats {
    /// Cold misses that began rendering the whole child source, including errors.
    pub attempted_stacks: u64,
    /// Calls to a transform or evaluator, including failed and discarded calls.
    pub attempted_stages: u64,
    /// Completed results discarded after another caller published the same
    /// persistent-cache key. Oversized repeat work has attempts but no winner
    /// retained in that cache, so it does not increment this field.
    pub duplicate_stacks: u64,
    /// Completed stacks whose source plus result exceeded the persistent budget.
    pub oversized_stacks: u64,
    /// Cold source/stack attempts currently running in this compositor.
    pub active_stacks: u64,
    /// Highest simultaneous cold source/stack attempt count observed.
    pub peak_active_stacks: u64,
    /// Full-child mask blend attempts, including repeated work on pass hits.
    pub mask_compositions: u64,
    /// Pixels visited by full-child mask blending, including failed masks.
    pub mask_pixels_visited: u64,
    /// Payload bytes of materialized masked output tiles. This
    /// excludes scratch buffers, retained copies, and allocator overhead.
    pub mask_tile_bytes_produced: u64,
    /// Mask tile payload bytes scanned to form frame-local reuse keys. Dense
    /// masks are rescanned on each lookup; this is not all mask-related work.
    pub mask_digest_bytes_visited: u64,
}

pub(super) struct FilterRuntime {
    pub(super) evaluator: Arc<dyn SmartFilterEvaluator>,
    cache: Mutex<HashMap<CacheKey, Arc<Cached>>>,
    budget: usize,
    evaluations: AtomicU64,
    attempted_stacks: AtomicU64,
    attempted_stages: AtomicU64,
    duplicate_stacks: AtomicU64,
    oversized_stacks: AtomicU64,
    active_stacks: AtomicU64,
    peak_active_stacks: AtomicU64,
    mask_compositions: AtomicU64,
    mask_pixels_visited: AtomicU64,
    mask_tile_bytes_produced: AtomicU64,
    mask_digest_bytes_visited: AtomicU64,
}

struct ActiveStack<'a>(&'a FilterRuntime);

impl Drop for ActiveStack<'_> {
    fn drop(&mut self) {
        self.0.active_stacks.fetch_sub(1, Ordering::Relaxed);
    }
}

impl FilterRuntime {
    pub fn new(budget: usize) -> Self {
        Self {
            evaluator: Arc::new(BasicFilters),
            cache: Mutex::new(HashMap::new()),
            budget,
            evaluations: AtomicU64::new(0),
            attempted_stacks: AtomicU64::new(0),
            attempted_stages: AtomicU64::new(0),
            duplicate_stacks: AtomicU64::new(0),
            oversized_stacks: AtomicU64::new(0),
            active_stacks: AtomicU64::new(0),
            peak_active_stacks: AtomicU64::new(0),
            mask_compositions: AtomicU64::new(0),
            mask_pixels_visited: AtomicU64::new(0),
            mask_tile_bytes_produced: AtomicU64::new(0),
            mask_digest_bytes_visited: AtomicU64::new(0),
        }
    }
    fn begin_stack(&self) -> ActiveStack<'_> {
        self.attempted_stacks.fetch_add(1, Ordering::Relaxed);
        let active = self.active_stacks.fetch_add(1, Ordering::Relaxed) + 1;
        self.peak_active_stacks.fetch_max(active, Ordering::Relaxed);
        ActiveStack(self)
    }
    fn stats(&self) -> FilterEvaluationStats {
        let get = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        FilterEvaluationStats {
            attempted_stacks: get(&self.attempted_stacks),
            attempted_stages: get(&self.attempted_stages),
            duplicate_stacks: get(&self.duplicate_stacks),
            oversized_stacks: get(&self.oversized_stacks),
            active_stacks: get(&self.active_stacks),
            peak_active_stacks: get(&self.peak_active_stacks),
            mask_compositions: get(&self.mask_compositions),
            mask_pixels_visited: get(&self.mask_pixels_visited),
            mask_tile_bytes_produced: get(&self.mask_tile_bytes_produced),
            mask_digest_bytes_visited: get(&self.mask_digest_bytes_visited),
        }
    }
    pub fn clear(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

/// Blend one masked output tile at a time so cancellation and work counters
/// observe completed tiles. The callback is a deterministic test checkpoint.
fn blend_mask_tiles(
    raster: &mut Raster,
    source: &Raster,
    mask: &Mask,
    rt: &FilterRuntime,
    cancel: Option<&CancellationToken>,
    mut after_tile: impl FnMut(u32, u32),
) -> EngineResult<bool> {
    let mut valid = true;
    let raster_extent = raster.extent();
    let (cols, rows) = raster_extent.tile_grid(TILE_SIZE);
    for ty in 0..rows {
        for tx in 0..cols {
            check_render_cancel(cancel)?;
            let mut visited = 0u64;
            let edit = raster.edit_region(
                Rect::of_tile(TileCoord::new(0, tx, ty), raster_extent),
                1,
                |x, y, p| {
                    visited = visited.saturating_add(1);
                    let m = mask.raster.pixel(x, y)[0];
                    valid &= m.is_finite() && (0.0..=1.0).contains(&m);
                    let t = 1.0 - mask.density * (1.0 - m);
                    let a = source.pixel(x, y);
                    let b = *p;
                    let alpha = a[3] + t * (b[3] - a[3]);
                    for c in 0..3 {
                        p[c] = if alpha > 0.0 {
                            (a[c] * a[3] * (1.0 - t) + b[c] * b[3] * t) / alpha
                        } else {
                            0.0
                        };
                    }
                    p[3] = alpha;
                },
            );
            rt.mask_pixels_visited.fetch_add(visited, Ordering::Relaxed);
            edit?;
            let produced = raster.tile(tx, ty).map(Tile::byte_len).unwrap_or(0);
            rt.mask_tile_bytes_produced.fetch_add(
                u64::try_from(produced).unwrap_or(u64::MAX),
                Ordering::Relaxed,
            );
            after_tile(tx, ty);
        }
    }
    check_render_cancel(cancel)?;
    Ok(valid)
}

pub(crate) struct FilteredSource {
    pub state: DocState,
    pub key: u64,
}

impl Compositor {
    /// Installs the host's filter implementation and invalidates all composites.
    pub fn set_filter_evaluator(&mut self, evaluator: Arc<dyn SmartFilterEvaluator>) {
        self.clear();
        self.filter_runtime.evaluator = evaluator;
    }

    /// Enabled, nonzero-opacity stages in accepted stack results since creation.
    /// Warm source/parameter cache hits and mask-only edits do not increment it.
    /// Concurrent cold misses may compute duplicate results; discarded duplicates
    /// and failed stacks do not increment this counter. Uncached results do.
    pub fn filter_evaluations(&self) -> u64 {
        self.filter_runtime.evaluations.load(Ordering::Relaxed)
    }

    /// Actual whole-image CPU filter attempts, including failed and duplicate work.
    pub fn filter_evaluation_stats(&self) -> FilterEvaluationStats {
        self.filter_runtime.stats()
    }

    pub(crate) fn filtered_source(
        &self,
        so: &SmartObject,
        pass: Option<&FilterPass>,
        style_pass: Option<&super::style_pass::StylePass>,
        cancel: Option<&CancellationToken>,
    ) -> EngineResult<Option<FilteredSource>> {
        check_render_cancel(cancel)?;
        if !so.filters.iter().any(|f| f.enabled) {
            return Ok(None);
        }
        let params = serde_json::to_vec(&so.filters)
            .map_err(|e| EngineError::invalid("smart filters", e.to_string()))?;
        // Stacks run on the child composite at native resolution, before any
        // smart-object resampling. The outer document's profile is not the input's.
        let context = FilterContext::native(&so.state);
        let key = (
            so.key,
            so.state.rev,
            *blake3::hash(&params).as_bytes(),
            context.cache_digest(),
        );
        let rt = &self.filter_runtime;
        let bytes = entry_bytes(so)?;
        check_render_cancel(cancel)?;
        let pass_cached = pass.and_then(|p| p.get(&key));
        let cached = if let Some(cached) = pass_cached {
            cached
        } else {
            let persistent = rt
                .cache
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&key)
                .cloned();
            if let Some(cached) = persistent {
                // Pin even a persistent hit for this pass: another stack may
                // evict it before later tiles reach this smart object.
                if let Some(pass) = pass {
                    pass.reserve(bytes)?.commit(key, cached)
                } else {
                    cached
                }
            } else {
                // Reserve before source_raster: nested smart objects need their own
                // simultaneous charge, and errors release this one by RAII.
                let reservation = pass.map(|p| p.reserve(bytes)).transpose()?;
                check_render_cancel(cancel)?;
                // Nested smart objects can recurse into this runtime: never hold its
                // cache lock while compositing the input document.
                let _active = rt.begin_stack();
                let source = self.source_raster(DocRef {
                    state: &so.state,
                    key: so.key,
                    pass,
                    style_pass,
                    cancel,
                })?;
                check_render_cancel(cancel)?;
                let cached = rt
                    .cache
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&key)
                    .cloned();
                let published = if let Some(cached) = cached {
                    cached
                } else {
                    // Evaluators and transforms can use Rayon or re-enter this cache.
                    // Never hold the mutex (or wait for per-key initialization) here:
                    // a worker may steal another tile that needs the same entry.
                    // Duplicate cold work is safe because evaluators are deterministic.
                    let mut result = source.clone();
                    let mut evaluations = 0;
                    for filter in so.filters.iter().filter(|f| f.enabled) {
                        check_render_cancel(cancel)?;
                        if !filter.blend.opacity.is_finite()
                            || !(0.0..=1.0).contains(&filter.blend.opacity)
                        {
                            return Err(EngineError::invalid(
                                "filter opacity",
                                "expected finite [0,1]",
                            ));
                        }
                        if filter.blend.opacity == 0.0 {
                            continue;
                        }
                        let next = if let Some(op) = filter.transform_op()? {
                            rt.attempted_stages.fetch_add(1, Ordering::Relaxed);
                            evaluate_transform(&result, &op, cancel)?
                        } else {
                            rt.attempted_stages.fetch_add(1, Ordering::Relaxed);
                            if let Some(cancel) = cancel {
                                rt.evaluator
                                    .evaluate_with_cancel(&result, filter, &context, cancel)?
                            } else {
                                rt.evaluator.evaluate(&result, filter, &context)?
                            }
                        };
                        check_render_cancel(cancel)?;
                        if next.extent() != source.extent()
                            || next.channels() != 4
                            || next.depth() != Depth::F32
                        {
                            return Err(EngineError::invalid(
                                "smart filter",
                                "evaluator changed raster layout",
                            ));
                        }
                        let old = result.clone();
                        let mut finite = true;
                        result.edit_region(Rect::of_extent(source.extent()), 1, |x, y, p| {
                            let a = old.pixel(x, y);
                            let b = next.pixel(x, y);
                            finite &= b.iter().all(|v| v.is_finite());
                            let blend = blend_pixel(
                                filter.blend.mode,
                                [a[0], a[1], a[2]],
                                [b[0], b[1], b[2]],
                            );
                            let mut t = filter.blend.opacity;
                            if filter.blend.mode == BlendMode::Dissolve {
                                t = if dissolve_threshold(x, y, 0) < t {
                                    1.0
                                } else {
                                    0.0
                                };
                            }
                            let alpha = a[3] + t * (b[3] - a[3]);
                            for c in 0..3 {
                                let v = a[c] * a[3] * (1.0 - t) + blend[c] * b[3] * t;
                                p[c] = if alpha > 0.0 { v / alpha } else { 0.0 };
                            }
                            p[3] = alpha;
                        })?;
                        if !finite {
                            return Err(EngineError::invalid("smart filter", "nonfinite result"));
                        }
                        evaluations += 1;
                    }
                    check_render_cancel(cancel)?;
                    let entry = Arc::new(Cached {
                        source,
                        result,
                        bytes,
                    });
                    // Publish only complete, validated results. A concurrent miss
                    // may have finished first; reuse it without charging bytes twice.
                    {
                        check_render_cancel(cancel)?;
                        let mut cache = rt.cache.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(cached) = cache.get(&key) {
                            rt.duplicate_stacks.fetch_add(1, Ordering::Relaxed);
                            cached.clone()
                        } else {
                            rt.evaluations.fetch_add(evaluations, Ordering::Relaxed);
                            if bytes > rt.budget {
                                rt.oversized_stacks.fetch_add(1, Ordering::Relaxed);
                            }
                            if bytes <= rt.budget {
                                if cache
                                    .values()
                                    .fold(0usize, |sum, v| sum.saturating_add(v.bytes))
                                    .saturating_add(bytes)
                                    > rt.budget
                                {
                                    cache.clear();
                                }
                                cache.insert(key, entry.clone());
                            }
                            entry
                        }
                    }
                };
                if let Some(reservation) = reservation {
                    check_render_cancel(cancel)?;
                    reservation.commit(key, published)
                } else {
                    published
                }
            }
        };
        check_render_cancel(cancel)?;
        let raster = if let Some(mask) = so.filter_mask.as_ref().filter(|m| m.enabled) {
            if mask.raster.extent() != cached.result.extent()
                || mask.raster.channels() != 1
                || !mask.density.is_finite()
                || !(0.0..=1.0).contains(&mask.density)
            {
                return Err(EngineError::invalid(
                    "smart filter mask",
                    "same-size one-channel mask and finite density required",
                ));
            }
            if mask.feather != 0.0 {
                return Err(EngineError::Unsupported {
                    what: "smart-filter mask feather".into(),
                });
            }
            let masked_key = if pass.is_some() {
                check_render_cancel(cancel)?;
                let digest = mask_digest(mask, cancel, &rt.mask_digest_bytes_visited)?;
                check_render_cancel(cancel)?;
                Some((key, digest))
            } else {
                None
            };
            if let Some(hit) =
                pass.and_then(|pass| masked_key.as_ref().and_then(|key| pass.get_masked(key)))
            {
                hit.as_ref().clone()
            } else {
                let reservation = pass
                    .map(|pass| pass.reserve(masked_entry_bytes(&cached.result)?))
                    .transpose()?;
                check_render_cancel(cancel)?;
                let mut raster = cached.result.clone();
                rt.mask_compositions.fetch_add(1, Ordering::Relaxed);
                let valid =
                    blend_mask_tiles(&mut raster, &cached.source, mask, rt, cancel, |_, _| {})?;
                if !valid {
                    return Err(EngineError::invalid(
                        "smart filter mask",
                        "expected finite [0,1] samples",
                    ));
                }
                if let (Some(reservation), Some(key)) = (reservation, masked_key) {
                    check_render_cancel(cancel)?;
                    reservation
                        .commit_masked(key, Arc::new(raster))
                        .as_ref()
                        .clone()
                } else {
                    raster
                }
            }
        } else {
            cached.result.clone()
        };
        check_render_cancel(cancel)?;
        let mut state = DocState::new(raster.extent(), Depth::F32);
        state.profile = context.profile;
        state.rev = so.state.rev;
        state.root.push(Arc::new(Layer::new(
            "filtered composite",
            LayerKind::Pixel(raster),
        )));
        Ok(Some(FilteredSource {
            state,
            key: next_doc_key(),
        }))
    }
}
