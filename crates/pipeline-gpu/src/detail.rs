//! Isolated Detail compute path. One upload/submission/readback, including bypass.
//! Only validation and settings packing happen on the host; pixels stay on GPU.
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::DetailSettings,
    tile::{Tile, TileLayout},
};
use wgpu::util::DeviceExt;

pub(crate) fn parameters(l: TileLayout, settings: &DetailSettings) -> EngineResult<Vec<f32>> {
    if l.channels != 3 {
        return Err(EngineError::invalid("tile", "expected three RGB planes"));
    }

    let sh = &settings.sharpening;
    let nr = &settings.noise_reduction;
    let controls = [
        sh.amount,
        sh.radius,
        sh.detail,
        sh.masking,
        nr.luminance,
        nr.luminance_detail,
        nr.luminance_contrast,
        nr.color,
        nr.color_detail,
        nr.color_smoothness,
    ];
    if controls.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid(
            "color/detail",
            "finite values required",
        ));
    }
    if !(0.0..=150.0).contains(&sh.amount)
        || !(0.5..=3.0).contains(&sh.radius)
        || controls[2..].iter().any(|v| !(0.0..=100.0).contains(v))
    {
        return Err(EngineError::invalid(
            "detail",
            "controls outside documented settings ranges",
        ));
    }
    let sharp = sh.amount > 0.0;
    let lum = nr.luminance > 0.0;
    let chroma = nr.color > 0.0;
    let sr = (3.0 * sh.radius).ceil() as u16;
    let cr = (1.0 + 4.0 * nr.color_smoothness / 100.0).ceil() as u16;
    let mut halo = if sharp { sr.max(2) } else { 0 };
    if lum {
        halo = halo.max(2);
    }
    if chroma {
        halo = halo.max(cr);
    }
    if l.halo < halo {
        return Err(EngineError::invalid(
            "detail",
            "three RGB planes and sufficient real-neighbour halo required",
        ));
    }
    // Integer-sized tile dimensions are exactly representable within device limits.
    let mut p = vec![
        l.plane_len() as f32,
        l.stride() as f32,
        l.halo as f32,
        l.extent.width as f32,
        l.extent.height as f32,
        u8::from(sharp) as f32,
        u8::from(lum) as f32,
        u8::from(chroma) as f32,
        sr as f32,
        cr as f32,
    ];
    p.extend(controls);
    Ok(p)
}

pub(crate) fn pipelines(ctx: &crate::GpuContext) -> EngineResult<Vec<wgpu::ComputePipeline>> {
    let scope = ctx.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Detail"),
            source: wgpu::ShaderSource::Wgsl(include_str!("detail.wgsl").into()),
        });
    let bindings: Vec<_> = (0..4)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage {
                    read_only: binding == 0 || binding == 2,
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let bgl = ctx
        .device
        .create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Detail buffers"),
            entries: &bindings,
        });
    let layout = ctx
        .device
        .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Detail"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
    let pipelines: Vec<_> = ["decompose", "main"]
        .into_iter()
        .map(|entry| {
            ctx.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some(entry),
                    layout: Some(&layout),
                    module: &shader,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    cache: None,
                })
        })
        .collect();
    if let Some(e) = pollster::block_on(scope.pop()) {
        return Err(internal(e));
    }
    Ok(pipelines)
}

pub(crate) fn run(
    ctx: &crate::GpuContext,
    input: &Tile,
    settings: &DetailSettings,
) -> EngineResult<Tile> {
    let l = input.layout();
    let samples = input.samples::<f32>()?;
    if samples.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid(
            "color/detail",
            "finite values required",
        ));
    }
    let p = parameters(l, settings)?;
    let pipelines = pipelines(ctx)?;
    let bgl = pipelines[0].get_bind_group_layout(0);
    let size = std::mem::size_of_val(samples) as u64;
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Detail upload"),
            contents: bytemuck::cast_slice(samples),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let params = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Detail parameters"),
            contents: bytemuck::cast_slice(&p),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let buffer = |label, size, usage| {
        ctx.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let dst = buffer(
        "Detail output",
        size,
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    );
    let decomposition = buffer(
        "Detail Y/Oklab",
        l.plane_len() as u64 * 16,
        wgpu::BufferUsages::STORAGE,
    );
    let staging = buffer(
        "Detail readback",
        size,
        wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
    );
    let entries: Vec<_> = [&src, &dst, &params, &decomposition]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("Detail"),
        layout: &bgl,
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    // Separate passes provide the storage dependency between decomposition and filtering.
    for pipeline in &pipelines {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(pipeline);
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
    Tile::from_samples(input.coord(), l, data)
}

fn internal(e: impl std::fmt::Display) -> EngineError {
    EngineError::internal(e.to_string())
}
