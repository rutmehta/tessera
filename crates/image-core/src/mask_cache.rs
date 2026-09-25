//! Byte-budgeted LRU of f32 procedural mask rasters, independent of local sliders.
use crate::CacheStats;
use engine_api::{
    EngineResult,
    id::Digest,
    recipe::LocalAdjustment,
    stage::{ParamHash, StageId},
};
use pipeline_cpu::{Image, masks::MaskOptions};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex},
};

/// Cached alpha payloads stay f32; local amount/parameters never enter the key.
/// The budget bounds cache-owned payloads (callers can retain an `Arc` after eviction).
pub struct MaskRasterCache {
    budget: usize,
    inner: Mutex<Inner>,
}
#[derive(Default)]
struct Inner {
    entries: HashMap<ParamHash, (u64, Arc<[f32]>)>,
    order: BTreeMap<u64, ParamHash>,
    tick: u64,
    bytes: usize,
    stats: CacheStats,
}
impl MaskRasterCache {
    /// Independent payload budget in bytes; zero disables retention.
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            inner: Mutex::new(Inner::default()),
        }
    }
    /// Retained payload bytes.
    pub fn bytes(&self) -> usize {
        self.inner.lock().unwrap().bytes
    }
    /// Hit/miss, insertion, eviction and rejection counters.
    pub fn stats(&self) -> CacheStats {
        self.inner.lock().unwrap().stats
    }
    /// Drop all retained rasters and reset counters.
    pub fn clear(&self) {
        *self.inner.lock().unwrap() = Inner::default();
    }

    /// Rasterize or reuse a mask at one pyramid level. `upstream` is the Color
    /// stage chain hash. Actual pixel bytes are additionally hashed: a reused
    /// ImageId, changed decoder output or different dimensions cannot alias.
    pub fn rasterize(
        &self,
        input: &Image,
        group: &LocalAdjustment,
        level: u8,
        upstream: ParamHash,
        options: MaskOptions<'_>,
    ) -> EngineResult<Arc<[f32]>> {
        if input.planes().len() != 3 || input.planes().iter().flatten().any(|v| !v.is_finite()) {
            return Err(engine_api::EngineError::invalid(
                "mask",
                "finite RGB image required",
            ));
        }
        // Geometric masks do not depend on RGB. In particular, don't miss on
        // cold-f32 versus warm-f16 upstream tiles during a slider-only edit.
        let uses_rgb = options.refinement.is_some()
            || group.components.iter().any(|c| {
                matches!(
                    c.kind,
                    engine_api::recipe::MaskKind::LuminanceRange { .. }
                        | engine_api::recipe::MaskKind::ColorRange { .. }
                )
            });
        let pixels: Vec<_> = if uses_rgb {
            input.planes().iter().map(|p| hash_plane(p)).collect()
        } else {
            Vec::new()
        };
        let depth = options.depth.map(hash_plane);
        let refinement = options.refinement.map(|r| (r.radius, r.epsilon.to_bits()));
        let key = ParamHash::of(
            StageId::Locals,
            &(
                "mask-raster-v1",
                &group.components,
                group.invert,
                level,
                upstream,
                input.width(),
                input.height(),
                pixels,
                depth,
                refinement,
                options.color_smoothness.to_bits(),
            ),
        );
        {
            let mut inner = self.inner.lock().unwrap();
            inner.tick += 1;
            let tick = inner.tick;
            if let Some((previous, raster)) = inner.entries.get(&key).cloned() {
                inner.order.remove(&previous);
                inner.order.insert(tick, key);
                inner.entries.insert(key, (tick, raster.clone()));
                inner.stats.hits += 1;
                return Ok(raster);
            }
            inner.stats.misses += 1;
        }
        // Do not hold the mutex while running expensive rasterization. Duplicate
        // concurrent misses may compute twice but never double-account payloads.
        let raster: Arc<[f32]> = pipeline_cpu::masks::rasterize(input, group, options)?.into();
        let bytes = std::mem::size_of_val(raster.as_ref());
        let mut inner = self.inner.lock().unwrap();
        if let Some((_, existing)) = inner.entries.get(&key) {
            return Ok(existing.clone());
        }
        if bytes > self.budget {
            inner.stats.rejected += 1;
            return Ok(raster);
        }
        while inner.bytes + bytes > self.budget {
            let (_, oldest) = inner.order.pop_first().expect("accounted entry");
            let (_, value) = inner.entries.remove(&oldest).expect("indexed entry");
            inner.bytes -= std::mem::size_of_val(value.as_ref());
            inner.stats.evictions += 1;
        }
        inner.tick += 1;
        let tick = inner.tick;
        inner.entries.insert(key, (tick, raster.clone()));
        inner.order.insert(tick, key);
        inner.bytes += bytes;
        inner.stats.inserts += 1;
        inner.stats.peak_bytes = inner.stats.peak_bytes.max(inner.bytes);
        Ok(raster)
    }
}
fn hash_plane(plane: &[f32]) -> Digest {
    let bytes: Vec<_> = plane
        .iter()
        .flat_map(|v| v.to_bits().to_le_bytes())
        .collect();
    Digest::derive("image-core mask input f32 v1", &bytes)
}
