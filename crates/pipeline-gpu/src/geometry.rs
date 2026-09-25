//! Whole-image inverse-map geometry. Host work is settings and planar transfers only.
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{GeometrySettings, NormalizedRect, Transform, UprightMode},
};
use wgpu::util::DeviceExt;

pub(crate) fn run(
    ctx: &crate::GpuContext,
    input: &pipeline_cpu::Image,
    s: &GeometrySettings,
) -> EngineResult<pipeline_cpu::Image> {
    let r = s.crop.rect;
    if !r.is_valid()
        || !s.crop.angle.is_finite()
        || !(-45.0..=45.0).contains(&s.crop.angle)
        || s.crop.aspect.is_some_and(|a| a.contains(&0))
    {
        return Err(EngineError::invalid(
            "crop",
            "valid rectangle, nonzero aspect and angle in -45..=45 required",
        ));
    }
    if s.orientation != 1
        || s.upright.mode != UprightMode::Off
        || !s.upright.guides.is_empty()
        || s.transform != Transform::default()
        || s.constrain_crop
    {
        return Err(EngineError::Unsupported {
            what: "EXIF orientation and constrain-crop are not implemented".into(),
        });
    }
    if r == NormalizedRect::FULL && s.crop.angle == 0. {
        return Ok(input.clone());
    }
    let (iw, ih) = (input.width() as f32, input.height() as f32);
    let (cw, ch) = ((r.right - r.left) * iw, (r.bottom - r.top) * ih);
    let (w, h) = (cw.round().max(1.) as u32, ch.round().max(1.) as u32);
    let (cx, cy) = ((r.left + r.right) * iw / 2., (r.top + r.bottom) * ih / 2.);
    let (sin, cos) = s.crop.angle.to_radians().sin_cos();
    let channels = input.planes().len() as u64;
    let input_size = u64::from(input.width()) * u64::from(input.height()) * channels * 4;
    let size = u64::from(w) * u64::from(h) * channels * 4;
    let limits = ctx.device.limits();
    if input_size.max(size)
        > limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size)
        || w.div_ceil(8) > limits.max_compute_workgroups_per_dimension
        || h.div_ceil(8) > limits.max_compute_workgroups_per_dimension
    {
        return Err(EngineError::Unsupported {
            what: "geometry image exceeds GPU buffer or dispatch limits".into(),
        });
    }
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("geometry Lanczos3"),
            source: wgpu::ShaderSource::Wgsl(include_str!("geometry.wgsl").into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("geometry Lanczos3"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(internal(e));
    }
    let packed: Vec<f32> = input.planes().iter().flatten().copied().collect();
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("geometry upload"),
            contents: bytemuck::cast_slice(&packed),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let p = [
        input.width(),
        input.height(),
        w,
        h,
        cw.to_bits(),
        ch.to_bits(),
        cx.to_bits(),
        cy.to_bits(),
        sin.to_bits(),
        cos.to_bits(),
    ];
    let params = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("geometry settings"),
            contents: bytemuck::cast_slice(&p),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("geometry output"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("geometry readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let entries: Vec<_> = [&src, &dst, &params]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("geometry"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(w.div_ceil(8), h.div_ceil(8), channels as u32);
    }
    encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
    ctx.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        let _ = tx.send(r);
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(internal)?;
    rx.recv().map_err(internal)?.map_err(internal)?;
    let planes = {
        let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
        bytemuck::cast_slice::<u8, f32>(&mapped)
            .chunks_exact(w as usize * h as usize)
            .map(|p| p.to_vec())
            .collect()
    };
    staging.unmap();
    pipeline_cpu::Image::new(w, h, planes)
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
