//! Explicit resident transform stage, not automatic SmartObject/filter integration.
//! Geometry is prepared on the CPU in f64 and rounded to source centers in f32
//! by `TransformOp::displacement`; all pixel reconstruction stays on the GPU.
use super::ResidentRenderer;
use engine_api::{EngineError, EngineResult, tile::Extent};
use transform::{Kernel, Operation, TransformOp};
use wgpu::util::DeviceExt;

/// Immutable reusable RG32Float absolute source-center texture and precise pipeline.
/// Keep this plan while geometry, kernel, dimensions and mip level are unchanged.
/// Pixels may change without rebuilding it. Resources belong to the preparing device.
pub struct TransformPlan {
    pipeline: gpu_core::PrecisePipeline,
    field: wgpu::TextureView,
    params: wgpu::Buffer,
    source: Extent,
    output: Extent,
    level: u8,
}
impl TransformPlan {
    /// Compilation is always IEEE; unsupported devices are rejected at preparation.
    pub fn precision(&self) -> gpu_core::Precision {
        self.pipeline.precision
    }
}
fn invalid(message: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("resident transform", message.to_string())
}
fn bytes(extent: Extent) -> EngineResult<u64> {
    let n = u64::from(extent.width) * u64::from(extent.height);
    if extent.width == 0 || extent.height == 0 || n > 100_000_000 {
        return Err(invalid("nonempty extent of at most 100 MP required"));
    }
    Ok(n * 16)
}
impl ResidentRenderer {
    /// Encode from this renderer's fully rendered level directly, without readback.
    /// The plan selects the level; partial/compact/invalid levels and mismatched
    /// source dimensions are rejected. Does not replace or mutate the level cache.
    /// Caller submits on the shared queue; normal document rendering is unchanged.
    pub fn encode_transform_level(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        destination: &wgpu::Buffer,
        plan: &TransformPlan,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) -> EngineResult<()> {
        let state = self.level(plan.level)?;
        state.require_valid(crate::geom::Rect::of_extent(state.extent))?;
        if state.extent != plan.source || state.region != crate::geom::Rect::of_extent(state.extent)
        {
            return Err(invalid(
                "plan source must match the complete rendered level",
            ));
        }
        self.encode_transform_buffers(encoder, &state.out, destination, plan, timestamp_writes)
    }

    /// Prepare and upload an absolute source-center displacement texture.
    /// Input/output dimensions are at `level`; operation geometry is level zero.
    /// Free, Warp, Perspective and Puppet use their shared CPU inverse mapper.
    /// ContentAwareScale is rejected (it has no geometry-only field).
    /// Includes CPU map generation and texture upload, but no pixel uploads or GPU wait.
    /// Compile lazily once per shared compositor device; never use relaxed math.
    pub fn prepare_transform(
        &self,
        op: &TransformOp,
        source: Extent,
        output: Extent,
        level: u8,
    ) -> EngineResult<TransformPlan> {
        let limits = self.device.limits();
        for extent in [source, output] {
            if bytes(extent)? > limits.max_storage_buffer_binding_size
                || bytes(extent)? > limits.max_buffer_size
            {
                return Err(invalid("image exceeds device storage buffer limits"));
            }
        }
        if output.width > limits.max_texture_dimension_2d
            || output.height > limits.max_texture_dimension_2d
        {
            return Err(invalid("displacement exceeds device texture dimensions"));
        }
        op.validate().map_err(invalid)?;
        if let Operation::Free(t) = &op.operation {
            let scale = 2.0f64.powi(i32::from(level));
            t.bounds(
                f64::from(source.width) * scale,
                f64::from(source.height) * scale,
            )
            .map_err(invalid)?;
        }
        if !cfg!(target_os = "macos")
            || !self
                .device
                .features()
                .contains(wgpu::Features::PASSTHROUGH_SHADERS)
        {
            return Err(EngineError::Unsupported {
                what: "precise transform requires Metal passthrough shaders".into(),
            });
        }
        let kernel = match op.effective_kernel(level) {
            Kernel::Nearest => 0,
            Kernel::Bilinear => 1,
            Kernel::Bicubic | Kernel::Automatic => 2,
            Kernel::Lanczos3 => 3,
        };
        let cached = &self.pipes.transform[kernel as usize];
        if cached.get().is_none() {
            let entries: Vec<_> = (0..4)
                .map(|binding| wgpu::BindGroupLayoutEntry {
                    binding,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    count: None,
                    ty: if binding == 1 {
                        wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        }
                    } else {
                        wgpu::BindingType::Buffer {
                            ty: if binding == 3 {
                                wgpu::BufferBindingType::Uniform
                            } else {
                                wgpu::BufferBindingType::Storage {
                                    read_only: binding == 0,
                                }
                            },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        }
                    },
                })
                .collect();
            let pipeline = gpu_core::precise_compute_pipeline(
                &self.device,
                "precise resident transform",
                &include_str!("transform_gpu.wgsl").replace("params.kernel", &format!("{kernel}u")),
                "main",
                (8, 8, 1),
                &entries,
            )?;
            if pipeline.precision != gpu_core::Precision::Ieee {
                return Err(invalid("IEEE compilation required"));
            }
            let _ = cached.set(pipeline);
        }
        let coordinates = op
            .displacement(output.width as usize, output.height as usize, level)
            .map_err(invalid)?;
        let size = wgpu::Extent3d {
            width: output.width,
            height: output.height,
            depth_or_array_layers: 1,
        };
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("transform absolute source centers"),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rg32Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        self.queue.write_texture(
            texture.as_image_copy(),
            bytemuck::cast_slice(&coordinates),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(output.width * 8),
                rows_per_image: Some(output.height),
            },
            size,
        );
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("transform dimensions"),
                contents: bytemuck::cast_slice(&[
                    source.width,
                    source.height,
                    output.width,
                    output.height,
                    kernel,
                    0,
                    0,
                    0,
                ]),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        Ok(TransformPlan {
            pipeline: cached.get().unwrap().clone(),
            field: texture.create_view(&Default::default()),
            params,
            source,
            output,
            level,
        })
    }

    /// Encode reconstruction between distinct same-device resident storage buffers.
    /// Both buffers are tightly interleaved premultiplied f32 RGBA at offset zero.
    /// Preserves negative lobes and transparent extension; no clamp/unpremultiply.
    /// No submission, wait, source readback or map regeneration. Optional timestamps
    /// measure only this compute pass (map preparation/upload excluded).
    pub fn encode_transform_buffers(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Buffer,
        destination: &wgpu::Buffer,
        plan: &TransformPlan,
        timestamp_writes: Option<wgpu::ComputePassTimestampWrites<'_>>,
    ) -> EngineResult<()> {
        if source == destination {
            return Err(invalid("source and destination must not alias"));
        }
        for (buffer, extent) in [(source, plan.source), (destination, plan.output)] {
            if buffer.size() < bytes(extent)?
                || buffer.size() > self.device.limits().max_storage_buffer_binding_size
                || !buffer.usage().contains(wgpu::BufferUsages::STORAGE)
            {
                return Err(invalid(
                    "requires sufficiently large bindable storage buffers",
                ));
            }
        }
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("precise transform"),
            layout: &plan.pipeline.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: source.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&plan.field),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: destination.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: plan.params.as_entire_binding(),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("precise transform"),
            timestamp_writes,
        });
        pass.set_pipeline(&plan.pipeline.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(
            plan.output.width.div_ceil(8),
            plan.output.height.div_ceil(8),
            1,
        );
        Ok(())
    }
}
