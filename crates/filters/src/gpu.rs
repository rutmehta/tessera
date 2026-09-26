//! Real Metal compute backend. Unsupported operators return an explicit error.
use crate::{Buffer, Effect, FilterParams, checkpoint, gaussian_kernel, validate};
use compositor::raster::Raster;
use engine_api::{EngineError, EngineResult};
use std::sync::atomic::AtomicBool;
use wgpu::util::DeviceExt;

pub struct GpuFilters {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
}
impl GpuFilters {
    pub fn new() -> EngineResult<Self> {
        let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
        desc.backends = wgpu::Backends::METAL;
        let instance = wgpu::Instance::new(desc);
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            ..Default::default()
        }))
        .map_err(|e| EngineError::internal(format!("filter Metal adapter: {e}")))?;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("filters"),
            // Default WebGPU storage limits would reject 20 MP RGBA (320 MB).
            required_limits: adapter.limits(),
            ..Default::default()
        }))
        .map_err(|e| EngineError::internal(format!("filter Metal device: {e}")))?;
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("filters"),
            source: wgpu::ShaderSource::Wgsl(
                [
                    include_str!("shaders/filters.wgsl"),
                    include_str!("shaders/adjust.wgsl"),
                ]
                .concat()
                .into(),
            ),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("filters"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(format!("filter shader: {e}")));
        }
        Ok(Self {
            device,
            queue,
            pipeline,
        })
    }
    pub fn apply(
        &self,
        effect: Effect,
        input: &Raster,
        p: &FilterParams,
        cancel: &AtomicBool,
    ) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        validate(p)?;
        if p.amount == 0.0 {
            return Ok(input.clone());
        }
        let src = Buffer::read(input, cancel)?;
        let mut data = vec![0.0_f32; 32];
        data[0] = src.w as f32;
        data[1] = src.h as f32;
        data[3] = p.amount;
        data[4] = p.radius;
        data[5] = p.angle;
        data[6] = p.threshold;
        data[7] = p.strength;
        let op = match effect {
            Effect::LensBlur => {
                let depth = p.depth.as_ref().ok_or_else(|| {
                    EngineError::invalid("GPU lens blur", "supply near-to-far depth/mask")
                })?;
                if depth.len() != src.pixels.len()
                    || depth
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || p.radius > 128.0
                    || p.focus
                        .iter()
                        .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                    || p.focus[0] > p.focus[1]
                {
                    return Err(EngineError::invalid(
                        "GPU lens blur",
                        "invalid depth, radius or focus",
                    ));
                }
                data[18] = p.focus[0];
                data[19] = p.focus[1];
                data.extend(depth);
                let descriptors = data.len();
                data[17] = descriptors as f32;
                data.resize(descriptors + 32, 0.0);
                for layer in 0..16 {
                    checkpoint(cancel)?;
                    let mut sum = 0.0_f64;
                    let mut n = 0;
                    for &d in depth {
                        let distance = (p.focus[0] - d).max(d - p.focus[1]).max(0.0);
                        if distance > 0.0 && ((d * 16.0) as usize).min(15) == layer {
                            sum += f64::from(distance);
                            n += 1;
                        }
                    }
                    if n == 0 {
                        continue;
                    }
                    let radius = p.radius * 100.0 / 100.0 * (sum / n as f64) as f32;
                    let start = data.len();
                    data[descriptors + layer * 2] = start as f32;
                    let r = radius.ceil() as i32;
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if (dx as f32).hypot(dy as f32) <= radius {
                                data.extend([dx as f32, dy as f32]);
                            }
                        }
                    }
                    data[descriptors + layer * 2 + 1] = ((data.len() - start) / 2) as f32;
                }
                18
            }
            Effect::Adjust => encode_adjustment(&p.adjust, &src, &mut data)?,
            Effect::Distort(kind) => {
                use crate::distort::Distortion::*;
                let d = &p.distort;
                if [
                    d.amount,
                    d.wavelength,
                    d.phase,
                    d.offset[0],
                    d.offset[1],
                    d.center[0],
                    d.center[1],
                ]
                .iter()
                .any(|v| !v.is_finite())
                    || d.wavelength <= 0.0
                    || d.center.iter().any(|v| !(0.0..=1.0).contains(v))
                    || (matches!(kind, Pinch | Spherize) && !(-1.0..=1.0).contains(&d.amount))
                    || (kind == Ripple
                        && 2.0 * f64::from(d.amount).abs() * std::f64::consts::TAU
                            / f64::from(d.wavelength)
                            >= 1.0)
                    || (matches!(kind, PolarToRectangular | RectangularToPolar)
                        && (src.w < 2 || src.h < 2))
                {
                    return Err(EngineError::invalid(
                        "GPU distortion",
                        "invalid parameters or dimensions",
                    ));
                }
                data[16..23].copy_from_slice(&[
                    d.amount,
                    d.wavelength,
                    d.phase,
                    d.offset[0],
                    d.offset[1],
                    d.center[0],
                    d.center[1],
                ]);
                // Normalize huge periodic offsets on the host before f32 shader subtraction.
                data[19] = f64::from(d.offset[0]).rem_euclid(src.w as f64) as f32;
                data[20] = f64::from(d.offset[1]).rem_euclid(src.h as f64) as f32;
                match kind {
                    Pinch => 10,
                    Spherize => 11,
                    Twirl => 12,
                    Wave => 13,
                    Ripple => 14,
                    PolarToRectangular => 15,
                    RectangularToPolar => 16,
                    Offset => 17,
                }
            }
            Effect::Gaussian | Effect::Box => 2,
            Effect::UnsharpMask => 3,
            Effect::HighPass => 4,
            Effect::Motion => 5,
            Effect::RadialSpin => 6,
            Effect::RadialZoom => 7,
            Effect::SurfaceBlur => 8,
            Effect::AddNoise => 9,
            _ => {
                return Err(EngineError::invalid(
                    "GPU filter",
                    "unsupported operator; use explicit CPU backend",
                ));
            }
        };
        let factor = if effect == Effect::Box {
            1
        } else {
            crate::large::factor(p.radius)
        };
        let convolution = matches!(
            effect,
            Effect::Gaussian | Effect::Box | Effect::UnsharpMask | Effect::HighPass
        );
        if convolution {
            let k = if effect == Effect::Box {
                let n = 2 * p.radius.ceil() as usize + 1;
                vec![1.0 / n as f32; n]
            } else {
                gaussian_kernel(p.radius / factor as f32)
            };
            data[8] = k.len() as f32;
            data.extend(k);
        }
        // Split seed into exactly representable integers, never numeric f32 seed conversion.
        data[9] = (p.seed & 65535) as f32;
        data[10] = (p.seed >> 16) as f32;
        data[11] = u8::from(p.monochrome) as f32;
        data[12] = u8::from(p.gaussian_noise) as f32;
        let bytes = bytemuck::cast_slice(&src.pixels);
        if bytes.len() as u64 > self.device.limits().max_storage_buffer_binding_size {
            return Err(EngineError::invalid(
                "GPU filter",
                "image exceeds storage buffer limit",
            ));
        }
        let original = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("source"),
                contents: bytes,
                usage: wgpu::BufferUsages::STORAGE,
            });
        let mut current = original.clone();
        let stages = if convolution && factor > 1 {
            let pad = crate::large::padding(p.radius, factor);
            let cw = (src.w + 2 * pad).div_ceil(factor);
            let ch = (src.h + 2 * pad).div_ceil(factor);
            data[16] = factor as f32;
            data[17] = pad as f32;
            data[18] = src.w as f32;
            data[19] = src.h as f32;
            data[20] = cw as f32;
            data[21] = ch as f32;
            data[0] = cw as f32;
            data[1] = ch as f32;
            data[2] = 40.0;
            current = self.dispatch(&original, &current, &data, cw * ch)?;
            for stage in [0, 1] {
                checkpoint(cancel)?;
                data[2] = stage as f32;
                current = self.dispatch(&original, &current, &data, cw * ch)?;
            }
            data[0] = src.w as f32;
            data[1] = src.h as f32;
            data[2] = 41.0;
            current = self.dispatch(&original, &current, &data, src.pixels.len())?;
            vec![op]
        } else if convolution {
            vec![0, 1, op]
        } else {
            vec![op]
        };
        for stage in stages {
            checkpoint(cancel)?;
            data[2] = stage as f32;
            current = self.dispatch(&original, &current, &data, src.pixels.len())?;
        }
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: bytes.len() as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&current, 0, &staging, 0, bytes.len() as u64);
        self.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |v| {
            let _ = tx.send(v);
        });
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| EngineError::internal(e.to_string()))?;
        rx.recv()
            .map_err(|e| EngineError::internal(e.to_string()))?
            .map_err(|e| EngineError::internal(e.to_string()))?;
        checkpoint(cancel)?;
        let pixels = bytemuck::cast_slice::<u8, [f32; 4]>(
            &staging
                .slice(..)
                .get_mapped_range()
                .map_err(|e| EngineError::internal(e.to_string()))?,
        )
        .to_vec();
        staging.unmap();
        Buffer {
            w: src.w,
            h: src.h,
            pixels,
        }
        .write(input, cancel)
    }
    fn dispatch(
        &self,
        original: &wgpu::Buffer,
        current: &wgpu::Buffer,
        data: &[f32],
        count: usize,
    ) -> EngineResult<wgpu::Buffer> {
        if (count as u64) * 16 > self.device.limits().max_storage_buffer_binding_size
            || (data.len() as u64) * 4 > self.device.limits().max_storage_buffer_binding_size
        {
            return Err(EngineError::invalid(
                "GPU filter",
                "intermediate/parameters exceed storage buffer limit",
            ));
        }
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let params = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("params"),
                contents: bytemuck::cast_slice(data),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let out = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("output"),
            size: (count * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let entries = [original, current, &out, &params]
            .into_iter()
            .enumerate()
            .map(|(i, b)| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: b.as_entire_binding(),
            })
            .collect::<Vec<_>>();
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: None,
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                (data[0] as u32).div_ceil(8),
                (data[1] as u32).div_ceil(8),
                1,
            );
        }
        self.queue.submit([encoder.finish()]);
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(EngineError::internal(e.to_string()));
        }
        Ok(out)
    }
}

// Only parameter preprocessing (spline slopes and whole-image statistics) runs
// on the host. All per-pixel adjustment evaluation runs in WGSL.
fn encode_adjustment(
    a: &crate::adjust::Adjustment,
    src: &Buffer,
    data: &mut Vec<f32>,
) -> EngineResult<u32> {
    use crate::adjust::Adjustment::*;
    a.validate()?;
    if a.is_identity() {
        return Ok(2);
    }
    let op = match a {
        Levels {
            input_black,
            input_white,
            gamma,
            output_black,
            output_white,
        } => {
            for v in [input_black, input_white, gamma, output_black, output_white] {
                data.extend(v);
            }
            20
        }
        Curves { points } => {
            for (c, points) in points.iter().enumerate() {
                data[16 + c * 2] = data.len() as f32;
                data[17 + c * 2] = points.len() as f32;
                let slopes = curve_slopes(points);
                for (p, m) in points.iter().zip(slopes) {
                    data.extend([p[0], p[1], m]);
                }
            }
            21
        }
        BrightnessContrast {
            brightness,
            contrast,
            legacy,
        } => {
            data.extend([*brightness, *contrast, u8::from(*legacy) as f32]);
            22
        }
        Exposure {
            stops,
            offset,
            gamma,
        } => {
            data.extend([stops.exp2(), *offset, 1.0 / gamma]);
            23
        }
        Threshold { level } => {
            data.push(*level);
            24
        }
        Posterize { levels } => {
            data.push(f32::from(*levels - 1));
            25
        }
        Hsl {
            hue_degrees,
            saturation,
            lightness,
        } => {
            data.extend([hue_degrees / 360.0, *saturation, *lightness]);
            26
        }
        Vibrance {
            vibrance,
            saturation,
        } => {
            data.extend([*vibrance, *saturation]);
            27
        }
        PhotoFilter {
            colour,
            density,
            preserve_luminosity,
        } => {
            data.extend(colour);
            data.extend([*density, u8::from(*preserve_luminosity) as f32]);
            28
        }
        ChannelMixer { matrix, constant } => {
            data.extend(matrix.iter().flatten());
            data.extend(constant);
            29
        }
        GradientMap { stops, reverse } => {
            data[16] = stops.len() as f32;
            data[17] = u8::from(*reverse) as f32;
            for (x, rgb) in stops {
                data.push(*x);
                data.extend(rgb);
            }
            30
        }
        SelectiveColour {
            corrections,
            relative,
        } => {
            data[16] = u8::from(*relative) as f32;
            data.extend(corrections.iter().flatten());
            31
        }
        BlackWhite { weights, tint } => {
            data.extend(weights);
            data.extend(tint.unwrap_or([1.0; 3]));
            32
        }
        MatchColour { target, amount } => {
            let (sm, ss) = statistics(src.pixels.iter().map(|p| [p[0], p[1], p[2]]));
            let (tm, ts) = statistics(target.iter().copied());
            data.extend(sm.map(|v| v as f32));
            data.extend(tm.map(|v| v as f32));
            data.extend((0..3).map(|c| {
                if ss[c] > 1e-8 {
                    (ts[c] / ss[c]) as f32
                } else {
                    1.0
                }
            }));
            data.push(*amount);
            for m in [
                RGB_TO_LMS,
                LMS_TO_LAB,
                LMS_TO_LAB.inverse()?,
                RGB_TO_LMS.inverse()?,
            ] {
                data.extend(m.0.iter().flatten().map(|v| *v as f32));
            }
            33
        }
        Invert => 34,
        Desaturate => 35,
    };
    Ok(op)
}
fn curve_slopes(p: &[[f32; 2]]) -> Vec<f32> {
    let d: Vec<_> = p
        .windows(2)
        .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
        .collect();
    let mut m = vec![0.0; p.len()];
    m[0] = d[0];
    m[p.len() - 1] = d[d.len() - 1];
    for i in 1..p.len() - 1 {
        m[i] = (d[i - 1] + d[i]) * 0.5;
    }
    for i in 0..d.len() {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let a = m[i] / d[i];
            let b = m[i + 1] / d[i];
            let norm = a.hypot(b);
            if norm > 3.0 {
                let t = 3.0 / norm;
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    m
}
const RGB_TO_LMS: engine_api::color::ColorMatrix3 = engine_api::color::ColorMatrix3([
    [0.6167558, 0.3601984, 0.0230458],
    [0.265133, 0.6358394, 0.0990276],
    [0.1001026, 0.2039065, 0.6959909],
]);
const LMS_TO_LAB: engine_api::color::ColorMatrix3 = engine_api::color::ColorMatrix3([
    [0.21045426, 0.7936178, -0.004072047],
    [1.9779985, -2.4285922, 0.4505937],
    [0.025904037, 0.78277177, -0.80867577],
]);
fn statistics(samples: impl Iterator<Item = [f32; 3]>) -> ([f64; 3], [f64; 3]) {
    let mut mean = [0.0; 3];
    let mut m2 = [0.0; 3];
    let mut n = 0.0_f64;
    for rgb in samples {
        n += 1.0;
        let lab = LMS_TO_LAB.apply(RGB_TO_LMS.apply(rgb.map(f64::from)).map(f64::cbrt));
        for c in 0..3 {
            let delta = lab[c] - mean[c];
            mean[c] += delta / n;
            m2[c] += delta * (lab[c] - mean[c]);
        }
    }
    (mean, m2.map(|v| (v / n.max(1.0)).max(0.0).sqrt()))
}
