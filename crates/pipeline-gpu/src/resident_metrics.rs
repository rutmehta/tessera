//! Full-resolution encoded-output reduction using the resident transaction.
use super::Batch;
use engine_api::{EngineError, EngineResult, jobs::CancellationToken};
use image_core::resident::{OutputMetrics, ResidentOutput, ResidentTile};
use std::sync::atomic::Ordering;
const WORDS: usize = 1282;
const BAND: usize = 16384;
impl Batch<'_> {
    pub(super) fn finish_metrics(
        mut self: Box<Self>,
        tiles: Vec<ResidentTile>,
        cancel: &CancellationToken,
    ) -> EngineResult<ResidentOutput> {
        let ctx = self.gpu.context();
        let pipeline = ctx.metrics_pipeline.get_or_init(|| {
            let module = ctx
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("critic full-resolution reduction"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("critic_metrics.wgsl").into()),
                });
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("critic full-resolution reduction"),
                    layout: None,
                    module: &module,
                    entry_point: Some("main"),
                    compilation_options: Default::default(),
                    cache: None,
                })
        });
        let bands: usize = tiles
            .iter()
            .map(|t| (t.layout.extent.area() as usize).div_ceil(BAND))
            .sum();
        let buffer = self.buffer(bands * WORDS * 4)?;
        let mut offset = 0;
        let mut metrics = OutputMetrics::default();
        for tile in &tiles {
            cancel.check()?;
            let n = tile.layout.extent.area() as usize;
            if tile.layout.halo != 0 || tile.layout.channels != 3 {
                return Err(EngineError::invalid(
                    "metrics",
                    "requires halo-free RGB output",
                ));
            }
            let src = self.storage(tile)?.clone();
            // dispatch() uses groups of 64; the kernel owns 16K pixels/group.
            self.dispatch(
                pipeline,
                &src,
                &buffer,
                bytemuck::cast_slice(&[n as u32, offset as u32]),
                (n.div_ceil(BAND) * 64) as u32,
            );
            offset += n.div_ceil(BAND);
            metrics.pixels += n as u64;
        }
        let data = self.read_now(&[&buffer])?;
        cancel.check()?;
        if let Some(lost) = ctx.device_failure() {
            return Err(EngineError::internal(lost));
        }
        let counts: &[u32] = bytemuck::cast_slice(&data[0]);
        for band in counts.as_chunks::<WORDS>().0 {
            for (i, n) in band[..1024].iter().enumerate() {
                metrics.histogram[i / 256][i % 256] += u64::from(*n);
            }
            for (i, n) in band[1024..1280].iter().enumerate() {
                metrics.linear_histogram[i] += u64::from(*n);
            }
            metrics.clipped_shadows += u64::from(band[1280]);
            metrics.clipped_highlights += u64::from(band[1281]);
        }
        self.pool.lock().unwrap().free.push(buffer);
        self.gpu
            .counters
            .histogram_readbacks
            .fetch_add(1, Ordering::Relaxed);
        self.gpu
            .counters
            .last_resident_dispatches
            .store(self.dispatches, Ordering::Relaxed);
        // Publish only after successful completion and cancellation/device checks.
        Self::print_profile(self.gpu, std::mem::take(&mut self.profile));
        if let Some(map) = self.pending_map.take() {
            *self.gpu.effects_map.lock().unwrap() = Some(map);
        }
        let mut cache = self.gpu.resident_cache.lock().unwrap();
        let mut pending: Vec<_> = self.pending.drain().collect();
        pending.sort_unstable_by_key(|(_, (tick, _))| *tick);
        for (key, (_, tile)) in pending {
            cache.insert(key, tile);
        }
        Ok(ResidentOutput {
            metrics: Some(metrics),
            ..Default::default()
        })
    }
}
