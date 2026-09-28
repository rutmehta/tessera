//! Diagnostic only: identical scalar inputs, production sampler, exact CPU map.
use super::*;
use engine_api::tile::{Extent, TILE_SIZE, TileCoord};
use image_core::StageOp;
use wgpu::util::DeviceExt;

fn fixed_coordinates(
    ctx: &GpuContext,
    input: &pipeline_cpu::Image,
    map: &pipeline_cpu::MapPlan,
) -> pipeline_cpu::Image {
    let (w, h) = map.output_extent(input.width(), input.height());
    let shader = include_str!("../../src/lens.wgsl");
    let start = shader
        .find("    let cw = f(10u); let ch = f(11u);")
        .unwrap();
    let end = shader[start..].find("    let plane_out = n;").unwrap() + start;
    // Only coordinate generation is replaced. The production Lanczos code below
    // is compiled verbatim, including its invalid-coordinate handling.
    let shader = format!(
        "{}    let sx = f(16u + 2u*i);\n    let sy = f(17u + 2u*i);\n    let valid = true;\n{}",
        &shader[..start],
        &shader[end..]
    );
    let product_sum =
        "sums += vec3<f32>(src[at], src[plane_in + at], src[2u * plane_in + at]) * weight;";
    assert!(shader.contains(product_sum));
    let shader = shader.replace(product_sum,
        "let product = fma(vec3<f32>(src[at], src[plane_in + at], src[2u * plane_in + at]), vec3<f32>(weight), vec3<f32>(0.)); sums = fma(product, vec3<f32>(1.), sums);");
    let module = ctx
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("diagnostic exact coordinates"),
            source: wgpu::ShaderSource::Wgsl(shader.into()),
        });
    let pipeline = ctx
        .device
        .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("diagnostic production Lanczos"),
            layout: None,
            module: &module,
            entry_point: Some("remap"),
            compilation_options: Default::default(),
            cache: None,
        });
    let mut params = vec![
        input.width(),
        input.height(),
        0,
        input.height(),
        w,
        h,
        0,
        h,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
        0,
    ];
    for y in 0..h {
        for x in 0..w {
            params.extend(
                map.source(x, y, input.width(), input.height())
                    .unwrap_or([-1., -1.])
                    .map(f32::to_bits),
            );
        }
    }
    let packed: Vec<f32> = input.planes().iter().flatten().copied().collect();
    let src = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diagnostic source"),
            contents: bytemuck::cast_slice(&packed),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let p = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("diagnostic coordinates"),
            contents: bytemuck::cast_slice(&params),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let size = u64::from(w) * u64::from(h) * 12;
    assert!(size <= ctx.device.limits().max_storage_buffer_binding_size);
    let dst = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("diagnostic output"),
        size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("diagnostic readback"),
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let entries: Vec<_> = [&src, &dst, &p]
        .iter()
        .enumerate()
        .map(|(i, b)| wgpu::BindGroupEntry {
            binding: i as u32,
            resource: b.as_entire_binding(),
        })
        .collect();
    let group = ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("diagnostic map"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &entries,
    });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups((w * h).div_ceil(64), 1, 1);
    }
    encoder.copy_buffer_to_buffer(&dst, 0, &staging, 0, size);
    ctx.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging.slice(..).map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let planes = {
        let bytes = staging.slice(..).get_mapped_range().unwrap();
        bytemuck::cast_slice::<u8, f32>(&bytes)
            .chunks_exact((w * h) as usize)
            .map(|v| v.to_vec())
            .collect()
    };
    staging.unmap();
    pipeline_cpu::Image::new(w, h, planes).unwrap()
}
fn error(actual: &pipeline_cpu::Image, expected: &pipeline_cpu::Image) -> (f32, f32, usize) {
    let mut maximum = 0f32;
    let mut ratio = 0f32;
    let mut count = 0;
    for (a, b) in actual
        .planes()
        .iter()
        .flatten()
        .zip(expected.planes().iter().flatten())
    {
        assert!(a.is_finite() && b.is_finite());
        let e = (a - b).abs();
        maximum = maximum.max(e);
        let r = e / (0.0005 * b.abs() + 0.0002);
        ratio = ratio.max(r);
        count += usize::from(r > 1.);
    }
    (maximum, ratio, count)
}
#[test]
fn identical_input_remap_and_fixed_coordinate_sampler_diagnostic() {
    let ctx = metal_context();
    let mut fixed_violations = 0;
    for hdr in [false, true] {
        let (image, mut settings) = if hdr {
            fixture_size(1801, true, false, 1029, 131)
        } else {
            fixture_tier(
                1802,
                false,
                false,
                4922,
                67,
                pipeline_cpu::SmartPreviewTier::Compact2048,
            )
        };
        settings.geometry.crop.rect.right = 0.91;
        settings.white_balance.mode = WhiteBalanceMode::Daylight;
        settings.tone.exposure = if hdr { 0.8 } else { 0.6 };
        if hdr {
            settings.geometry.crop.rect.left = 0.07;
            settings.geometry.crop.angle = 3.;
        }
        let proxy = image.camera_linear_proxy().unwrap();
        let developed = pipeline_cpu::render_linear_before_geometry(
            &settings,
            &pipeline_cpu::RenderSource::CameraLinear(proxy),
            None,
        )
        .unwrap();
        let input = &developed;
        let map = proxy
            .resident_tail_plan(&settings)
            .unwrap()
            .unwrap()
            .map
            .unwrap();
        let mut expected = map.apply(input).unwrap();
        let mut fixed = fixed_coordinates(&ctx, input, &map);
        let gpu = GpuStageOp::new(ctx.clone());
        let mut batch = gpu.begin_resident().unwrap();
        let extent = Extent::new(input.width(), input.height());
        let mut tiles = std::collections::HashMap::new();
        for y in 0..input.height().div_ceil(TILE_SIZE) {
            for x in 0..input.width().div_ceil(TILE_SIZE) {
                let c = TileCoord::new(0, x, y);
                tiles.insert(c, batch.upload(&input.tile(c, 0, 1).unwrap()).unwrap());
            }
        }
        let (w, h) = map.output_extent(input.width(), input.height());
        let tile = batch
            .remap(
                extent,
                &tiles,
                (0, input.height()),
                &map,
                Extent::new(w, h),
                0..h,
                TileCoord::new(0, 0, 0),
            )
            .unwrap();
        let mut materialized = Vec::new();
        for y in 0..h.div_ceil(TILE_SIZE) {
            for x in 0..w.div_ceil(TILE_SIZE) {
                let (ox, oy) = (x * TILE_SIZE, y * TILE_SIZE);
                materialized.push(
                    batch
                        .crop(
                            &tile,
                            TileCoord::new(0, x, y),
                            (ox, oy),
                            Extent::new((w - ox).min(TILE_SIZE), (h - oy).min(TILE_SIZE)),
                        )
                        .unwrap(),
                );
            }
        }
        let before = gpu.stats();
        let output = batch
            .finish(materialized, false, None, &CancellationToken::new())
            .unwrap();
        assert!(gpu.stats().submissions > before.submissions);
        let mut original =
            pipeline_cpu::Image::new(w, h, common::assemble_f32(Extent::new(w, h), &output.tiles))
                .unwrap();
        for image in [&mut expected, &mut fixed, &mut original] {
            if hdr {
                *image = image.downsample_crop([0, 0, w, h], 2).unwrap();
            } else {
                let coords: Vec<_> = image.coords().collect();
                for coord in coords {
                    let tile = pipeline_cpu::display_linear(
                        &image.tile(coord, 0, 1).unwrap(),
                        settings.output.gamut_mapping,
                        4.,
                    )
                    .unwrap();
                    image.put(&tile).unwrap();
                }
            }
        }
        let normal_error = error(&original, &expected);
        let fixed_error = error(&fixed, &expected);
        eprintln!(
            "post-tail same-input rounded-product remap hdr={hdr} normal(max,ratio,count)={normal_error:?} exact-coordinate(max,ratio,count)={fixed_error:?}"
        );
        fixed_violations += fixed_error.2;
    }
    assert_eq!(
        fixed_violations, 0,
        "fixed coordinates alone still fail unchanged scalar tolerance; do not widen it"
    );
}
