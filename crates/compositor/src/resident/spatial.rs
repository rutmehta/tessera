//! Resident backdrop bases. Two alpha-weighted edge-aware separable passes;
//! every prefix and intermediate stays on the GPU.
use engine_api::EngineResult;
use wgpu::util::DeviceExt;

pub(super) fn pipeline(device: &wgpu::Device) -> EngineResult<wgpu::ComputePipeline> {
    Ok(gpu_core::precise_compute_pipeline(
        device,
        "resident bilateral",
        include_str!("spatial.wgsl"),
        "main",
        (256, 1, 1),
        &(0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 3 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage {
                            read_only: binding != 2,
                        }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect::<Vec<_>>(),
    )?
    .pipeline)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn encode(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    original: &wgpu::Buffer,
    aux: &wgpu::Buffer,
    width: u32,
    height: u32,
    radius: f32,
    offset: u32,
) {
    let create = |label| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: u64::from(width) * u64::from(height) * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    };
    let horizontal = create("bilateral horizontal");
    let vertical = create("bilateral vertical");
    for axis in 0..2u32 {
        let params = [width, height, axis, radius.to_bits()];
        let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bilateral params"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        // Bind a distinct buffer for the unused horizontal input on pass zero.
        let (input, output) = if axis == 0 {
            (&vertical, &horizontal)
        } else {
            (&horizontal, &vertical)
        };
        let buffers = [original, input, output, &uniform];
        let entries: Vec<_> = buffers
            .iter()
            .enumerate()
            .map(|(binding, b)| wgpu::BindGroupEntry {
                binding: binding as u32,
                resource: b.as_entire_binding(),
            })
            .collect();
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bilateral"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("bilateral pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        let (x, y) = super::split((width * height).div_ceil(256));
        pass.dispatch_workgroups(x, y, 1);
    }
    encoder.copy_buffer_to_buffer(
        &vertical,
        0,
        aux,
        u64::from(offset) * 4,
        u64::from(width) * u64::from(height) * 4,
    );
}

#[cfg(test)]
mod tests {
    #[test]
    fn spatial_and_document_shaders_validate_without_adapter() {
        let doc = crate::gpu::shader(&format!(
            "{}\n{}\n{}",
            include_str!("pages.wgsl").replace("ACCESS", "read"),
            include_str!("doc.wgsl"),
            include_str!("adjustments.wgsl")
        ));
        for source in [include_str!("spatial.wgsl"), doc.as_str()] {
            let module = wgpu::naga::front::wgsl::parse_str(source)
                .unwrap_or_else(|e| panic!("{}", e.emit_to_string(source)));
            wgpu::naga::valid::Validator::new(
                wgpu::naga::valid::ValidationFlags::all(),
                wgpu::naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}
