//! Ordered smart-filter evaluation before smart-object resampling. The filter
//! crate installs an evaluator to avoid a compositor -> filters dependency cycle.
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use super::{Compositor, DocRef};
use crate::blend::{BlendMode, blend_pixel, dissolve_threshold};
use crate::document::{DocState, Layer, LayerKind, SmartFilter, SmartObject};
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

/// Adapter supplied by the filters crate (or a host extension). Implementations
/// must return same-size straight F32 RGBA, preserve input, and be deterministic.
pub trait SmartFilterEvaluator: Send + Sync {
    /// Evaluate a whole nested composite; neighbourhood operators gather their
    /// halos internally, never from separately filtered compositor tiles.
    fn evaluate(&self, input: &Raster, filter: &SmartFilter) -> EngineResult<Raster>;
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
    ) -> EngineResult<wgpu::Buffer>;
}

struct BasicFilters;
impl SmartFilterEvaluator for BasicFilters {
    fn evaluate(&self, input: &Raster, filter: &SmartFilter) -> EngineResult<Raster> {
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
fn evaluate_transform(input: &Raster, op: &transform::TransformOp) -> EngineResult<Raster> {
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| Vec::with_capacity(w * h));
    for y in 0..e.height {
        for x in 0..e.width {
            let p = input.pixel(x, y);
            for c in 0..4 {
                planes[c].push(if c == 3 { p[3] } else { p[c] * p[3] });
            }
        }
    }
    let image = transform::Image::new(w, h, planes)
        .map_err(|e| EngineError::invalid("transform", e.to_string()))?;
    // A content-aware resize changes content bounds, not the child canvas.
    // Keep stack masks/blends in the original canvas, clipping or padding at origin.
    let (rw, rh) = match &op.operation {
        transform::Operation::ContentAwareScale(p) => (p.target_width, p.target_height),
        _ => (w, h),
    };
    let result = op
        .apply(&image, rw, rh, 0)
        .map_err(|e| EngineError::invalid("transform", e.to_string()))?;
    let mut out = Raster::new(e, 4, Depth::F32, 0.0);
    out.edit_region(Rect::of_extent(e), 1, |x, y, p| {
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
    Ok(out)
}

type CacheKey = (u64, u64, [u8; 32]);
struct Cached {
    source: Raster,
    result: Raster,
    bytes: usize,
}
pub(super) struct FilterRuntime {
    evaluator: Arc<dyn SmartFilterEvaluator>,
    cache: Mutex<HashMap<CacheKey, Arc<Cached>>>,
    budget: usize,
    evaluations: AtomicU64,
}
impl FilterRuntime {
    pub fn new(budget: usize) -> Self {
        Self {
            evaluator: Arc::new(BasicFilters),
            cache: Mutex::new(HashMap::new()),
            budget,
            evaluations: AtomicU64::new(0),
        }
    }
    pub fn clear(&self) {
        self.cache.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
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

    /// Number of enabled filter evaluations since this compositor was created.
    /// Warm source/parameter cache hits and mask-only edits do not increment it.
    pub fn filter_evaluations(&self) -> u64 {
        self.filter_runtime.evaluations.load(Ordering::Relaxed)
    }

    pub(crate) fn filtered_source(&self, so: &SmartObject) -> EngineResult<Option<FilteredSource>> {
        if !so.filters.iter().any(|f| f.enabled) {
            return Ok(None);
        }
        let params = serde_json::to_vec(&so.filters)
            .map_err(|e| EngineError::invalid("smart filters", e.to_string()))?;
        let key = (so.key, so.state.rev, *blake3::hash(&params).as_bytes());
        let rt = &self.filter_runtime;
        let cached = rt
            .cache
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
            .cloned();
        let cached = if let Some(cached) = cached {
            cached
        } else {
            // Nested smart objects can recurse into this runtime: never hold its
            // cache lock while compositing the input document.
            let source = self.source_raster(DocRef {
                state: &so.state,
                key: so.key,
            })?;
            let mut cache = rt.cache.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(cached) = cache.get(&key) {
                cached.clone()
            } else {
                let mut result = source.clone();
                for filter in so.filters.iter().filter(|f| f.enabled) {
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
                        evaluate_transform(&result, &op)?
                    } else {
                        rt.evaluator.evaluate(&result, filter)?
                    };
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
                        let blend =
                            blend_pixel(filter.blend.mode, [a[0], a[1], a[2]], [b[0], b[1], b[2]]);
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
                    rt.evaluations.fetch_add(1, Ordering::Relaxed);
                }
                let bytes = source.extent().area() as usize * 32;
                let entry = Arc::new(Cached {
                    source,
                    result,
                    bytes,
                });
                if bytes <= rt.budget {
                    if cache
                        .values()
                        .map(|v| v.bytes)
                        .sum::<usize>()
                        .saturating_add(bytes)
                        > rt.budget
                    {
                        cache.clear();
                    }
                    cache.insert(key, entry.clone());
                }
                entry
            }
        };
        let mut raster = cached.result.clone();
        if let Some(mask) = so.filter_mask.as_ref().filter(|m| m.enabled) {
            if mask.raster.extent() != raster.extent()
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
            let mut valid = true;
            raster.edit_region(Rect::of_extent(raster.extent()), 1, |x, y, p| {
                let m = mask.raster.pixel(x, y)[0];
                valid &= m.is_finite() && (0.0..=1.0).contains(&m);
                let t = 1.0 - mask.density * (1.0 - m);
                let a = cached.source.pixel(x, y);
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
            })?;
            if !valid {
                return Err(EngineError::invalid(
                    "smart filter mask",
                    "expected finite [0,1] samples",
                ));
            }
        }
        let mut state = DocState::new(raster.extent(), Depth::F32);
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
