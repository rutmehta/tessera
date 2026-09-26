//! Standalone module tests: the parent wires this module into resident/mod.rs.
use compositor::{document, edit, geom, gpu, render, resident};
#[path = "../src/resident/smart_gpu.rs"]
mod smart_gpu;

use compositor::Affine;
use engine_api::tile::{Extent, TileCoord};
use wgpu::util::DeviceExt;

#[test]
fn identity_writes_straight_planar_at_page_offset() {
    if std::env::var_os("CI").is_some() {
        eprintln!("skipping: CI runner without a Metal device");
        return;
    }
    let gpu = gpu::GpuCompositor::new().expect("Metal GPU required");
    let (device, queue) = gpu.handles();
    let plan = smart_gpu::SmartPlan::new(
        Affine::IDENTITY,
        Extent::new(2, 1),
        Extent::new(2, 1),
        TileCoord::new(0, 0, 0),
    )
    .unwrap();
    assert_eq!(plan.child_level(), 0);
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[0.25f32, 0.125, 0.0, 0.5, 0.0, 0.0, 0.0, 0.0]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[-7.0f32; 16]),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let pipe = smart_gpu::SmartGpu::new(device).unwrap();
    let mut enc = device.create_command_encoder(&Default::default());
    pipe.encode(device, &mut enc, &input, &out, 4, &plan)
        .unwrap();
    queue.submit([enc.finish()]);
    let values = read(device, queue, &out);
    assert_eq!(&values[..4], &[-7.0; 4]);
    assert_eq!(&values[4..12], &[0.5, 0.0, 0.25, 0.0, 0.0, 0.0, 0.5, 0.0]);
    assert_eq!(&values[12..], &[-7.0; 4]);
}

#[test]
fn gpu_child_and_resampling_match_cpu_smart_tiles() {
    use compositor::{
        Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, SmartObject,
    };
    use engine_api::tile::Tile;
    if std::env::var_os("CI").is_some() {
        eprintln!("skipping: CI runner without a Metal device");
        return;
    }
    let gpu = gpu::GpuCompositor::new().unwrap();
    let (device, queue) = gpu.handles();
    let pipe = smart_gpu::SmartGpu::new(device).unwrap();
    let ce = Extent::new(259, 5);
    let mut raster = Raster::new(ce, 4, Depth::F32, 0.0);
    for tx in 0..2 {
        let layout = raster.layout(tx, 0);
        let n = (layout.extent.width * layout.extent.height) as usize;
        let mut values = vec![0.0; n * 4];
        for y in 0..layout.extent.height {
            for x in 0..layout.extent.width {
                let i = (y * layout.extent.width + x) as usize;
                let gx = tx * 256 + x;
                values[i] = (gx % 7) as f32 / 6.0;
                values[n + i] = y as f32 / 4.0;
                values[2 * n + i] = 0.7;
                values[3 * n + i] = if gx % 3 == 0 { 0.0 } else { 0.4 };
            }
        }
        raster
            .set_slot(
                tx,
                0,
                Some(Tile::from_samples(TileCoord::new(0, tx, 0), layout, values).unwrap()),
                1,
            )
            .unwrap();
    }
    let mut child = DocState::new(ce, Depth::F32);
    child.root.push(std::sync::Arc::new(Layer::new(
        "pattern",
        LayerKind::Pixel(raster),
    )));
    let cases = [
        (Affine::IDENTITY, 0),
        (Affine::scale_translate(1.0, 1.0, 0.37, -0.21), 0),
        (Affine::scale_translate(0.25, 0.25, 3.3, 2.1), 0),
        (
            Affine {
                m: [0.91, -0.2, 2.0, 0.13, 1.1, -1.0],
            },
            0,
        ),
        (Affine::scale_translate(-1.0, 1.0, 259.0, 0.0), 0),
        (Affine::IDENTITY, 2),
    ];
    for (transform, level) in cases {
        let pe = Extent::new(263, 9);
        let so = SmartObject::new(child.clone(), transform);
        let mut parent = DocState::new(pe, Depth::F32);
        parent.root.push(std::sync::Arc::new(Layer::new(
            "smart",
            LayerKind::SmartObject(so.clone()),
        )));
        let parent = Document::new(parent);
        let cpu = Compositor::new(32 << 20);
        let (cols, rows) = pe.at_level(level).tile_grid(256);
        for ty in 0..rows {
            for tx in 0..cols {
                let coord = TileCoord::new(level, tx, ty);
                let plan = smart_gpu::SmartPlan::new(transform, ce, pe, coord).unwrap();
                let renderer = smart_gpu::render_child(&gpu, &so, &plan).unwrap();
                // Test-only readback bridges the private resident level field.
                // Production passes renderer.levels[&plan.child_level()].out directly.
                let (_, pm) = renderer.read_level(plan.child_level(), true).unwrap();
                let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: None,
                    contents: bytemuck::cast_slice(&pm),
                    usage: wgpu::BufferUsages::STORAGE,
                });
                let oe = plan.output_extent();
                let output = device.create_buffer(&wgpu::BufferDescriptor {
                    label: None,
                    size: u64::from(oe.width * oe.height) * 16,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                });
                let mut enc = device.create_command_encoder(&Default::default());
                pipe.encode(device, &mut enc, &input, &output, 0, &plan)
                    .unwrap();
                queue.submit([enc.finish()]);
                let got = read(device, queue, &output);
                let want = cpu.render_tile(&parent, coord).unwrap();
                for (i, (a, b)) in got.iter().zip(want.samples::<f32>().unwrap()).enumerate() {
                    assert!(
                        (a - b).abs() < 2e-5,
                        "{transform:?} {coord:?} sample {i}: GPU {a} CPU {b}"
                    );
                }
            }
        }
    }
}

#[test]
fn large_parent_coordinates_keep_fractional_footprints() {
    use compositor::{Compositor, Depth, DocState, Document, Fill, Layer, LayerKind, SmartObject};
    if std::env::var_os("CI").is_some() {
        eprintln!("skipping: CI runner without a Metal device");
        return;
    }
    let gpu = gpu::GpuCompositor::new().unwrap();
    let (device, queue) = gpu.handles();
    let ce = Extent::new(2, 1);
    let pe = Extent::new(16_777_220, 1);
    let transform = Affine::scale_translate(1.0, 1.0, 16_777_216.125, 0.0);
    let coord = TileCoord::new(0, 65_536, 0);
    let plan = smart_gpu::SmartPlan::new(transform, ce, pe, coord).unwrap();
    let mut child = DocState::new(ce, Depth::F32);
    let mut solid = Layer::new(
        "solid",
        LayerKind::Fill(Fill::Solid {
            color: [0.5, 0.25, 0.75],
        }),
    );
    solid.props.opacity = 0.4;
    child.root.push(std::sync::Arc::new(solid));
    let mut parent = DocState::new(pe, Depth::F32);
    parent.root.push(std::sync::Arc::new(Layer::new(
        "smart",
        LayerKind::SmartObject(SmartObject::new(child, transform)),
    )));
    let want = Compositor::new(4 << 20)
        .render_tile(&Document::new(parent), coord)
        .unwrap();
    let input = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: bytemuck::cast_slice(&[0.2f32, 0.1, 0.3, 0.4, 0.2, 0.1, 0.3, 0.4]),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    smart_gpu::SmartGpu::new(device)
        .unwrap()
        .encode(device, &mut enc, &input, &out, 0, &plan)
        .unwrap();
    queue.submit([enc.finish()]);
    for (a, b) in read(device, queue, &out)
        .iter()
        .zip(want.samples::<f32>().unwrap())
    {
        assert!((a - b).abs() < 1e-6, "GPU {a} CPU {b}");
    }
}

#[test]
fn plan_rejects_invalid_geometry_and_selects_determinant_mip() {
    let e = Extent::new(259, 5);
    let c = TileCoord::new(0, 0, 0);
    for t in [
        Affine::scale_translate(0.0, 1.0, 0.0, 0.0),
        Affine::scale_translate(f64::NAN, 1.0, 0.0, 0.0),
    ] {
        assert!(smart_gpu::SmartPlan::new(t, e, e, c).is_err());
    }
    assert!(
        smart_gpu::SmartPlan::new(
            Affine::IDENTITY,
            e,
            e,
            TileCoord::new(render::MAX_LEVEL, 0, 0)
        )
        .is_err()
    );
    assert!(smart_gpu::SmartPlan::new(Affine::IDENTITY, e, e, TileCoord::new(0, 2, 0)).is_err());
    // Area-based selection, NOT max-axis footprint: determinant is 1/16.
    let plan =
        smart_gpu::SmartPlan::new(Affine::scale_translate(1.0 / 16.0, 1.0, 0.0, 0.0), e, e, c)
            .unwrap();
    assert_eq!(plan.child_level(), 2);
}

fn read(device: &wgpu::Device, queue: &wgpu::Queue, buffer: &wgpu::Buffer) -> Vec<f32> {
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: buffer.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
    queue.submit([enc.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let result = bytemuck::cast_slice(&staging.slice(..).get_mapped_range().unwrap()).to_vec();
    staging.unmap();
    result
}
