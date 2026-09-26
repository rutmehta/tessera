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
        Self::from_device(&device, &queue)
    }

    /// Reuses the caller's device and queue; never opens another adapter.
    pub fn from_device(device: &wgpu::Device, queue: &wgpu::Queue) -> EngineResult<Self> {
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
            device: device.clone(),
            queue: queue.clone(),
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
        let bytes = bytemuck::cast_slice(&src.pixels);
        if bytes.len() as u64 > self.device.limits().max_storage_buffer_binding_size
            || bytes.len() as u64 > self.device.limits().max_buffer_size
        {
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
        let source_statistics = if matches!(p.adjust, crate::adjust::Adjustment::MatchColour { .. })
        {
            Some(statistics(src.pixels.iter().map(|p| [p[0], p[1], p[2]])))
        } else {
            None
        };
        let current = self.run_buffer(
            effect,
            &original,
            input.extent(),
            p,
            cancel,
            source_statistics,
        )?;
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
    /// Operators whose kernels need no source-pixel CPU preprocessing.
    pub fn supports_resident(effect: Effect, p: &FilterParams) -> bool {
        match effect {
            Effect::Gaussian
            | Effect::Box
            | Effect::Motion
            | Effect::RadialSpin
            | Effect::RadialZoom
            | Effect::LensBlur
            | Effect::SurfaceBlur
            | Effect::UnsharpMask
            | Effect::HighPass
            | Effect::AddNoise
            | Effect::Distort(_) => true,
            Effect::Adjust => !matches!(p.adjust, crate::adjust::Adjustment::MatchColour { .. }),
            _ => false,
        }
    }

    /// Evaluate tight interleaved straight f32 RGBA on this device. Only
    /// parameters (including caller-supplied lens depth) are uploaded. Pixels
    /// are never mapped, read back, or uploaded. Returns STORAGE | COPY_SRC.
    /// The caller guarantees finite source samples and shared-device ownership.
    pub fn apply_buffer(
        &self,
        effect: Effect,
        input: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        p: &FilterParams,
        cancel: &AtomicBool,
    ) -> EngineResult<wgpu::Buffer> {
        if !Self::supports_resident(effect, p) {
            return Err(EngineError::Unsupported {
                what: format!("resident filter {effect:?}"),
            });
        }
        self.run_buffer(effect, input, extent, p, cancel, None)
    }

    fn run_buffer(
        &self,
        effect: Effect,
        original: &wgpu::Buffer,
        extent: engine_api::tile::Extent,
        p: &FilterParams,
        cancel: &AtomicBool,
        source_statistics: Option<([f64; 3], [f64; 3])>,
    ) -> EngineResult<wgpu::Buffer> {
        checkpoint(cancel)?;
        validate(p)?;
        p.adjust.validate()?;
        let (w, h) = (extent.width as usize, extent.height as usize);
        let size = extent
            .area()
            .checked_mul(16)
            .ok_or_else(|| EngineError::invalid("GPU filter", "image size overflow"))?;
        let limits = self.device.limits();
        if w == 0
            || h == 0
            || original.size() != size
            || !original.usage().contains(wgpu::BufferUsages::STORAGE)
            || size > limits.max_storage_buffer_binding_size
            || size > limits.max_buffer_size
            || extent.width.div_ceil(8) > limits.max_compute_workgroups_per_dimension
            || extent.height.div_ceil(8) > limits.max_compute_workgroups_per_dimension
        {
            return Err(EngineError::invalid(
                "GPU filter",
                "invalid storage buffer, extent or device limits",
            ));
        }
        let count = extent.area() as usize;
        let mut data = vec![0.0_f32; 32];
        data[0] = w as f32;
        data[1] = h as f32;
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
                if depth.len() != count
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
            Effect::Adjust => encode_adjustment(&p.adjust, source_statistics, &mut data)?,
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
                    || (matches!(kind, PolarToRectangular | RectangularToPolar) && (w < 2 || h < 2))
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
                data[19] = f64::from(d.offset[0]).rem_euclid(w as f64) as f32;
                data[20] = f64::from(d.offset[1]).rem_euclid(h as f64) as f32;
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
        let mut current = original.clone();
        let stages = if convolution && factor > 1 {
            let pad = crate::large::padding(p.radius, factor);
            let cw = (w + 2 * pad).div_ceil(factor);
            let ch = (h + 2 * pad).div_ceil(factor);
            data[16] = factor as f32;
            data[17] = pad as f32;
            data[18] = w as f32;
            data[19] = h as f32;
            data[20] = cw as f32;
            data[21] = ch as f32;
            data[0] = cw as f32;
            data[1] = ch as f32;
            data[2] = 40.0;
            current = self.dispatch(original, &current, &data, cw * ch)?;
            for stage in [0, 1] {
                checkpoint(cancel)?;
                data[2] = stage as f32;
                current = self.dispatch(original, &current, &data, cw * ch)?;
            }
            data[0] = w as f32;
            data[1] = h as f32;
            data[2] = 41.0;
            current = self.dispatch(original, &current, &data, count)?;
            vec![op]
        } else if convolution {
            vec![0, 1, op]
        } else {
            vec![op]
        };
        for stage in stages {
            checkpoint(cancel)?;
            data[2] = stage as f32;
            current = self.dispatch(original, &current, &data, count)?;
        }
        Ok(current)
    }

    fn dispatch(
        &self,
        original: &wgpu::Buffer,
        current: &wgpu::Buffer,
        data: &[f32],
        count: usize,
    ) -> EngineResult<wgpu::Buffer> {
        let limits = self.device.limits();
        if (count as u64) * 16 > limits.max_storage_buffer_binding_size
            || (count as u64) * 16 > limits.max_buffer_size
            || (data.len() as u64) * 4 > limits.max_storage_buffer_binding_size
            || (data.len() as u64) * 4 > limits.max_buffer_size
            || (data[0] as u32).div_ceil(8) > limits.max_compute_workgroups_per_dimension
            || (data[1] as u32).div_ceil(8) > limits.max_compute_workgroups_per_dimension
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
    source_statistics: Option<([f64; 3], [f64; 3])>,
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
            let (sm, ss) = source_statistics.ok_or_else(|| EngineError::Unsupported {
                what: "resident MatchColour requires source statistics".into(),
            })?;
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

#[cfg(test)]
mod resident_tests {
    use super::*;
    use engine_api::tile::Extent;

    #[test]
    fn resident_adapter_chain_and_supported_inventory() {
        use crate::{CompositorFilters, Filter};
        use compositor::{document::SmartFilter, geom::Rect, raster::Depth};
        let gpu = GpuFilters::new().expect("Metal device");
        let extent = Extent::new(9, 7);
        let cancel = AtomicBool::new(false);
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                *p = [x as f32 / 9.0, y as f32 / 7.0, 0.25, (x + y) as f32 / 16.0];
            })
            .unwrap();
        let pixels = Buffer::read(&raster, &cancel).unwrap();
        let input = gpu
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&pixels.pixels),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: input.size(),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let check = |out: &wgpu::Buffer, expected: &Raster| {
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            enc.copy_buffer_to_buffer(out, 0, &staging, 0, out.size());
            gpu.queue.submit([enc.finish()]);
            let (tx, rx) = std::sync::mpsc::channel();
            staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                tx.send(r).unwrap();
            });
            gpu.device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            rx.recv().unwrap().unwrap();
            {
                let mapped = staging.slice(..).get_mapped_range().unwrap();
                let actual = bytemuck::cast_slice::<u8, [f32; 4]>(&mapped);
                for (a, b) in actual
                    .iter()
                    .zip(Buffer::read(expected, &cancel).unwrap().pixels)
                {
                    for c in 0..4 {
                        assert!((a[c] - b[c]).abs() < 1e-4, "{a:?} vs {b:?}");
                    }
                }
            }
            staging.unmap();
        };
        let p = FilterParams {
            amount: 0.75,
            radius: 1.5,
            angle: 0.1,
            depth: Some(vec![0.8; extent.area() as usize]),
            ..Default::default()
        };
        for effect in Effect::inventory() {
            if !GpuFilters::supports_resident(effect, &p) {
                continue;
            }
            let out = gpu
                .apply_buffer(effect, &input, extent, &p, &cancel)
                .unwrap();
            check(&out, &effect.apply(&raster, &p, &cancel).unwrap());
        }
        let node = SmartFilter {
            name: "adjust".into(),
            params: serde_json::json!({"adjust": "invert"}),
            ..Default::default()
        };
        let adapter = CompositorFilters;
        assert!(adapter.supports(&node).unwrap());
        let once = adapter
            .evaluate_resident(&gpu.device, &gpu.queue, &input, extent, &node)
            .unwrap();
        let twice = adapter
            .evaluate_resident(&gpu.device, &gpu.queue, &once, extent, &node)
            .unwrap();
        check(&twice, &raster);
    }

    #[test]
    fn resident_buffer_shared_device_and_validation() {
        let owner = GpuFilters::new().expect("Metal device");
        let gpu = GpuFilters::from_device(&owner.device, &owner.queue).unwrap();
        let pixels = [[0.25_f32, 0.5, 0.75, 0.5]; 4];
        let input = owner
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: None,
                contents: bytemuck::cast_slice(&pixels),
                usage: wgpu::BufferUsages::STORAGE,
            });
        let p = FilterParams {
            amount: 1.0,
            adjust: crate::adjust::Adjustment::Invert,
            ..Default::default()
        };
        let out = gpu
            .apply_buffer(
                Effect::Adjust,
                &input,
                Extent::new(2, 2),
                &p,
                &AtomicBool::new(false),
            )
            .unwrap();
        assert_eq!(out.size(), 64);
        assert!(
            out.usage()
                .contains(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC)
        );
        let staging = owner.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = owner.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&out, 0, &staging, 0, 64);
        owner.queue.submit([encoder.finish()]);
        let (tx, rx) = std::sync::mpsc::channel();
        staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
            tx.send(r).unwrap();
        });
        owner
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        rx.recv().unwrap().unwrap();
        {
            let mapped = staging.slice(..).get_mapped_range().unwrap();
            assert_eq!(
                bytemuck::cast_slice::<u8, [f32; 4]>(&mapped),
                &[[0.75, 0.5, 0.25, 0.5]; 4]
            );
        }
        staging.unmap();
        assert!(
            gpu.apply_buffer(
                Effect::Adjust,
                &input,
                Extent::new(0, 2),
                &p,
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(
            gpu.apply_buffer(
                Effect::Adjust,
                &input,
                Extent::new(3, 2),
                &p,
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
}
