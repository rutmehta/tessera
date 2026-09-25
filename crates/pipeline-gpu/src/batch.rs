use crate::{GpuContext, operator::parameters};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    stage::StageId,
    tile::{TILE_SIZE, Tile},
};
use image_core::{CpuStageOp, Op, StageOp};
use raw_decode::CfaLayout;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use wgpu::util::DeviceExt;

/// A GPU-resident per-pixel (vignette mask, grain value) map and its key.
pub(crate) type EffectsMap = (Vec<u32>, wgpu::Buffer);

/// Transient GPU memory retained between resident transactions.
const RECYCLE_BYTES: u64 = 1 << 30;

/// Retains up to [`RECYCLE_BYTES`] of idle transient buffers from an ended
/// transaction (most recently retired first).
pub(crate) fn recycle(recycled: &std::sync::Mutex<Vec<wgpu::Buffer>>, mut free: Vec<wgpu::Buffer>) {
    let mut recycled = recycled.lock().unwrap();
    free.append(&mut recycled);
    let mut bytes = 0;
    free.retain(|b| {
        bytes += b.size();
        bytes <= RECYCLE_BYTES
    });
    *recycled = free;
}

/// Transfer diagnostics, shared by clones of a backend (not by all contexts).
#[derive(Debug, Default, Clone, Copy)]
pub struct GpuStats {
    /// Point-stage dispatches (one per tile regardless of fused stage count).
    pub fused_dispatches: u64,
    pub uploads: u64,
    pub readbacks: u64,
    pub submissions: u64,
    /// Surface presentation reads only the histogram, never pixels.
    pub histogram_readbacks: u64,
    pub pixel_readback_bytes: u64,
    /// Unique resident payload allocations in the last batch (excludes parameters).
    pub last_resident_allocated_bytes: u64,
    pub last_resident_buffers: u64,
    /// Compute dispatches encoded in the last resident transaction.
    pub last_resident_dispatches: u64,
}
#[derive(Default)]
pub(crate) struct Counters {
    pub(crate) fused_dispatches: AtomicU64,
    pub(crate) uploads: AtomicU64,
    pub(crate) readbacks: AtomicU64,
    pub(crate) submissions: AtomicU64,
    pub(crate) histogram_readbacks: AtomicU64,
    pub(crate) pixel_readback_bytes: AtomicU64,
    pub(crate) last_resident_allocated_bytes: AtomicU64,
    pub(crate) last_resident_buffers: AtomicU64,
    pub(crate) last_resident_dispatches: AtomicU64,
}

/// Metal operators. X-Trans neighbourhood and unported M2 operators fall back
/// to CPU; remaining contiguous operations still run on GPU. See OPERATORS.md.
#[derive(Clone)]
pub struct GpuStageOp {
    context: Arc<GpuContext>,
    pub(crate) managed_output: Option<Arc<crate::GpuManagedOutput>>,
    pub(crate) counters: Arc<Counters>,
    pub(crate) resident_cache: Arc<std::sync::Mutex<crate::resident::Cache>>,
    pub(crate) resident_pipeline: wgpu::ComputePipeline,
    pub(crate) gather_pipeline: wgpu::ComputePipeline,
    pub(crate) surface_pipeline: wgpu::ComputePipeline,
    pub(crate) zero_pipeline: wgpu::ComputePipeline,
    pub(crate) histogram_pipeline: wgpu::ComputePipeline,
    pub(crate) fused_pipeline: wgpu::ComputePipeline,
    /// Builds cached per-pixel vignette/grain constants (compiled on use).
    pub(crate) effects_map_pipeline: Arc<std::sync::OnceLock<wgpu::ComputePipeline>>,
    /// The last effects constants map: (parameter key, buffer).
    pub(crate) effects_map: Arc<std::sync::Mutex<Option<EffectsMap>>>,
    pub(crate) detail_pipelines: Vec<wgpu::ComputePipeline>,
    /// Resident Texture/Clarity/Dehaze kernels, compiled on first use.
    pub(crate) local_tone:
        Arc<std::sync::OnceLock<Result<Arc<crate::resident::LocalTonePipelines>, String>>>,
    /// Placeholder binding for fused dispatches without an effects map.
    pub(crate) no_map: wgpu::Buffer,
    /// Transient buffers retired by completed resident transactions.
    pub(crate) recycled: Arc<std::sync::Mutex<Vec<wgpu::Buffer>>>,
    /// Exact Dehaze (airlight, confidence) keyed by the renderer's input identity.
    pub(crate) dehaze_stats: Arc<std::sync::Mutex<crate::resident::DehazeStatistics>>,
}
impl GpuStageOp {
    pub fn new(context: Arc<GpuContext>) -> Self {
        Self::with_cache_budget(context, 512 * 1024 * 1024)
    }
    pub fn with_cache_budget(context: Arc<GpuContext>, budget: usize) -> Self {
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident transfers"),
                source: wgpu::ShaderSource::Wgsl(include_str!("resident.wgsl").into()),
            });
        let resident_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("resident transfers"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident gather"),
                source: wgpu::ShaderSource::Wgsl(include_str!("gather.wgsl").into()),
            });
        let gather_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("resident gather"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("surface writer"),
                source: wgpu::ShaderSource::Wgsl(include_str!("surface.wgsl").into()),
            });
        let surface_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("surface writer"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("display histogram"),
                source: wgpu::ShaderSource::Wgsl(include_str!("histogram.wgsl").into()),
            });
        let histogram_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("display histogram"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        let module = context
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("resident zero fill"),
                source: wgpu::ShaderSource::Wgsl(include_str!("zero.wgsl").into()),
            });
        let zero_pipeline =
            context
                .device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("resident zero fill"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                });
        Self {
            fused_pipeline: crate::fused::pipeline(&context),
            detail_pipelines: crate::detail::pipelines(&context).expect("valid Detail pipelines"),
            local_tone: Arc::default(),
            no_map: context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("no effects map"),
                size: 8,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: false,
            }),
            effects_map_pipeline: Arc::default(),
            effects_map: Arc::default(),
            recycled: Arc::default(),
            dehaze_stats: Arc::default(),
            context,
            counters: Arc::default(),
            managed_output: None,
            resident_cache: crate::resident::cache(budget),
            resident_pipeline,
            gather_pipeline,
            surface_pipeline,
            histogram_pipeline,
            zero_pipeline,
        }
    }
    /// Diagnostic name of a resident pipeline (GPU profiling output).
    pub(crate) fn pipeline_name(&self, p: &wgpu::ComputePipeline, groups: [u32; 2]) -> String {
        let local = self.local_tone.get().and_then(|r| r.as_ref().ok());
        let name = if p == &self.fused_pipeline {
            "fused point chain"
        } else if p == &self.context.pipeline {
            "operator"
        } else if p == &self.resident_pipeline {
            "resident transfer"
        } else if p == &self.gather_pipeline {
            "gather"
        } else if p == &self.histogram_pipeline {
            "surface + histogram"
        } else if p == &self.surface_pipeline {
            "surface"
        } else if p == &self.zero_pipeline {
            "zero"
        } else if self.detail_pipelines.first() == Some(p) {
            "detail decompose"
        } else if self.detail_pipelines.get(1) == Some(p) {
            "detail filter"
        } else if self.effects_map_pipeline.get() == Some(p) {
            "effects map"
        } else {
            local.and_then(|l| l.name(p)).unwrap_or("other")
        };
        format!("{name} {}x{}", groups[0], groups[1])
    }
    pub fn cache_bytes(&self) -> usize {
        self.resident_cache.lock().unwrap().bytes()
    }
    pub fn clear_cache(&self) {
        self.recycled.lock().unwrap().clear();
        let mut cache = self.resident_cache.lock().unwrap();
        let budget = cache.budget();
        *cache = crate::resident::Cache::new(budget);
        self.dehaze_stats.lock().unwrap().clear();
        *self.effects_map.lock().unwrap() = None;
    }
    pub fn context(&self) -> &Arc<GpuContext> {
        &self.context
    }
    pub fn stats(&self) -> GpuStats {
        GpuStats {
            fused_dispatches: self.counters.fused_dispatches.load(Ordering::Relaxed),
            uploads: self.counters.uploads.load(Ordering::Relaxed),
            readbacks: self.counters.readbacks.load(Ordering::Relaxed),
            submissions: self.counters.submissions.load(Ordering::Relaxed),
            histogram_readbacks: self.counters.histogram_readbacks.load(Ordering::Relaxed),
            pixel_readback_bytes: self.counters.pixel_readback_bytes.load(Ordering::Relaxed),
            last_resident_allocated_bytes: self
                .counters
                .last_resident_allocated_bytes
                .load(Ordering::Relaxed),
            last_resident_buffers: self.counters.last_resident_buffers.load(Ordering::Relaxed),
            last_resident_dispatches: self
                .counters
                .last_resident_dispatches
                .load(Ordering::Relaxed),
        }
    }

    fn execute(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: &[Tile],
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        let ctx = &self.context;
        let ops: Vec<_> = chain.iter().map(|(_, op)| *op).collect();
        // Managed display must consume scene-linear output, never fused sRGB.
        let scene_ops =
            if self.managed_output.is_some() && matches!(ops.last(), Some(Op::Display { .. })) {
                &ops[..ops.len() - 1]
            } else {
                &ops[..]
            };
        let fused = crate::fused::supports(scene_ops);
        let pipeline = if fused {
            &self.fused_pipeline
        } else {
            &ctx.pipeline
        };
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        let mut pending = Vec::with_capacity(inputs.len());
        for input in inputs {
            cancel.check()?;
            if chain.iter().any(|(_, op)| {
                matches!(
                    op,
                    Op::ToneExtra(_) | Op::Color(_) | Op::Effects(..) | Op::EffectsInCrop(..)
                )
            }) && input.samples::<f32>()?.iter().any(|v| !v.is_finite())
            {
                return Err(EngineError::invalid(
                    "tone input",
                    "finite samples required",
                ));
            }
            let mut src = ctx
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("tile upload"),
                    contents: bytemuck::cast_slice(input.samples::<f32>()?),
                    usage: wgpu::BufferUsages::STORAGE,
                });
            let mut layout = input.layout();
            for (index, (_, op)) in chain.iter().enumerate() {
                if fused && index > 0 && index < scene_ops.len() {
                    continue;
                }
                cancel.check()?;
                if matches!(op, Op::Display { .. }) && index + 1 != chain.len() {
                    return Err(EngineError::invalid(
                        "GPU chain",
                        "display must be last (U8 output)",
                    ));
                }
                let input_layout = layout;
                let (p, next_layout) = if fused && index == 0 {
                    crate::fused::parameters(scene_ops, layout, input.coord())?
                } else {
                    parameters(op, layout, input.coord().pixel_origin(TILE_SIZE))?
                };
                layout = next_layout;
                let params = ctx
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: None,
                        contents: bytemuck::cast_slice(&p),
                        usage: wgpu::BufferUsages::STORAGE,
                    });
                let size = (layout.plane_len() * layout.channels as usize * 4) as u64;
                let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                if matches!(op, Op::Display { .. })
                    && let Some(output) = &self.managed_output
                {
                    let flags = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("managed gamut mask"),
                        size: layout.extent.area() * 4,
                        usage: wgpu::BufferUsages::STORAGE,
                        mapped_at_creation: false,
                    });
                    let group = output.bindings(&src, &dst, &flags, input_layout, true)?;
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&output.pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups((layout.plane_len() as u32).div_ceil(64), 1, 1);
                    drop(pass);
                    src = dst;
                    continue;
                }
                let mut entries: Vec<_> = [&src, &dst, &params]
                    .iter()
                    .enumerate()
                    .map(|(i, b)| wgpu::BindGroupEntry {
                        binding: i as u32,
                        resource: b.as_entire_binding(),
                    })
                    .collect();
                // Unused effects-map slot: tile batches evaluate constants inline.
                if fused {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 3,
                        resource: self.no_map.as_entire_binding(),
                    });
                }
                let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
                {
                    let mut pass = encoder.begin_compute_pass(&Default::default());
                    pass.set_pipeline(pipeline);
                    pass.set_bind_group(0, &group, &[]);
                    pass.dispatch_workgroups((layout.plane_len() as u32).div_ceil(64), 1, 1);
                }
                // wgpu's encoder retains the resources, even when host handles drop.
                src = dst;
            }
            let size = (layout.plane_len() * layout.channels as usize * 4) as u64;
            let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("chain readback"),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            encoder.copy_buffer_to_buffer(&src, 0, &staging, 0, size);
            pending.push((input.coord(), layout, staging));
        }
        cancel.check()?;
        ctx.queue.submit([encoder.finish()]);
        if fused {
            self.counters
                .fused_dispatches
                .fetch_add(inputs.len() as u64, Ordering::Relaxed);
        }
        self.counters.submissions.fetch_add(1, Ordering::Relaxed);
        self.counters
            .uploads
            .fetch_add(inputs.len() as u64, Ordering::Relaxed);
        let mut receivers = Vec::new();
        for (_, _, buffer) in &pending {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            receivers.push(rx);
        }
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        let mut output = Vec::with_capacity(inputs.len());
        for ((coord, layout, buffer), rx) in pending.into_iter().zip(receivers) {
            rx.recv().map_err(internal)?.map_err(internal)?;
            let data: Vec<f32> = {
                let mapped = buffer.slice(..).get_mapped_range().map_err(internal)?;
                bytemuck::cast_slice(&mapped).to_vec()
            };
            buffer.unmap();
            self.counters.readbacks.fetch_add(1, Ordering::Relaxed);
            cancel.check()?;
            let tile = if matches!(chain.last(), Some((_, Op::Display { .. }))) {
                Tile::from_samples(coord, layout, data.into_iter().map(|v| v as u8).collect())?
            } else {
                Tile::from_samples(coord, layout, data)?
            };
            output.push(tile);
        }
        Ok(output)
    }
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
fn cpu_fallback(op: &Op<'_>) -> bool {
    if let Op::ToneExtra(s) = op {
        return s.texture != 0.0 || s.clarity != 0.0 || s.dehaze != 0.0;
    }
    matches!(
        op,
        Op::Highlights {
            cfa: CfaLayout::XTrans(_),
            ..
        } | Op::Demosaic {
            cfa: CfaLayout::XTrans(_),
            ..
        } | Op::Geometry(_)
    )
}
impl StageOp for GpuStageOp {
    fn blend_local(
        &self,
        base: &pipeline_cpu::Image,
        adjusted: &pipeline_cpu::Image,
        mask: &[f32],
    ) -> EngineResult<pipeline_cpu::Image> {
        GpuStageOp::blend_local(self, base, adjusted, mask)
    }

    fn begin_resident(&self) -> Option<Box<dyn image_core::resident::ResidentBatch + '_>> {
        Some(Box::new(crate::resident::Batch::new(self)))
    }

    fn run_image(
        &self,
        stage: StageId,
        op: &Op<'_>,
        input: pipeline_cpu::Image,
        cancel: &CancellationToken,
    ) -> EngineResult<pipeline_cpu::Image> {
        cancel.check()?;
        if let Op::ToneExtra(s) = op
            && (s.texture != 0.0 || s.clarity != 0.0 || s.dehaze != 0.0)
        {
            let limits = self.context.device.limits();
            let bytes = u64::from(input.width()) * u64::from(input.height()) * 16;
            if bytes
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
                    .min(128 << 20)
            {
                return CpuStageOp.run_image(stage, op, input, cancel);
            }
            // Validate curves before any GPU work; local tone precedes curves.
            crate::curves::parameters(s, &mut vec![0.0; 33])?;
            let filtered = crate::tone_local::run(&self.context, &input, s)?;
            let transfers = if s.dehaze != 0.0 { 3 } else { 1 };
            self.counters.uploads.fetch_add(1, Ordering::Relaxed);
            self.counters
                .submissions
                .fetch_add(transfers, Ordering::Relaxed);
            self.counters
                .readbacks
                .fetch_add(transfers, Ordering::Relaxed);
            cancel.check()?;
            // Presence-only edits do not need a second upload/dispatch/map of
            // every pixel merely to evaluate the default identity curves.
            // Curve validation above is intentionally not bypassed.
            if s.curves == Default::default() {
                return Ok(filtered);
            }
            let curves = engine_api::recipe::settings::ToneSettings {
                texture: 0.0,
                clarity: 0.0,
                dehaze: 0.0,
                ..(*s).clone()
            };
            return self.run_image(stage, &Op::ToneExtra(&curves), filtered, cancel);
        }
        if let Op::Geometry(s) = op {
            // run_image receives host pixels. The renderer materializes resident
            // output at its M2 image barrier before reaching this fallback.
            if !crate::geometry::supports(s) {
                return CpuStageOp.run_image(stage, op, input, cancel);
            }
            let limits = self.context.device.limits();
            let bytes = u64::from(input.width())
                * u64::from(input.height())
                * input.planes().len() as u64
                * 4;
            if bytes
                > limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
            {
                return CpuStageOp.run_image(stage, op, input, cancel);
            }
            let output = crate::geometry::run(&self.context, &input, s)?;
            if s.crop.rect != engine_api::recipe::settings::NormalizedRect::FULL
                || s.crop.angle != 0.0
            {
                self.counters.uploads.fetch_add(1, Ordering::Relaxed);
                self.counters.submissions.fetch_add(1, Ordering::Relaxed);
                self.counters.readbacks.fetch_add(1, Ordering::Relaxed);
            }
            cancel.check()?;
            return Ok(output);
        }
        if cpu_fallback(op) {
            return CpuStageOp.run_image(stage, op, input, cancel);
        }
        cancel.check()?;
        let mut output = input.clone();
        let halo = match op {
            Op::Detail(s) => pipeline_cpu::detail_halo(s),
            _ => 0,
        };
        let coords: Vec<_> = input.coords().collect();
        for batch in coords.chunks(self.batch_size()) {
            let tiles = batch
                .iter()
                .map(|&coord| input.tile(coord, halo, 1))
                .collect::<EngineResult<Vec<_>>>()?;
            for tile in self.run_chain_batch(&[(stage, *op)], tiles, cancel)? {
                output.put(&tile)?;
            }
        }
        cancel.check()?;
        Ok(output)
    }
    fn run(&self, stage: StageId, op: &Op<'_>, input: Tile) -> EngineResult<Tile> {
        self.run_chain_batch(&[(stage, *op)], vec![input], &CancellationToken::new())?
            .pop()
            .ok_or_else(|| EngineError::internal("missing GPU result"))
    }
    fn batch_size(&self) -> usize {
        16
    }
    fn run_chain_batch(
        &self,
        chain: &[(StageId, Op<'_>)],
        mut inputs: Vec<Tile>,
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        cancel.check()?;
        if chain.is_empty() || inputs.is_empty() {
            return Ok(inputs);
        }
        let ops: Vec<_> = chain.iter().map(|(_, op)| *op).collect();
        if crate::fused::supports(&ops) {
            let mut output = Vec::with_capacity(inputs.len());
            for batch in inputs.chunks(self.batch_size()) {
                output.extend(self.execute(chain, batch, cancel)?);
            }
            return Ok(output);
        }
        // Split only at genuine CPU fallback boundaries, not between GPU stages.
        let mut start = 0;
        while start < chain.len() {
            if matches!(
                chain[start].1,
                Op::Detail(_) | Op::Effects(..) | Op::EffectsInCrop(..)
            ) {
                inputs = inputs
                    .into_iter()
                    .map(|input| {
                        cancel.check()?;
                        let output = match chain[start].1 {
                            Op::Detail(s) => crate::detail::run(&self.context, &input, s)?,
                            Op::Effects(s, extent) => crate::effects::run(
                                &self.context,
                                &input,
                                s,
                                extent,
                                &Default::default(),
                            )?,
                            Op::EffectsInCrop(s, extent, crop) => {
                                crate::effects::run(&self.context, &input, s, extent, crop)?
                            }
                            _ => unreachable!(),
                        };
                        self.counters.uploads.fetch_add(1, Ordering::Relaxed);
                        self.counters.submissions.fetch_add(1, Ordering::Relaxed);
                        self.counters.readbacks.fetch_add(1, Ordering::Relaxed);
                        cancel.check()?;
                        Ok(output)
                    })
                    .collect::<EngineResult<Vec<_>>>()?;
                start += 1;
            } else if cpu_fallback(&chain[start].1) {
                inputs = CpuStageOp.run_chain_batch(&chain[start..start + 1], inputs, cancel)?;
                start += 1;
            } else {
                let end = (start..chain.len())
                    .find(|&i| {
                        cpu_fallback(&chain[i].1)
                            || matches!(
                                chain[i].1,
                                Op::Detail(_) | Op::Effects(..) | Op::EffectsInCrop(..)
                            )
                    })
                    .unwrap_or(chain.len());
                let mut output = Vec::with_capacity(inputs.len());
                for batch in inputs.chunks(self.batch_size()) {
                    output.extend(self.execute(&chain[start..end], batch, cancel)?);
                }
                inputs = output;
                start = end;
            }
        }
        Ok(inputs)
    }
}
