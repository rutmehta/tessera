//! Real Metal parity through the existing resident displacement renderer.
use compositor::{gpu::GpuCompositor, resident::ResidentRenderer};
use engine_api::tile::Extent;
use transform::adaptive::{Adaptive, CameraModel, LineConstraint, LineOrientation, Projection};
use transform::{Image, Kernel, Operation, TransformOp};
use wgpu::util::DeviceExt;

#[test]
fn solved_adaptive_displacement_renders_on_gpu() {
    let gpu = GpuCompositor::new().unwrap();
    let renderer = ResidentRenderer::new(&gpu).unwrap();
    let (device, queue) = gpu.handles();
    let mut recipe = Adaptive::new(
        48,
        32,
        CameraModel::Manual {
            focal_px: 30.,
            center: [24., 16.],
            projection: Projection::Equidistant,
        },
    );
    recipe.lines.push(LineConstraint {
        points: vec![[8., 9.], [24., 8.], [40., 9.]],
        orientation: LineOrientation::Horizontal,
        weight: 1.,
    });
    let field = recipe.solve().unwrap();
    for level in [0, 1] {
        let (w, h) = (48 >> level, 32 >> level);
        let input = Image::new(
            w,
            h,
            std::array::from_fn(|c| {
                (0..w * h)
                    .map(|i| {
                        if c == 3 {
                            0.75
                        } else {
                            ((i + c * 3) % 17) as f32 / 24.
                        }
                    })
                    .collect()
            }),
        )
        .unwrap();
        let pixels: Vec<[f32; 4]> = (0..w * h)
            .map(|i| std::array::from_fn(|c| input.planes[c][i]))
            .collect();
        let source = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&pixels),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let dst = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: (w * h * 16) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let extent = Extent::new(w as u32, h as u32);
        for kernel in [
            Kernel::Nearest,
            Kernel::Bilinear,
            Kernel::Bicubic,
            Kernel::Lanczos3,
        ] {
            let op = TransformOp {
                version: 1,
                operation: Operation::Displacement(field.clone()),
                kernel,
            };
            let plan = renderer
                .prepare_transform(&op, extent, extent, level)
                .unwrap();
            let mut enc = device.create_command_encoder(&Default::default());
            renderer
                .encode_transform_buffers(&mut enc, &source, &dst, &plan, None)
                .unwrap();
            queue.submit([enc.finish()]);
            let bytes = gpu_core::read_buffer(device, queue, &dst, 0, dst.size()).unwrap();
            let actual: &[f32] = bytemuck::cast_slice(&bytes);
            let cpu = op.apply(&input, w, h, level).unwrap();
            let mut max = 0.0f32;
            for (i, a) in actual.iter().enumerate() {
                assert!(a.is_finite());
                max = max.max((a - cpu.planes[i % 4][i / 4]).abs());
            }
            println!("adaptive Metal {kernel:?} level {level}: max error {max}");
            assert!(max < 1e-4);
        }
    }
}
