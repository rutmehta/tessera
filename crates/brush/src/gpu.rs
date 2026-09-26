//! WGSL dab rasterizer for round and sampled tips.
//!
//! Gating: [`GpuDabRenderer::new`] fails when no adapter is available or
//! `TESSERA_BRUSH_GPU=0`; a [`Stroke`](crate::Stroke) only uses it for
//! brushes where [`Brush::gpu_compatible`](crate::Brush::gpu_compatible)
//! holds and falls back to the CPU path on any GPU error. The shader is a
//! line-for-line port of the CPU coverage and accumulation maths; results
//! agree to float rounding of `sin`/`cos`.

use compositor::Rect;
use engine_api::{EngineError, EngineResult};
use wgpu::util::DeviceExt;

use crate::planner::Dab;
use crate::tip::{Tip, TipShape};

const HEADER: usize = 16;
const STRIDE: usize = 10;
const BATCH: usize = 4096;

/// GPU dab rasterizer.
pub struct GpuDabRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    adapter: String,
}

impl std::fmt::Debug for GpuDabRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuDabRenderer")
            .field("adapter", &self.adapter)
            .finish()
    }
}

fn err(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(format!("brush gpu: {e}"))
}

impl GpuDabRenderer {
    /// Acquires an adapter and compiles the shader.
    pub fn new() -> EngineResult<Self> {
        if std::env::var("TESSERA_BRUSH_GPU").is_ok_and(|v| v == "0") {
            return Err(err("disabled by TESSERA_BRUSH_GPU=0"));
        }
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::PRIMARY;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(err)?;
        let name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("brush"),
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(err)?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("brush dab"),
            source: wgpu::ShaderSource::Wgsl(include_str!("dab.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("brush dab"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(err(e));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
            adapter: name,
        })
    }

    /// Adapter name.
    pub fn adapter(&self) -> &str {
        &self.adapter
    }

    /// Accumulates `dabs` into `mask`, the row-major stroke buffer over
    /// `rect`.
    pub fn render(
        &self,
        rect: Rect,
        mask: &mut [f32],
        dabs: &[Dab],
        tip: &Tip,
        wet_edges: bool,
    ) -> EngineResult<()> {
        let (w, h) = (rect.width().max(0) as usize, rect.height().max(0) as usize);
        if mask.len() != w * h {
            return Err(EngineError::invalid("mask", "does not match rect"));
        }
        if w == 0 || h == 0 || dabs.is_empty() {
            return Ok(());
        }
        let limit = self.device.limits().max_storage_buffer_binding_size;
        if (mask.len() as u64) * 4 > limit {
            return Err(err("dirty rect exceeds the storage buffer limit"));
        }
        let (kind, tw, th, hardness, texels): (f32, u32, u32, f32, Vec<f32>) = match &tip.shape {
            TipShape::Round { hardness } => (0.0, 1, 1, *hardness, vec![0.0]),
            TipShape::Sampled(t) => (1.0, t.width, t.height, 0.0, t.data.clone()),
        };
        if (texels.len() as u64) * 4 > limit {
            return Err(err("tip exceeds the storage buffer limit"));
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let usage = wgpu::BufferUsages::STORAGE;
        let mask_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stroke mask"),
                contents: bytemuck::cast_slice(mask),
                usage: usage | wgpu::BufferUsages::COPY_SRC,
            });
        let tip_buf = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tip"),
                contents: bytemuck::cast_slice(&texels),
                usage,
            });
        let layout = self.pipeline.get_bind_group_layout(0);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        let mut keep = Vec::new();
        for chunk in dabs.chunks(BATCH) {
            let mut p = vec![0.0f32; HEADER + STRIDE * chunk.len()];
            p[..10].copy_from_slice(&[
                rect.x0 as f32,
                rect.y0 as f32,
                w as f32,
                h as f32,
                chunk.len() as f32,
                kind,
                tw as f32,
                th as f32,
                hardness,
                if wet_edges { 1.0 } else { 0.0 },
            ]);
            for (k, d) in chunk.iter().enumerate() {
                let b = HEADER + STRIDE * k;
                p[b..b + STRIDE].copy_from_slice(&[
                    d.x,
                    d.y,
                    d.size,
                    d.angle,
                    d.roundness,
                    d.flow,
                    d.opacity,
                    if d.flip_x { 1.0 } else { 0.0 },
                    if d.flip_y { 1.0 } else { 0.0 },
                    tip.half_extent(d.size),
                ]);
            }
            let params = self
                .device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("dabs"),
                    contents: bytemuck::cast_slice(&p),
                    usage,
                });
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: params.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: tip_buf.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: mask_buf.as_entire_binding(),
                    },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: None,
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups((w as u32).div_ceil(8), (h as u32).div_ceil(8), 1);
            }
            keep.push(params);
        }
        let bytes = (mask.len() * 4) as u64;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&mask_buf, 0, &staging, 0, bytes);
        self.queue.submit([encoder.finish()]);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(err(e));
        }
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |v| {
            let _ = tx.send(v);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(err)?;
        rx.recv().map_err(err)?.map_err(err)?;
        {
            let view = staging.slice(..).get_mapped_range().map_err(err)?;
            mask.copy_from_slice(bytemuck::cast_slice::<u8, f32>(&view));
        }
        staging.unmap();
        drop(keep);
        Ok(())
    }
}
