use crate::{GpuStageOp, operator::parameters};
#[path = "resident_band.rs"]
mod band;
#[path = "resident_cfa.rs"]
mod cfa;
#[path = "export_resize.rs"]
pub(crate) mod export_resize;
#[path = "lens.rs"]
mod lens;

use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    stage::{MemoKey, StageId},
    tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout},
};
use image_core::{
    Op,
    resident::{ResidentBatch, ResidentOutput, ResidentTile, SurfaceTarget},
};
use std::{
    collections::{BTreeMap, HashMap},
    sync::{Arc, Mutex, atomic::Ordering},
};

/// Export waits this long after the last interactive job before resuming
/// (a drag's next event follows within a display frame or two).
pub const EXPORT_QUIET: std::time::Duration = std::time::Duration::from_millis(50);
/// Longest single yield: export still advances under a never-idle viewport.
pub const EXPORT_MAX_YIELD: std::time::Duration = std::time::Duration::from_millis(1000);

struct Storage {
    buffer: wgpu::Buffer,
    packed: bool,
    device: wgpu::Device,
    pool: std::sync::Weak<Mutex<Pool>>,
}
// Recycling is batch-local. Command order guarantees all encoded consumers
// precede the next write; no reuse crosses a submission or a live handle.
impl Drop for Storage {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.upgrade() {
            pool.lock().unwrap().free.push(self.buffer.clone());
        }
    }
}
#[derive(Default)]
struct Pool {
    free: Vec<wgpu::Buffer>,
    allocated_bytes: u64,
    allocations: u64,
}

struct Entry {
    tile: ResidentTile,
    tick: u64,
    bytes: usize,
}
/// Budget counts GPU payloads owned by this cache, excluding in-flight
/// command resources and handles retained by render transactions. Computed
/// stage outputs are packed f16. Decode sources and the extra output-demosaic
/// checkpoint retain f32 precision: rounding CFA samples changes highlight
/// decisions, and rounding the extra checkpoint compounds error before WB.
pub(crate) struct Cache {
    budget: usize,
    bytes: usize,
    tick: u64,
    entries: HashMap<MemoKey, Entry>,
    order: BTreeMap<u64, MemoKey>,
}
impl Cache {
    pub fn new(budget: usize) -> Self {
        Self {
            budget,
            bytes: 0,
            tick: 0,
            entries: HashMap::new(),
            order: BTreeMap::new(),
        }
    }
    pub fn budget(&self) -> usize {
        self.budget
    }
    pub fn bytes(&self) -> usize {
        self.bytes
    }
    fn get(&mut self, key: &MemoKey) -> Option<ResidentTile> {
        let e = self.entries.get_mut(key)?;
        self.order.remove(&e.tick);
        self.tick += 1;
        e.tick = self.tick;
        self.order.insert(e.tick, *key);
        Some(e.tile.clone())
    }
    fn insert(&mut self, key: MemoKey, tile: ResidentTile) {
        if let Some(e) = self.entries.remove(&key) {
            self.bytes -= e.bytes;
            self.order.remove(&e.tick);
        }
        let bytes = cache_payload_bytes(&key, &tile);
        if bytes > self.budget {
            return;
        }
        while self.bytes + bytes > self.budget {
            let (_, k) = self.order.pop_first().unwrap();
            self.bytes -= self.entries.remove(&k).unwrap().bytes;
        }
        self.tick += 1;
        self.bytes += bytes;
        self.order.insert(self.tick, key);
        self.entries.insert(
            key,
            Entry {
                tile,
                tick: self.tick,
                bytes,
            },
        );
    }
}

// Flush only host transfers if encoding is abandoned. wgpu otherwise retains
// their staging memory until some future submit, which cancellation may postpone
// indefinitely. The abandoned compute encoder is never submitted.
struct PendingUploads {
    queue: wgpu::Queue,
    dirty: std::cell::Cell<bool>,
}
impl Drop for PendingUploads {
    fn drop(&mut self) {
        if self.dirty.get() {
            self.queue.submit(std::iter::empty());
        }
    }
}

// Bind groups retain buffers/views without retaining ResidentTile handles. This
// preserves the planner's last-use recycling while keeping resources alive for
// replay. Dispatch order is also the scratch-buffer reuse dependency order.
struct Dispatch {
    pipeline: wgpu::ComputePipeline,
    group: wgpu::BindGroup,
    workgroups: [u32; 2],
}

#[path = "resident_metrics.rs"]
mod metrics;
#[path = "resident_tone.rs"]
mod tone;
pub(crate) use tone::{Pipelines as LocalTonePipelines, Statistics as DehazeStatistics};

pub(crate) struct Batch<'a> {
    metrics_only: bool,
    gpu: &'a GpuStageOp,
    encoder: wgpu::CommandEncoder,
    pending: HashMap<MemoKey, (u64, ResidentTile)>,
    access_tick: u64,
    pool: Arc<Mutex<Pool>>,
    dispatches: u64,
    commands: Vec<Dispatch>,
    uploads: PendingUploads,
    /// Returns idle buffers to the backend when the transaction ends.
    _recycler: Recycler,
    /// Diagnostic timestamp readbacks (see `profile_dispatches`).
    profile: Vec<(wgpu::Buffer, Vec<String>)>,
    /// Placeholder for the fused kernel's effects-map binding.
    no_map: wgpu::Buffer,
    /// A constants map built by this transaction, published on completion.
    pending_map: Option<crate::batch::EffectsMap>,
    /// Mapped-at-creation parameter arena for [`Batch::dispatch_with`]: one
    /// allocation per thousands of dispatches instead of a buffer (and a
    /// staging copy) per dispatch. Unmapped before every submission.
    params: Option<(wgpu::Buffer, u64)>,
}
/// Parameter arena size; blocks are offset-aligned.
const PARAM_ARENA: u64 = 1 << 20;
// Every submitted command of a transaction has completed (finish/read_now
// wait) or was never submitted, so its free buffers are idle when the
// transaction ends: keep them for later transactions, including after
// capability probes and cancellation.
struct Recycler {
    pool: Arc<Mutex<Pool>>,
    recycled: Arc<Mutex<Vec<wgpu::Buffer>>>,
    /// Export transactions retain at most their scratch share.
    cap: u64,
}
impl Drop for Recycler {
    fn drop(&mut self) {
        let free = std::mem::take(&mut self.pool.lock().unwrap().free);
        if !free.is_empty() {
            crate::batch::recycle(&self.recycled, free, self.cap);
        }
    }
}

impl<'a> Batch<'a> {
    pub fn new(gpu: &'a GpuStageOp) -> Self {
        // Buffers retired by completed transactions: no submitted work uses
        // them any more, and reuse skips allocation and wgpu's zero fill.
        let free = std::mem::take(&mut *gpu.recycled.lock().unwrap());
        let pool = Arc::new(Mutex::new(Pool {
            free,
            ..Default::default()
        }));
        Self {
            _recycler: Recycler {
                // Scratch is retired after the final reduction as for pixels.
                pool: pool.clone(),
                recycled: gpu.recycled.clone(),
                cap: if gpu.export_float {
                    gpu.export_scratch
                } else {
                    u64::MAX
                },
            },
            metrics_only: false,
            gpu,
            encoder: gpu
                .context()
                .device
                .create_command_encoder(&Default::default()),
            pending: HashMap::new(),
            access_tick: 0,
            pool,
            dispatches: 0,
            commands: Vec::new(),
            uploads: PendingUploads {
                queue: gpu.context().queue.clone(),
                dirty: std::cell::Cell::new(false),
            },
            no_map: gpu.no_map.clone(),
            pending_map: None,
            params: None,
            profile: Vec::new(),
        }
    }
    // Queue writes share wgpu's pending transfer encoder, avoiding a separate
    // mapped-at-creation transfer encoder for every parameter upload.
    // Always allocate fresh: queue writes execute before the compute encoder,
    // so uploading into a recycled compute buffer would overwrite earlier data.
    fn host_buffer(
        &self,
        label: Option<&str>,
        contents: &[u8],
        usage: wgpu::BufferUsages,
    ) -> wgpu::Buffer {
        let ctx = self.gpu.context();
        let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label,
            size: contents.len() as u64,
            usage: usage | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        ctx.queue.write_buffer(&buffer, 0, contents);
        self.uploads.dirty.set(true);
        buffer
    }
    fn buffer(&self, bytes: usize) -> EngineResult<wgpu::Buffer> {
        let device = &self.gpu.context().device;
        if bytes == 0 || bytes as u64 > device.limits().max_storage_buffer_binding_size {
            return Err(EngineError::invalid(
                "resident buffer",
                "size exceeds device storage limit",
            ));
        }
        let mut pool = self.pool.lock().unwrap();
        if let Some(i) = pool.free.iter().position(|b| b.size() == bytes as u64) {
            return Ok(pool.free.swap_remove(i));
        }
        if self.gpu.export_float
            && pool.allocated_bytes.saturating_add(bytes as u64) > self.gpu.export_scratch
        {
            return Err(EngineError::Unsupported {
                what: "export GPU scratch exceeds its budget".into(),
            });
        }
        pool.allocated_bytes += bytes as u64;
        pool.allocations += 1;
        Ok(device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident tile"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }))
    }
    fn tile(&self, coord: TileCoord, layout: TileLayout, buffer: wgpu::Buffer) -> ResidentTile {
        ResidentTile {
            coord,
            layout,
            storage: Arc::new(Storage {
                buffer,
                packed: false,
                device: self.gpu.context().device.clone(),
                pool: Arc::downgrade(&self.pool),
            }),
        }
    }
    fn storage<'b>(&self, tile: &'b ResidentTile) -> EngineResult<&'b wgpu::Buffer> {
        let storage = tile
            .storage
            .downcast_ref::<Storage>()
            .ok_or_else(|| EngineError::invalid("resident tile", "foreign backend"))?;
        if storage.device != self.gpu.context().device {
            return Err(EngineError::invalid("resident tile", "foreign device"));
        }
        Ok(&storage.buffer)
    }
    fn dispatch(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        params: &[u8],
        count: u32,
    ) {
        self.dispatch_with(pipeline, src, dst, params, count, None);
    }
    /// [`Batch::dispatch`] with an optional fourth read-only binding.
    fn dispatch_with(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        params: &[u8],
        count: u32,
        extra: Option<&wgpu::Buffer>,
    ) {
        let ctx = self.gpu.context();
        let (p, offset, size) = self.param_block(params);
        let mut entries: Vec<_> = [src, dst]
            .into_iter()
            .map(|b| b.as_entire_binding())
            .collect();
        entries.push(wgpu::BindingResource::Buffer(wgpu::BufferBinding {
            buffer: &p,
            offset,
            size: std::num::NonZeroU64::new(size),
        }));
        entries.extend(extra.map(|b| b.as_entire_binding()));
        let entries: Vec<_> = entries
            .into_iter()
            .enumerate()
            .map(|(i, resource)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource,
            })
            .collect();
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        self.record(pipeline, group, count.div_ceil(64));
    }
    fn record(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        group: wgpu::BindGroup,
        workgroups: u32,
    ) {
        // Linear kernels fold rows of at most 65535 workgroups back into one
        // index (x + y * rows * 64), so whole levels exceed the 1D limit.
        const ROW: u32 = 65535;
        let grid = if workgroups > ROW {
            [ROW, workgroups.div_ceil(ROW)]
        } else {
            [workgroups, 1]
        };
        self.record_2d(pipeline, group, grid);
    }
    fn record_2d(
        &mut self,
        pipeline: &wgpu::ComputePipeline,
        group: wgpu::BindGroup,
        workgroups: [u32; 2],
    ) {
        self.dispatches += 1;
        self.commands.push(Dispatch {
            pipeline: pipeline.clone(),
            group,
            workgroups,
        });
    }
    /// The per-image effects constants map for the fused block in `p`
    /// (vignette mask, grain value per pixel), built once per parameter key
    /// and kept GPU-resident; sets the block's map flag when bound. Values
    /// are produced by the same WGSL functions as the inline path.
    fn effects_map(&mut self, p: &mut [f32]) -> Option<wgpu::Buffer> {
        let base = p[36] as usize;
        if base == 0 || p[base + 26] != 0.0 {
            return None;
        }
        let key: Vec<u32> = EFFECTS_MAP_KEY
            .iter()
            .map(|&k| p[base + k].to_bits())
            .collect();
        let cached = self
            .pending_map
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, b)| b.clone())
            .or_else(|| {
                self.gpu
                    .effects_map
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|(k, _)| *k == key)
                    .map(|(_, b)| b.clone())
            });
        let map = match cached {
            Some(map) => map,
            None => {
                let (w, h) = (p[base + 5].to_bits(), p[base + 6].to_bits());
                let bytes = u64::from(w) * u64::from(h) * 8;
                let limits = self.gpu.context().device.limits();
                if bytes == 0
                    || bytes
                        > limits
                            .max_storage_buffer_binding_size
                            .min(limits.max_buffer_size)
                    || w.div_ceil(16) > limits.max_compute_workgroups_per_dimension
                    || h.div_ceil(16) > limits.max_compute_workgroups_per_dimension
                {
                    return None;
                }
                let ctx = self.gpu.context();
                let pipeline = self
                    .gpu
                    .effects_map_pipeline
                    .get_or_init(|| effects_map_pipeline(ctx))
                    .clone();
                let map = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("effects constants map"),
                    size: bytes,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                });
                let params = self.host_buffer(
                    Some("effects map parameters"),
                    bytemuck::cast_slice(&p[base..base + 28]),
                    wgpu::BufferUsages::STORAGE,
                );
                let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("effects constants map"),
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: params.as_entire_binding(),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: map.as_entire_binding(),
                        },
                    ],
                });
                self.record_2d(&pipeline, group, [w.div_ceil(16), h.div_ceil(16)]);
                self.pending_map = Some((key, map.clone()));
                map
            }
        };
        p[base + 27] = 1.0;
        Some(map)
    }
    /// Submits everything encoded so far and reads `buffers` back. Only global
    /// reductions use this (exact Dehaze statistics on a cache miss). The GPU
    /// is idle afterwards, so pooled buffers may be recycled across it.
    fn read_now(&mut self, buffers: &[&wgpu::Buffer]) -> EngineResult<Vec<Vec<u8>>> {
        self.encode_compute();
        let ctx = self.gpu.context();
        let staging: Vec<_> = buffers
            .iter()
            .map(|b| {
                let s = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resident statistics readback"),
                    size: b.size(),
                    usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                self.encoder.copy_buffer_to_buffer(b, 0, &s, 0, b.size());
                s
            })
            .collect();
        let encoder = std::mem::replace(
            &mut self.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        if let Some(lost) = ctx.device_failure() {
            return Err(EngineError::internal(lost));
        }
        ctx.queue.submit([encoder.finish()]);
        self.uploads.dirty.set(false);
        self.gpu
            .counters
            .submissions
            .fetch_add(1, Ordering::Relaxed);
        let receivers: Vec<_> = staging
            .iter()
            .map(|b| {
                let (tx, rx) = std::sync::mpsc::channel();
                b.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    let _ = tx.send(r);
                });
                rx
            })
            .collect();
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| EngineError::internal(e.to_string()))?;
        let mut out = Vec::with_capacity(staging.len());
        for (b, rx) in staging.iter().zip(receivers) {
            rx.recv()
                .map_err(|e| EngineError::internal(e.to_string()))?
                .map_err(|e| EngineError::internal(e.to_string()))?;
            out.push(
                b.slice(..)
                    .get_mapped_range()
                    .map_err(|e| EngineError::internal(e.to_string()))?
                    .to_vec(),
            );
            b.unmap();
        }
        self.gpu.counters.readbacks.fetch_add(1, Ordering::Relaxed);
        Ok(out)
    }
    fn clear(&mut self, buffer: &wgpu::Buffer) {
        let pipeline = &self.gpu.zero_pipeline;
        let group = self
            .gpu
            .context()
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("ordered resident zero fill"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
        self.record(pipeline, group, (buffer.size() / 4).div_ceil(64) as u32);
    }
    /// A parameter block in the current arena: (buffer, offset, size).
    fn param_block(&mut self, bytes: &[u8]) -> (wgpu::Buffer, u64, u64) {
        let device = &self.gpu.context().device;
        let size = (bytes.len() as u64).max(4).next_multiple_of(4);
        if size > PARAM_ARENA / 16 {
            let buffer = self.host_buffer(
                Some("resident parameters"),
                bytes,
                wgpu::BufferUsages::STORAGE,
            );
            return (buffer, 0, size);
        }
        let align = u64::from(device.limits().min_storage_buffer_offset_alignment).max(4);
        if self
            .params
            .as_ref()
            .is_none_or(|(_, used)| used + size > PARAM_ARENA)
        {
            self.seal_params();
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("resident parameter arena"),
                size: PARAM_ARENA,
                usage: wgpu::BufferUsages::STORAGE,
                mapped_at_creation: true,
            });
            self.params = Some((buffer, 0));
        }
        let (buffer, used) = self.params.as_mut().expect("parameter arena");
        let offset = *used;
        match buffer.slice(offset..offset + size).get_mapped_range_mut() {
            Ok(mut view) => {
                if bytes.len() as u64 == size {
                    view.copy_from_slice(bytes);
                } else {
                    let mut padded = bytes.to_vec();
                    padded.resize(size as usize, 0);
                    view.copy_from_slice(&padded);
                }
            }
            Err(_) => {
                let buffer = self.host_buffer(
                    Some("resident parameters"),
                    bytes,
                    wgpu::BufferUsages::STORAGE,
                );
                return (buffer, 0, size);
            }
        }
        *used = (offset + size).next_multiple_of(align);
        (buffer.clone(), offset, size)
    }
    /// Unmaps the parameter arena: required before any submission using it.
    fn seal_params(&mut self) {
        if let Some((buffer, _)) = self.params.take() {
            buffer.unmap();
        }
    }
    fn encode_compute(&mut self) {
        self.seal_params();
        if self.commands.is_empty() {
            return;
        }
        // wgpu-core opens a Metal command buffer per COMPUTE PASS, not per
        // CommandEncoder. One pass avoids exhausting Metal's 4096-buffer cap.
        // wgpu tracks storage hazards between dispatches, including pooled
        // buffers reused for a different role later in this ordered stream.
        if self.profile_dispatches() {
            return;
        }
        let mut pass = self.encoder.begin_compute_pass(&Default::default());
        for command in &self.commands {
            pass.set_pipeline(&command.pipeline);
            pass.set_bind_group(0, &command.group, &[]);
            pass.dispatch_workgroups(command.workgroups[0], command.workgroups[1], 1);
        }
        drop(pass);
        self.commands.clear();
    }
    /// Diagnostic (`TESSERA_GPU_PROFILE=1`, timestamp-capable adapters): one
    /// timestamped pass per dispatch, printed after completion. Not for
    /// production frames: separate passes change scheduling.
    fn profile_dispatches(&mut self) -> bool {
        let ctx = self.gpu.context();
        let n = self.commands.len() as u32;
        if std::env::var_os("TESSERA_GPU_PROFILE").is_none()
            || !ctx.capabilities.timestamp_query
            || n > 2048
        {
            return false;
        }
        let set = ctx.device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("dispatch profile"),
            ty: wgpu::QueryType::Timestamp,
            count: 2 * n,
        });
        for (i, command) in self.commands.iter().enumerate() {
            let mut pass = self
                .encoder
                .begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: Some(wgpu::ComputePassTimestampWrites {
                        query_set: &set,
                        beginning_of_pass_write_index: Some(2 * i as u32),
                        end_of_pass_write_index: Some(2 * i as u32 + 1),
                    }),
                });
            pass.set_pipeline(&command.pipeline);
            pass.set_bind_group(0, &command.group, &[]);
            pass.dispatch_workgroups(command.workgroups[0], command.workgroups[1], 1);
        }
        let bytes = u64::from(2 * n) * 8;
        let resolved = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dispatch profile resolve"),
            size: bytes,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("dispatch profile readback"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.encoder.resolve_query_set(&set, 0..2 * n, &resolved, 0);
        self.encoder
            .copy_buffer_to_buffer(&resolved, 0, &staging, 0, bytes);
        let labels = self
            .commands
            .drain(..)
            .map(|c| self.gpu.pipeline_name(&c.pipeline, c.workgroups))
            .collect();
        self.profile.push((staging, labels));
        true
    }
    fn print_profile(gpu: &GpuStageOp, profile: Vec<(wgpu::Buffer, Vec<String>)>) {
        let ctx = gpu.context();
        let period = f64::from(ctx.queue.get_timestamp_period());
        for (staging, labels) in profile {
            let (tx, rx) = std::sync::mpsc::channel();
            staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            let _ = ctx.device.poll(wgpu::PollType::wait_indefinitely());
            if !matches!(rx.recv(), Ok(Ok(()))) {
                continue;
            }
            let data = staging.slice(..).get_mapped_range().unwrap();
            let ticks: &[u64] = bytemuck::cast_slice(&data);
            let mut totals: BTreeMap<String, (u32, f64)> = BTreeMap::new();
            for (i, label) in labels.iter().enumerate() {
                let ms = (ticks[2 * i + 1].saturating_sub(ticks[2 * i])) as f64 * period / 1e6;
                let e = totals.entry(label.clone()).or_default();
                e.0 += 1;
                e.1 += ms;
            }
            let sum: f64 = totals.values().map(|v| v.1).sum();
            eprintln!(
                "GPU_PROFILE total {sum:.3} ms over {} dispatches",
                labels.len()
            );
            for (label, (count, ms)) in totals {
                eprintln!("GPU_PROFILE {label:<40} x{count:<4} {ms:.3} ms");
            }
        }
    }
    fn convert(&mut self, t: &ResidentTile, pack: bool) -> EngineResult<ResidentTile> {
        let n = t.layout.len();
        let dst = self.buffer(if pack { n.div_ceil(2) * 4 } else { n * 4 })?;
        let src = self.storage(t)?.clone();
        self.dispatch(
            &self.gpu.resident_pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&[u32::from(!pack), n as u32]),
            if pack { n.div_ceil(2) as u32 } else { n as u32 },
        );
        let mut output = self.tile(t.coord, t.layout, dst);
        Arc::get_mut(&mut output.storage)
            .unwrap()
            .downcast_mut::<Storage>()
            .unwrap()
            .packed = pack;
        Ok(output)
    }
    // Copy contiguous spans from halo-free source tiles, preserving CFA phase
    // at frame boundaries. No samples are read or interpolated on the host.
    fn assemble(
        &mut self,
        frame: Extent,
        coord: TileCoord,
        layout: TileLayout,
        origin: (i64, i64),
        period: u32,
        tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let dst = self.buffer(layout.len() * 4)?;
        let clamp = |v: i64, n: u32| -> u32 {
            if (0..i64::from(n)).contains(&v) {
                return v as u32;
            }
            let phase = v.rem_euclid(i64::from(period)) as u32;
            if phase >= n {
                v.clamp(0, i64::from(n) - 1) as u32
            } else if v < 0 {
                phase
            } else {
                phase + (n - 1 - phase) / period * period
            }
        };
        let xs: Vec<_> = (0..layout.stride())
            .map(|x| clamp(origin.0 + x as i64, frame.width))
            .collect();
        let mut runs = Vec::new();
        let mut x = 0;
        while x < xs.len() {
            let mut end = x + 1;
            while end < xs.len()
                && xs[end] == xs[end - 1] + 1
                && xs[end] / TILE_SIZE == xs[x] / TILE_SIZE
            {
                end += 1;
            }
            runs.push((x, end));
            x = end;
        }
        let mut spans: BTreeMap<TileCoord, Vec<[u32; 3]>> = BTreeMap::new();
        for y in 0..layout.plane_len() / layout.stride() {
            let sy = clamp(origin.1 + y as i64, frame.height);
            for &(x, end) in &runs {
                let sx = xs[x];
                let source = tiles
                    .get(&TileCoord::new(coord.level, sx / TILE_SIZE, sy / TILE_SIZE))
                    .ok_or_else(|| EngineError::internal("resident gather source missing"))?;
                for c in 0..layout.channels as usize {
                    let from = c * source.layout.plane_len()
                        + (sy % TILE_SIZE) as usize * source.layout.stride()
                        + (sx % TILE_SIZE) as usize;
                    let to = c * layout.plane_len() + y * layout.stride() + x;
                    spans.entry(source.coord).or_default().push([
                        from as u32,
                        to as u32,
                        (end - x) as u32,
                    ]);
                }
            }
        }
        for (coord, spans) in spans {
            let src = self.storage(&tiles[&coord])?.clone();
            self.dispatch(
                &self.gpu.gather_pipeline,
                &src,
                &dst,
                bytemuck::cast_slice(&spans),
                spans.len() as u32 * 64,
            );
        }
        Ok(self.tile(coord, layout, dst))
    }
}
impl Batch<'_> {
    /// [`ResidentBatch::run`] for a tile whose interior starts at pixel
    /// `origin` of its frame.
    fn run_origin(
        &mut self,
        op: &Op<'_>,
        tile: &ResidentTile,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        if let Op::Demosaic {
            cfa: raw_decode::CfaLayout::XTrans(pattern),
            ..
        }
        | Op::Highlights {
            cfa: raw_decode::CfaLayout::XTrans(pattern),
            ..
        } = op
        {
            use engine_api::recipe::settings::HighlightReconstruction;
            let (opcode, halo, channels) = match op {
                Op::Demosaic { .. } => (4, 3, 3),
                Op::Highlights {
                    mode: HighlightReconstruction::Clip,
                    ..
                } => (5, 0, 1),
                Op::Highlights {
                    mode: HighlightReconstruction::ReconstructColor,
                    ..
                } => (6, 4, 1),
                _ => return Err(EngineError::invalid("highlights", "unsupported mode")),
            };
            let l = tile.layout;
            if l.channels != 1 || l.halo < halo {
                return Err(EngineError::invalid(
                    "CFA tile",
                    format!("one plane and halo >= {halo} required"),
                ));
            }
            if pattern.iter().flatten().any(|&c| c >= 3)
                || !(0..3).all(|c| pattern.iter().flatten().any(|&v| v == c))
            {
                return Err(EngineError::invalid("CFA", "malformed X-Trans pattern"));
            }
            let (ox, oy) = origin;
            // Integer phase avoids f32 origin precision loss. Both demosaic
            // algorithms use the CPU reference's same X-Trans mean filter.
            let mut p = vec![
                opcode,
                l.extent.width,
                l.extent.height,
                l.halo as u32,
                l.stride() as u32,
                ox % 6,
                oy % 6,
            ];
            p.extend(pattern.iter().flatten().map(|&c| u32::from(c)));
            let layout = TileLayout {
                halo: 0,
                channels,
                ..l
            };
            let dst = self.buffer(layout.len() * 4)?;
            let src = self.storage(tile)?.clone();
            self.dispatch(
                &self.gpu.resident_pipeline,
                &src,
                &dst,
                bytemuck::cast_slice(&p),
                layout.plane_len() as u32,
            );
            return Ok(self.tile(tile.coord, layout, dst));
        }
        if let Op::Detail(settings) = op {
            let l = tile.layout;
            let mut p = crate::detail::parameters(l, settings)?;
            // Validated, inactive Detail is an exact copy of a halo-free tile:
            // share the immutable buffer instead of three full-tile passes.
            if l.halo == 0 && p[5] == 0.0 && p[6] == 0.0 && p[7] == 0.0 {
                return Ok(tile.clone());
            }
            // Interior-only output: the halo-free tile without a strip pass.
            p[20] = 1.0;
            let params = self.host_buffer(
                Some("resident detail"),
                bytemuck::cast_slice(&p),
                wgpu::BufferUsages::STORAGE,
            );
            let src = self.storage(tile)?.clone();
            let layout = TileLayout { halo: 0, ..l };
            let dst = self.buffer(layout.len() * 4)?;
            let decomposition = self.buffer(l.plane_len() * 16)?;
            let entries: Vec<_> = [&src, &dst, &params, &decomposition]
                .iter()
                .enumerate()
                .map(|(i, b)| wgpu::BindGroupEntry {
                    binding: i as u32,
                    resource: b.as_entire_binding(),
                })
                .collect();
            let group = self
                .gpu
                .context()
                .device
                .create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("resident Detail"),
                    layout: &self.gpu.detail_pipelines[0].get_bind_group_layout(0),
                    entries: &entries,
                });
            let [decompose, main] = &self.gpu.detail_pipelines[..] else {
                return Err(EngineError::internal("Detail pipelines"));
            };
            let (decompose, main) = (decompose.clone(), main.clone());
            self.record(
                &decompose,
                group.clone(),
                (l.plane_len() as u32).div_ceil(64),
            );
            self.record(&main, group, (layout.plane_len() as u32).div_ceil(64));
            self.pool.lock().unwrap().free.push(decomposition);
            return Ok(self.tile(tile.coord, layout, dst));
        }
        if op.is_encoded_display()
            && let Some(output) = &self.gpu.managed_output
        {
            let layout = TileLayout {
                halo: 0,
                ..tile.layout
            };
            let dst = self.buffer(layout.len() * 4)?;
            let flags = self.buffer(layout.plane_len() * 4)?;
            let src = self.storage(tile)?.clone();
            let group = output.bindings(&src, &dst, &flags, tile.layout, !self.gpu.export_float)?;
            self.record(
                &output.pipeline,
                group,
                (layout.plane_len() as u32).div_ceil(64),
            );
            self.pool.lock().unwrap().free.push(flags);
            return Ok(self.tile(tile.coord, layout, dst));
        }
        let (p, layout) = parameters(op, tile.layout, origin)?;
        let dst = self.buffer(layout.len() * 4)?;
        let src = self.storage(tile)?.clone();
        self.dispatch(
            &self.gpu.context().pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&p),
            layout.plane_len() as u32,
        );
        Ok(self.tile(tile.coord, layout, dst))
    }
    /// [`ResidentBatch::run_chain`] for a tile at pixel `origin`.
    fn run_chain_origin(
        &mut self,
        ops: &[Op<'_>],
        tile: &ResidentTile,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        if let Some((display, scene)) = ops.split_last()
            && display.is_encoded_display()
            && self.gpu.managed_output.is_some()
        {
            let scene = self.run_chain_origin(scene, tile, origin)?;
            return self.run_origin(display, &scene, origin);
        }
        if crate::fused::supports(ops) {
            let (mut p, layout) =
                crate::fused::parameters_at(ops, tile.layout, tile.coord.level, origin)?;
            let map = match self.effects_map(&mut p) {
                Some(map) => map,
                None => self.no_map.clone(),
            };
            let dst = self.buffer(layout.len() * 4)?;
            let src = self.storage(tile)?.clone();
            self.dispatch_with(
                &self.gpu.fused_pipeline.clone(),
                &src,
                &dst,
                bytemuck::cast_slice(&p),
                layout.plane_len() as u32,
                Some(&map),
            );
            self.gpu
                .counters
                .fused_dispatches
                .fetch_add(1, Ordering::Relaxed);
            return Ok(self.tile(tile.coord, layout, dst));
        }
        let mut output = tile.clone();
        for op in ops {
            output = self.run_origin(op, &output, origin)?;
        }
        Ok(output)
    }
}
impl ResidentBatch for Batch<'_> {
    fn supports_cfa(&self) -> bool {
        true
    }
    fn upload_cfa(
        &mut self,
        full: &image_core::cfa::PackedCfa,
        coord: TileCoord,
        layout: TileLayout,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        self.upload_cfa_impl(full, coord, layout, origin)
    }
    fn blend_cfa(
        &mut self,
        original: &ResidentTile,
        full: &ResidentTile,
        amount: f32,
    ) -> EngineResult<ResidentTile> {
        self.blend_cfa_impl(original, full, amount)
    }

    fn enable_metrics(&mut self) -> bool {
        // Metrics are encoded SDR sRGB, not a monitor/print proof transform.
        self.metrics_only = !self.gpu.export_float && self.gpu.managed_output.is_none();
        self.metrics_only
    }
    fn metrics_enabled(&self) -> bool {
        self.metrics_only
    }
    fn checkpoint(&mut self, cancel: &CancellationToken) -> EngineResult<()> {
        cancel.check()?;
        // Export yields the device to interactive renders at dependency
        // boundaries: drain its own submitted work, then wait for the
        // viewport (spec 08 §2). Nothing is read back or published.
        let yielding = self.gpu.export_float && jobs::interactive_pending() > 0;
        if !yielding
            && (!self.gpu.export_float || self.pool.lock().unwrap().allocated_bytes < 128 << 20)
        {
            return Ok(());
        }
        // Queue uploads cannot reuse buffers inside an unsubmitted encoder:
        // all queue writes precede its compute commands. Retire those uploads
        // at a dependency boundary, retaining live resident outputs on device.
        // This is a submission, not a readback.
        self.encode_compute();
        let ctx = self.gpu.context();
        let encoder = std::mem::replace(
            &mut self.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        ctx.queue.submit([encoder.finish()]);
        self.uploads.dirty.set(false);
        self.gpu
            .counters
            .submissions
            .fetch_add(1, Ordering::Relaxed);
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| EngineError::internal(e.to_string()))?;
        let mut pool = self.pool.lock().unwrap();
        let retired: u64 = pool.free.drain(..).map(|buffer| buffer.size()).sum();
        pool.allocated_bytes = pool.allocated_bytes.saturating_sub(retired);
        drop(pool);
        if yielding {
            jobs::yield_to_interactive(cancel, EXPORT_QUIET, EXPORT_MAX_YIELD)?;
        }
        cancel.check()
    }
    fn cached(&mut self, key: &MemoKey) -> EngineResult<Option<ResidentTile>> {
        self.access_tick += 1;
        let packed = if let Some((tick, tile)) = self.pending.get_mut(key) {
            *tick = self.access_tick;
            Some(tile.clone())
        } else {
            self.gpu.resident_cache.lock().unwrap().get(key)
        };
        packed
            .map(|t| {
                if t.storage
                    .downcast_ref::<Storage>()
                    .is_some_and(|s| s.packed)
                {
                    self.convert(&t, false)
                } else {
                    Ok(t)
                }
            })
            .transpose()
    }
    fn cache(&mut self, key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        if key.stage == StageId::Decode {
            return self.cache_exact(key, tile);
        }
        let packed = self.convert(tile, true)?;
        // Use the same f16-rounded samples on cold, warm and budget-rejected
        // paths, making render results independent of cache residency.
        let rounded = self.convert(&packed, false)?;
        if packed.layout.len().div_ceil(2) * 4 <= self.gpu.resident_cache.lock().unwrap().budget {
            self.access_tick += 1;
            self.pending.insert(key, (self.access_tick, packed));
        }
        Ok(rounded)
    }
    fn cache_exact(&mut self, key: MemoKey, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        // Export transactions (no memo budget) still reuse each uploaded
        // sensor tile across the demosaic halos of its neighbours instead of
        // uploading it once per dependent chunk. Publication at finish keeps
        // applying the cache budget.
        if retain_exact_page(
            self.gpu.export_float,
            key.stage,
            self.storage(tile)?.size() as usize,
            self.gpu.resident_cache.lock().unwrap().budget,
        ) {
            self.access_tick += 1;
            self.pending.insert(key, (self.access_tick, tile.clone()));
        }
        Ok(tile.clone())
    }
    fn upload(&mut self, tile: &Tile) -> EngineResult<ResidentTile> {
        let buffer = self.host_buffer(
            Some("raw upload"),
            bytemuck::cast_slice(tile.samples::<f32>()?),
            wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
        );
        {
            let mut pool = self.pool.lock().unwrap();
            pool.allocations += 1;
            pool.allocated_bytes += buffer.size();
        }
        self.gpu.counters.uploads.fetch_add(1, Ordering::Relaxed);
        Ok(self.tile(tile.coord(), tile.layout(), buffer))
    }
    fn upload_cached(&mut self, key: MemoKey, tile: &Tile) -> EngineResult<ResidentTile> {
        let uploaded = self.upload(tile)?;
        self.cache(key, &uploaded)
    }
    fn supports_level(&self, frame: Extent, halo: u16) -> bool {
        let limits = self.gpu.context().device.limits();
        let padded = u64::from(frame.width + 2 * u32::from(halo))
            * u64::from(frame.height + 2 * u32::from(halo));
        // Operator parameters carry plane lengths as f32 (exact below 2^24).
        padded < 1 << 24
            && padded * 16
                <= limits
                    .max_storage_buffer_binding_size
                    .min(limits.max_buffer_size)
    }
    fn supports_output_level(&self, frame: Extent) -> bool {
        let limits = self.gpu.context().device.limits();
        frame.area() * 12
            <= limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
    }
    fn gather_level(
        &mut self,
        frame: Extent,
        coord: TileCoord,
        halo: u16,
        tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let channels = tiles
            .values()
            .next()
            .ok_or_else(|| EngineError::internal("empty resident level"))?
            .layout
            .channels;
        let layout = TileLayout {
            extent: frame,
            halo,
            channels,
        };
        let h = i64::from(halo);
        self.assemble(frame, coord, layout, (-h, -h), 1, tiles)
    }
    fn crop(
        &mut self,
        tile: &ResidentTile,
        coord: TileCoord,
        origin: (u32, u32),
        extent: Extent,
    ) -> EngineResult<ResidentTile> {
        let l = tile.layout;
        if l.halo != 0
            || origin.0 + extent.width > l.extent.width
            || origin.1 + extent.height > l.extent.height
        {
            return Err(EngineError::invalid(
                "resident crop",
                "outside halo-free tile",
            ));
        }
        let layout = TileLayout {
            extent,
            halo: 0,
            channels: l.channels,
        };
        let dst = self.buffer(layout.len() * 4)?;
        let src = self.storage(tile)?.clone();
        self.dispatch(
            &self.gpu.resident_pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&[
                7u32,
                layout.len() as u32,
                extent.width,
                extent.height,
                0,
                l.extent.width,
                l.plane_len() as u32,
                origin.0,
                origin.1,
            ]),
            layout.len() as u32,
        );
        Ok(self.tile(coord, layout, dst))
    }
    fn supports_local_tone(&self, frame: Extent) -> bool {
        tone::supported(self.gpu.context(), frame)
    }
    fn local_tone(
        &mut self,
        settings: &engine_api::recipe::settings::ToneSettings,
        frame: Extent,
        tiles: &HashMap<TileCoord, ResidentTile>,
        outputs: &[TileCoord],
        options: &image_core::resident::LocalToneOptions,
    ) -> EngineResult<HashMap<TileCoord, ResidentTile>> {
        self.local_tone_impl(settings, frame, tiles, outputs, options)
    }
    fn run(&mut self, op: &Op<'_>, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        self.run_origin(op, tile, tile.coord.pixel_origin(TILE_SIZE))
    }
    fn run_chain(&mut self, ops: &[Op<'_>], tile: &ResidentTile) -> EngineResult<ResidentTile> {
        self.run_chain_origin(ops, tile, tile.coord.pixel_origin(TILE_SIZE))
    }
    fn lateral_ca(
        &mut self,
        tile: &ResidentTile,
        frame: Extent,
        plan: &pipeline_cpu::CaPlan,
    ) -> EngineResult<ResidentTile> {
        self.lateral_ca_impl(tile, tile.coord.pixel_origin(TILE_SIZE), frame, plan)
    }
    fn lens_gain(
        &mut self,
        tile: &ResidentTile,
        frame: Extent,
        plan: &pipeline_cpu::VignettePlan,
    ) -> EngineResult<ResidentTile> {
        self.lens_gain_impl(tile, tile.coord.pixel_origin(TILE_SIZE), frame, plan)
    }
    fn remap(
        &mut self,
        frame: Extent,
        tiles: &HashMap<TileCoord, ResidentTile>,
        source: (u32, u32),
        plan: &pipeline_cpu::MapPlan,
        output: Extent,
        rows: std::ops::Range<u32>,
        coord: TileCoord,
    ) -> EngineResult<ResidentTile> {
        self.remap_impl(frame, tiles, source, plan, output, rows, coord)
    }
    fn gather(
        &mut self,
        frame: Extent,
        coord: TileCoord,
        halo: u16,
        period: u32,
        tiles: &HashMap<TileCoord, ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let (x, y) = coord.pixel_origin(TILE_SIZE);
        let channels = tiles
            .values()
            .next()
            .ok_or_else(|| EngineError::internal("empty resident gather"))?
            .layout
            .channels;
        let layout = TileLayout {
            extent: Extent::new(
                (frame.width - x).min(TILE_SIZE),
                (frame.height - y).min(TILE_SIZE),
            ),
            halo,
            channels,
        };
        self.assemble(
            frame,
            coord,
            layout,
            (
                i64::from(x) - i64::from(halo),
                i64::from(y) - i64::from(halo),
            ),
            period,
            tiles,
        )
    }
    fn supports_bands(&self) -> bool {
        // Export transactions only: viewport renders keep pyramid tiles
        // (memoization and surface presentation are tile-addressed).
        self.gpu.export_float
    }
    fn upload_rows(
        &mut self,
        samples: &[f32],
        width: u32,
        rows: std::ops::Range<u32>,
    ) -> EngineResult<ResidentTile> {
        self.upload_rows_impl(samples, width, rows)
    }
    fn gather_rows(
        &mut self,
        frame: Extent,
        source: &ResidentTile,
        source_row: u32,
        rows: std::ops::Range<u32>,
        halo: u16,
        period: u32,
    ) -> EngineResult<ResidentTile> {
        self.gather_rows_impl(frame, source, source_row, rows, halo, period)
    }
    fn run_at(
        &mut self,
        op: &Op<'_>,
        tile: &ResidentTile,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        self.run_origin(op, tile, origin)
    }
    fn run_chain_at(
        &mut self,
        ops: &[Op<'_>],
        tile: &ResidentTile,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        let mut tile = tile.clone();
        // Point stages develop in their own pixel domain (see develop_tiles).
        tile.coord.level = 0;
        self.run_chain_origin(ops, &tile, origin)
    }
    fn lateral_ca_at(
        &mut self,
        tile: &ResidentTile,
        origin: (u32, u32),
        frame: Extent,
        plan: &pipeline_cpu::CaPlan,
    ) -> EngineResult<ResidentTile> {
        self.lateral_ca_impl(tile, origin, frame, plan)
    }
    fn lens_gain_at(
        &mut self,
        tile: &ResidentTile,
        origin: (u32, u32),
        frame: Extent,
        plan: &pipeline_cpu::VignettePlan,
    ) -> EngineResult<ResidentTile> {
        self.lens_gain_impl(tile, origin, frame, plan)
    }
    fn resample_rows(
        &mut self,
        crop: [u32; 4],
        level: u8,
        rows: std::ops::Range<u32>,
        source: &ResidentTile,
        source_row: u32,
    ) -> EngineResult<ResidentTile> {
        self.resample_rows_impl(crop, level, rows, source, source_row)
    }
    fn remap_rows(
        &mut self,
        frame: Extent,
        band: &ResidentTile,
        source: (u32, u32),
        plan: &pipeline_cpu::MapPlan,
        output: Extent,
        rows: std::ops::Range<u32>,
    ) -> EngineResult<ResidentTile> {
        let coord = band.coord;
        self.remap_band(frame, band, source, plan, output, rows, coord)
    }
    fn finish_rows(
        self: Box<Self>,
        band: ResidentTile,
        dst: &mut [f32],
        cancel: &CancellationToken,
    ) -> EngineResult<()> {
        self.finish_rows_impl(band, dst, cancel)
    }
    fn resample(
        &mut self,
        crop: [u32; 4],
        coord: TileCoord,
        tiles: &HashMap<TileCoord, ResidentTile>,
        accumulated: Option<ResidentTile>,
    ) -> EngineResult<ResidentTile> {
        let [left, top, w, h] = crop;
        let scale = 1u32 << coord.level;
        let e = Extent::new(w, h).at_level(coord.level);
        let (x, y) = coord.pixel_origin(TILE_SIZE);
        let layout = TileLayout {
            extent: Extent::new((e.width - x).min(TILE_SIZE), (e.height - y).min(TILE_SIZE)),
            halo: 0,
            channels: 3,
        };
        let output = if let Some(t) = accumulated {
            t
        } else {
            let buffer = self.buffer(layout.len() * 4)?;
            // Reused scratch is not implicitly zeroed by wgpu.
            self.clear(&buffer);
            self.tile(coord, layout, buffer)
        };
        let dst = self.storage(&output)?.clone();
        let mut sources: Vec<_> = tiles.values().collect();
        sources.sort_by_key(|t| (t.coord.y, t.coord.x));
        let end_x = left + (x * scale + layout.extent.width * scale).min(w);
        let end_y = top + (y * scale + layout.extent.height * scale).min(h);
        for source in sources {
            let (sx, sy) = source.coord.pixel_origin(TILE_SIZE);
            let sw = source.layout.extent.width;
            let sh = source.layout.extent.height;
            if sx >= end_x
                || sy >= end_y
                || sx + sw <= left + x * scale
                || sy + sh <= top + y * scale
            {
                continue;
            }
            let (dx, dy, dw, dh) = contribution_region(
                crop,
                coord,
                layout.extent,
                source.coord,
                source.layout.extent,
            );
            let src = self.storage(source)?.clone();
            self.dispatch(
                &self.gpu.resident_pipeline,
                &src,
                &dst,
                bytemuck::cast_slice(&[
                    2,
                    layout.extent.width,
                    layout.extent.height,
                    sw,
                    sh,
                    scale,
                    left,
                    top,
                    x,
                    y,
                    w,
                    h,
                    sx,
                    sy,
                    dx,
                    dy,
                    dw,
                    dh,
                ]),
                dw * dh * 3,
            );
        }
        Ok(output)
    }
    fn finish(
        mut self: Box<Self>,
        tiles: Vec<ResidentTile>,
        display: bool,
        surface: Option<SurfaceTarget>,
        cancel: &CancellationToken,
    ) -> EngineResult<ResidentOutput> {
        cancel.check()?;
        if self.metrics_only {
            if !display || surface.is_some() {
                return Err(EngineError::invalid(
                    "metrics",
                    "requires SDR display output without a surface",
                ));
            }
            return self.finish_metrics(tiles, cancel);
        }
        let tiles = if let Some(resize) = self.gpu.export_resize {
            self.resize_export(tiles, resize)?
        } else {
            tiles
        };
        let ctx = self.gpu.context();
        let histogram = surface.is_some_and(|s| s.histogram);
        let histogram_buffer = if histogram {
            let buffer = self.buffer(4096)?;
            self.clear(&buffer);
            Some(buffer)
        } else {
            None
        };
        // An EDR surface always runs the fused writer; its histogram is
        // discarded when not requested.
        let mut scratch_histogram = None;
        if let Some(target) = surface {
            let (texture, format) = crate::write_to_iosurface(ctx, target.id)?;
            let float = format == crate::SurfaceFormat::Rgba16Float;
            // RGBA8 takes the encoded SDR Output stage; RGBA16F takes the
            // display-linear EDR transform (`Op::Display` with a headroom).
            if float == display {
                return Err(EngineError::invalid(
                    "IOSurface",
                    if float {
                        "RGBA16F surfaces take display-linear output"
                    } else {
                        "RGBA8 surfaces take encoded display output"
                    },
                ));
            }
            if float && histogram_buffer.is_none() {
                scratch_histogram = Some(self.buffer(4096)?);
            }
            let histogram_binding = histogram_buffer.as_ref().or(scratch_histogram.as_ref());
            let view = texture.create_view(&Default::default());
            let pipeline = if float {
                self.gpu
                    .hdr_surface_pipeline
                    .get_or_init(|| hdr_surface_pipeline(ctx))
            } else if histogram {
                &self.gpu.histogram_pipeline
            } else {
                &self.gpu.surface_pipeline
            };
            let fused = float || histogram;
            for tile in &tiles {
                let (x, y) = tile.coord.pixel_origin(TILE_SIZE);
                let e = tile.layout.extent;
                if x + e.width > texture.width() || y + e.height > texture.height() {
                    return Err(EngineError::invalid(
                        "IOSurface",
                        "render exceeds surface extent",
                    ));
                }
                let p = self.host_buffer(
                    None,
                    bytemuck::cast_slice(&[e.width, e.height, x, y]),
                    wgpu::BufferUsages::STORAGE,
                );
                let mut entries = vec![
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: self.storage(tile)?.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: histogram_binding
                            .map_or(wgpu::BindingResource::TextureView(&view), |buffer| {
                                buffer.as_entire_binding()
                            }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: p.as_entire_binding(),
                    },
                ];
                if fused {
                    entries.push(wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&view),
                    });
                }
                let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: None,
                    layout: &pipeline.get_bind_group_layout(0),
                    entries: &entries,
                });
                // The histogram kernel covers 4096 pixels per workgroup and
                // writes the same quantized pixels to the IOSurface in that pass.
                let pixels_per_group = if fused { 4096 } else { 64 };
                self.record(
                    pipeline,
                    group,
                    (e.area() as u32).div_ceil(pixels_per_group),
                );
            }
        }
        if let Some(scratch) = scratch_histogram {
            self.pool.lock().unwrap().free.push(scratch);
        }
        cancel.check()?;
        self.encode_compute();
        // Final copies must follow the entire compute pass.
        let pixel_bytes = if surface.is_none() {
            tiles.iter().map(|t| t.layout.len() * 4).sum()
        } else {
            0
        };
        let bytes = if histogram { 4096 } else { pixel_bytes };
        if self.gpu.export_float
            && self
                .pool
                .lock()
                .unwrap()
                .allocated_bytes
                .saturating_add(bytes as u64)
                > self.gpu.export_scratch
        {
            return Err(EngineError::Unsupported {
                what: "export GPU scratch plus readback exceeds its budget".into(),
            });
        }
        let staging = if bytes > 0 {
            let buffer = ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(if histogram {
                    "histogram readback (no pixels)"
                } else {
                    "level pixel readback"
                }),
                size: bytes as u64,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            if let Some(hist) = &histogram_buffer {
                self.encoder
                    .copy_buffer_to_buffer(hist, 0, &buffer, 0, 4096);
            } else {
                let mut offset = 0;
                for tile in &tiles {
                    let src = self.storage(tile)?.clone();
                    let size = (tile.layout.len() * 4) as u64;
                    self.encoder
                        .copy_buffer_to_buffer(&src, 0, &buffer, offset, size);
                    offset += size;
                }
            }
            Some(buffer)
        } else {
            None
        };
        let (allocated, buffers) = {
            let p = self.pool.lock().unwrap();
            (p.allocated_bytes, p.allocations)
        };
        self.gpu
            .counters
            .last_resident_allocated_bytes
            .store(allocated, Ordering::Relaxed);
        self.gpu
            .counters
            .last_resident_buffers
            .store(buffers, Ordering::Relaxed);
        self.gpu
            .counters
            .last_resident_dispatches
            .store(self.dispatches, Ordering::Relaxed);
        let diagnostics = format!(
            "{}; tiles={}; dispatches={}; unique_payload_buffers={buffers}; allocated_payload_bytes={allocated}; staging_bytes={bytes}",
            ctx.adapter_info.name,
            tiles.len(),
            self.dispatches
        );
        let failure = |e: &dyn std::fmt::Display| {
            EngineError::internal(format!(
                "resident completion failed: {e}; {diagnostics}; device_status={}",
                ctx.device_failure()
                    .unwrap_or_else(|| "no device-loss callback received".into())
            ))
        };
        cancel.check()?;
        if let Some(lost) = ctx.device_failure() {
            return Err(failure(&lost));
        }
        ctx.queue.submit([self.encoder.finish()]);
        self.uploads.dirty.set(false);
        self.gpu
            .counters
            .submissions
            .fetch_add(1, Ordering::Relaxed);
        let receiver = staging.as_ref().map(|buffer| {
            let (tx, rx) = std::sync::mpsc::channel();
            buffer.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                let _ = tx.send(r);
            });
            rx
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| failure(&e))?;
        if let Some(rx) = receiver {
            rx.recv()
                .map_err(|e| failure(&e))?
                .map_err(|e| failure(&e))?;
        }
        if let Some(lost) = ctx.device_failure() {
            return Err(failure(&lost));
        }
        cancel.check()?;
        // A failed command buffer must never publish corrupt cache entries.
        Self::print_profile(self.gpu, std::mem::take(&mut self.profile));
        if let Some(map) = self.pending_map.take() {
            *self.gpu.effects_map.lock().unwrap() = Some(map);
        }
        {
            let mut cache = self.gpu.resident_cache.lock().unwrap();
            let mut pending: Vec<_> = self.pending.into_iter().collect();
            pending.sort_unstable_by_key(|(_, (tick, _))| *tick);
            for (key, (_, tile)) in pending {
                cache.insert(key, tile);
            }
        }
        let mut output = ResidentOutput::default();
        if let Some(buffer) = staging {
            let mapped = buffer
                .slice(..)
                .get_mapped_range()
                .map_err(|e| failure(&e))?;
            if histogram {
                let counts: &[u32] = bytemuck::cast_slice(&mapped);
                output.histogram = Some(std::array::from_fn(|c| {
                    std::array::from_fn(|i| counts[c * 256 + i])
                }));
                self.gpu
                    .counters
                    .histogram_readbacks
                    .fetch_add(1, Ordering::Relaxed);
            } else {
                let data: &[f32] = bytemuck::cast_slice(&mapped);
                let mut offset = 0;
                for tile in &tiles {
                    let samples = &data[offset..offset + tile.layout.len()];
                    offset += samples.len();
                    output.tiles.push(if display && !self.gpu.export_float {
                        Tile::from_samples(
                            tile.coord,
                            tile.layout,
                            samples.iter().map(|v| *v as u8).collect(),
                        )?
                    } else {
                        Tile::from_samples(tile.coord, tile.layout, samples.to_vec())?
                    });
                }
                self.gpu.counters.readbacks.fetch_add(1, Ordering::Relaxed);
                self.gpu
                    .counters
                    .pixel_readback_bytes
                    .fetch_add(bytes as u64, Ordering::Relaxed);
            }
            drop(mapped);
            buffer.unmap();
        }
        // Completed: Drop recycles this transaction's idle buffers.
        drop(tiles);
        Ok(output)
    }
}

/// Parameter entries that determine the amount-independent per-pixel
/// constants (vignette mask, grain value): extent, crop frame, vignette
/// shape and grain size/roughness. Amounts, style and highlights excluded.
const EFFECTS_MAP_KEY: [usize; 17] = [
    5, 6, 7, 8, 9, 10, 11, 12, 14, 15, 16, 19, 20, 21, 22, 23, 24,
];

fn hdr_surface_pipeline(ctx: &crate::GpuContext) -> wgpu::ComputePipeline {
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("EDR surface writer"),
            source: wgpu::ShaderSource::Wgsl(include_str!("hdr_surface.wgsl").into()),
        });
    ctx.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("EDR surface writer"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}

/// Builds one map texel per pixel of the block's extent.
fn effects_map_pipeline(ctx: &crate::GpuContext) -> wgpu::ComputePipeline {
    let functions = include_str!("effects.wgsl")
        .split("const MAX")
        .nth(1)
        .unwrap()
        .split("@compute")
        .next()
        .unwrap();
    let source = format!(
        "@group(0) @binding(0) var<storage, read> p: array<f32>;\n\
         @group(0) @binding(1) var<storage, read_write> out_map: array<vec2<f32>>;\n\
         const MAX{functions}\n\
         @compute @workgroup_size(16, 16)\n\
         fn main(@builtin(global_invocation_id) id: vec3<u32>) {{\n\
         let w = bitcast<u32>(p[5]); let h = bitcast<u32>(p[6]);\n\
         if id.x >= w || id.y >= h {{ return; }}\n\
         let uv = effects_uv(f32(id.x), f32(id.y));\n\
         out_map[id.y * w + id.x] = vec2<f32>(vignette_mask(uv), grain_value(uv));\n\
         }}\n"
    );
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects constants map"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
    ctx.device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("effects constants map"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        })
}

// Only dispatch output samples whose averaging blocks intersect this source.
fn contribution_region(
    crop: [u32; 4],
    coord: TileCoord,
    extent: Extent,
    source: TileCoord,
    se: Extent,
) -> (u32, u32, u32, u32) {
    let (x, y) = coord.pixel_origin(TILE_SIZE);
    let (sx, sy) = source.pixel_origin(TILE_SIZE);
    let scale = 1u32 << coord.level;
    let dx = (sx.saturating_sub(crop[0]) / scale)
        .saturating_sub(x)
        .min(extent.width);
    let dy = (sy.saturating_sub(crop[1]) / scale)
        .saturating_sub(y)
        .min(extent.height);
    let end_x = (sx + se.width)
        .saturating_sub(crop[0])
        .div_ceil(scale)
        .saturating_sub(x)
        .min(extent.width);
    let end_y = (sy + se.height)
        .saturating_sub(crop[1])
        .div_ceil(scale)
        .saturating_sub(y)
        .min(extent.height);
    (dx, dy, end_x - dx, end_y - dy)
}

// Export owns uploaded source pages for its transaction even with no persistent
// memo budget. Both raw Decode and unpacked full-strength CFA pages are reused
// by overlapping demosaic dependency chunks. RGB intermediates remain scratch.
fn retain_exact_page(export: bool, stage: StageId, bytes: usize, budget: usize) -> bool {
    (export && matches!(stage, StageId::Decode | StageId::Denoise)) || bytes <= budget
}

fn cache_payload_bytes(key: &MemoKey, tile: &ResidentTile) -> usize {
    if let Some(storage) = tile.storage.downcast_ref::<Storage>() {
        return storage.buffer.size() as usize;
    }
    if key.stage == StageId::Decode {
        tile.layout.len() * 4
    } else {
        tile.layout.len().div_ceil(2) * 4
    }
}

pub(crate) fn cache(budget: usize) -> Arc<Mutex<Cache>> {
    Arc::new(Mutex::new(Cache::new(budget)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::{
        id::ImageId,
        stage::{ParamHash, StageId},
    };
    fn key(x: u32) -> MemoKey {
        MemoKey {
            image_id: ImageId(1),
            stage: StageId::Demosaic,
            params_hash: ParamHash::default(),
            tile: TileCoord::new(0, x, 0),
        }
    }
    fn tile(x: u32) -> ResidentTile {
        ResidentTile {
            coord: key(x).tile,
            layout: TileLayout {
                extent: Extent::new(3, 1),
                halo: 0,
                channels: 3,
            },
            storage: Arc::new(()),
        }
    }
    #[test]
    fn export_sensor_pages_survive_a_zero_memo_budget() {
        for stage in [StageId::Decode, StageId::Denoise] {
            assert!(retain_exact_page(true, stage, 4096, 0));
        }
        assert!(!retain_exact_page(true, StageId::Demosaic, 4096, 0));
        assert!(!retain_exact_page(false, StageId::Denoise, 4096, 0));
        assert!(retain_exact_page(false, StageId::Denoise, 4096, 4096));
    }

    #[test]
    fn lru_packed_alignment_replacement_and_oversize() {
        let mut c = Cache::new(40);
        c.insert(key(0), tile(0));
        c.insert(key(1), tile(1));
        assert_eq!(c.bytes(), 40); // 9 halves padded to 20 bytes each
        let held = c.get(&key(0)).unwrap();
        c.insert(key(2), tile(2));
        assert!(c.get(&key(1)).is_none());
        assert!(c.get(&key(0)).is_some());
        assert_eq!(held.coord, key(0).tile);
        c.insert(key(0), tile(0));
        assert_eq!(c.bytes(), 40);
        let mut huge = tile(3);
        huge.layout.extent = Extent::new(256, 256);
        c.insert(key(3), huge);
        assert_eq!(c.bytes(), 40);
        assert!(c.get(&key(3)).is_none());
        let mut zero = Cache::new(0);
        zero.insert(key(0), tile(0));
        assert_eq!(zero.bytes(), 0);
    }
    #[test]
    fn resample_dispatch_covers_only_intersecting_blocks() {
        let crop = [3, 5, 690, 521];
        let coord = TileCoord::new(2, 0, 0);
        let extent = Extent::new(173, 131);
        for sy in 0..3 {
            for sx in 0..3 {
                let source = TileCoord::new(0, sx, sy);
                let se = Extent::new(256, 256);
                let (dx, dy, dw, dh) = contribution_region(crop, coord, extent, source, se);
                for y in 0..extent.height {
                    for x in 0..extent.width {
                        let bx = 3 + x * 4;
                        let by = 5 + y * 4;
                        let intersects = bx < sx * 256 + 256
                            && bx + 4 > sx * 256
                            && by < sy * 256 + 256
                            && by + 4 > sy * 256;
                        assert_eq!(x >= dx && x < dx + dw && y >= dy && y < dy + dh, intersects);
                    }
                }
            }
        }
    }
    #[test]
    #[ignore = "requires Metal; checks >4096 dispatches and ordered clear in one pass"]
    fn single_pass_preserves_dispatch_and_clear_order() {
        let gpu = GpuStageOp::new(Arc::new(crate::GpuContext::new().unwrap()));
        let mut batch = Box::new(Batch::new(&gpu));
        let input = Tile::from_samples(key(0).tile, tile(0).layout, vec![0.25_f32; 9]).unwrap();
        let source = batch.upload(&input).unwrap();
        let packed = batch.convert(&source, true).unwrap();
        let source_buffer = batch.storage(&source).unwrap().clone();
        // A copy of the original values must precede clearing that source.
        // Previously these compute passes exhausted Metal's command-buffer cap.
        for _ in 0..4200 {
            batch.clear(&source_buffer);
        }
        let restored = batch.convert(&packed, false).unwrap();
        assert_eq!(batch.commands.len(), 4202);
        let output = batch
            .finish(
                vec![restored, source],
                false,
                None,
                &CancellationToken::new(),
            )
            .unwrap();
        assert_eq!(output.tiles[0].samples::<f32>().unwrap(), &[0.25; 9]);
        assert_eq!(output.tiles[1].samples::<f32>().unwrap(), &[0.0; 9]);
        assert_eq!(gpu.stats().submissions, 1);
    }

    #[test]
    fn resident_shaders_validate_without_a_device() {
        for source in [
            include_str!("resident.wgsl"),
            include_str!("cfa.wgsl"),
            include_str!("surface.wgsl"),
            include_str!("gather.wgsl"),
            include_str!("histogram.wgsl"),
            include_str!("critic_metrics.wgsl"),
            include_str!("hdr_surface.wgsl"),
            include_str!("zero.wgsl"),
            include_str!("presence.wgsl"),
            include_str!("lens.wgsl"),
        ] {
            let module = naga::front::wgsl::parse_str(source).unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
