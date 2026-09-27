//! Native-resolution style barrier with a real source halo around the viewport.
//! Source and effects are independently reduced as straight RGBA, never by
//! styling a mip. Halo boundaries align to the mip grid before reduction.
use std::{collections::HashMap, sync::Arc};

use super::{
    ResidentRenderer,
    program::Program,
    styles_gpu::{GpuStylePlane, StylesGpu},
};
use crate::{
    Document, Rect,
    document::LayerProps,
    render::styles::{LayerStyles, StyleEffect},
};
use engine_api::{EngineError, EngineResult, tile::Extent};

type Key = [u8; 32];

struct Cached {
    source: wgpu::Buffer,
    planes: Vec<GpuStylePlane>,
    region: Rect,
}
impl Cached {
    fn bytes(&self) -> u64 {
        self.planes
            .iter()
            .fold(self.source.size(), |n, p| n.saturating_add(p.pixels.size()))
    }
}
struct Entry {
    value: Arc<Cached>,
    used: u64,
}

#[derive(Default)]
pub(super) struct StyleRuntime {
    gpu: Option<StylesGpu>,
    cache: HashMap<Key, Entry>,
    clock: u64,
    evaluations: u64,
}
impl StyleRuntime {
    pub(super) fn clear(&mut self) {
        self.cache.clear();
    }

    pub(super) fn bytes(&self) -> u64 {
        self.cache
            .values()
            .fold(0u64, |n, e| n.saturating_add(e.value.bytes()))
    }

    fn get(&mut self, key: &Key) -> Option<Arc<Cached>> {
        let entry = self.cache.get_mut(key)?;
        self.clock = self.clock.saturating_add(1);
        entry.used = self.clock;
        Some(entry.value.clone())
    }

    fn insert(&mut self, key: Key, value: Arc<Cached>, budget: u64) {
        let bytes = value.bytes();
        if bytes > budget {
            return;
        }
        self.cache.remove(&key);
        while self.bytes().saturating_add(bytes) > budget {
            let Some(oldest) = self
                .cache
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| *k)
            else {
                break;
            };
            self.cache.remove(&oldest);
        }
        self.clock = self.clock.saturating_add(1);
        self.cache.insert(
            key,
            Entry {
                value,
                used: self.clock,
            },
        );
    }
}

impl ResidentRenderer {
    // Avoid rebuilding auxiliary reservations and hashing megabytes of zeros on
    // an unchanged styled viewport. Public invalidation clears `last` as usual.
    pub(super) fn styles_idle(&self, doc: &Document, level: u8, viewport: Option<Rect>) -> bool {
        let Some(st) = self.levels.get(&level) else {
            return false;
        };
        let Some(last) = &st.last else {
            return false;
        };
        let extent = Rect::of_extent(st.extent);
        let region = viewport.unwrap_or(extent).intersect(&extent);
        last.key == doc.key()
            && last.epoch == doc.epoch()
            && last.rev == doc.state().rev
            && st.require_valid(region).is_ok()
    }

    /// Native-resolution style stacks evaluated; cache hits do not increment it.
    pub fn style_evaluations(&self) -> u64 {
        self.styles.evaluations
    }

    /// Retained source and effect buffers (bounded by this renderer's budget).
    pub fn style_cache_bytes(&self) -> u64 {
        self.styles.bytes()
    }

    /// Populate style metadata and reserve auxiliary storage for GPU-only copies.
    /// Returned offsets are bytes; all shader offsets are f32 element indices.
    pub(super) fn prepare_styles(
        &mut self,
        doc: &Document,
        level: u8,
        viewport: Rect,
        program: &mut Program,
    ) -> EngineResult<Vec<(wgpu::Buffer, u64)>> {
        let mut copies = Vec::new();
        let state = doc.state();
        let extent = state.canvas;
        let limit = self.device.limits().max_storage_buffer_binding_size;
        for (step_index, layer) in &program.styles {
            layer.props.styles.validate()?;
            state.global_light.validate()?;
            let region = halo_region(viewport, extent, level, &layer.props.styles);
            // Include every source tile: nested child effects may reach outside
            // this layer's halo. Unrelated layer edits retain cached planes.
            let (cols, rows) = extent.tile_grid(engine_api::tile::TILE_SIZE);
            let revision = (0..rows)
                .flat_map(|y| (0..cols).map(move |x| layer.stamp(0, x, y)))
                .max()
                .unwrap_or(0);
            let bytes = serde_json::to_vec(&(
                doc.key(),
                doc.epoch(),
                revision,
                extent,
                state.depth,
                layer.id,
                &layer.props.styles,
                state.global_light,
                level,
                [region.x0, region.y0, region.x1, region.y1],
            ))
            .map_err(|e| EngineError::invalid("style cache key", e.to_string()))?;
            let key = *blake3::hash(&bytes).as_bytes();
            let cached = if let Some(cached) = self.styles.get(&key) {
                cached
            } else {
                // Match render::effects exactly: neutralize only properties,
                // retaining the layer's raster/vector masks and all its content.
                let mut source = (**layer).clone();
                source.props = LayerProps::default();
                let mut source_state = (**state).clone();

                source_state.root = vec![Arc::new(source)];
                let source_doc = Document::new(source_state);
                let mut child = Self::with_budget(&self.gpu, self.budget)?;
                // Keep the page pool in the source storage depth, but evaluate
                // adjustment intermediates like the CPU F32 style-source state.
                child.float_adjustments = true;
                child.set_smart_quality(self.smart_quality)?;
                if let Some(evaluator) = self.stack.evaluator.clone() {
                    child.set_filter_evaluator(evaluator)?;
                }
                child.render_viewport(&source_doc, 0, region, 0)?;
                let region = child.levels[&0].region;
                let local = Extent::new(region.width() as u32, region.height() as u32);
                let native = self.convert(&child.levels[&0].out, local, 0)?;
                // Drop the temporary renderer before allocating effect fields.
                drop(child);
                if self.styles.gpu.is_none() {
                    self.styles.gpu = Some(StylesGpu::new(&self.device)?);
                }
                let mut planes = self
                    .styles
                    .gpu
                    .as_ref()
                    .expect("initialized styles")
                    .render(
                        &self.device,
                        &self.queue,
                        &native,
                        local,
                        &layer.props.styles,
                        state.global_light,
                        [region.x0 as u32, region.y0 as u32],
                    )?;
                // StylesGpu's stable CPU-compatible ordering must survive here.
                let source = self.style_mip(native, local, level)?;
                for plane in &mut planes {
                    plane.pixels = self.style_mip(plane.pixels.clone(), local, level)?;
                }
                let value = Arc::new(Cached {
                    source,
                    planes,
                    region,
                });
                self.styles.evaluations = self.styles.evaluations.saturating_add(1);
                self.styles.insert(key, value.clone(), self.budget);
                value
            };

            let source_offset = reserve_aux(&mut program.aux, cached.source.size(), limit)?;
            copies.push((cached.source.clone(), u64::from(source_offset) * 4));
            let metadata_bytes = (cached.planes.len() as u64) * 16;
            let metadata_offset = reserve_aux(&mut program.aux, metadata_bytes, limit)?;
            for (i, plane) in cached.planes.iter().enumerate() {
                let offset = reserve_aux(&mut program.aux, plane.pixels.size(), limit)?;
                let metadata = plane_metadata(
                    offset,
                    plane.mode.index(),
                    plane.opacity,
                    plane.outside,
                    plane.stroke,
                );
                let start = metadata_offset as usize + i * 4;
                program.aux[start..start + 4].copy_from_slice(&metadata);
                copies.push((plane.pixels.clone(), u64::from(offset) * 4));
            }
            let step = &mut program.steps[*step_index];
            step.table = source_offset;
            step.aux = metadata_offset;
            step.aux_n = cached.planes.len() as u32;
            let scale = 1u32 << level;
            step.p[0] = [
                (cached.region.x0 as u32 / scale) as f32,
                (cached.region.y0 as u32 / scale) as f32,
                (cached.region.width() as u32).div_ceil(scale) as f32,
                0.0,
            ];
        }
        Ok(copies)
    }

    fn style_mip(
        &mut self,
        mut pixels: wgpu::Buffer,
        extent: Extent,
        level: u8,
    ) -> EngineResult<wgpu::Buffer> {
        // render::Compositor::mip_float: y-major 2x2 traversal, ignore missing
        // edge samples, sum premultiplied RGB/alpha, then return straight RGB.
        // Each level repeats this reduction (not a single large box average).
        for l in 1..=level {
            let input = extent.at_level(l - 1);
            let output = extent.at_level(l);
            pixels = self.stack_op(
                &pixels,
                &pixels,
                [
                    4,
                    input.width,
                    input.height,
                    0,
                    0,
                    output.width,
                    output.height,
                    0,
                ],
            )?;
        }
        Ok(pixels)
    }
}

fn reserve_aux(aux: &mut Vec<f32>, bytes: u64, limit: u64) -> EngineResult<u32> {
    let exhausted = || EngineError::ResourceExhausted {
        resource: "style auxiliary storage exceeds device or host limits".into(),
    };
    let start = u32::try_from(aux.len()).map_err(|_| exhausted())?;
    let words = usize::try_from(bytes / 4).map_err(|_| exhausted())?;
    let end = aux.len().checked_add(words).ok_or_else(exhausted)?;
    if !bytes.is_multiple_of(4) || end as u64 > limit / 4 || end > u32::MAX as usize {
        return Err(exhausted());
    }
    aux.try_reserve(words).map_err(|_| exhausted())?;
    aux.resize(end, 0.0);
    Ok(start)
}

fn halo_region(view: Rect, canvas: Extent, level: u8, styles: &LayerStyles) -> Rect {
    let sc = styles.scale;
    let radius = styles
        .effects
        .iter()
        .map(|e| match e {
            StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => {
                (s.size * sc).ceil() + (s.spread * sc).ceil() + (s.distance * sc).ceil() + 1.0
            }
            StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => {
                (s.size * sc).ceil() + (s.spread * sc).ceil()
            }
            StyleEffect::Bevel(s) => (s.size * sc).ceil() + (s.soften * sc).ceil() + 1.0,
            StyleEffect::Satin(s) => (s.size * sc).ceil() + (s.distance * sc).ceil() + 1.0,
            StyleEffect::Stroke(s) => (s.size * sc).ceil(),
            _ => 0.0,
        })
        .fold(0.0, f32::max) as i64
        + 2;
    let scale = 1i64 << level;
    let alignment = scale.max(16);
    let down = |v: i64| v.max(0) / alignment * alignment;
    let up = |v: i64| (v.max(0) + alignment - 1) / alignment * alignment;
    Rect::new(
        down(view.x0 * scale - radius),
        down(view.y0 * scale - radius),
        up(view.x1 * scale + radius),
        up(view.y1 * scale + radius),
    )
    .intersect(&Rect::of_extent(canvas))
}

fn plane_metadata(offset: u32, mode: u32, opacity: f32, outside: bool, stroke: bool) -> [f32; 4] {
    [
        f32::from_bits(offset),
        f32::from_bits(mode),
        opacity,
        f32::from_bits(u32::from(outside) | (u32::from(stroke) << 1)),
    ]
}

#[cfg(test)]
mod tests {
    use super::plane_metadata;

    #[test]
    fn metadata_preserves_integer_bits_and_plane_flags() {
        for outside in [false, true] {
            for stroke in [false, true] {
                let m = plane_metadata(0x0100_0001, 23, 0.375, outside, stroke);
                assert_eq!(m[0].to_bits(), 0x0100_0001);
                assert_eq!(m[1].to_bits(), 23);
                assert_eq!(m[2], 0.375);
                assert_eq!(
                    m[3].to_bits(),
                    u32::from(outside) | (u32::from(stroke) << 1)
                );
            }
        }
    }
}
