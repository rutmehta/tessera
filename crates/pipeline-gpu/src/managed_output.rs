//! Typed managed SDR output. CMM setup is host-side; pixels stay on GPU.
use crate::GpuContext;
use color_mgmt::GamutWarning;
use engine_api::{
    EngineError, EngineResult,
    recipe::{
        DevelopSettings,
        settings::{GamutMapping, OutputSettings},
    },
    tile::{Tile, TileLayout},
};
use pipeline_cpu::{OutputContext, OutputTarget};
use std::sync::Arc;
use wgpu::util::DeviceExt;

pub struct ManagedTile {
    pub pixels: Tile,
    /// Row-major, before gamut mapping. LUT-interpolated preview warnings.
    pub gamut_warnings: Vec<GamutWarning>,
}

/// Immutable profile/proof selection, suitable for sharing across render threads.
/// Recreate when output settings or monitor profile change. Scene settings may vary.
/// The 33³ preview approximates the CMM; exports retain the direct CPU CMM.
pub struct GpuManagedOutput {
    pub(crate) context: Arc<GpuContext>,
    pub(crate) pipeline: wgpu::ComputePipeline,
    nodes: wgpu::Buffer,
    warnings: wgpu::Buffer,
    transfer: wgpu::Buffer,
    linear_destination: bool,
    gamut_threshold: f32,
    settings: OutputSettings,
}

impl GpuManagedOutput {
    pub fn new(
        context: Arc<GpuContext>,
        settings: &DevelopSettings,
        output: &mut OutputContext<'_>,
    ) -> EngineResult<Self> {
        let mut scene = settings.clone();
        scene.output.proof_profile = None;
        pipeline_cpu::validate_settings(&scene)?;
        let transform = output.resolve(settings)?;
        let target = match output.target {
            OutputTarget::Display(p) | OutputTarget::Export(p) => p,
        };
        let linear = output.registry.linearized_rgb(target).map_err(internal)?;
        let (lut, curves) = if let Some(linear) = &linear {
            let mut linear_output = OutputContext {
                registry: &mut *output.registry,
                target: match output.target {
                    OutputTarget::Display(_) => OutputTarget::Display(linear),
                    OutputTarget::Export(_) => OutputTarget::Export(linear),
                },
                proof: output.proof,
                options: output.options,
            };
            let lut = linear_output.resolve(settings)?.lut33();
            let transfer =
                color_mgmt::Transform::new(linear, target, output.options).map_err(internal)?;
            let mut curves = Vec::with_capacity(4099);
            curves.push(transfer.apply([-1.0; 3]));
            curves.extend((0..=4096).map(|i| transfer.apply([i as f32 / 4096.0; 3])));
            curves.push(transfer.apply([2.0; 3]));
            (lut, curves)
        } else {
            (transform.lut33(), vec![[0.0; 3]])
        };
        let mut warnings = Vec::with_capacity(33 * 33 * 33);
        for b in 0..33 {
            for g in 0..33 {
                for r in 0..33 {
                    // A separate extended warning domain retains over-white and
                    // negative working colors. Threshold only after interpolation.
                    let w = transform.gamut_delta([
                        r as f32 / 8.0 - 0.5,
                        g as f32 / 8.0 - 0.5,
                        b as f32 / 8.0 - 0.5,
                    ]);
                    warnings.push([w[0], w[1], 0.0]);
                }
            }
        }
        let device = &context.device;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("managed output"),
            source: wgpu::ShaderSource::Wgsl(include_str!("managed_output.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("managed output"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffer = |label, data: &[[f32; 3]]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        // Remove an sRGB-like transfer knee before interpolation, then restore
        // it in WGSL. This reversible output shaper is also valid for other
        // ICC targets: it does not substitute a destination transfer function.
        let shaped: Vec<_> = lut
            .values
            .iter()
            .map(|rgb| {
                rgb.map(|v| {
                    if linear.is_some() {
                        v
                    } else if v <= 0.04045 {
                        v / 12.92
                    } else {
                        ((v + 0.055) / 1.055).powf(2.4)
                    }
                })
            })
            .collect();
        let nodes = buffer("managed ICC LUT", &shaped);
        let transfer = buffer("display transfer curves", &curves);
        let warnings = buffer("managed gamut LUT", &warnings);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(e.to_string()));
        }
        Ok(Self {
            context,
            pipeline,
            nodes,
            warnings,
            transfer,
            linear_destination: linear.is_some(),
            gamut_threshold: output.options.gamut_threshold,
            settings: settings.output.clone(),
        })
    }

    pub fn validate(&self, settings: &DevelopSettings) -> EngineResult<()> {
        if settings.output != self.settings {
            return Err(EngineError::invalid(
                "output",
                "settings do not match prepared ICC output",
            ));
        }
        let mut scene = settings.clone();
        scene.output.proof_profile = None;
        pipeline_cpu::validate_settings(&scene)
    }

    /// Proof identity is checked before removing its already-consumed control.
    pub fn scene_settings(&self, settings: &DevelopSettings) -> EngineResult<DevelopSettings> {
        self.validate(settings)?;
        let mut scene = settings.clone();
        scene.output.proof_profile = None;
        Ok(scene)
    }

    pub fn render_region(
        &self,
        renderer: &image_core::Renderer,
        image: &image_core::RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: image_core::PixelRect,
    ) -> EngineResult<Vec<ManagedTile>> {
        let scene = self.scene_settings(settings)?;
        renderer
            .render_region_as(
                image,
                &scene,
                level,
                rect,
                image_core::RenderOutput::SceneLinear,
            )?
            .iter()
            .map(|tile| self.apply(tile))
            .collect()
    }

    pub(crate) fn bindings(
        &self,
        src: &wgpu::Buffer,
        dst: &wgpu::Buffer,
        flags: &wgpu::Buffer,
        layout: TileLayout,
        quantize: bool,
    ) -> EngineResult<wgpu::BindGroup> {
        let stride = u64::from(layout.extent.width) + 2 * u64::from(layout.halo);
        let rows = u64::from(layout.extent.height) + 2 * u64::from(layout.halo);
        let expected_bytes = stride.checked_mul(rows).and_then(|n| n.checked_mul(12));
        if layout.channels != 3
            || expected_bytes != Some(src.size())
            || layout.extent.area() == 0
            || src.size() > self.context.device.limits().max_storage_buffer_binding_size
            || !src.usage().contains(wgpu::BufferUsages::STORAGE)
        {
            return Err(EngineError::invalid(
                "managed output",
                "planar RGB storage buffer required",
            ));
        }
        let ln_a = (0.18 * (0.18f32.powf(-1.0) - 1.0).powf(1.0 / 1.5)).ln();
        let params = self
            .context
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("managed output parameters"),
                contents: bytemuck::cast_slice(&[
                    layout.extent.width,
                    layout.extent.height,
                    u32::from(layout.halo),
                    u32::from(self.settings.gamut_mapping == GamutMapping::Perceptual),
                    u32::from(quantize),
                    ln_a.to_bits(),
                    u32::from(self.linear_destination),
                    self.gamut_threshold.to_bits(),
                ]),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let entries: Vec<_> = [
            src,
            &self.nodes,
            &self.warnings,
            dst,
            flags,
            &params,
            &self.transfer,
        ]
        .into_iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry {
            binding: binding as u32,
            resource: buffer.as_entire_binding(),
        })
        .collect();
        Ok(self
            .context
            .device
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("managed output"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &entries,
            }))
    }

    /// Resident planar scene RGB -> encoded RGB + two-bit gamut mask. No submit,
    /// pixel upload or readback. The caller supplies this context's device/encoder.
    pub fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        src: &wgpu::Buffer,
        layout: TileLayout,
    ) -> EngineResult<(wgpu::Buffer, wgpu::Buffer)> {
        let n = layout.extent.area();
        let limits = self.context.device.limits();
        if n == 0
            || n > limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
                / 12
            || n.div_ceil(64) > u64::from(limits.max_compute_workgroups_per_dimension)
        {
            return Err(EngineError::invalid(
                "managed output",
                "buffer exceeds device limits",
            ));
        }
        let buffer = |size| {
            self.context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("managed output result"),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            })
        };
        let dst = buffer(n * 12);
        let flags = buffer(n * 4);
        let group = self.bindings(src, &dst, &flags, layout, false)?;
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(n.div_ceil(64) as u32, 1, 1);
        drop(pass);
        Ok((dst, flags))
    }

    pub fn apply(&self, input: &Tile) -> EngineResult<ManagedTile> {
        let data = input.samples::<f32>()?;
        if data.iter().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid(
                "managed output",
                "finite samples required",
            ));
        }
        let ctx = &self.context;
        let src = ctx
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("managed output input"),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let mut encoder = ctx.device.create_command_encoder(&Default::default());
        let (dst, flags) = self.encode(&mut encoder, &src, input.layout())?;
        let size = dst.size() + flags.size();
        let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("managed output readback"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, dst.size());
        encoder.copy_buffer_to_buffer(&flags, 0, &staging, dst.size(), flags.size());
        ctx.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        ctx.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        rx.recv().map_err(internal)?.map_err(internal)?;
        let result = {
            let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
            let (rgb, flags) = mapped.split_at(dst.size() as usize);
            ManagedTile {
                pixels: Tile::from_samples(
                    input.coord(),
                    TileLayout {
                        halo: 0,
                        ..input.layout()
                    },
                    bytemuck::cast_slice::<u8, f32>(rgb).to_vec(),
                )?,
                gamut_warnings: bytemuck::cast_slice::<u8, u32>(flags)
                    .iter()
                    .map(|v| GamutWarning {
                        monitor: v & 1 != 0,
                        proof: v & 2 != 0,
                    })
                    .collect(),
            }
        };
        staging.unmap();
        Ok(result)
    }
}
/// Renderer with an immutable output selection. Owns separate output caches so
/// ICC identities cannot collide with legacy sRGB or another prepared context.
pub struct ManagedRenderer {
    output: Arc<GpuManagedOutput>,
    renderer: image_core::Renderer,
    ops: Arc<crate::GpuStageOp>,
}
impl ManagedRenderer {
    pub fn new(output: Arc<GpuManagedOutput>, config: image_core::RendererConfig) -> Self {
        Self::build(output, config, false)
    }

    /// Managed, unquantized resident output for CPU encoders. No display cache
    /// or surface presentation may be used through this instance.
    pub fn new_export(output: Arc<GpuManagedOutput>, config: image_core::RendererConfig) -> Self {
        Self::build(output, config, true)
    }

    /// Lanczos-3 filtering stays resident, including its horizontal halo.
    pub fn new_export_resized(
        output: Arc<GpuManagedOutput>,
        config: image_core::RendererConfig,
        resize: crate::ExportResize,
    ) -> Self {
        Self::build_resized(output, config, true, Some(resize))
    }

    fn build(
        output: Arc<GpuManagedOutput>,
        config: image_core::RendererConfig,
        export: bool,
    ) -> Self {
        Self::build_resized(output, config, export, None)
    }

    /// An export renderer whose resident transactions allocate at most
    /// `scratch` bytes (its share of the device budget); larger requests fail
    /// as unsupported so the caller can fall back or use smaller bands.
    pub fn new_export_budgeted(
        output: Arc<GpuManagedOutput>,
        config: image_core::RendererConfig,
        resize: Option<crate::ExportResize>,
        scratch: u64,
    ) -> Self {
        Self::build_with(output, config, true, resize, scratch)
    }

    fn build_resized(
        output: Arc<GpuManagedOutput>,
        config: image_core::RendererConfig,
        export: bool,
        resize: Option<crate::ExportResize>,
    ) -> Self {
        Self::build_with(output, config, export, resize, 512 << 20)
    }

    fn build_with(
        output: Arc<GpuManagedOutput>,
        config: image_core::RendererConfig,
        export: bool,
        resize: Option<crate::ExportResize>,
        scratch: u64,
    ) -> Self {
        let mut ops =
            crate::GpuStageOp::with_cache_budget(output.context.clone(), config.cache_budget_bytes);
        ops.managed_output = Some(output.clone());
        ops.export_float = export;
        ops.export_resize = resize;
        ops.export_scratch = scratch;
        let ops = Arc::new(ops);
        let renderer = image_core::Renderer::with_ops(
            ops.clone(),
            Arc::new(image_core::TileCache::new(config.cache_budget_bytes)),
            config,
        );
        Self {
            output,
            renderer,
            ops,
        }
    }

    /// The renderer for another export band with its own resize request:
    /// shares the compiled pipelines, device and output (no recompilation).
    /// Resident memo caches are fresh (export renderers do not memoize).
    pub fn export_band(&self, resize: Option<crate::ExportResize>) -> Self {
        let mut ops = (*self.ops).clone();
        ops.export_resize = resize;
        ops.resident_cache = crate::resident::cache(self.renderer.config().cache_budget_bytes);
        ops.recycled = Arc::default();
        let ops = Arc::new(ops);
        let config = self.renderer.config().clone();
        let renderer = image_core::Renderer::with_ops(
            ops.clone(),
            Arc::new(image_core::TileCache::new(config.cache_budget_bytes)),
            config,
        );
        Self {
            output: self.output.clone(),
            renderer,
            ops,
        }
    }

    pub fn render_region(
        &self,
        image: &image_core::RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: image_core::PixelRect,
    ) -> EngineResult<Vec<Tile>> {
        if self.ops.export_float {
            return Err(EngineError::invalid(
                "renderer",
                "use render_export for float output",
            ));
        }
        let scene = self.output.scene_settings(settings)?;
        self.renderer.render_region(image, &scene, level, rect)
    }

    /// One resident submission/readback. None means the caller must use its
    /// reference path; unsupported operators never silently lose precision.
    pub fn render_export(
        &self,
        image: &image_core::RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: image_core::PixelRect,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<Option<Vec<Tile>>> {
        if !self.ops.export_float {
            return Err(EngineError::invalid(
                "renderer",
                "float export renderer required",
            ));
        }
        let scene = self.output.scene_settings(settings)?;
        self.renderer
            .render_resident_region(image, &scene, level, rect, cancel)
    }

    /// [`ManagedRenderer::render_export`] with resolved lens corrections and
    /// geometry. With a map, `rect` addresses the mapped output frame.
    pub fn render_export_lens(
        &self,
        image: &image_core::RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: image_core::PixelRect,
        lens: &pipeline_cpu::LensPlan,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<Option<Vec<Tile>>> {
        if !self.ops.export_float {
            return Err(EngineError::invalid(
                "renderer",
                "float export renderer required",
            ));
        }
        let scene = self.output.scene_settings(settings)?;
        self.renderer
            .render_resident_lens(image, &scene, level, rect, Some(lens), cancel)
    }

    /// Uses the resident ICC Output kernel and existing IOSurface writer.
    /// Returns false for scene operators without a resident implementation.
    pub fn render_to_surface(
        &self,
        image: &image_core::RawImage,
        settings: &DevelopSettings,
        level: u8,
        surface: u32,
        cancel: &engine_api::jobs::CancellationToken,
    ) -> EngineResult<bool> {
        if self.ops.export_float {
            return Err(EngineError::invalid(
                "renderer",
                "export renderer cannot present",
            ));
        }
        let scene = self.output.scene_settings(settings)?;
        self.renderer
            .render_to_surface(image, &scene, level, surface, cancel)
    }

    pub fn stats(&self) -> crate::GpuStats {
        self.ops.stats()
    }
}

fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
