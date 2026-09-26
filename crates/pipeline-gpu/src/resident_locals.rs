//! Procedural local adjustments on full-frame, scene-linear Rec.2020 buffers.
use crate::{GpuContext, GpuStageOp, ResidentBuffer};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    recipe::{
        mask::{LocalAdjustment, MaskKind},
        settings::{ColorSettings, ToneSettings},
    },
    tile::{Extent, TileCoord, TileLayout},
};
use image_core::{Op, StageOp};
use std::sync::Arc;
use wgpu::util::DeviceExt;

fn invalid(reason: &str) -> EngineError {
    EngineError::invalid("resident locals", reason)
}
impl GpuContext {
    /// Apply local groups against the immutable input and sum masked deltas.
    /// Input must be finite, tightly packed planar f32 RGB, with STORAGE |
    /// COPY_SRC usage on this context's device. The caller owns finite-sample
    /// validation: no image pixels are mapped or read back by this method.
    /// Output has independent storage even for an empty collection. Commands
    /// are submitted on the shared queue before returning; retain the returned
    /// guard until consuming GPU work completes. No CPU fallback is performed.
    /// Point/detail work is row-tiled; local tone and mask blending still need
    /// full-frame buffers fitting the device storage binding limit. This is
    /// not a constant-memory streaming API.
    pub fn locals_resident(
        self: &Arc<Self>,
        input: &wgpu::Buffer,
        extent: Extent,
        groups: &[LocalAdjustment],
    ) -> EngineResult<ResidentBuffer> {
        let layout = TileLayout {
            extent,
            halo: 0,
            channels: 3,
        };
        let coord = TileCoord::new(0, 0, 0);
        let base = self.import_resident_buffer(coord, layout, input)?;
        let gpu = GpuStageOp::new(self.clone());
        let mut batch = gpu
            .begin_resident()
            .ok_or_else(|| invalid("resident backend unavailable"))?;
        if !batch.supports_output_level(extent) {
            return Err(invalid("extent exceeds resident storage limits"));
        }
        let output = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident locals output"),
            size: layout.len() as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut copy = self.device.create_command_encoder(&Default::default());
        copy.copy_buffer_to_buffer(input, 0, &output, 0, output.size());
        self.queue.submit([copy.finish()]);
        for group in groups
            .iter()
            .filter(|g| g.enabled && g.amount != 0.0 && !g.components.is_empty())
        {
            if !group.amount.is_finite() || !(0.0..=200.0).contains(&group.amount) {
                return Err(invalid("invalid amount"));
            }
            let p = &group.params;
            let values = [
                p.exposure,
                p.contrast,
                p.highlights,
                p.shadows,
                p.whites,
                p.blacks,
                p.temperature,
                p.tint,
                p.texture,
                p.clarity,
                p.dehaze,
                p.saturation,
                p.sharpness,
                p.noise,
                p.moire,
                p.hue,
                p.defringe,
            ];
            if values.iter().any(|v| !v.is_finite()) {
                return Err(invalid("parameters must be finite"));
            }
            if p.defringe != 0.0 || p.color_overlay.is_some() {
                return Err(invalid("defringe and colour overlay are not implemented"));
            }
            let data = mask_parameters(group, extent)?;
            let mut initial = base.clone();
            if p.temperature != 0.0 || p.tint != 0.0 {
                use engine_api::color::{ChromaticAdaptation, WorkingSpace};
                let slider = |v: f32| (v * (group.amount / 100.)).clamp(-100., 100.);
                let work = WorkingSpace::LinearRec2020.to_xyz();
                let source = pipeline_cpu::temperature_white(6504., 0.)?;
                let target = pipeline_cpu::temperature_white(
                    6504. * (-slider(p.temperature) / 100.).exp2(),
                    -slider(p.tint),
                )?;
                initial = tiled_run(
                    self,
                    &gpu,
                    extent,
                    0,
                    &Op::Matrix(
                        work.inverse()? * ChromaticAdaptation::Cat16.matrix(source, target)? * work,
                    ),
                    &initial,
                )?;
            }
            let mut adjusted = tiled_run(
                self,
                &gpu,
                extent,
                0,
                &Op::Tone(&ToneSettings {
                    exposure: (p.exposure * (group.amount / 100.)).clamp(-10., 10.),
                    contrast: (p.contrast * (group.amount / 100.)).clamp(-100., 100.),
                    highlights: (p.highlights * (group.amount / 100.)).clamp(-100., 100.),
                    shadows: (p.shadows * (group.amount / 100.)).clamp(-100., 100.),
                    whites: (p.whites * (group.amount / 100.)).clamp(-100., 100.),
                    blacks: (p.blacks * (group.amount / 100.)).clamp(-100., 100.),
                    ..Default::default()
                }),
                &initial,
            )?;
            if p.texture != 0.0 || p.clarity != 0.0 || p.dehaze != 0.0 {
                use engine_api::{
                    id::ImageId,
                    stage::{MemoKey, ParamHash, StageId},
                };
                use image_core::resident::LocalToneOptions;
                let slider = |v: f32| (v * (group.amount / 100.)).clamp(-100., 100.);
                let tone = ToneSettings {
                    texture: slider(p.texture),
                    clarity: slider(p.clarity),
                    dehaze: slider(p.dehaze),
                    ..Default::default()
                };
                let tiles = std::collections::HashMap::from([(coord, adjusted)]);
                adjusted = batch
                    .local_tone(
                        &tone,
                        extent,
                        &tiles,
                        &[coord],
                        &LocalToneOptions {
                            preview: false,
                            statistics_key: MemoKey {
                                image_id: ImageId(0),
                                stage: StageId::Tone,
                                params_hash: ParamHash::of(StageId::Tone, &0u64),
                                tile: coord,
                            },
                        },
                    )?
                    .remove(&coord)
                    .ok_or_else(|| invalid("missing local tone output"))?;
            }
            batch.finish(vec![], false, None, &CancellationToken::new())?;
            batch = gpu.begin_resident().unwrap();
            if p.saturation != 0.0 {
                adjusted = tiled_run(
                    self,
                    &gpu,
                    extent,
                    0,
                    &Op::Color(&ColorSettings {
                        saturation: (p.saturation * (group.amount / 100.)).clamp(-100., 100.),
                        ..Default::default()
                    }),
                    &adjusted,
                )?;
            }
            let adjusted = self.resident_buffer(&adjusted)?;
            batch.finish(vec![], false, None, &CancellationToken::new())?;
            let hue_buffer;
            let adjusted_buffer = if p.hue != 0.0 {
                hue_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resident local hue"),
                    size: output.size(),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let angle =
                    (f64::from(p.hue) * f64::from(group.amount / 100.)).rem_euclid(360.) as f32;
                let (sin, cos) = angle.to_radians().sin_cos();
                dispatch(
                    self,
                    "transform",
                    &[adjusted.buffer(), adjusted.buffer(), &hue_buffer],
                    &[extent.width as f32, extent.height as f32, 0., sin, cos],
                    extent.width * extent.height,
                )?;
                &hue_buffer
            } else {
                adjusted.buffer()
            };
            let mut detail_tile = self.import_resident_buffer(coord, layout, adjusted_buffer)?;
            let slider = |v: f32| (v * (group.amount / 100.)).clamp(-100., 100.);
            for (sharpness, noise, reverse) in [
                (
                    slider(p.sharpness).max(0.),
                    (-slider(p.sharpness)).max(0.),
                    false,
                ),
                (0., slider(p.noise).abs(), p.noise < 0.),
            ] {
                if sharpness == 0. && noise == 0. {
                    continue;
                }
                let mut detail = engine_api::recipe::settings::DetailSettings::default();
                detail.sharpening.amount = sharpness;
                detail.noise_reduction.color = 0.;
                detail.noise_reduction.luminance = noise;
                let filtered = tiled_run(
                    self,
                    &gpu,
                    extent,
                    pipeline_cpu::detail_halo(&detail),
                    &Op::Detail(&detail),
                    &detail_tile,
                )?;
                let filtered_buffer = self.resident_buffer(&filtered)?;
                if reverse {
                    let previous = self.resident_buffer(&detail_tile)?;
                    let reversed = self.device.create_buffer(&wgpu::BufferDescriptor {
                        label: Some("negative local noise"),
                        size: output.size(),
                        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                        mapped_at_creation: false,
                    });
                    dispatch(
                        self,
                        "transform",
                        &[previous.buffer(), filtered_buffer.buffer(), &reversed],
                        &[extent.width as f32, extent.height as f32, 1., 0., 0.],
                        extent.width * extent.height,
                    )?;
                    detail_tile = self.import_resident_buffer(coord, layout, &reversed)?;
                } else {
                    detail_tile = filtered;
                }
            }
            let detail_output = self.resident_buffer(&detail_tile)?;
            dispatch(
                self,
                "blend",
                &[input, detail_output.buffer(), &output],
                &data,
                extent.width * extent.height,
            )?;
            batch = gpu.begin_resident().unwrap();
        }
        batch.finish(vec![], false, None, &CancellationToken::new())?;
        debug_assert_eq!(gpu.stats().uploads, 0);
        debug_assert_eq!(gpu.stats().readbacks, 0);
        debug_assert_eq!(gpu.stats().pixel_readback_bytes, 0);
        let result = self.import_resident_buffer(coord, layout, &output)?;
        self.resident_buffer(&result)
    }
}

/// Run f32-indexed operators only on bounded row tiles. Spatial tiles gather
/// their complete real halo from the immutable full-frame source, never from
/// an already filtered neighbour. Only the image boundary replicates samples.
/// Integer-offset GPU copies assemble halo-free results for local tone/masks.
fn tiled_run(
    ctx: &Arc<GpuContext>,
    gpu: &GpuStageOp,
    extent: Extent,
    halo: u16,
    op: &Op<'_>,
    source: &image_core::resident::ResidentTile,
) -> EngineResult<image_core::resident::ResidentTile> {
    let layout = TileLayout {
        extent,
        halo: 0,
        channels: 3,
    };
    let output = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("resident locals assembled tiles"),
        size: layout.len() as u64 * 4,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut rows = extent.height.min(128);
    let probe = gpu
        .begin_resident()
        .ok_or_else(|| invalid("resident backend unavailable"))?;
    while !probe.supports_level(Extent::new(extent.width, rows), halo) {
        if rows == 1 {
            return Err(invalid("row tile exceeds operator limits"));
        }
        rows = rows.div_ceil(2);
    }
    drop(probe);
    for start in (0..extent.height).step_by(rows as usize) {
        let end = (start + rows).min(extent.height);
        let mut batch = gpu.begin_resident().unwrap();
        let tile = batch.gather_rows(extent, source, 0, start..end, halo, 1)?;
        let filtered = batch.run(op, &tile)?;
        let buffer = ctx.resident_buffer(&filtered)?;
        batch.finish(vec![], false, None, &CancellationToken::new())?;
        // Operators return tightly packed, halo-free interiors.
        let band_bytes = u64::from(extent.width) * u64::from(end - start) * 4;
        let plane_bytes = extent.area() * 4;
        let mut copy = ctx.device.create_command_encoder(&Default::default());
        for c in 0..3u64 {
            copy.copy_buffer_to_buffer(
                buffer.buffer(),
                c * band_bytes,
                &output,
                c * plane_bytes + u64::from(start) * u64::from(extent.width) * 4,
                band_bytes,
            );
        }
        ctx.queue.submit([copy.finish()]);
    }
    ctx.import_resident_buffer(TileCoord::new(0, 0, 0), layout, &output)
}

// Only recipe geometry/sample metadata is uploaded, never host image pixels.
fn mask_parameters(group: &LocalAdjustment, extent: Extent) -> EngineResult<Vec<f32>> {
    use engine_api::recipe::mask::MaskCombine;
    let bounded = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    let coords = |v: &[f32]| v.iter().all(|&v| bounded(v, -16., 16.));
    let mut data = vec![
        extent.width as f32,
        extent.height as f32,
        group.components.len() as f32,
        f32::from(group.invert),
    ];
    let mut stamps = 0usize;
    for c in &group.components {
        let start = data.len();
        data.extend([
            0.,
            match c.combine {
                MaskCombine::Add => 0.,
                MaskCombine::Subtract => 1.,
                MaskCombine::Intersect => 2.,
            },
            f32::from(c.invert),
            0.,
        ]);
        match &c.kind {
            MaskKind::Linear { start: a, end: b } => {
                if !coords(a) || !coords(b) || (b[0] - a[0]).hypot(b[1] - a[1]) < 1e-6 {
                    return Err(invalid("invalid linear mask"));
                }
                data.extend([a[0], a[1], b[0], b[1]]);
            }
            MaskKind::Radial {
                center,
                radii,
                angle,
                feather,
            } => {
                if !coords(center)
                    || !radii.iter().all(|&v| bounded(v, 1e-6, 16.))
                    || !angle.is_finite()
                    || !bounded(*feather, 0., 100.)
                {
                    return Err(invalid("invalid radial mask"));
                }
                data[start] = 1.;
                let (sin, cos) = angle.to_radians().sin_cos();
                data.extend([
                    center[0],
                    center[1],
                    radii[0],
                    radii[1],
                    sin,
                    cos,
                    *feather / 100.,
                ]);
            }
            MaskKind::LuminanceRange { range, smoothness } => {
                if !bounded(range[0], 0., 1.)
                    || !bounded(range[1], range[0], 1.)
                    || !bounded(*smoothness, 0., 100.)
                {
                    return Err(invalid("invalid luminance mask"));
                }
                data[start] = 2.;
                data.extend([range[0], range[1], *smoothness / 200.]);
            }
            MaskKind::ColorRange { samples, amount } => {
                if !bounded(*amount, 0., 100.) || samples.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(invalid("invalid color range"));
                }
                data[start] = 3.;
                data.extend([*amount / 100., samples.len() as f32]);
                data.extend(samples.iter().flatten());
            }
            MaskKind::Brush { strokes } => {
                data[start] = 4.;
                data.push(0.);
                let count_at = data.len() - 1;
                let before = stamps;
                for stroke in strokes {
                    if !bounded(stroke.radius, 1e-6, 16.)
                        || !bounded(stroke.feather, 0., 100.)
                        || !bounded(stroke.flow, 0., 100.)
                        || stroke
                            .points
                            .iter()
                            .any(|p| !coords(&p[..2]) || !bounded(p[2], 0., 1.))
                    {
                        return Err(invalid("invalid brush"));
                    }
                    let radius = stroke.radius * extent.width as f32;
                    let mut stamp = |p: [f32; 3]| -> EngineResult<()> {
                        stamps += 1;
                        if stamps > 1_000_000 {
                            return Err(invalid("brush interpolation exceeds one million stamps"));
                        }
                        data.extend([
                            p[0] * extent.width as f32,
                            p[1] * extent.height as f32,
                            p[2],
                            radius,
                            stroke.feather / 100.,
                            stroke.flow / 100.,
                            f32::from(stroke.erase),
                        ]);
                        Ok(())
                    };
                    if let Some(&p) = stroke.points.first() {
                        stamp(p)?;
                    }
                    for pair in stroke.points.windows(2) {
                        let [a, b] = [pair[0], pair[1]];
                        let distance = ((b[0] - a[0]) * extent.width as f32)
                            .hypot((b[1] - a[1]) * extent.height as f32);
                        let steps = (distance / (radius * 0.25).max(0.25)).ceil().max(1.) as usize;
                        for step in 1..=steps {
                            let t = step as f32 / steps as f32;
                            stamp(std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t))?;
                        }
                    }
                }
                data[count_at] = (stamps - before) as f32;
            }
            _ => {
                return Err(invalid(
                    "AI/depth masks require external rasters and are unsupported",
                ));
            }
        }
        data[start + 3] = (data.len() - start) as f32;
        if data.len() > 16_777_216 {
            return Err(invalid("mask metadata exceeds exact index range"));
        }
    }
    Ok(data)
}

fn dispatch(
    ctx: &GpuContext,
    entry: &str,
    buffers: &[&wgpu::Buffer],
    params: &[f32],
    n: u32,
) -> EngineResult<()> {
    if params.len() as u64 * 4 > ctx.device.limits().max_storage_buffer_binding_size {
        return Err(invalid("mask metadata exceeds storage binding limit"));
    }
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("resident locals"),
            source: wgpu::ShaderSource::Wgsl(include_str!("resident_locals.wgsl").into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("resident locals"),
            layout: Some(
                &ctx.device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: None,
                        bind_group_layouts: &[Some(
                            &ctx.device.create_bind_group_layout(
                                &wgpu::BindGroupLayoutDescriptor {
                                    label: None,
                                    entries: &(0..4)
                                        .map(|binding| wgpu::BindGroupLayoutEntry {
                                            binding,
                                            visibility: wgpu::ShaderStages::COMPUTE,
                                            ty: wgpu::BindingType::Buffer {
                                                ty: wgpu::BufferBindingType::Storage {
                                                    read_only: binding != 2,
                                                },
                                                has_dynamic_offset: false,
                                                min_binding_size: None,
                                            },
                                            count: None,
                                        })
                                        .collect::<Vec<_>>(),
                                },
                            ),
                        )],
                        immediate_size: 0,
                    }),
            ),
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
    let params = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident locals parameters"),
            contents: bytemuck::cast_slice(params),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let mut entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    entries.push(wgpu::BindGroupEntry {
        binding: 3,
        resource: params.as_entire_binding(),
    });
    let binding = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &binding, &[]);
        let groups = n.div_ceil(256);
        pass.dispatch_workgroups(groups.min(65535), groups.div_ceil(65535), 1);
    }
    ctx.queue.submit([encoder.finish()]);
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(invalid(&e.to_string()));
    }
    Ok(())
}
