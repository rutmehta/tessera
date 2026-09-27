//! Region-local GPU counterpart of `render::styles` at the requested level.
//!
//! Geometry stays resident: padded alpha, separable square morphology, Gaussian
//! convolution, bilinear offsets, and bevel lighting are compute passes. Gaussian
//! weights and light vectors deliberately use the CPU reference's f32 formulas.
//! Solid paints are uniforms; gradients upload stops and patterns upload texels.
//!
//! The caller must provide finite source alpha (as for other resident stages).
//! Unlike the CPU renderer this asynchronous path cannot report non-finite input
//! samples without a GPU readback. Contour/jitter remain metadata, like the CPU.
use crate::{
    blend::BlendMode,
    document::{Fill, GradientKind},
    render::styles::{BevelKind, GlobalLight, LayerStyles, StrokePosition, StyleEffect},
};
use engine_api::{EngineError, EngineResult, tile::Extent};
use wgpu::util::DeviceExt;

/// A region-local straight RGBA effect. Opacity is NOT baked into its pixels.
pub(super) struct GpuStylePlane {
    pub pixels: wgpu::Buffer,
    pub mode: BlendMode,
    pub opacity: f32,
    pub outside: bool,
    pub stroke: bool,
}

pub(super) struct StylesGpu {
    pipeline: wgpu::ComputePipeline,
}

// All fields are vec4-sized, matching uniform-buffer alignment in styles.wgsl.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    dims: [u32; 4],
    control: [u32; 4],
    flags: [u32; 4],
    geometry: [f32; 4],
    color: [f32; 4],
    lighting: [f32; 4],
    relief: [f32; 4],
    paint: [f32; 4],
    origin: [u32; 4],
}

/// Owns a command batch for one effect. Submitting between effects lets the
/// backend retire intermediate masks without retaining an entire style stack.
struct Render<'a> {
    gpu: &'a StylesGpu,
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: Option<wgpu::CommandEncoder>,
    base: Params,
    dummy: wgpu::Buffer,
}

impl StylesGpu {
    /// Exact retained output expansion. Enabled zero-opacity/zero-size effects
    /// still emit planes; only disabled effects are absent from the GPU stack.
    pub(super) fn plane_count(styles: &LayerStyles) -> usize {
        styles
            .effects
            .iter()
            .map(|effect| match effect {
                StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => usize::from(s.enabled),
                StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => usize::from(s.enabled),
                StyleEffect::Overlay(s)
                | StyleEffect::ColorOverlay(s)
                | StyleEffect::GradientOverlay(s)
                | StyleEffect::PatternOverlay(s) => usize::from(s.enabled),
                StyleEffect::Stroke(s) => usize::from(s.enabled),
                StyleEffect::Satin(s) => usize::from(s.enabled),
                StyleEffect::Bevel(s) => {
                    if !s.enabled {
                        0
                    } else {
                        match s.kind {
                            BevelKind::Inner | BevelKind::Outer => 2,
                            BevelKind::Emboss | BevelKind::Pillow => 4,
                        }
                    }
                }
            })
            .sum()
    }

    pub(super) fn new(device: &wgpu::Device) -> EngineResult<Self> {
        let entries = (0..6)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                count: None,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 5 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 4,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
            })
            .collect::<Vec<_>>();
        Ok(Self {
            pipeline: gpu_core::precise_compute_pipeline(
                device,
                "resident layer effects",
                include_str!("styles.wgsl"),
                "main",
                (8, 8, 1),
                &entries,
            )?
            .pipeline,
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[cfg(test)]
    pub(super) fn render(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: Extent,
        styles: &LayerStyles,
        light: GlobalLight,
        origin: [u32; 2],
    ) -> EngineResult<Vec<GpuStylePlane>> {
        self.render_at(device, queue, input, extent, styles, light, origin, 0)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn render_at(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        input: &wgpu::Buffer,
        extent: Extent,
        styles: &LayerStyles,
        light: GlobalLight,
        origin: [u32; 2],
        level: u8,
    ) -> EngineResult<Vec<GpuStylePlane>> {
        styles.validate()?;
        light.validate()?;
        if styles.effects.is_empty() || extent.width == 0 || extent.height == 0 {
            return Ok(Vec::new());
        }
        let pad = (styles.effects.iter().map(support).fold(0.0, f32::max) * styles.scale).ceil()
            as u32
            + 2;
        let w = extent
            .width
            .checked_add(2 * pad)
            .ok_or_else(|| invalid("padded width overflow"))?;
        let h = extent
            .height
            .checked_add(2 * pad)
            .ok_or_else(|| invalid("padded height overflow"))?;

        // Bound only the region actually computed, never the document canvas.
        if u64::from(w) * u64::from(h) > 16_777_216 {
            return Err(invalid("style alpha region exceeds pixel limit"));
        }

        let canvas_bytes = extent
            .area()
            .checked_mul(16)
            .ok_or_else(|| invalid("canvas byte size overflow"))?;
        check_size(device, canvas_bytes)?;
        check_size(device, u64::from(w) * u64::from(h) * 4)?;
        if input.size() < canvas_bytes || !input.usage().contains(wgpu::BufferUsages::STORAGE) {
            return Err(invalid(
                "styles require a full-canvas straight RGBA storage buffer",
            ));
        }
        check_size(device, input.size())?;
        if w.div_ceil(8) > device.limits().max_compute_workgroups_per_dimension
            || h.div_ceil(8) > device.limits().max_compute_workgroups_per_dimension
        {
            return Err(EngineError::ResourceExhausted {
                resource: "style canvas exceeds compute dispatch limit".into(),
            });
        }
        let base = Params {
            dims: [w, h, extent.width, extent.height],
            control: [pad, 0, 0, 0],
            flags: [0; 4],
            geometry: [0.0; 4],
            color: [0.0; 4],
            lighting: [0.0; 4],
            relief: [0.0; 4],
            paint: [0.0; 4],
            origin: [origin[0], origin[1], 0, 1u32 << level],
        };
        let mut run = Render {
            gpu: self,
            device,
            queue,
            encoder: None,
            base,
            dummy: device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("style unused input"),
                contents: bytemuck::cast_slice(&[0.0f32; 4]),
                usage: wgpu::BufferUsages::STORAGE,
            }),
        };
        let dummy = run.dummy.clone();
        let alpha = run.dispatch(base, [input, &dummy, &dummy, &dummy])?;
        run.submit();
        let mut effects: Vec<_> = styles.effects.iter().collect();
        effects.sort_by_key(|e| rank(e));
        let mut planes = Vec::new();
        let sc = styles.scale;
        for effect in effects {
            let mut p = base;
            p.control[1] = 4; // emit a straight RGBA plane
            match effect {
                StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) if s.enabled => {
                    let inner = matches!(effect, StyleEffect::InnerShadow(_));
                    let field = run.morphology(&alpha, s.spread * sc, !inner)?;
                    let field = run.blur(&field, s.size * sc)?;
                    let (dx, dy) = offset(
                        if s.use_global_light {
                            light.angle
                        } else {
                            s.angle
                        },
                        s.distance * sc,
                    );
                    p.flags[1] = u32::from(inner);
                    p.geometry[1] = dx;
                    p.geometry[2] = dy;
                    p.color = s.color;
                    planes.push(run.emit(
                        p,
                        [&alpha, &field, &dummy, &dummy],
                        s.mode,
                        s.opacity,
                        !inner,
                        false,
                    )?);
                }
                StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) if s.enabled => {
                    let inner = matches!(effect, StyleEffect::InnerGlow(_));
                    let field = run.morphology(&alpha, s.spread * sc, !inner)?;
                    let field = run.blur(&field, s.size * sc)?;
                    p.flags[1] = if !inner {
                        2
                    } else if s.center {
                        4
                    } else {
                        3
                    };
                    p.color = s.color;
                    planes.push(run.emit(
                        p,
                        [&alpha, &field, &dummy, &dummy],
                        s.mode,
                        s.opacity,
                        !inner,
                        false,
                    )?);
                }
                StyleEffect::Overlay(s)
                | StyleEffect::ColorOverlay(s)
                | StyleEffect::GradientOverlay(s)
                | StyleEffect::PatternOverlay(s)
                    if s.enabled =>
                {
                    p.flags[1] = 5;
                    let paint = run.paint(&s.fill, &mut p)?;
                    planes.push(run.emit(
                        p,
                        [&alpha, &dummy, &dummy, &paint],
                        s.mode,
                        s.opacity,
                        false,
                        false,
                    )?);
                }
                StyleEffect::Stroke(s) if s.enabled => {
                    let r = s.size * sc;
                    let (outer, inner) = match s.position {
                        StrokePosition::Outside => {
                            (run.morphology(&alpha, r, true)?, alpha.clone())
                        }
                        StrokePosition::Inside => {
                            (alpha.clone(), run.morphology(&alpha, r, false)?)
                        }
                        StrokePosition::Center => (
                            run.morphology(&alpha, r * 0.5, true)?,
                            run.morphology(&alpha, r * 0.5, false)?,
                        ),
                    };
                    p.flags[1] = 6;
                    let paint = run.paint(&s.fill, &mut p)?;
                    planes.push(run.emit(
                        p,
                        [&alpha, &outer, &inner, &paint],
                        s.mode,
                        s.opacity,
                        s.position == StrokePosition::Outside,
                        true,
                    )?);
                }
                StyleEffect::Satin(s) if s.enabled => {
                    let field = run.blur(&alpha, s.size * sc)?;
                    let (dx, dy) = offset(s.angle, s.distance * sc);
                    p.flags[1] = 7;
                    p.flags[2] = u32::from(s.invert);
                    p.geometry[1] = dx;
                    p.geometry[2] = dy;
                    p.color = s.color;
                    planes.push(run.emit(
                        p,
                        [&alpha, &field, &dummy, &dummy],
                        s.mode,
                        s.opacity,
                        false,
                        false,
                    )?);
                }
                StyleEffect::Bevel(s) if s.enabled => {
                    let height = run.blur(&alpha, s.size * sc)?;
                    let height = run.blur(&height, s.soften * sc)?;
                    let l = if s.use_global_light {
                        light
                    } else {
                        GlobalLight {
                            angle: s.angle,
                            elevation: s.elevation,
                        }
                    };
                    let angle = l.angle.rem_euclid(360.0).to_radians();
                    let elevation = l.elevation.to_radians();
                    p.lighting = [
                        angle.cos() * elevation.cos(),
                        -angle.sin() * elevation.cos(),
                        elevation.sin(),
                        s.depth * s.size * sc,
                    ];
                    let outer = run.morphology(&alpha, s.size * sc, true)?;
                    let inner = run.morphology(&alpha, s.size * sc, false)?;
                    p.flags[1] = 8;
                    for outside in [true, false] {
                        if (outside && s.kind == BevelKind::Inner)
                            || (!outside && s.kind == BevelKind::Outer)
                        {
                            continue;
                        }
                        p.flags[2] = u32::from(outside);
                        p.relief[0] = if s.down { -1.0 } else { 1.0 }
                            * if s.kind == BevelKind::Pillow && !outside {
                                -1.0
                            } else {
                                1.0
                            };
                        for highlight in [false, true] {
                            let (color, mode, opacity) = if highlight {
                                (s.highlight_color, s.highlight_mode, s.highlight_opacity)
                            } else {
                                (s.shadow_color, s.shadow_mode, s.shadow_opacity)
                            };
                            p.color = color;
                            p.flags[3] = u32::from(highlight);
                            planes.push(run.emit(
                                p,
                                [
                                    &alpha,
                                    &height,
                                    if outside { &outer } else { &inner },
                                    &dummy,
                                ],
                                mode,
                                opacity,
                                outside,
                                false,
                            )?);
                        }
                    }
                }
                _ => {}
            }
            run.submit();
        }
        // Both sorts are stable: equal-kind insertion order and bevel's
        // shadow-before-highlight order survive the outside/inside partition.
        planes.sort_by_key(|plane| !plane.outside);
        Ok(planes)
    }
}

impl Render<'_> {
    fn submit(&mut self) {
        if let Some(encoder) = self.encoder.take() {
            self.queue.submit([encoder.finish()]);
        }
    }

    fn dispatch(&mut self, p: Params, inputs: [&wgpu::Buffer; 4]) -> EngineResult<wgpu::Buffer> {
        let (width, height, channels) = if p.control[1] == 4 {
            (p.dims[2], p.dims[3], 4u64)
        } else {
            (p.dims[0], p.dims[1], 1u64)
        };
        let size = u64::from(width) * u64::from(height) * channels * 4;
        check_size(self.device, size)?;
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("resident style field/plane"),
            size,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_SRC
                | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("style parameters"),
                contents: bytemuck::bytes_of(&p),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let buffers = [inputs[0], inputs[1], inputs[2], inputs[3], &out, &uniform];
        let entries = buffers
            .iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: buffer.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("style inputs"),
            layout: &self.gpu.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let encoder = self
            .encoder
            .get_or_insert_with(|| self.device.create_command_encoder(&Default::default()));
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.gpu.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        Ok(out)
    }

    fn integer_morphology(
        &mut self,
        alpha: &wgpu::Buffer,
        radius: u32,
        dilate: bool,
    ) -> EngineResult<wgpu::Buffer> {
        if radius == 0 {
            return Ok(alpha.clone());
        }
        let mut p = self.base;
        p.control[1] = 1;
        p.control[2] = radius;
        p.flags[0] = u32::from(dilate);
        let dummy = self.dummy.clone();
        let horizontal = self.dispatch(p, [alpha, &dummy, &dummy, &dummy])?;
        p.control[3] = 1;
        self.dispatch(p, [&horizontal, &dummy, &dummy, &dummy])
    }

    fn morphology(
        &mut self,
        alpha: &wgpu::Buffer,
        radius: f32,
        dilate: bool,
    ) -> EngineResult<wgpu::Buffer> {
        let lo = radius.floor() as u32;
        let low = self.integer_morphology(alpha, lo, dilate)?;
        let fraction = radius - lo as f32;
        if fraction <= 0.0 {
            return Ok(low);
        }
        let high = self.integer_morphology(alpha, lo + 1, dilate)?;
        let mut p = self.base;
        p.control[1] = 3;
        p.geometry[0] = fraction;
        let dummy = self.dummy.clone();
        self.dispatch(p, [&low, &high, &dummy, &dummy])
    }

    fn blur(&mut self, alpha: &wgpu::Buffer, size: f32) -> EngineResult<wgpu::Buffer> {
        if size <= 0.0 {
            return Ok(alpha.clone());
        }
        let radius = size.ceil() as i32;
        let sigma = (size / 3.0).max(0.01);
        let mut weights: Vec<f32> = (-radius..=radius)
            .map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp())
            .collect();
        let sum: f32 = weights.iter().sum();
        for weight in &mut weights {
            *weight /= sum;
        }
        let weights = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("CPU reference style Gaussian weights"),
                contents: bytemuck::cast_slice(&weights),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let mut p = self.base;
        p.control[1] = 2;
        p.control[2] = radius as u32;
        let dummy = self.dummy.clone();
        let horizontal = self.dispatch(p, [alpha, &dummy, &dummy, &weights])?;
        p.control[3] = 1;
        self.dispatch(p, [&horizontal, &dummy, &dummy, &weights])
    }

    fn paint(&self, fill: &Fill, p: &mut Params) -> EngineResult<wgpu::Buffer> {
        if let Fill::Solid { color } = fill {
            p.color = [color[0], color[1], color[2], 1.0];
            return Ok(self.dummy.clone());
        }
        let mut data = Vec::new();
        match fill {
            Fill::Solid { .. } => unreachable!(),
            Fill::Pattern {
                width,
                height,
                rgba,
                origin,
            } => {
                p.flags[0] = 1;
                p.paint = [*width as f32, *height as f32, origin[0], origin[1]];
                data.extend_from_slice(rgba);
            }
            Fill::Gradient {
                gradient,
                start,
                end,
                stops,
            } => {
                p.flags[0] = match gradient {
                    GradientKind::Linear => 2,
                    GradientKind::Radial => 3,
                };
                p.paint = [start[0], start[1], end[0], end[1]];
                p.origin[2] = stops.len() as u32;
                for stop in stops {
                    data.push(stop.position);
                    data.extend_from_slice(&stop.color);
                }
            }
        }
        check_size(self.device, data.len() as u64 * 4)?;
        Ok(self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("style Fill stops or pattern texels"),
                contents: bytemuck::cast_slice(&data),
                usage: wgpu::BufferUsages::STORAGE,
            }))
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        p: Params,
        inputs: [&wgpu::Buffer; 4],
        mode: BlendMode,
        opacity: f32,
        outside: bool,
        stroke: bool,
    ) -> EngineResult<GpuStylePlane> {
        Ok(GpuStylePlane {
            pixels: self.dispatch(p, inputs)?,
            mode,
            opacity,
            outside,
            stroke,
        })
    }
}

fn invalid(message: &str) -> EngineError {
    EngineError::invalid("GPU layer styles", message)
}
fn check_size(device: &wgpu::Device, size: u64) -> EngineResult<()> {
    let limits = device.limits();
    if size == 0 || size > limits.max_storage_buffer_binding_size || size > limits.max_buffer_size {
        return Err(EngineError::ResourceExhausted {
            resource: "style buffer exceeds device storage/buffer limit".into(),
        });
    }
    Ok(())
}
fn offset(angle: f32, distance: f32) -> (f32, f32) {
    let a = angle.rem_euclid(360.0).to_radians();
    (-a.cos() * distance, a.sin() * distance)
}
fn rank(e: &StyleEffect) -> u8 {
    match e {
        StyleEffect::DropShadow(_) => 0,
        StyleEffect::OuterGlow(_) => 1,
        StyleEffect::PatternOverlay(_) => 2,
        StyleEffect::GradientOverlay(_) => 3,
        StyleEffect::ColorOverlay(_) | StyleEffect::Overlay(_) => 4,
        StyleEffect::Satin(_) => 5,
        StyleEffect::InnerGlow(_) => 6,
        StyleEffect::InnerShadow(_) => 7,
        StyleEffect::Stroke(_) => 8,
        StyleEffect::Bevel(_) => 9,
    }
}
fn support(e: &StyleEffect) -> f32 {
    match e {
        StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => s.size + s.spread,
        StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => s.size + s.spread,
        StyleEffect::Bevel(s) => s.size + s.soften,
        StyleEffect::Satin(s) => s.size,
        StyleEffect::Stroke(s) => s.size,
        _ => 0.0,
    }
}
