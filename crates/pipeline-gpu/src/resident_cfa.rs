//! One fresh packed payload upload, followed by GPU unpack and Amount blend.
use super::*;
use image_core::cfa::PackedCfa;
fn pipelines(ctx: &crate::GpuContext) -> &[wgpu::ComputePipeline; 2] {
    ctx.cfa_pipelines.get_or_init(|| {
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("CFA handoff"),
                source: wgpu::ShaderSource::Wgsl(include_str!("cfa.wgsl").into()),
            });
        ["unpack", "blend"].map(|entry| {
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: None,
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        })
    })
}
fn error() -> EngineError {
    EngineError::invalid("resident CFA", "invalid layout, origin, or Amount")
}
fn fold(v: i64, n: u32) -> u32 {
    if (0..i64::from(n)).contains(&v) {
        v as u32
    } else {
        let phase = v.rem_euclid(2) as u32;
        if v < 0 {
            phase
        } else {
            phase + (n - 1 - phase) / 2 * 2
        }
    }
}
impl Batch<'_> {
    pub(super) fn upload_cfa_impl(
        &mut self,
        full: &PackedCfa,
        coord: TileCoord,
        layout: TileLayout,
        origin: (u32, u32),
    ) -> EngineResult<ResidentTile> {
        let frame = full.frame();
        if layout.channels != 1
            || layout.extent.width == 0
            || layout.extent.height == 0
            || u64::from(origin.0) + u64::from(layout.extent.width) > u64::from(frame.width)
            || u64::from(origin.1) + u64::from(layout.extent.height) > u64::from(frame.height)
        {
            return Err(error());
        }
        // Bounds in packed coordinates. Copy contiguous channel-row spans;
        // do not unpack a sensor plane on the host. Folding includes every
        // same-phase edge sample touched by the output's halo.
        let bounds = |start: u32, count: usize, n: u32| {
            (0..count)
                .map(|i| fold(i64::from(start) + i as i64 - i64::from(layout.halo), n))
                .fold((u32::MAX, 0), |(lo, hi), v| (lo.min(v), hi.max(v)))
        };
        let (x0, x1) = bounds(origin.0, layout.stride(), frame.width);
        let (y0, y1) = bounds(origin.1, layout.rows(), frame.height);
        let a = full.rotated_site(x0, y0);
        let b = full.rotated_site(x1, y1);
        let (px, py) = (a.0.min(b.0) / 2, a.1.min(b.1) / 2);
        let (pw, ph) = (a.0.max(b.0) / 2 - px + 1, a.1.max(b.1) / 2 - py + 1);
        let e = full.packed_extent();
        let plane = e.area() as usize;
        let len = pw as usize * ph as usize * 4 * (1 + usize::from(full.mask().is_some()));
        let limit = self
            .gpu
            .context()
            .device
            .limits()
            .max_storage_buffer_binding_size;
        if len as u64 * 4 > limit {
            return Err(error());
        }
        let mut payload = Vec::with_capacity(len);
        for data in std::iter::once(full.samples()).chain(full.mask()) {
            for c in 0..4 {
                for y in py..py + ph {
                    let from = c * plane + (y * e.width + px) as usize;
                    payload.extend_from_slice(&data[from..from + pw as usize]);
                }
            }
        }
        // Never obtain this from the compute pool: queue writes execute before
        // the entire pending compute submission, regardless of encode order.
        let src = self.host_buffer(
            Some("packed CFA output"),
            bytemuck::cast_slice(&payload),
            wgpu::BufferUsages::STORAGE,
        );
        {
            let mut pool = self.pool.lock().unwrap();
            pool.allocations += 1;
            pool.allocated_bytes += src.size();
        }
        self.gpu.counters.uploads.fetch_add(1, Ordering::Relaxed);
        let out_layout = TileLayout {
            channels: 2,
            ..layout
        };
        let dst = self.buffer(out_layout.len() * 4)?;
        let p = [
            layout.stride() as u32,
            layout.rows() as u32,
            u32::from(layout.halo),
            origin.0,
            origin.1,
            frame.width,
            frame.height,
            u32::from(full.turns()),
            px,
            py,
            pw,
            ph,
            u32::from(full.mask().is_some()),
        ];
        let pipeline = pipelines(self.gpu.context())[0].clone();
        self.dispatch(
            &pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&p),
            layout.plane_len() as u32,
        );
        Ok(self.tile(coord, out_layout, dst))
    }
    pub(super) fn blend_cfa_impl(
        &mut self,
        original: &ResidentTile,
        full: &ResidentTile,
        amount: f32,
    ) -> EngineResult<ResidentTile> {
        if !(0.0..=1.0).contains(&amount)
            || original.layout.channels != 1
            || full.layout.channels != 2
            || original.layout.extent != full.layout.extent
            || original.layout.halo != full.layout.halo
        {
            return Err(error());
        }
        // Validate device ownership even on the bypass.
        let src = self.storage(original)?.clone();
        let extra = self.storage(full)?.clone();
        if amount == 0.0 {
            return Ok(original.clone());
        }
        let dst = self.buffer(original.layout.len() * 4)?;
        let p = [original.layout.len() as u32, amount.to_bits()];
        let pipeline = pipelines(self.gpu.context())[1].clone();
        self.dispatch_with(
            &pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&p),
            original.layout.len() as u32,
            Some(&extra),
        );
        Ok(self.tile(original.coord, original.layout, dst))
    }
}
