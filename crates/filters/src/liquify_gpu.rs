//! Metal compute liquify using an RG32Float inverse-displacement texture.
//! Sampling is explicit f32, including alpha and negative/HDR RGB. No CPU fallback.
use crate::{
    Buffer, checkpoint,
    liquify::{Interpolation, Mesh},
};
use compositor::raster::Raster;
use engine_api::{EngineError, EngineResult};
use std::sync::atomic::AtomicBool;
use wgpu::util::DeviceExt;

// Keep a SIMD group's adjacent lanes on adjacent RGBA pixels. The shared shader
// defaults to 8x8; specialize its workgroup shape without changing its math.
const WORKGROUP_X: u32 = 32;
const WORKGROUP_Y: u32 = 4;

pub struct GpuLiquify {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
}
/// Reusable GPU-resident frame. Source, displacement, output, uniform, binding,
/// and readback staging allocations live until this frame is dropped. Raster
/// metadata/tiles are shared, not copied per dispatch. Output is row-major RGBA
/// f32 (including straight alpha and HDR/negative values), 16 bytes per pixel.
///
/// `submit_wait` only encodes/submits compute and waits; it does not download or
/// allocate/copy image-sized buffers. Readback is an explicit, expensive operation.
/// Cancellation is checked at boundaries, never inside an in-flight dispatch.
pub struct ResidentLiquify<'a> {
    gpu: &'a GpuLiquify,
    template: Raster,
    cell_size: u32,
    texture: wgpu::Texture,
    _source: wgpu::Buffer,
    _params: wgpu::Buffer,
    out: wgpu::Buffer,
    staging: wgpu::Buffer,
    bind: wgpu::BindGroup,
    ready: bool,
}

impl ResidentLiquify<'_> {
    /// Stable STORAGE | COPY_SRC buffer for downstream GPU stages on `gpu.device()`.
    /// Contents are valid only after a successful `submit_wait`; mesh updates
    /// invalidate them. Do not destroy or overwrite resources exposed by this API.
    pub fn output_buffer(&self) -> &wgpu::Buffer {
        &self.out
    }

    /// Upload only displacement nodes, retaining all allocations and source pixels.
    /// Dimensions and cell size must match preparation. Invalid/cancelled updates
    /// leave the previous mesh/output intact. The next submit flushes this upload.
    pub fn update_mesh(&mut self, mesh: &Mesh, cancel: &AtomicBool) -> EngineResult<()> {
        checkpoint(cancel)?;
        mesh.validate()?;
        let extent = self.template.extent();
        if mesh.width != extent.width
            || mesh.height != extent.height
            || mesh.cell_size != self.cell_size
        {
            return Err(invalid("resident mesh dimensions or cell size differ"));
        }
        let (gw, gh) = mesh.grid();
        self.gpu.queue.write_texture(
            self.texture.as_image_copy(),
            bytemuck::cast_slice(&mesh.displacement),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(gw as u32 * 8),
                rows_per_image: Some(gh as u32),
            },
            self.texture.size(),
        );
        self.ready = false;
        Ok(())
    }

    /// Compute-only render, including submission and GPU completion wait. Pending
    /// mesh uploads are also flushed. No full-image upload, copy, or allocation.
    pub fn submit_wait(&mut self, cancel: &AtomicBool) -> EngineResult<()> {
        checkpoint(cancel)?;
        self.ready = false;
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident liquify"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.gpu.pipeline);
            pass.set_bind_group(0, &self.bind, &[]);
            let extent = self.template.extent();
            pass.dispatch_workgroups(
                extent.width.div_ceil(WORKGROUP_X),
                extent.height.div_ceil(WORKGROUP_Y),
                1,
            );
        }
        self.gpu.queue.submit([encoder.finish()]);
        self.gpu
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        checkpoint(cancel)?;
        self.ready = true;
        Ok(())
    }

    /// Download the last successful render and convert to a Raster with the
    /// original depth/channels/revision semantics. Reuses staging, but necessarily
    /// copies the full image to CPU memory. Never included in resident render time.
    pub fn readback(&mut self, cancel: &AtomicBool) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        if !self.ready {
            return Err(invalid("resident output needs a successful dispatch"));
        }
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.out, 0, &self.staging, 0, self.out.size());
        self.gpu.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        self.staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |v| {
                let _ = tx.send(v);
            });
        if let Err(e) = self.gpu.device.poll(wgpu::PollType::wait_indefinitely()) {
            self.staging.unmap();
            return Err(internal(e));
        }
        let result: EngineResult<Vec<[f32; 4]>> = (|| {
            rx.recv().map_err(internal)?.map_err(internal)?;
            checkpoint(cancel)?;
            let view = self
                .staging
                .slice(..)
                .get_mapped_range()
                .map_err(internal)?;
            Ok(bytemuck::cast_slice::<u8, [f32; 4]>(&view).to_vec())
        })();
        self.staging.unmap();
        let extent = self.template.extent();
        Buffer {
            w: extent.width as usize,
            h: extent.height as usize,
            pixels: result?,
        }
        .write(&self.template, cancel)
    }
}

fn invalid(s: &str) -> EngineError {
    EngineError::invalid("GPU liquify", s)
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(format!("GPU liquify: {e}"))
}
impl GpuLiquify {
    pub fn new() -> EngineResult<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(internal)?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("liquify"),
            required_features: adapter.features() & wgpu::Features::TIMESTAMP_QUERY,
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(internal)?;
        // Fail rather than silently dispatch the wrong grid if the shared shader
        // changes its workgroup declaration in the future.
        let (prefix, suffix) = include_str!("shaders/liquify.wgsl")
            .split_once("@workgroup_size(8,8)")
            .ok_or_else(|| internal("unexpected shader workgroup declaration"))?;
        let shader = format!("{prefix}@workgroup_size({WORKGROUP_X},{WORKGROUP_Y}){suffix}");
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("liquify"),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
        let types = [
            wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: false },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Uniform,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
        ];
        let entries: Vec<_> = types
            .into_iter()
            .enumerate()
            .map(|(binding, ty)| wgpu::BindGroupLayoutEntry {
                binding: binding as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty,
                count: None,
            })
            .collect();
        let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("liquify"),
            entries: &entries,
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("liquify"),
            bind_group_layouts: &[Some(&bind_layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("liquify"),
            layout: Some(&layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(internal(e));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }
    pub fn render(
        &self,
        mesh: &Mesh,
        input: &Raster,
        interpolation: Interpolation,
        cancel: &AtomicBool,
    ) -> EngineResult<Raster> {
        self.render_inner(mesh, input, interpolation, cancel, false)
            .map(|(r, _)| r)
    }
    /// Returns compute-pass GPU timestamp duration when supported. This excludes
    /// allocation, uploads, readback, CPU raster packing, and device initialization.
    /// Identity has no dispatch and returns None, as do unavailable/zero counter
    /// samples. Use wall time for end-to-end cost; cancellation is checked before
    /// submission and after completion, not inside an in-flight GPU dispatch.
    pub fn render_profiled(
        &self,
        mesh: &Mesh,
        input: &Raster,
        interpolation: Interpolation,
        cancel: &AtomicBool,
    ) -> EngineResult<(Raster, Option<std::time::Duration>)> {
        self.render_inner(mesh, input, interpolation, cancel, true)
    }
    /// Pack and upload the source once. Completes pending uploads before returning.
    /// Subsequent dispatches retain every image-sized resource; only mesh nodes
    /// need uploading when editing. Device/pipeline initialization is separate.
    pub fn prepare<'a>(
        &'a self,
        input: &Raster,
        mesh: &Mesh,
        interpolation: Interpolation,
        cancel: &AtomicBool,
    ) -> EngineResult<ResidentLiquify<'a>> {
        self.prepare_inner(input, mesh, interpolation, cancel, false)?
            .ok_or_else(|| internal("missing resident frame"))
    }

    fn prepare_inner<'a>(
        &'a self,
        input: &Raster,
        mesh: &Mesh,
        interpolation: Interpolation,
        cancel: &AtomicBool,
        skip_identity: bool,
    ) -> EngineResult<Option<ResidentLiquify<'a>>> {
        checkpoint(cancel)?;
        mesh.validate()?;
        if input.extent().width != mesh.width || input.extent().height != mesh.height {
            return Err(invalid("mesh and raster dimensions differ"));
        }
        let (gw, gh) = mesh.grid();
        let limits = self.device.limits();
        let size = u64::from(mesh.width)
            .checked_mul(u64::from(mesh.height))
            .and_then(|n| n.checked_mul(16))
            .ok_or_else(|| invalid("image size overflow"))?;
        if size > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || gw > limits.max_texture_dimension_2d as usize
            || gh > limits.max_texture_dimension_2d as usize
            || mesh.width.div_ceil(WORKGROUP_X) > limits.max_compute_workgroups_per_dimension
            || mesh.height.div_ceil(WORKGROUP_Y) > limits.max_compute_workgroups_per_dimension
        {
            return Err(invalid("image or displacement field exceeds device limits"));
        }
        let src = Buffer::read(input, cancel)?;
        if skip_identity && mesh.displacement.iter().all(|d| *d == [0., 0.]) {
            return Ok(None);
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("liquify displacement"),
            size: wgpu::Extent3d {
                width: gw as u32,
                height: gh as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(&mesh.displacement),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(gw as u32 * 8),
                rows_per_image: Some(gh as u32),
            },
            texture.size(),
        );
        let view = texture.create_view(&Default::default());
        let source = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquify source"),
                contents: bytemuck::cast_slice(&src.pixels),
                usage: wgpu::BufferUsages::STORAGE,
            });
        // Upload owns its bytes now; release the full-image CPU packing buffer.
        drop(src);
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("liquify params"),
                contents: bytemuck::cast_slice(&[
                    mesh.width,
                    mesh.height,
                    mesh.cell_size,
                    u32::from(interpolation == Interpolation::Bicubic),
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquify output"),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("liquify readback"),
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("liquify"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: out.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: params.as_entire_binding(),
                },
            ],
        });
        // Pay wgpu's security zero-initialization cost once during preparation,
        // not as a hidden full-image clear in the first resident dispatch.
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.clear_buffer(&out, 0, None);
        self.queue.submit([encoder.finish()]);
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        checkpoint(cancel)?;
        Ok(Some(ResidentLiquify {
            gpu: self,
            template: input.clone(),
            cell_size: mesh.cell_size,
            texture,
            _source: source,
            _params: params,
            out,
            staging,
            bind,
            ready: false,
        }))
    }

    /// Device for constructing downstream stages that consume resident output.
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Queue shared with downstream stages; submit them after `submit_wait`.
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    fn render_inner(
        &self,
        mesh: &Mesh,
        input: &Raster,
        interpolation: Interpolation,
        cancel: &AtomicBool,
        profile: bool,
    ) -> EngineResult<(Raster, Option<std::time::Duration>)> {
        let Some(job) = self.prepare_inner(input, mesh, interpolation, cancel, true)? else {
            return Ok((input.clone(), None));
        };
        let bind = &job.bind;
        let out = &job.out;
        let staging = &job.staging;
        let size = out.size();
        checkpoint(cancel)?;
        let timing = if profile
            && self
                .device
                .features()
                .contains(wgpu::Features::TIMESTAMP_QUERY)
        {
            Some((
                self.device.create_query_set(&wgpu::QuerySetDescriptor {
                    label: Some("liquify timestamps"),
                    ty: wgpu::QueryType::Timestamp,
                    count: 2,
                }),
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("liquify timestamp resolve"),
                    size: 16,
                    usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("liquify timestamp readback"),
                    size: 16,
                    usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
                    mapped_at_creation: false,
                }),
            ))
        } else {
            None
        };
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("liquify"),
                timestamp_writes: timing.as_ref().map(|(queries, _, _)| {
                    wgpu::ComputePassTimestampWrites {
                        query_set: queries,
                        beginning_of_pass_write_index: Some(0),
                        end_of_pass_write_index: Some(1),
                    }
                }),
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(
                mesh.width.div_ceil(WORKGROUP_X),
                mesh.height.div_ceil(WORKGROUP_Y),
                1,
            );
        }
        if let Some((queries, resolve, readback)) = &timing {
            encoder.resolve_query_set(queries, 0..2, resolve, 0);
            encoder.copy_buffer_to_buffer(resolve, 0, readback, 0, 16);
        }
        encoder.copy_buffer_to_buffer(out, 0, staging, 0, size);
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |v| {
            let _ = tx.send(v);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        checkpoint(cancel)?;
        let pixels = bytemuck::cast_slice::<u8, [f32; 4]>(
            &staging.slice(..).get_mapped_range().map_err(internal)?,
        )
        .to_vec();
        staging.unmap();
        let dispatch = if let Some((_, _, readback)) = &timing {
            let (tx, rx) = std::sync::mpsc::channel();
            readback.slice(..).map_async(wgpu::MapMode::Read, move |v| {
                let _ = tx.send(v);
            });
            self.device
                .poll(wgpu::PollType::wait_indefinitely())
                .map_err(internal)?;
            rx.recv().map_err(internal)?.map_err(internal)?;
            let view = readback.slice(..).get_mapped_range().map_err(internal)?;
            let ticks: &[u64] = bytemuck::cast_slice(&view);
            let elapsed = ticks[1]
                .checked_sub(ticks[0])
                .ok_or_else(|| internal("nonmonotonic GPU timestamps"))?;
            let duration = std::time::Duration::from_secs_f64(
                elapsed as f64 * f64::from(self.queue.get_timestamp_period()) * 1e-9,
            );
            drop(view);
            readback.unmap();
            // Some Metal counter samples occasionally resolve to identical ticks.
            // Never advertise those as a zero-cost dispatch.
            (!duration.is_zero()).then_some(duration)
        } else {
            None
        };
        Ok((
            Buffer {
                w: mesh.width as usize,
                h: mesh.height as usize,
                pixels,
            }
            .write(input, cancel)?,
            dispatch,
        ))
    }
}
