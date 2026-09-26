//! Metal-only, real-device parity tests. No adapter skips or CPU fallbacks.

mod common;

#[test]
fn rejects_invalid_geometry_dimensions_and_buffer_contracts() {
    let gpu = GpuCompositor::new().unwrap();
    let renderer = ResidentRenderer::new(&gpu).unwrap();
    let extent = Extent::new(3, 2);
    let op = TransformOp {
        version: 1,
        kernel: Kernel::Bilinear,
        operation: Operation::Free(FreeTransform::identity()),
    };
    for (source, output) in [
        (Extent::new(0, 2), extent),
        (extent, Extent::new(2, 0)),
        (extent, Extent::new(u32::MAX, u32::MAX)),
        (
            extent,
            Extent::new(gpu.handles().0.limits().max_texture_dimension_2d + 1, 1),
        ),
    ] {
        assert!(renderer.prepare_transform(&op, source, output, 0).is_err());
    }
    let mut invalid = op.clone();
    invalid.version = 2;
    assert!(
        renderer
            .prepare_transform(&invalid, extent, extent, 0)
            .is_err()
    );
    for matrix in [
        [[0.; 3]; 3],
        [[1., 0., 0.], [0., 1., 0.], [-1., 0., 1.]],
        [[f64::NAN, 0., 0.], [0., 1., 0.], [0., 0., 1.]],
    ] {
        invalid.version = 1;
        invalid.operation = Operation::Free(FreeTransform { matrix });
        assert!(
            renderer
                .prepare_transform(&invalid, extent, extent, 0)
                .is_err()
        );
    }
    invalid.operation = Operation::ContentAwareScale(transform::seam::ContentAwareScale {
        target_width: 3,
        target_height: 2,
        amount: 1.,
        protect: None,
    });
    assert!(
        renderer
            .prepare_transform(&invalid, extent, extent, 0)
            .is_err()
    );
    let plan = renderer.prepare_transform(&op, extent, extent, 0).unwrap();
    let (device, _) = gpu.handles();
    let buffer = |size, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let good = buffer(96, wgpu::BufferUsages::STORAGE);
    let short = buffer(16, wgpu::BufferUsages::STORAGE);
    let wrong_usage = buffer(96, wgpu::BufferUsages::COPY_DST);
    let mut encoder = device.create_command_encoder(&Default::default());
    assert!(
        renderer
            .encode_transform_buffers(&mut encoder, &good, &good.clone(), &plan, None)
            .is_err()
    );
    for bad in [&short, &wrong_usage] {
        assert!(
            renderer
                .encode_transform_buffers(&mut encoder, &good, bad, &plan, None)
                .is_err()
        );
        assert!(
            renderer
                .encode_transform_buffers(&mut encoder, bad, &good, &plan, None)
                .is_err()
        );
    }
    let impulse = Image::new(1, 1, [vec![0.4], vec![0.2], vec![0.1], vec![0.5]]).unwrap();
    for kernel in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
        Kernel::Automatic,
    ] {
        let op = TransformOp {
            version: 1,
            kernel,
            operation: Operation::Free(FreeTransform::translate(2.27, 2.63).unwrap()),
        };
        compare(&renderer, &gpu, &impulse, &op, Extent::new(7, 7), 0);
        let cpu = op.apply(&impulse, 7, 7, 0).unwrap();
        if matches!(
            kernel,
            Kernel::Bicubic | Kernel::Lanczos3 | Kernel::Automatic
        ) {
            assert!(
                cpu.planes[3].iter().any(|v| *v < 0.),
                "fixture must exercise negative lobes"
            );
        }
    }
}

/// Run: CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-21 cargo test
/// -p compositor --release --test transform_gpu benchmark_36mp -- --ignored --nocapture
/// GPU timestamps exclude map construction/upload. End-to-end includes both and
/// submission + completion, but excludes the already-resident source, allocation
/// of image buffers, pipeline compilation, and pixel readback.
#[test]
#[ignore = "36 MP real Metal benchmark; run explicitly in release mode"]
fn benchmark_36mp() {
    use std::time::Instant;
    let gpu = GpuCompositor::new().unwrap();
    let renderer = ResidentRenderer::new(&gpu).unwrap();
    let (device, queue) = gpu.handles();
    assert!(device.features().contains(wgpu::Features::TIMESTAMP_QUERY));
    let extent = Extent::new(6000, 6000);
    let pixels: Vec<[f32; 4]> = (0..36_000_000usize)
        .map(|i| {
            let x = (i % 6000) as f32 / 6000.;
            let y = (i / 6000) as f32 / 6000.;
            [x * 0.7, y * 0.7, 0.21, 0.7]
        })
        .collect();
    let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("36MP resident source"),
        contents: bytemuck::cast_slice(&pixels),
        usage: wgpu::BufferUsages::STORAGE,
    });
    drop(pixels);
    let dst = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 36_000_000 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let queries = device.create_query_set(&wgpu::QuerySetDescriptor {
        label: None,
        ty: wgpu::QueryType::Timestamp,
        count: 2,
    });
    let resolved = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let dispatch = |plan: &compositor::resident::TransformPlan| {
        let mut encoder = device.create_command_encoder(&Default::default());
        renderer
            .encode_transform_buffers(
                &mut encoder,
                &src,
                &dst,
                plan,
                Some(wgpu::ComputePassTimestampWrites {
                    query_set: &queries,
                    beginning_of_pass_write_index: Some(0),
                    end_of_pass_write_index: Some(1),
                }),
            )
            .unwrap();
        queue.submit([encoder.finish()]);
        renderer.wait().unwrap();
    };
    let read_ms = || {
        // Resolve only after pass completion: Metal may emulate timestamps
        // using command-buffer completion, so same-submission resolve can be stale.
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.resolve_query_set(&queries, 0..2, &resolved, 0);
        queue.submit([encoder.finish()]);
        let bytes = gpu_core::read_buffer(device, queue, &resolved, 0, 16).unwrap();
        let ticks: &[u64] = bytemuck::cast_slice(&bytes);
        (ticks[1] - ticks[0]) as f64 * f64::from(queue.get_timestamp_period()) / 1e6
    };
    eprintln!(
        "36MP Metal adapter={} release={} source/output=576000000 bytes each; map=288000000 bytes",
        gpu.adapter,
        !cfg!(debug_assertions)
    );
    for kernel in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
    ] {
        let op = TransformOp {
            version: 1,
            kernel,
            operation: Operation::Free(FreeTransform {
                matrix: [
                    [0.997, -0.031, 90.123456789],
                    [0.031, 0.997, -70.987654321],
                    [0., 0., 1.],
                ],
            }),
        };
        // Compile lazily outside the end-to-end measurement.
        drop(
            renderer
                .prepare_transform(&op, Extent::new(1, 1), Extent::new(1, 1), 0)
                .unwrap(),
        );
        queue.submit([]);
        renderer.wait().unwrap();
        let start = Instant::now();
        let plan = renderer.prepare_transform(&op, extent, extent, 0).unwrap();
        let prepare_ms = start.elapsed().as_secs_f64() * 1000.;
        dispatch(&plan);
        let end_to_end_ms = start.elapsed().as_secs_f64() * 1000.;
        let first_gpu_ms = read_ms();
        dispatch(&plan); // warm up
        let mut times = Vec::new();
        let mut wall = Vec::new();
        for _ in 0..5 {
            let start = Instant::now();
            dispatch(&plan);
            wall.push(start.elapsed().as_secs_f64() * 1000.);
            times.push(read_ms());
        }
        times.sort_by(f64::total_cmp);
        wall.sort_by(f64::total_cmp);
        eprintln!(
            "{kernel:?}: GPU median={:.3}ms range={:.3}..{:.3}ms (<30ms={}); cached submit+wait median={:.3}ms; prepare+queue_write={prepare_ms:.3}ms; end_to_end_map_upload_dispatch_wait={end_to_end_ms:.3}ms; first_gpu={first_gpu_ms:.3}ms",
            times[2],
            times[0],
            times[4],
            times[2] < 30.,
            wall[2]
        );
        // Actual output is nonzero and premultiplied. Check interior samples
        // against analytic bilinear/nearest gradients; all kernels reconstruct
        // this linear ramp within tolerance away from the transparent edge.
        let inverse = match &op.operation {
            Operation::Free(t) => t.inverse().unwrap(),
            _ => unreachable!(),
        };
        for (x, y) in [(1000u64, 1000u64), (3000, 3000), (5000, 5000)] {
            let bytes =
                gpu_core::read_buffer(device, queue, &dst, (y * 6000 + x) * 16, 16).unwrap();
            let got: &[f32] = bytemuck::cast_slice(&bytes);
            let p = inverse
                .map([x as f64 + 0.5, y as f64 + 0.5])
                .unwrap()
                .map(|v| v as f32);
            let xy = if kernel == Kernel::Nearest {
                [p[0].floor(), p[1].floor()]
            } else {
                [p[0] - 0.5, p[1] - 0.5]
            };
            let expected = [xy[0] / 6000. * 0.7, xy[1] / 6000. * 0.7, 0.21, 0.7];
            for c in 0..4 {
                assert!(
                    (got[c] - expected[c]).abs() < 1e-4,
                    "benchmark output {kernel:?}: {got:?} vs {expected:?}"
                );
            }
        }
    }
}

#[test]
fn resident_level_transform_is_explicit_and_non_destructive() {
    let gpu = GpuCompositor::new().unwrap();
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    let extent = Extent::new(35, 23);
    let mut doc = common::doc(extent, compositor::Depth::F32);
    common::add(
        &mut doc,
        None,
        common::layer_fn("source", extent, compositor::Depth::F32, |x, y| {
            [
                x as f32 / 35.,
                y as f32 / 23.,
                0.7,
                if x % 3 == 0 { 0. } else { 0.6 },
            ]
        }),
    );
    let op = TransformOp {
        version: 1,
        kernel: Kernel::Lanczos3,
        operation: Operation::Free(FreeTransform::translate(0.31, -0.77).unwrap()),
    };
    let plan = renderer.prepare_transform(&op, extent, extent, 0).unwrap();
    let (device, queue) = gpu.handles();
    let dst = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(extent.width) * u64::from(extent.height) * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    assert!(
        renderer
            .encode_transform_level(&mut enc, &dst, &plan, None)
            .is_err()
    );
    renderer
        .render_viewport(&doc, 0, compositor::Rect::new(0, 0, 8, 8), 0)
        .unwrap();
    assert!(
        renderer
            .encode_transform_level(&mut enc, &dst, &plan, None)
            .is_err()
    );
    renderer.render(&doc, 0).unwrap();
    let (_, before) = renderer.read_level(0, true).unwrap();
    let input = Image::new(
        extent.width as usize,
        extent.height as usize,
        std::array::from_fn(|c| before.as_chunks::<4>().0.iter().map(|v| v[c]).collect()),
    )
    .unwrap();
    renderer
        .encode_transform_level(&mut enc, &dst, &plan, None)
        .unwrap();
    queue.submit([enc.finish()]);
    let bytes = gpu_core::read_buffer(device, queue, &dst, 0, dst.size()).unwrap();
    let actual: &[f32] = bytemuck::cast_slice(&bytes);
    let expected = op.apply(&input, input.width, input.height, 0).unwrap();
    for (i, v) in actual.iter().enumerate() {
        assert!((v - expected.planes[i % 4][i / 4]).abs() < 1e-4);
    }
    assert_eq!(before, renderer.read_level(0, true).unwrap().1);
}

#[test]
fn warp_projective_edges_and_large_coordinates_match_cpu() {
    use transform::warp::{WarpMesh, WarpPreset};
    let gpu = GpuCompositor::new().unwrap();
    let renderer = ResidentRenderer::new(&gpu).unwrap();
    let input = image(39, 27);
    let operations = [
        Operation::Warp(WarpMesh::preset(39., 27., WarpPreset::Twist, 0.7).unwrap()),
        Operation::Free(FreeTransform {
            matrix: [[1.05, 0.1, -0.31], [-0.2, 0.99, 1.7], [0.003, -0.001, 1.]],
        }),
        Operation::Free(FreeTransform::translate(0.5, 0.5).unwrap()),
        Operation::Free(FreeTransform::translate(1e18, -1e18).unwrap()),
    ];
    for operation in operations {
        for kernel in [
            Kernel::Nearest,
            Kernel::Bilinear,
            Kernel::Bicubic,
            Kernel::Lanczos3,
        ] {
            let op = TransformOp {
                version: 1,
                operation: operation.clone(),
                kernel,
            };
            for level in [0, 1] {
                compare(&renderer, &gpu, &input, &op, Extent::new(43, 29), level);
            }
        }
    }
    // Rounding f64 geometry to f32 *before* sampling matters at wide-image edges.
    let input = image(8191, 2);
    for kernel in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
    ] {
        let op = TransformOp {
            version: 1,
            kernel,
            operation: Operation::Free(
                FreeTransform::translate(-8160.499877929688, 0.1234567).unwrap(),
            ),
        };
        compare(&renderer, &gpu, &input, &op, Extent::new(37, 5), 0);
    }
}

use compositor::{gpu::GpuCompositor, resident::ResidentRenderer};
use engine_api::tile::Extent;
use transform::{Image, Kernel, Operation, TransformOp, free::FreeTransform};
use wgpu::util::DeviceExt;

fn image(w: usize, h: usize) -> Image {
    let planes = std::array::from_fn(|c| {
        (0..w * h)
            .map(|i| {
                let a = if i % 7 == 0 {
                    0.0
                } else {
                    (i % 19) as f32 / 18.0
                };
                if c == 3 {
                    a
                } else {
                    a * ((i * (c + 3) + c * 17) % 101) as f32 / 100.0
                }
            })
            .collect()
    });
    Image::new(w, h, planes).unwrap()
}

fn compare(
    renderer: &ResidentRenderer,
    gpu: &GpuCompositor,
    input: &Image,
    op: &TransformOp,
    output: Extent,
    level: u8,
) {
    let (device, queue) = gpu.handles();
    let pixels: Vec<[f32; 4]> = (0..input.width * input.height)
        .map(|i| std::array::from_fn(|c| input.planes[c][i]))
        .collect();
    let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("transform parity input"),
        contents: bytemuck::cast_slice(&pixels),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let dst = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(output.width) * u64::from(output.height) * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let plan = renderer
        .prepare_transform(
            op,
            Extent::new(input.width as u32, input.height as u32),
            output,
            level,
        )
        .unwrap();
    assert_eq!(plan.precision(), gpu_core::Precision::Ieee);
    let mut enc = device.create_command_encoder(&Default::default());
    renderer
        .encode_transform_buffers(&mut enc, &src, &dst, &plan, None)
        .unwrap();
    queue.submit([enc.finish()]);
    let bytes = gpu_core::read_buffer(device, queue, &dst, 0, dst.size()).unwrap();
    let actual: &[f32] = bytemuck::cast_slice(&bytes);
    let expected = op
        .apply(input, output.width as usize, output.height as usize, level)
        .unwrap();
    let mut max = 0.0f32;
    for (i, value) in actual.iter().enumerate() {
        assert!(value.is_finite());
        max = max.max((value - expected.planes[i % 4][i / 4]).abs());
    }
    eprintln!("{:?} level={level} max_abs={max:e}", op.kernel);
    assert!(max < 1e-4, "{:?}: {max:e}", op.kernel);
}

#[test]
fn precise_free_transform_matches_cpu_all_kernels() {
    let gpu = GpuCompositor::new().unwrap();
    eprintln!("Metal adapter: {}", gpu.adapter);
    let renderer = ResidentRenderer::new(&gpu).unwrap();
    let input = image(47, 31);
    for kernel in [
        Kernel::Nearest,
        Kernel::Bilinear,
        Kernel::Bicubic,
        Kernel::Lanczos3,
        Kernel::Automatic,
    ] {
        let op = TransformOp {
            version: 1,
            operation: Operation::Free(FreeTransform {
                matrix: [
                    [0.91, -0.27, 3.23456789],
                    [0.23, 1.12, -1.98765432],
                    [0., 0., 1.],
                ],
            }),
            kernel,
        };
        for level in [0, 2] {
            compare(&renderer, &gpu, &input, &op, Extent::new(53, 37), level);
        }
    }
}
