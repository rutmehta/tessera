//! Nonresident linear-light local blend. Rasterization and adjustment are CPU reference work.
use engine_api::{EngineError, EngineResult};
use pipeline_cpu::Image;
use std::sync::atomic::Ordering;
use wgpu::util::DeviceExt;

impl crate::GpuStageOp {
    /// Blend one adjusted image against its immutable pre-local base.
    pub fn blend_local(&self, base: &Image, adjusted: &Image, mask: &[f32]) -> EngineResult<Image> {
        let n = base.width() as usize * base.height() as usize;
        if base.width() != adjusted.width()
            || base.height() != adjusted.height()
            || base.planes().len() != 3
            || adjusted.planes().len() != 3
            || mask.len() != n
            || mask
                .iter()
                .any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
        {
            return Err(EngineError::invalid(
                "local blend",
                "matching RGB images and finite unit alpha required",
            ));
        }
        let ctx = self.context();
        let size = (n * 3 * 4) as u64;
        let groups = (n as u64 * 3).div_ceil(64);
        if size > ctx.device.limits().max_storage_buffer_binding_size
            || groups > u64::from(ctx.device.limits().max_compute_workgroups_per_dimension)
        {
            return pipeline_cpu::blend_local(base, adjusted, mask);
        }
        let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let shader = ctx
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("local blend"),
                source: wgpu::ShaderSource::Wgsl(include_str!("locals.wgsl").into()),
            });
        let pipeline = ctx
            .device
            .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("local blend"),
                layout: None,
                module: &shader,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(internal(e));
        }
        let upload = |data: &[f32]| {
            ctx.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("local blend upload"),
                    contents: bytemuck::cast_slice(data),
                    usage: wgpu::BufferUsages::STORAGE,
                })
        };
        let a = upload(&base.planes().concat());
        let b = upload(&adjusted.planes().concat());
        let m = upload(mask);
        let buffer = |usage| {
            ctx.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("local blend output"),
                size,
                usage,
                mapped_at_creation: false,
            })
        };
        let dst = buffer(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
        let staging = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
        let entries: Vec<_> = [&a, &b, &m, &dst]
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
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(groups as u32, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
        ctx.queue.submit([encoder.finish()]);
        self.counters.uploads.fetch_add(3, Ordering::Relaxed);
        self.counters.submissions.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        let data = {
            let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
            bytemuck::cast_slice::<u8, f32>(&mapped)
                .chunks(n)
                .map(<[f32]>::to_vec)
                .collect()
        };
        staging.unmap();
        self.counters.readbacks.fetch_add(1, Ordering::Relaxed);
        self.counters
            .pixel_readback_bytes
            .fetch_add(size, Ordering::Relaxed);
        Image::new(base.width(), base.height(), data)
    }
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
