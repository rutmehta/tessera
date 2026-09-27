//! Level-local style barrier with a source halo around the requested viewport.
//! Geometry scales at the requested level; paints retain canvas coordinates.
use std::{collections::HashMap, sync::Arc};

use super::{
    ResidentRenderer,
    program::Program,
    styles_gpu::{GpuStylePlane, StylesGpu},
};
use crate::{
    Document, Rect,
    document::LayerProps,
    render::styles::{self, LayerStyles},
};
use engine_api::{EngineError, EngineResult, tile::Extent};

type Key = [u8; 32];

pub(super) struct PreparedStyles {
    pub copies: Vec<(wgpu::Buffer, u64)>,
    pub words: usize,
}

impl super::Persistent {
    // GPU plane storage has no CPU shadow. Only upload the metadata prefix.
    pub(super) fn write_prefix(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bytes: &[u8],
        total_bytes: u64,
    ) {
        let len = total_bytes.max(256);
        if self.buffer.as_ref().is_none_or(|b| b.size() < len) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: len,
                usage: self.usage,
                mapped_at_creation: false,
            }));
            self.shadow.clear();
        }
        self.write(device, queue, bytes);
    }
}

// Batch-local witness of the last immutable straight-alpha conversion.
struct ConvertedSource {
    level: u8,
    region: Rect,
    output: wgpu::Buffer,
    source: wgpu::Buffer,
}

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

    /// Level-local style stacks evaluated; cache hits do not increment it.
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
    ) -> EngineResult<PreparedStyles> {
        let mut copies = Vec::new();
        let mut pending = Vec::new();
        // Validate cumulative data plus the growing metadata prefix before
        // retaining each pending style. Final offsets are assigned below.
        let mut reserved_words = program.aux.len();
        let mut reserved_sources = Vec::new();
        let mut previous_source: Option<ConvertedSource> = None;
        // One temporary page resolver per batch, not one per styled layer.
        // Shared raster tiles can then share their GPU uploads and mip pages.
        let mut source_renderer = None;
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
                if !reserved_sources.contains(&cached.source) {
                    reserve_words(&mut reserved_words, cached.source.size(), limit)?;
                    reserved_sources.push(cached.source.clone());
                }
                reserve_plane_words(
                    &mut reserved_words,
                    cached.planes.iter().map(|plane| plane.pixels.size()),
                    limit,
                )?;
                cached
            } else {
                // Match render::effects exactly: neutralize only properties,
                // retaining the layer's raster/vector masks and all its content.
                let mut source = (**layer).clone();
                source.props = LayerProps::default();
                let mut source_state = (**state).clone();

                source_state.root = vec![Arc::new(source)];
                let source_doc = Document::new(source_state);
                if source_renderer.is_none() {
                    let mut child = Self::with_budget(&self.gpu, self.budget)?;
                    if let Some(fonts) = self.live.text_renderer_snapshot() {
                        child.set_text_renderer(fonts);
                    }
                    // Preserve native raster storage while generating F32 intermediates.
                    child.float_intermediates = true;
                    child.set_smart_quality(self.smart_quality)?;
                    if let Some(evaluator) = self.stack.evaluator.clone() {
                        child.set_filter_evaluator(evaluator)?;
                    }
                    source_renderer = Some(child);
                }
                let child = source_renderer
                    .as_mut()
                    .expect("source renderer initialized");
                let frame = child.render_viewport(&source_doc, level, region, 0)?;
                let region = child.levels[&level].region;
                let local = Extent::new(region.width() as u32, region.height() as u32);
                // Reserve the complete expansion before converting the source or
                // creating any effect planes, including earlier pending layers.
                let settings = styles::at_level(&layer.props.styles, level);
                let source_bytes = plane_bytes(local)?;
                let output = &child.levels[&level].out;
                // Existing damage tracking has checked content, masks, transforms
                // and child nodes. Zero dispatch cannot mutate an existing output;
                // viewport overlap copies always target a newly allocated buffer.
                let reusable = previous_source.as_ref().filter(|last| {
                    frame.blocks == 0
                        && last.level == level
                        && last.region == region
                        && last.output == *output
                });
                if reusable.is_none() {
                    reserve_words(&mut reserved_words, source_bytes, limit)?;
                }
                reserve_plane_words(
                    &mut reserved_words,
                    std::iter::repeat_n(source_bytes, StylesGpu::plane_count(&settings)),
                    limit,
                )?;
                let source = if let Some(last) = reusable {
                    last.source.clone()
                } else {
                    let source = self.convert(output, local, 0)?;
                    reserved_sources.push(source.clone());
                    previous_source = Some(ConvertedSource {
                        level,
                        region,
                        output: output.clone(),
                        source: source.clone(),
                    });
                    source
                };

                if self.styles.gpu.is_none() {
                    self.styles.gpu = Some(StylesGpu::new(&self.device)?);
                }
                let planes = self
                    .styles
                    .gpu
                    .as_ref()
                    .expect("initialized styles")
                    .render_at(
                        &self.device,
                        &self.queue,
                        &source,
                        local,
                        &settings,
                        state.global_light,
                        [region.x0 as u32, region.y0 as u32],
                        level,
                    )?;
                debug_assert_eq!(planes.len(), StylesGpu::plane_count(&settings));
                let value = Arc::new(Cached {
                    source,
                    planes,
                    region,
                });
                self.styles.evaluations = self.styles.evaluations.saturating_add(1);
                self.styles.insert(key, value.clone(), self.budget);
                value
            };

            let metadata_bytes = (cached.planes.len() as u64) * 16;
            let metadata_offset = reserve_aux(&mut program.aux, metadata_bytes, limit)?;
            pending.push((*step_index, cached, metadata_offset));
        }
        let mut words = program.aux.len();
        let mut source_offsets: Vec<(wgpu::Buffer, u32)> = Vec::new();
        for (step_index, cached, metadata_offset) in pending {
            // Equality is GPU buffer identity, never matching dimensions/content
            // guesses. Converted sources are immutable after queue submission.
            let source_offset = if let Some((_, offset)) = source_offsets
                .iter()
                .find(|(source, _)| *source == cached.source)
            {
                *offset
            } else {
                let offset = reserve_words(&mut words, cached.source.size(), limit)?;
                copies.push((cached.source.clone(), u64::from(offset) * 4));
                source_offsets.push((cached.source.clone(), offset));
                offset
            };
            for (i, plane) in cached.planes.iter().enumerate() {
                let offset = reserve_words(&mut words, plane.pixels.size(), limit)?;
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
            let step = &mut program.steps[step_index];
            step.table = source_offset;
            step.aux = metadata_offset;
            step.aux_n = cached.planes.len() as u32;
            step.p[0] = [
                cached.region.x0 as f32,
                cached.region.y0 as f32,
                cached.region.width() as f32,
                0.0,
            ];
        }
        Ok(PreparedStyles { copies, words })
    }
}

fn plane_bytes(extent: Extent) -> EngineResult<u64> {
    extent
        .area()
        .checked_mul(16)
        .ok_or_else(|| EngineError::ResourceExhausted {
            resource: "style source byte size overflow".into(),
        })
}

fn reserve_plane_words(
    words: &mut usize,
    plane_bytes: impl IntoIterator<Item = u64>,
    limit: u64,
) -> EngineResult<()> {
    for bytes in plane_bytes {
        reserve_words(words, 16, limit)?;
        reserve_words(words, bytes, limit)?;
    }
    Ok(())
}

fn reserve_aux(aux: &mut Vec<f32>, bytes: u64, limit: u64) -> EngineResult<u32> {
    let mut words = aux.len();
    let start = reserve_words(&mut words, bytes, limit)?;
    aux.try_reserve(words - aux.len())
        .map_err(|_| EngineError::ResourceExhausted {
            resource: "style metadata allocation".into(),
        })?;
    aux.resize(words, 0.0);
    Ok(start)
}

fn reserve_words(words: &mut usize, bytes: u64, limit: u64) -> EngineResult<u32> {
    let exhausted = || EngineError::ResourceExhausted {
        resource: "style auxiliary storage exceeds device or host limits".into(),
    };
    let start = u32::try_from(*words).map_err(|_| exhausted())?;
    let count = usize::try_from(bytes / 4).map_err(|_| exhausted())?;
    let end = words.checked_add(count).ok_or_else(exhausted)?;
    if !bytes.is_multiple_of(4) || end as u64 > limit / 4 || end > u32::MAX as usize {
        return Err(exhausted());
    }
    *words = end;
    Ok(start)
}

fn halo_region(view: Rect, canvas: Extent, level: u8, styles: &LayerStyles) -> Rect {
    view.inflate(styles::halo(&styles::at_level(styles, level)))
        .intersect(&Rect::of_extent(canvas.at_level(level)))
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
    fn unchanged_neutralized_sources_share_conversion_and_auxiliary_copy() {
        use crate::{gpu::GpuCompositor, render::styles::*, *};
        use engine_api::tile::Extent;
        use std::sync::Arc;

        let extent = Extent::new(64, 48);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                *p = [0.6, 0.2, 0.4, if (x + y) % 7 < 4 { 0.7 } else { 0.3 }];
            })
            .unwrap();
        let mut state = DocState::new(extent, Depth::F32);
        for i in 0..3 {
            let mut layer = Layer::new("shared source", LayerKind::Pixel(raster.clone()));
            layer
                .props
                .styles
                .effects
                .push(StyleEffect::ColorOverlay(Overlay {
                    opacity: 0.1 + i as f32 * 0.1,
                    ..Default::default()
                }));
            state.assign_ids(&mut layer, true);
            state.root.push(Arc::new(layer));
        }
        let doc = Document::new(state);
        let gpu = GpuCompositor::new().unwrap();
        let mut resident = super::ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
        for level in [0, 1, 2] {
            let extent = extent.at_level(level);
            let mut program = super::Program::compile(&doc.state().root, 1).unwrap();
            let prepared = resident
                .prepare_styles(&doc, level, Rect::of_extent(extent), &mut program)
                .unwrap();
            assert_eq!(
                prepared.copies.len(),
                4,
                "one immutable source plus three effect planes"
            );
            let offsets: Vec<_> = program
                .styles
                .iter()
                .map(|(i, _)| program.steps[*i].table)
                .collect();
            assert!(offsets.windows(2).all(|pair| pair[0] == pair[1]));
            let frame = resident.render(&doc, level).unwrap();
            assert!(frame.blocks > 0);
            let (_, actual) = resident.read_level(level, false).unwrap();
            let (_, expected) = Compositor::new(8 << 20)
                .render_level_rgba(&doc, level)
                .unwrap();
            assert_eq!(actual.len(), expected.len());
            assert!(
                actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| (a - b).abs() <= 1e-4)
            );
            let evaluations = resident.style_evaluations();
            resident.invalidate();
            assert!(resident.render(&doc, level).unwrap().blocks > 0);
            assert_eq!(resident.style_evaluations(), evaluations);
        }
    }

    #[test]
    fn changed_pixels_masks_children_and_transforms_never_alias_style_sources() {
        use crate::{gpu::GpuCompositor, render::styles::*, *};
        use engine_api::tile::Extent;
        use std::sync::Arc;

        let extent = Extent::new(64, 48);
        let pixel = |color: f32| {
            let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
            raster
                .edit_region(Rect::new(3, 5, 51, 39), 1, |_, _, p| {
                    *p = [color, 0.2, 0.4, 0.6];
                })
                .unwrap();
            Layer::new("source", LayerKind::Pixel(raster))
        };
        let original = pixel(0.6);
        let mut masked = original.clone();
        masked.mask = Some(Mask::hide_all(extent, Depth::F32));
        masked.mask.as_mut().unwrap().density = 0.4;
        let child = |mut layer: Layer| {
            let mut state = DocState::new(extent, Depth::F32);
            state.assign_ids(&mut layer, true);
            state.root.push(Arc::new(layer));
            // Smart cache identity includes a real child document revision.
            Document::new(state).state().as_ref().clone()
        };
        let smart = SmartObject::new(child(original.clone()), Affine::IDENTITY);
        // Independent child documents need independent cache namespaces. A
        // cloned key with hand-built equal tile stamps is not a real edit;
        // same-namespace edits are exercised through EditSmartObject below.
        let changed_child = SmartObject::new(child(pixel(0.9)), Affine::IDENTITY);
        let mut translated = smart.clone();
        translated.transform = Affine::scale_translate(1.0, 1.0, 7.0, 3.0);
        let group = |layer: Layer| {
            let mut group = Layer::group("isolated", GroupMode::Isolated);
            let LayerKind::Group { children, .. } = &mut group.kind else {
                unreachable!()
            };
            children.push(Arc::new(layer));
            group
        };
        let mut dissolve = original.clone();
        dissolve.props.blend_mode = BlendMode::Dissolve;
        let pairs = [
            (original.clone(), pixel(0.9)),
            (original.clone(), masked),
            (group(original), group(pixel(0.9))),
            (
                Layer::new("smart", LayerKind::SmartObject(smart.clone())),
                Layer::new("changed child", LayerKind::SmartObject(changed_child)),
            ),
            (
                Layer::new("smart", LayerKind::SmartObject(smart)),
                Layer::new("translated", LayerKind::SmartObject(translated)),
            ),
            // Distinct child IDs must retain their distinct Dissolve patterns.
            (group(dissolve.clone()), group(dissolve)),
        ];
        let gpu = GpuCompositor::new().unwrap();
        for (case, (first, second)) in pairs.into_iter().enumerate() {
            let mut state = DocState::new(extent, Depth::F32);
            for mut layer in [first, second] {
                layer
                    .props
                    .styles
                    .effects
                    .push(StyleEffect::ColorOverlay(Overlay {
                        opacity: 0.2,
                        ..Default::default()
                    }));
                state.assign_ids(&mut layer, true);
                state.root.push(Arc::new(layer));
            }
            let mut doc = Document::new(state);
            let cpu = Compositor::new(8 << 20);
            let mut resident = super::ResidentRenderer::with_budget(&gpu, 8 << 20).unwrap();
            for level in [0, 1, 2] {
                let mut program = super::Program::compile(&doc.state().root, 1).unwrap();
                let prepared = resident
                    .prepare_styles(
                        &doc,
                        level,
                        Rect::of_extent(extent.at_level(level)),
                        &mut program,
                    )
                    .unwrap();
                assert_eq!(
                    prepared.copies.len(),
                    4,
                    "case {case} L{level}: two distinct sources and planes"
                );
                let indices: Vec<_> = program.styles.iter().map(|(i, _)| *i).collect();
                assert_ne!(
                    program.steps[indices[0]].table, program.steps[indices[1]].table,
                    "case {case} L{level}"
                );
                resident.render(&doc, level).unwrap();
                let (_, actual) = resident.read_level(level, false).unwrap();
                let (_, expected) = cpu.render_level_rgba(&doc, level).unwrap();
                assert_eq!(actual.len(), expected.len());
                for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                    assert!(
                        (a - b).abs() <= 1e-4,
                        "case {case} L{level} sample {i}: {a} != {b}"
                    );
                }
            }
            // Reuse the same document/renderer across a real edit. Smart cases
            // change their child through the edit API so revision propagation is
            // exercised, rather than replacing state with an unchanged stamp.
            let target = &doc.state().root[1];
            let mask = Some(Mask::hide_all(extent, Depth::F32));
            let op = if let LayerKind::SmartObject(smart) = &target.kind {
                DocOp::EditSmartObject {
                    id: target.id,
                    op: Box::new(DocOp::SetMask {
                        id: smart.state.root[0].id,
                        mask,
                    }),
                }
            } else {
                DocOp::SetMask {
                    id: target.id,
                    mask,
                }
            };
            let evaluations = resident.style_evaluations();
            doc.apply(op).unwrap();
            assert!(resident.render(&doc, 0).unwrap().blocks > 0);
            assert!(resident.style_evaluations() > evaluations);
            let (_, actual) = resident.read_level(0, false).unwrap();
            let (_, expected) = cpu.render_level_rgba(&doc, 0).unwrap();
            assert_eq!(actual.len(), expected.len());
            for (i, (a, b)) in actual.iter().zip(expected).enumerate() {
                assert!(
                    (a - b).abs() <= 1e-4,
                    "edited case {case} sample {i}: {a} != {b}"
                );
            }
        }
    }

    #[test]
    fn oversized_effect_stack_is_rejected_before_creating_style_pipeline() {
        use crate::{gpu::GpuCompositor, render::styles::*, *};
        use engine_api::{EngineError, tile::Extent};
        use std::sync::Arc;

        // A 1 MiB binding limit makes 64 four-plane bevels exceed the aggregate
        // limit on a 16x16 image; the regression never allocates giant buffers.
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(descriptor);
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let mut limits = gpu_core::limits(&adapter.limits());
        limits.max_storage_buffer_binding_size = 1 << 20;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_features: adapter.features() & wgpu::Features::PASSTHROUGH_SHADERS,
            required_limits: limits,
            ..Default::default()
        }))
        .unwrap();
        let gpu = GpuCompositor::from_device(device, queue, adapter.get_info().name).unwrap();
        let mut state = DocState::new(Extent::new(16, 16), Depth::F32);
        let mut layer = Layer::new(
            "many bevels",
            LayerKind::Fill(Fill::Solid { color: [0.5; 3] }),
        );
        layer.props.styles.effects = vec![
            StyleEffect::Bevel(Bevel {
                kind: BevelKind::Emboss,
                size: 0.0,
                soften: 0.0,
                ..Default::default()
            });
            64
        ];
        state.root.push(Arc::new(layer));
        let doc = Document::new(state);
        let mut resident = super::ResidentRenderer::with_budget(&gpu, 1 << 20).unwrap();
        assert!(matches!(
            resident.render(&doc, 0),
            Err(EngineError::ResourceExhausted { .. })
        ));
        assert!(
            resident.styles.gpu.is_none(),
            "must reject before allocating any effect plane"
        );
        assert_eq!(resident.style_evaluations(), 0);
        assert_eq!(resident.style_cache_bytes(), 0);
    }

    #[test]
    fn pending_style_reservations_include_cumulative_data_and_metadata() {
        use super::{reserve_plane_words, reserve_words};

        // Existing metadata prefix, then the first style's source and plane.
        let mut words = 4;
        reserve_words(&mut words, 16, 96).unwrap();
        reserve_plane_words(&mut words, [16], 96).unwrap();
        assert_eq!(words, 16);

        // Each style fits alone, but their cumulative data does not.
        let mut cumulative = words;
        reserve_words(&mut cumulative, 32, 96).unwrap();
        assert!(reserve_plane_words(&mut cumulative, [16], 96).is_err());

        // The second style's data fits exactly; its metadata must also count.
        let mut metadata_overflow = words;
        reserve_words(&mut metadata_overflow, 16, 96).unwrap();
        assert!(reserve_plane_words(&mut metadata_overflow, [16], 96).is_err());

        // Include both planes' metadata at the exact binding-size boundary.
        reserve_words(&mut words, 16, 112).unwrap();
        reserve_plane_words(&mut words, [16], 112).unwrap();
        assert_eq!(words, 28);
    }

    #[test]
    fn auxiliary_size_overflow_is_rejected_without_allocating() {
        use super::{plane_bytes, reserve_plane_words, reserve_words};
        assert!(plane_bytes(engine_api::tile::Extent::new(u32::MAX, u32::MAX)).is_err());
        let mut overflowing_words = usize::MAX;
        assert!(reserve_words(&mut overflowing_words, 16, u64::MAX).is_err());
        assert!(reserve_words(&mut 0, u64::MAX - 3, u64::MAX).is_err());
        assert!(reserve_plane_words(&mut 0, [u64::MAX - 3], u64::MAX).is_err());
        let mut words = 0;
        assert!(reserve_plane_words(&mut words, [16, 16], 63).is_err());
    }

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
