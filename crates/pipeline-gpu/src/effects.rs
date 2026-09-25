//! Crop-frame creative effects. One pixel upload, submission and readback even for bypass.
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{Crop, EffectsSettings, VignetteStyle},
    tile::{Extent, TILE_SIZE, Tile},
};
use wgpu::util::DeviceExt;

pub(crate) fn run(
    ctx: &crate::GpuContext,
    input: &Tile,
    s: &EffectsSettings,
    extent: Extent,
    crop: &Crop,
) -> EngineResult<Tile> {
    if !crop.rect.is_valid() || !crop.angle.is_finite() {
        return Err(EngineError::invalid(
            "crop",
            "invalid effects coordinate frame",
        ));
    }
    let v = &s.vignette;
    let g = &s.grain;
    if [
        v.amount,
        v.midpoint,
        v.roundness,
        v.feather,
        v.highlights,
        g.amount,
        g.size,
        g.roughness,
    ]
    .iter()
    .any(|x| !x.is_finite())
    {
        return Err(EngineError::invalid("effects", "parameters must be finite"));
    }
    if s.lens_blur.is_some() {
        return Err(EngineError::Unsupported {
            what: "M2 lens blur requires depth inference".into(),
        });
    }
    let l = input.layout();
    let coord = input.coord();
    let e = extent.at_level(coord.level);
    let ox = u64::from(coord.x) * u64::from(TILE_SIZE);
    let oy = u64::from(coord.y) * u64::from(TILE_SIZE);
    if extent.width == 0
        || extent.height == 0
        || l.channels != 3
        || ox + u64::from(l.extent.width) > u64::from(e.width)
        || oy + u64::from(l.extent.height) > u64::from(e.height)
    {
        return Err(EngineError::invalid(
            "effects tile",
            "RGB tile must fit nonempty full image extent at its level",
        ));
    }
    let samples = input.samples::<f32>()?;
    if samples.iter().any(|x| !x.is_finite()) {
        return Err(EngineError::invalid(
            "effects tile",
            "finite samples required",
        ));
    }
    let r = crop.rect;
    let (sin, cos) = crop.angle.to_radians().sin_cos();
    // Integer parameters are bit-packed, preserving large domain coordinates.
    let p = [
        f32::from_bits(l.plane_len() as u32),
        f32::from_bits(l.stride() as u32),
        f32::from_bits(l.halo as u32),
        f32::from_bits(ox as u32),
        f32::from_bits(oy as u32),
        f32::from_bits(e.width),
        f32::from_bits(e.height),
        (r.right - r.left) * e.width as f32,
        (r.bottom - r.top) * e.height as f32,
        (r.left + r.right) * e.width as f32 / 2.,
        (r.top + r.bottom) * e.height as f32 / 2.,
        sin,
        cos,
        v.amount.clamp(-100., 100.) / 100.,
        2. + 3. * (1. - v.roundness.clamp(-100., 100.) / 100.),
        0.05 + 0.9 * v.midpoint.clamp(0., 100.) / 100.,
        v.feather.clamp(0., 100.) / 100.,
        v.highlights.clamp(0., 100.) / 100.,
        g.amount.clamp(0., 100.) / 100.,
        0.5 + 7.5 * g.size.clamp(0., 100.) / 100.,
        g.roughness.clamp(0., 100.) / 100.,
        r.right - r.left,
        r.bottom - r.top,
        extent.width as f32,
        extent.height as f32,
        match v.style {
            VignetteStyle::HighlightPriority => 0.,
            VignetteStyle::ColorPriority => 1.,
            VignetteStyle::PaintOverlay => 2.,
        },
        if v.amount == 0. && g.amount == 0. {
            1.
        } else {
            0.
        },
    ];
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Effects"),
            source: wgpu::ShaderSource::Wgsl(include_str!("effects.wgsl").into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("Effects"),
            layout: None,
            module: &shader,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(internal(e));
    }
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Effects upload"),
            contents: bytemuck::cast_slice(samples),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let params = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Effects parameters"),
            contents: bytemuck::cast_slice(&p),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let size = std::mem::size_of_val(samples) as u64;
    let buffer = |usage| {
        ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let dst = buffer(wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC);
    let staging = buffer(wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST);
    let entries: Vec<_> = [&src, &dst, &params]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((l.plane_len() as u32).div_ceil(64), 1, 1);
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
    let data = {
        let mapped = staging.slice(..).get_mapped_range().map_err(internal)?;
        bytemuck::cast_slice::<u8, f32>(&mapped).to_vec()
    };
    staging.unmap();
    Tile::from_samples(coord, l, data)
}
fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
