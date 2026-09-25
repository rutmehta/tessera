//! Export row bands: full-width tiles with one dispatch per stage (see
//! `image_core::Renderer::render_export_rows` and band.wgsl).
use super::*;

/// Operator parameters carry plane lengths as f32 (exact below 2^24).
const MAX_BAND_PIXELS: u64 = 1 << 24;

fn pipelines(ctx: &crate::GpuContext) -> &[wgpu::ComputePipeline; 2] {
    ctx.band_pipelines.get_or_init(|| {
        let module = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("export bands"),
                source: wgpu::ShaderSource::Wgsl(include_str!("band.wgsl").into()),
            });
        ["gather", "interleave"].map(|entry| {
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

/// image-core's `clamp_phase`: the in-range position with the same phase.
fn fold(v: i64, n: u32, period: u32) -> u32 {
    if v >= 0 && v < i64::from(n) {
        return v as u32;
    }
    let phase = v.rem_euclid(i64::from(period)) as u32;
    if phase >= n {
        return v.clamp(0, i64::from(n) - 1) as u32;
    }
    if v < 0 {
        phase
    } else {
        phase + (n - 1 - phase) / period * period
    }
}

fn band_error(what: &str) -> EngineError {
    EngineError::invalid("resident band", what)
}

fn check_area(layout: TileLayout) -> EngineResult<()> {
    if layout.plane_len() as u64 >= MAX_BAND_PIXELS {
        return Err(EngineError::Unsupported {
            what: "export band exceeds the operator plane limit".into(),
        });
    }
    Ok(())
}

impl Batch<'_> {
    pub(super) fn upload_rows_impl(
        &mut self,
        samples: &[f32],
        width: u32,
        rows: std::ops::Range<u32>,
    ) -> EngineResult<ResidentTile> {
        let (from, to) = (
            rows.start as usize * width as usize,
            rows.end as usize * width as usize,
        );
        if rows.is_empty() || width == 0 || to > samples.len() {
            return Err(band_error("rows outside the frame"));
        }
        let layout = TileLayout {
            extent: Extent::new(width, rows.len() as u32),
            halo: 0,
            channels: 1,
        };
        check_area(layout)?;
        let buffer = self.host_buffer(
            Some("sensor rows"),
            bytemuck::cast_slice(&samples[from..to]),
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
        Ok(self.tile(TileCoord::new(0, 0, 0), layout, buffer))
    }

    pub(super) fn gather_rows_impl(
        &mut self,
        frame: Extent,
        source: &ResidentTile,
        source_row: u32,
        rows: std::ops::Range<u32>,
        halo: u16,
        period: u32,
    ) -> EngineResult<ResidentTile> {
        let s = source.layout;
        if s.halo != 0 || s.extent.width != frame.width || period == 0 {
            return Err(band_error("gather needs a halo-free full-width source"));
        }
        if rows.is_empty() || rows.end > frame.height {
            return Err(band_error("gather rows outside the frame"));
        }
        let h = i64::from(halo);
        let (mut first, mut last) = (u32::MAX, 0);
        for v in i64::from(rows.start) - h..i64::from(rows.end) + h {
            let y = fold(v, frame.height, period);
            first = first.min(y);
            last = last.max(y);
        }
        if first < source_row || last >= source_row + s.extent.height {
            return Err(band_error("gather source does not cover the halo"));
        }
        let layout = TileLayout {
            extent: Extent::new(frame.width, rows.len() as u32),
            halo,
            channels: s.channels,
        };
        check_area(layout)?;
        let dst = self.buffer(layout.len() * 4)?;
        let src = self.storage(source)?.clone();
        let p = [
            layout.stride() as u32,
            rows.len() as u32 + 2 * u32::from(halo),
            u32::from(s.channels),
            u32::from(halo),
            rows.start,
            frame.width,
            frame.height,
            period,
            source_row,
            s.extent.height,
        ];
        let pipeline = pipelines(self.gpu.context())[0].clone();
        self.dispatch(
            &pipeline,
            &src,
            &dst,
            bytemuck::cast_slice(&p),
            layout.len() as u32,
        );
        Ok(self.tile(source.coord, layout, dst))
    }

    pub(super) fn resample_rows_impl(
        &mut self,
        crop: [u32; 4],
        level: u8,
        rows: std::ops::Range<u32>,
        source: &ResidentTile,
        source_row: u32,
    ) -> EngineResult<ResidentTile> {
        let [left, top, w, h] = crop;
        let scale = 1u32 << level;
        let e = Extent::new(w, h).at_level(level);
        let s = source.layout;
        if s.halo != 0 || s.channels != 3 || left + w > s.extent.width {
            return Err(band_error("resample needs a halo-free RGB sensor band"));
        }
        if rows.is_empty() || rows.end > e.height {
            return Err(band_error("resample rows outside the level"));
        }
        let (first, end) = (top + rows.start * scale, top + (rows.end * scale).min(h));
        if first < source_row || end > source_row + s.extent.height {
            return Err(band_error("resample source does not cover the rows"));
        }
        let layout = TileLayout {
            extent: Extent::new(e.width, rows.len() as u32),
            halo: 0,
            channels: 3,
        };
        check_area(layout)?;
        let buffer = self.buffer(layout.len() * 4)?;
        // Reused scratch is not implicitly zeroed; the kernel accumulates.
        self.clear(&buffer);
        let src = self.storage(source)?.clone();
        let (ow, oh) = (layout.extent.width, layout.extent.height);
        let p = [
            2,
            ow,
            oh,
            s.extent.width,
            s.extent.height,
            scale,
            left,
            top,
            0,
            rows.start,
            w,
            h,
            0,
            source_row,
            0,
            0,
            ow,
            oh,
        ];
        self.dispatch(
            &self.gpu.resident_pipeline.clone(),
            &src,
            &buffer,
            bytemuck::cast_slice(&p),
            ow * oh * 3,
        );
        Ok(self.tile(TileCoord::new(level, 0, 0), layout, buffer))
    }

    pub(super) fn finish_rows_impl(
        mut self: Box<Self>,
        band: ResidentTile,
        dst: &mut [f32],
        cancel: &CancellationToken,
    ) -> EngineResult<()> {
        cancel.check()?;
        let band = match self.gpu.export_resize {
            Some(request) => {
                let rect = request.support_rect()?;
                if band.layout.halo != 0
                    || band.layout.channels != 3
                    || band.layout.extent != Extent::new(rect.width, rect.height)
                {
                    return Err(band_error("resize band must hold its source rows"));
                }
                let src = self.storage(&band)?.clone();
                self.lanczos(&src, rect, request)?
            }
            None => band,
        };
        let l = band.layout;
        let n = l.extent.area() as usize;
        if l.halo != 0 || l.channels != 3 || dst.len() != n * 3 {
            return Err(band_error(
                "readback needs a halo-free RGB band of dst's size",
            ));
        }
        let bytes = n * 12;
        let interleaved = self.buffer(bytes)?;
        let src = self.storage(&band)?.clone();
        let pipeline = pipelines(self.gpu.context())[1].clone();
        self.dispatch(
            &pipeline,
            &src,
            &interleaved,
            bytemuck::cast_slice(&[n as u32]),
            (n * 3) as u32,
        );
        drop(band);
        cancel.check()?;
        self.encode_compute();
        let ctx = self.gpu.context();
        let (allocated, buffers) = {
            let p = self.pool.lock().unwrap();
            (p.allocated_bytes, p.allocations)
        };
        if allocated.saturating_add(bytes as u64) > self.gpu.export_scratch {
            return Err(EngineError::Unsupported {
                what: "export GPU scratch plus readback exceeds its budget".into(),
            });
        }
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("band pixel readback"),
            size: bytes as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.encoder
            .copy_buffer_to_buffer(&interleaved, 0, &staging, 0, bytes as u64);
        let counters = &self.gpu.counters;
        counters
            .last_resident_allocated_bytes
            .store(allocated, Ordering::Relaxed);
        counters
            .last_resident_buffers
            .store(buffers, Ordering::Relaxed);
        counters
            .last_resident_dispatches
            .store(self.dispatches, Ordering::Relaxed);
        let failure = |e: &dyn std::fmt::Display| {
            EngineError::internal(format!(
                "resident band failed: {e}; device_status={}",
                ctx.device_failure()
                    .unwrap_or_else(|| "no device-loss callback received".into())
            ))
        };
        if let Some(lost) = ctx.device_failure() {
            return Err(failure(&lost));
        }
        let encoder = std::mem::replace(
            &mut self.encoder,
            ctx.device.create_command_encoder(&Default::default()),
        );
        let submission = ctx.queue.submit([encoder.finish()]);
        self.uploads.dirty.set(false);
        counters.submissions.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        // Only this band's submission: another band in flight may follow it.
        ctx.device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission),
                timeout: None,
            })
            .map_err(|e| failure(&e))?;
        rx.recv()
            .map_err(|e| failure(&e))?
            .map_err(|e| failure(&e))?;
        if let Some(lost) = ctx.device_failure() {
            return Err(failure(&lost));
        }
        cancel.check()?;
        Self::print_profile(self.gpu, std::mem::take(&mut self.profile));
        // Later bands of this export reuse the effects constants map.
        if let Some(map) = self.pending_map.take() {
            *self.gpu.effects_map.lock().unwrap() = Some(map);
        }
        {
            let mapped = staging
                .slice(..)
                .get_mapped_range()
                .map_err(|e| failure(&e))?;
            dst.copy_from_slice(bytemuck::cast_slice(&mapped));
        }
        staging.unmap();
        counters.readbacks.fetch_add(1, Ordering::Relaxed);
        counters
            .pixel_readback_bytes
            .fetch_add(bytes as u64, Ordering::Relaxed);
        Ok(())
    }
}
