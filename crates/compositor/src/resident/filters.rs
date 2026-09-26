//! Native-resolution stack evaluation. Cache values are straight RGBA except
//! final mip keys (premultiplied). Prefix hashes preserve earlier stages on edits.
use super::ResidentRenderer;
use crate::document::{Layer, LayerKind};
use crate::render::smart_filters::ResidentFilterEvaluator;
use crate::{Compositor, Document};
use engine_api::{EngineError, EngineResult, tile::Extent};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;

type Key = [u8; 32];
struct Entry {
    pixels: wgpu::Buffer,
    used: u64,
}

pub(super) struct StackRuntime {
    pub evaluator: Option<Arc<dyn ResidentFilterEvaluator>>,
    pub cpu: Compositor,
    cache: HashMap<Key, Entry>,
    pipe: Option<wgpu::ComputePipeline>,
    pub evaluations: u64,
    pub fallbacks: u64,
    clock: u64,
}
impl StackRuntime {
    pub fn new(budget: u64) -> Self {
        Self {
            evaluator: None,
            cpu: Compositor::new(budget as usize),
            cache: HashMap::new(),
            pipe: None,
            evaluations: 0,
            fallbacks: 0,
            clock: 0,
        }
    }
    pub fn bytes(&self) -> u64 {
        self.cache.values().map(|e| e.pixels.size()).sum()
    }
    pub fn clear(&mut self) {
        self.cache.clear();
        self.cpu.clear();
    }
    fn get(&mut self, key: Key) -> Option<wgpu::Buffer> {
        self.clock += 1;
        let entry = self.cache.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.pixels.clone())
    }
    fn insert(&mut self, key: Key, pixels: &wgpu::Buffer, budget: u64) {
        if pixels.size() > budget {
            return;
        }
        self.cache.remove(&key);
        while self.bytes().saturating_add(pixels.size()) > budget {
            let oldest = self
                .cache
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| *k);
            if let Some(k) = oldest {
                self.cache.remove(&k);
            } else {
                break;
            }
        }
        self.clock += 1;
        self.cache.insert(
            key,
            Entry {
                pixels: pixels.clone(),
                used: self.clock,
            },
        );
    }
}
fn hash(previous: Key, bytes: &[u8]) -> Key {
    let mut h = blake3::Hasher::new();
    h.update(&previous);
    h.update(bytes);
    *h.finalize().as_bytes()
}
impl ResidentRenderer {
    /// Install shared-device filters, including their CPU fallback evaluator.
    /// Child renderers inherit this adapter. Replacing it discards stack results.
    pub fn set_filter_evaluator(
        &mut self,
        evaluator: Arc<dyn ResidentFilterEvaluator>,
    ) -> EngineResult<()> {
        self.stack.cpu.set_filter_evaluator(evaluator.clone());
        self.stack.evaluator = Some(evaluator);
        self.stack.clear();
        self.children.clear();
        self.smarts.clear();
        self.layers.clear();
        self.invalidate();
        Ok(())
    }
    /// GPU stages executed (cache hits do not increment this counter).
    pub fn filter_evaluations(&self) -> u64 {
        self.stack.evaluations
    }
    /// Layer-local CPU stack evaluations, not whole-document fallbacks.
    pub fn filter_fallbacks(&self) -> u64 {
        self.stack.fallbacks
    }
    /// Retained per-stage/mip buffers, bounded by the renderer's budget.
    pub fn filter_cache_bytes(&self) -> u64 {
        self.stack.bytes()
    }

    fn stack_buffer(&self, e: Extent) -> EngineResult<wgpu::Buffer> {
        let size = e
            .area()
            .checked_mul(16)
            .ok_or_else(|| EngineError::invalid("stack", "size overflow"))?;
        if size == 0 || size > self.device.limits().max_storage_buffer_binding_size {
            return Err(EngineError::ResourceExhausted {
                resource: "stack image exceeds device binding limit".into(),
            });
        }
        Ok(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident filter stage"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        }))
    }
    fn stack_op(
        &mut self,
        input: &wgpu::Buffer,
        next: &wgpu::Buffer,
        data: [u32; 8],
    ) -> EngineResult<wgpu::Buffer> {
        self.stack_op_mask(input, next, input, data)
    }
    fn stack_op_mask(
        &mut self,
        input: &wgpu::Buffer,
        next: &wgpu::Buffer,
        mask: &wgpu::Buffer,
        data: [u32; 8],
    ) -> EngineResult<wgpu::Buffer> {
        if self.stack.pipe.is_none() {
            let entries = (0..5)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    count: None,
                    ty: wgpu::BindingType::Buffer {
                        ty: if binding == 3 {
                            wgpu::BufferBindingType::Uniform
                        } else {
                            wgpu::BufferBindingType::Storage {
                                read_only: binding != 2,
                            }
                        },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                })
                .collect::<Vec<_>>();
            let source = format!(
                "{}\n{}",
                include_str!("../blend.wgsl"),
                include_str!("filters.wgsl")
            );
            self.stack.pipe = Some(
                gpu_core::precise_compute_pipeline(
                    &self.device,
                    "smart filter plumbing",
                    &source,
                    "main",
                    (8, 8, 1),
                    &entries,
                )?
                .pipeline,
            );
        }
        let out = self.stack_buffer(Extent::new(data[5], data[6]))?;
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stack parameters"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let pipe = self.stack.pipe.as_ref().unwrap();
        let entries = [input, next, &out, &params, mask]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stack"),
            layout: &pipe.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(pipe);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(data[5].div_ceil(8), data[6].div_ceil(8), 1);
        }
        self.queue.submit([encoder.finish()]);
        Ok(out)
    }
    fn convert(&mut self, input: &wgpu::Buffer, e: Extent, op: u32) -> EngineResult<wgpu::Buffer> {
        self.stack_op(
            input,
            input,
            [op, e.width, e.height, 0, 0, e.width, e.height, 0],
        )
    }
    pub(super) fn filtered_buffer(
        &mut self,
        layer: &Layer,
        level: u8,
    ) -> EngineResult<wgpu::Buffer> {
        let LayerKind::SmartObject(so) = &layer.kind else {
            unreachable!()
        };
        let e = so.state.canvas;
        let mut key = hash([0; 32], &so.key.to_le_bytes());
        key = hash(key, &so.state.rev.to_le_bytes());
        key = hash(key, &e.width.to_le_bytes());
        key = hash(key, &e.height.to_le_bytes());
        let base_key = key;
        let mut stage_keys = Vec::new();
        let mut supported = !so.state.has_layer_styles();
        for filter in so.filters.iter().filter(|f| f.enabled) {
            if !filter.blend.opacity.is_finite() || !(0.0..=1.0).contains(&filter.blend.opacity) {
                return Err(EngineError::invalid(
                    "filter opacity",
                    "expected finite [0,1]",
                ));
            }
            key = hash(
                key,
                &serde_json::to_vec(filter)
                    .map_err(|e| EngineError::invalid("filter", e.to_string()))?,
            );
            stage_keys.push(key);
            if filter.blend.opacity == 0.0 {
                continue;
            }
            if let Some(op) = filter.transform_op()? {
                supported &= !matches!(op.operation, transform::Operation::ContentAwareScale(_));
            } else if let Some(adapter) = &self.stack.evaluator {
                supported &= adapter.supports(filter)?;
            } else {
                supported &= filter.name == "invert";
            }
        }
        // Content revision captures shared-mask changes; stage prefixes exclude it.
        let result_key = hash(
            hash(key, &layer.id.0.to_le_bytes()),
            &layer.content_rev.to_le_bytes(),
        );
        let final_key = hash(result_key, &[level, 255]);
        if let Some(result) = self.stack.get(final_key) {
            return Ok(result);
        }
        if !supported {
            // The CPU compositor reconstructs nested smart sources bilinearly.
            // Do not silently change an explicitly requested Lanczos render.
            if self.smart_quality == super::SmartQuality::Lanczos3 {
                return Err(EngineError::Unsupported {
                    what: "CPU-only smart-filter stacks require bilinear smart quality".into(),
                });
            }
            let filtered = self.stack.cpu.filtered_source(so)?;
            let doc = Document::new(filtered.map_or_else(|| (*so.state).clone(), |f| f.state));
            let (extent, pixels) = self.stack.cpu.render_level_rgba(&doc, level)?;
            let straight = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("CPU-only layer stack"),
                    contents: bytemuck::cast_slice(&pixels),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let output = self.convert(&straight, extent, 1)?;
            self.stack.insert(final_key, &output, self.budget);
            self.stack.fallbacks += 1;
            return Ok(output);
        }
        let mut current = if let Some(input) = self.stack.get(base_key) {
            input
        } else {
            if self
                .children
                .get(&layer.id)
                .is_none_or(|(state, _)| !Arc::ptr_eq(state, &so.state))
            {
                let mut child = Self::with_budget(&self.gpu, self.budget)?;
                child.set_smart_quality(self.smart_quality)?;
                if let Some(adapter) = self.stack.evaluator.clone() {
                    child.set_filter_evaluator(adapter)?;
                }
                self.children.insert(layer.id, (so.state.clone(), child));
            }
            let child = &mut self.children.get_mut(&layer.id).unwrap().1;
            child.render(&Document::new((*so.state).clone()), 0)?;
            let input = child.levels[&0].out.clone();
            let straight = self.convert(&input, e, 0)?;
            self.stack.insert(base_key, &straight, self.budget);
            straight
        };
        let original = current.clone();
        let fused = super::fusion::fuse(so)?;
        let stages: Vec<_> = if let Some(filter) = &fused {
            vec![(filter, *stage_keys.last().unwrap())]
        } else {
            so.filters
                .iter()
                .filter(|f| f.enabled)
                .zip(stage_keys)
                .collect()
        };
        for (filter, key) in stages {
            if let Some(cached) = self.stack.get(key) {
                current = cached;
                continue;
            }
            if filter.blend.opacity == 0.0 {
                self.stack.insert(key, &current, self.budget);
                continue;
            }
            let next = if let Some(op) = filter.transform_op()? {
                let plan = self.prepare_transform(&op, e, e, 0)?;
                let input = self.convert(&current, e, 1)?;
                let output = self.stack_buffer(e)?;
                let mut encoder = self.device.create_command_encoder(&Default::default());
                self.encode_transform_buffers(&mut encoder, &input, &output, &plan, None)?;
                self.queue.submit([encoder.finish()]);
                self.convert(&output, e, 5)?
            } else if let Some(adapter) = &self.stack.evaluator {
                adapter.evaluate_resident(&self.device, &self.queue, &current, e, filter)?
            } else {
                self.convert(&current, e, 2)?
            };
            current = self.stack_op(
                &current,
                &next,
                [
                    3,
                    e.width,
                    e.height,
                    filter.blend.mode as u32,
                    filter.blend.opacity.to_bits(),
                    e.width,
                    e.height,
                    0,
                ],
            )?;
            self.stack.evaluations += 1;
            self.stack.insert(key, &current, self.budget);
        }
        if let Some(mask) = so.filter_mask.as_ref().filter(|m| m.enabled) {
            if mask.raster.extent() != e
                || mask.raster.channels() != 1
                || !mask.density.is_finite()
                || !(0.0..=1.0).contains(&mask.density)
            {
                return Err(EngineError::invalid(
                    "filter mask",
                    "same-size one-channel mask and density [0,1] required",
                ));
            }
            if mask.feather != 0.0 {
                return Err(EngineError::Unsupported {
                    what: "smart-filter mask feather".into(),
                });
            }
            let pixels: Vec<f32> = (0..e.height)
                .flat_map(|y| (0..e.width).map(move |x| mask.raster.pixel(x, y)[0]))
                .collect();
            if pixels
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(EngineError::invalid(
                    "filter mask",
                    "samples must be finite [0,1]",
                ));
            }
            let buffer = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("filter mask"),
                    contents: bytemuck::cast_slice(&pixels),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            current = self.stack_op_mask(
                &original,
                &current,
                &buffer,
                [
                    6,
                    e.width,
                    e.height,
                    0,
                    mask.density.to_bits(),
                    e.width,
                    e.height,
                    0,
                ],
            )?;
        }
        let mut extent = e;
        for _ in 0..level {
            let next = extent.at_level(1);
            current = self.stack_op(
                &current,
                &current,
                [
                    4,
                    extent.width,
                    extent.height,
                    0,
                    0,
                    next.width,
                    next.height,
                    0,
                ],
            )?;
            extent = next;
        }
        let output = self.convert(&current, extent, 1)?;
        self.stack.insert(final_key, &output, self.budget);
        Ok(output)
    }
}
