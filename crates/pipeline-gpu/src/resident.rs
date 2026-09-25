use crate::{GpuStageOp, operator::parameters};
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
    workgroups: u32,
}

pub(crate) struct Batch<'a> {
    gpu: &'a GpuStageOp,
    encoder: wgpu::CommandEncoder,
    pending: HashMap<MemoKey, (u64, ResidentTile)>,
    access_tick: u64,
    pool: Arc<Mutex<Pool>>,
    dispatches: u64,
    commands: Vec<Dispatch>,
    uploads: PendingUploads,
}
impl<'a> Batch<'a> {
    pub fn new(gpu: &'a GpuStageOp) -> Self {
        Self {
            gpu,
            encoder: gpu
                .context()
                .device
                .create_command_encoder(&Default::default()),
            pending: HashMap::new(),
            access_tick: 0,
            pool: Arc::default(),
            dispatches: 0,
            commands: Vec::new(),
            uploads: PendingUploads {
                queue: gpu.context().queue.clone(),
                dirty: std::cell::Cell::new(false),
            },
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
        let ctx = self.gpu.context();
        let p = self.host_buffer(
            Some("resident parameters"),
            params,
            wgpu::BufferUsages::STORAGE,
        );
        let entries: Vec<_> = [src, dst, &p]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
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
        self.dispatches += 1;
        self.commands.push(Dispatch {
            pipeline: pipeline.clone(),
            group,
            workgroups,
        });
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
    fn encode_compute(&mut self) {
        if self.commands.is_empty() {
            return;
        }
        // wgpu-core opens a Metal command buffer per COMPUTE PASS, not per
        // CommandEncoder. One pass avoids exhausting Metal's 4096-buffer cap.
        // wgpu tracks storage hazards between dispatches, including pooled
        // buffers reused for a different role later in this ordered stream.
        let mut pass = self.encoder.begin_compute_pass(&Default::default());
        for command in &self.commands {
            pass.set_pipeline(&command.pipeline);
            pass.set_bind_group(0, &command.group, &[]);
            pass.dispatch_workgroups(command.workgroups, 1, 1);
        }
        drop(pass);
        self.commands.clear();
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
impl ResidentBatch for Batch<'_> {
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
        if self.storage(tile)?.size() as usize <= self.gpu.resident_cache.lock().unwrap().budget {
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
    fn run(&mut self, op: &Op<'_>, tile: &ResidentTile) -> EngineResult<ResidentTile> {
        if let Op::Detail(settings) = op {
            let l = tile.layout;
            let p = crate::detail::parameters(l, settings)?;
            let params = self.host_buffer(
                Some("resident detail"),
                bytemuck::cast_slice(&p),
                wgpu::BufferUsages::STORAGE,
            );
            let src = self.storage(tile)?.clone();
            let dst = self.buffer(l.len() * 4)?;
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
            for pipeline in &self.gpu.detail_pipelines {
                self.record(pipeline, group.clone(), (l.plane_len() as u32).div_ceil(64));
            }
            let layout = TileLayout { halo: 0, ..l };
            let interior = self.buffer(layout.len() * 4)?;
            self.dispatch(
                &self.gpu.resident_pipeline,
                &dst,
                &interior,
                bytemuck::cast_slice(&[
                    3u32,
                    layout.len() as u32,
                    l.extent.width,
                    l.extent.height,
                    l.halo as u32,
                    l.stride() as u32,
                    l.plane_len() as u32,
                ]),
                layout.len() as u32,
            );
            self.pool.lock().unwrap().free.extend([dst, decomposition]);
            return Ok(self.tile(tile.coord, layout, interior));
        }
        if matches!(op, Op::Display { .. })
            && let Some(output) = &self.gpu.managed_output
        {
            let layout = TileLayout {
                halo: 0,
                ..tile.layout
            };
            let dst = self.buffer(layout.len() * 4)?;
            let flags = self.buffer(layout.plane_len() * 4)?;
            let src = self.storage(tile)?.clone();
            let group = output.bindings(&src, &dst, &flags, tile.layout, true)?;
            self.record(
                &output.pipeline,
                group,
                (layout.plane_len() as u32).div_ceil(64),
            );
            self.pool.lock().unwrap().free.push(flags);
            return Ok(self.tile(tile.coord, layout, dst));
        }
        let (p, layout) = parameters(op, tile.layout, tile.coord.pixel_origin(TILE_SIZE))?;
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
    fn run_chain(&mut self, ops: &[Op<'_>], tile: &ResidentTile) -> EngineResult<ResidentTile> {
        if let [Op::Tone(_), Op::Display { .. }] = ops
            && self.gpu.managed_output.is_none()
        {
            let origin = tile.coord.pixel_origin(TILE_SIZE);
            let (tone, _) = parameters(&ops[0], tile.layout, origin)?;
            let (mut p, layout) = parameters(&ops[1], tile.layout, origin)?;
            p[0] = 7.0;
            p[25..32].copy_from_slice(&tone[25..32]);
            let dst = self.buffer(layout.len() * 4)?;
            let src = self.storage(tile)?.clone();
            self.dispatch(
                &self.gpu.context().pipeline,
                &src,
                &dst,
                bytemuck::cast_slice(&p),
                layout.plane_len() as u32,
            );
            return Ok(self.tile(tile.coord, layout, dst));
        }
        let mut output = tile.clone();
        for op in ops {
            output = self.run(op, &output)?;
        }
        Ok(output)
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
        let ctx = self.gpu.context();
        let histogram = surface.is_some_and(|s| s.histogram);
        let histogram_buffer = if histogram {
            let buffer = self.buffer(4096)?;
            self.clear(&buffer);
            Some(buffer)
        } else {
            None
        };
        if let Some(target) = surface {
            if !display {
                return Err(EngineError::invalid("IOSurface", "display output required"));
            }
            let texture = crate::write_to_iosurface(ctx, target.id)?;
            let view = texture.create_view(&Default::default());
            let pipeline = if histogram {
                &self.gpu.histogram_pipeline
            } else {
                &self.gpu.surface_pipeline
            };
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
                        resource: histogram_buffer
                            .as_ref()
                            .map_or(wgpu::BindingResource::TextureView(&view), |buffer| {
                                buffer.as_entire_binding()
                            }),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: p.as_entire_binding(),
                    },
                ];
                if histogram {
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
                let pixels_per_group = if histogram { 4096 } else { 64 };
                self.record(
                    pipeline,
                    group,
                    (e.area() as u32).div_ceil(pixels_per_group),
                );
            }
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
                for tile in tiles {
                    let samples = &data[offset..offset + tile.layout.len()];
                    offset += samples.len();
                    output.tiles.push(if display {
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
        Ok(output)
    }
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
            include_str!("surface.wgsl"),
            include_str!("gather.wgsl"),
            include_str!("histogram.wgsl"),
            include_str!("zero.wgsl"),
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
