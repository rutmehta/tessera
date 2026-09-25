use crate::GpuContext;
use engine_api::{EngineError, EngineResult, tile::Tile};
use std::sync::Arc;
use wgpu::util::DeviceExt;

/// Reusable 33³ f32 RGB output LUT executed by Metal compute, never CPU fallback.
///
/// Nodes are indexed `(blue * 33 + green) * 33 + red`. Inputs are clamped to
/// [0, 1] before trilinear interpolation. Output values are not clamped or
/// transfer-encoded: the supplied LUT defines the complete output transform.
/// No Oklab conversion from the experimental spike is applied.
/// Construct once per output profile, then reuse across tiles.
pub struct GpuOutputLut {
    context: Arc<GpuContext>,
    nodes: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
}

impl GpuOutputLut {
    /// Upload an ICC-generated `Transform::lut33()` lattice once per profile.
    pub fn from_lut(context: Arc<GpuContext>, lut: &color_mgmt::Lut3d) -> EngineResult<Self> {
        if lut.size != 33 {
            return Err(EngineError::invalid("output LUT", "33^3 lattice required"));
        }
        Self::new(context, &lut.values)
    }

    /// Render a region through the scene pipeline and this raw ICC LUT.
    /// This omits tone mapping, gamut mapping and proof identity validation;
    /// use `GpuManagedOutput` or `ManagedRenderer` for complete managed output.
    ///
    /// The LUT must take linear Rec.2020 (the renderer's working space) as input.
    /// Returns planar f32 destination-profile RGB, including the LUT's transfer
    /// encoding; do not apply the legacy display conversion to these tiles.
    /// Scene caches remain profile-independent, so switching output contexts does
    /// not require invalidation. Legacy `Renderer` display APIs are unchanged.
    ///
    /// This convenience path reads scene tiles back, then uploads/transforms each
    /// tile. It is not the zero-readback IOSurface presentation path. Use `encode`
    /// when integrating with an existing resident buffer/command encoder.
    pub fn render_region(
        &self,
        renderer: &image_core::Renderer,
        image: &image_core::RawImage,
        settings: &engine_api::recipe::DevelopSettings,
        level: u8,
        rect: image_core::PixelRect,
    ) -> EngineResult<Vec<Tile>> {
        renderer
            .render_region_as(
                image,
                settings,
                level,
                rect,
                image_core::RenderOutput::SceneLinear,
            )?
            .iter()
            .map(|tile| self.apply(tile))
            .collect()
    }

    pub fn new(context: Arc<GpuContext>, nodes: &[[f32; 3]]) -> EngineResult<Self> {
        if nodes.len() != 33 * 33 * 33 || nodes.iter().flatten().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid(
                "output LUT",
                "exactly 33^3 finite RGB nodes required",
            ));
        }
        let device = &context.device;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("output LUT"),
            source: wgpu::ShaderSource::Wgsl(include_str!("output_lut.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("output LUT"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let nodes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("33 cubed f32 output LUT"),
            contents: bytemuck::cast_slice(nodes),
            usage: wgpu::BufferUsages::STORAGE,
        });
        if let Some(error) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(format!("output LUT: {error}")));
        }
        Ok(Self {
            context,
            nodes,
            pipeline,
        })
    }

    /// Encode an output transform without submitting or reading pixels back.
    ///
    /// `src` must contain exactly three equally sized planar f32 RGB planes,
    /// with STORAGE usage. The returned f32 buffer has STORAGE | COPY_SRC usage.
    /// The caller must use this LUT's device for both buffer and encoder, and
    /// guarantee finite samples (no host scan/readback occurs here). Device
    /// ownership violations are reported by wgpu validation.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Buffer,
    ) -> EngineResult<wgpu::Buffer> {
        let ctx = &self.context;
        let size = src.size();
        let limits = ctx.device.limits();
        if size == 0
            || !size.is_multiple_of(12)
            || size > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || (size / 12).div_ceil(64) > u64::from(limits.max_compute_workgroups_per_dimension)
            || !src.usage().contains(wgpu::BufferUsages::STORAGE)
        {
            return Err(EngineError::invalid(
                "output LUT buffer",
                "planar RGB storage buffer within device limits required",
            ));
        }
        if let Some(error) = ctx.device_failure() {
            return Err(EngineError::internal(error));
        }
        let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output LUT result"),
            size,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let entries: Vec<_> = [src, &self.nodes, &dst]
            .iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("output LUT"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups((size / 12).div_ceil(64) as u32, 1, 1);
        }
        Ok(dst)
    }

    /// Transform all samples (including halos) of a planar RGB f32 tile.
    /// Performs one upload, one compute dispatch and one f32 readback.
    /// This is an explicit output API; it does not change `Op::Display`.
    pub fn apply(&self, input: &Tile) -> EngineResult<Tile> {
        let samples = input.samples::<f32>()?;
        if input.layout().channels != 3 || samples.iter().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid(
                "output LUT input",
                "finite planar RGB f32 samples required",
            ));
        }
        let ctx = &self.context;
        let size = std::mem::size_of_val(samples) as u64;
        let limits = ctx.device.limits();
        if size == 0
            || size > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || input.layout().plane_len().div_ceil(64)
                > limits.max_compute_workgroups_per_dimension as usize
        {
            return Err(EngineError::invalid(
                "output LUT input",
                "size exceeds device limits",
            ));
        }
        if let Some(error) = ctx.device_failure() {
            return Err(EngineError::internal(error));
        }
        let src = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("output LUT input"),
                contents: bytemuck::cast_slice(samples),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        let dst = self.encode(&mut encoder, &src)?;
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output LUT readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
        ctx.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = tx.send(result);
            });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| EngineError::internal(format!("output LUT poll: {e}")))?;
        rx.recv()
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(|e| EngineError::internal(e.to_string()))?;
        let result = {
            let mapped = staging
                .slice(..)
                .get_mapped_range()
                .map_err(|e| EngineError::internal(e.to_string()))?;
            Tile::from_samples(
                input.coord(),
                input.layout(),
                bytemuck::cast_slice::<u8, f32>(&mapped).to_vec(),
            )
        };
        staging.unmap();
        result
    }
}
