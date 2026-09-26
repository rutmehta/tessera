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

/// Independent f64 pixel reference: zero extension, normalized six-tap axes,
/// filtering premultiplied RGBA before returning straight planar samples.
fn lanczos_reference(
    input: &[[f32; 4]],
    ce: Extent,
    pe: Extent,
    t: Affine,
    coord: TileCoord,
) -> Vec<f32> {
    let rect = geom::Rect::of_tile(coord, pe.at_level(coord.level));
    let n = (rect.width() * rect.height()) as usize;
    let mut out = vec![0.0; n * 4];
    let inv = t.inverse().unwrap();
    let kernel = |x: f64| {
        if x.abs() < 1e-12 {
            1.0
        } else if x.abs() >= 3.0 {
            0.0
        } else {
            let p = std::f64::consts::PI * x;
            p.sin() * (p / 3.0).sin() / (p * p / 3.0)
        }
    };
    for y in rect.y0..rect.y1 {
        for x in rect.x0..rect.x1 {
            let (qx, qy) = inv.apply(x as f64 + 0.5, y as f64 + 0.5);
            let (fx, fy) = (qx - 0.5, qy - 0.5);
            let xs: Vec<_> = (-2..=3).map(|k| fx.floor() as i64 + k).collect();
            let ys: Vec<_> = (-2..=3).map(|k| fy.floor() as i64 + k).collect();
            let normx: f64 = xs.iter().map(|&i| kernel(fx - i as f64)).sum();
            let normy: f64 = ys.iter().map(|&i| kernel(fy - i as f64)).sum();
            let mut v = [0.0f64; 4];
            for &iy in &ys {
                for &ix in &xs {
                    if ix >= 0 && iy >= 0 && ix < ce.width as i64 && iy < ce.height as i64 {
                        let w = kernel(fx - ix as f64) * kernel(fy - iy as f64) / (normx * normy);
                        for c in 0..4 {
                            v[c] += input[(iy * ce.width as i64 + ix) as usize][c] as f64 * w;
                        }
                    }
                }
            }
            let i = ((y - rect.y0) * rect.width() + x - rect.x0) as usize;
            for c in 0..3 {
                out[c * n + i] = if v[3] > 0.0 {
                    (v[c] / v[3]) as f32
                } else {
                    0.0
                };
            }
            out[3 * n + i] = v[3] as f32;
        }
    }
    out
}

#[test]
fn lanczos_level_zero_matches_independent_reference_and_is_deterministic() {
    if std::env::var_os("CI").is_some() {
        return;
    }
    let gpu = gpu::GpuCompositor::new().expect("Metal GPU required");
    let (device, queue) = gpu.handles();
    let pipe = smart_gpu::SmartGpu::new(device).unwrap();
    let ce = Extent::new(19, 13);
    let pe = Extent::new(9, 7);
    let input: Vec<[f32; 4]> = (0..ce.width * ce.height)
        .map(|i| {
            let a = if i % 5 == 0 { 0.0 } else { 0.7 };
            [
                ((i * 17) % 23) as f32 / 23.0 * a,
                ((i * 7) % 11) as f32 / 11.0 * a,
                0.3 * a,
                a,
            ]
        })
        .collect();
    for t in [
        Affine::scale_translate(1.0, 1.0, 0.37, -0.21),
        Affine {
            m: [0.91, -0.2, -3.0, 0.13, 1.1, -2.0],
        },
    ] {
        let coord = TileCoord::new(0, 0, 0);
        let mut plan =
            smart_gpu::SmartPlan::with_quality(t, ce, pe, coord, smart_gpu::SmartQuality::Lanczos3)
                .unwrap();
        let region = plan.child_region();
        let compact: Vec<[f32; 4]> = (region.y0..region.y1)
            .flat_map(|y| {
                let input = &input;
                (region.x0..region.x1).map(move |x| input[(y * ce.width as i64 + x) as usize])
            })
            .collect();
        plan.rebase(region);
        let src = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&compact),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(pe.width * pe.height) * 16,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut enc = device.create_command_encoder(&Default::default());
            pipe.encode(device, &mut enc, &src, &out, 0, &plan).unwrap();
            queue.submit([enc.finish()]);
            runs.push(read(device, queue, &out));
        }
        assert_eq!(runs[0], runs[1]);
        let want = lanczos_reference(&input, ce, pe, t, coord);
        for (i, (&a, &b)) in runs[0].iter().zip(&want).enumerate() {
            assert!(
                (a - b).abs() < 2e-5,
                "Lanczos sample {i}: GPU {a}, independent reference {b}"
            );
        }
    }
}

#[test]
fn resident_quality_switch_invalidates_nested_caches_and_higher_levels_stay_bilinear() {
    use compositor::{Depth, DocState, Document, Fill, Layer, LayerKind, SmartObject};
    use resident::{ResidentRenderer, SmartQuality};
    if std::env::var_os("CI").is_some() {
        return;
    }
    let gpu = gpu::GpuCompositor::new().unwrap();
    let e = Extent::new(17, 11);
    let mut child = DocState::new(e, Depth::F32);
    child.root.push(std::sync::Arc::new(Layer::new(
        "fill",
        LayerKind::Fill(Fill::Solid {
            color: [0.7, 0.2, 0.4],
        }),
    )));
    let mut middle = DocState::new(e, Depth::F32);
    middle.root.push(std::sync::Arc::new(Layer::new(
        "inner",
        LayerKind::SmartObject(SmartObject::new(
            child,
            Affine::scale_translate(1.0, 1.0, 0.37, 0.21),
        )),
    )));
    let mut parent = DocState::new(e, Depth::F32);
    parent.root.push(std::sync::Arc::new(Layer::new(
        "outer",
        LayerKind::SmartObject(SmartObject::new(middle, Affine::IDENTITY)),
    )));
    let doc = Document::new(parent);
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    renderer.render(&doc, 0).unwrap();
    let legacy = renderer.read_level(0, true).unwrap().1;
    renderer.render(&doc, 1).unwrap();
    let higher = renderer.read_level(1, true).unwrap().1;
    renderer.set_smart_quality(SmartQuality::Lanczos3).unwrap();
    assert!(renderer.read_level(0, true).is_err());
    assert!(renderer.render(&doc, 0).unwrap().smart_pages > 0);
    let quality = renderer.read_level(0, true).unwrap().1;
    assert_ne!(
        legacy, quality,
        "quality must propagate to cached nested children"
    );
    renderer.set_smart_quality(SmartQuality::Lanczos3).unwrap();
    assert_eq!(renderer.render(&doc, 0).unwrap().smart_pages, 0);
    assert_eq!(quality, renderer.read_level(0, true).unwrap().1);
    renderer.render(&doc, 1).unwrap();
    assert_eq!(higher, renderer.read_level(1, true).unwrap().1);
    renderer
        .set_smart_quality(SmartQuality::LegacyBilinear)
        .unwrap();
    assert!(renderer.render(&doc, 0).unwrap().smart_pages > 0);
    assert_eq!(legacy, renderer.read_level(0, true).unwrap().1);
}

#[test]
fn lanczos_support_reaches_beyond_object_bounds_into_adjacent_tile() {
    use compositor::{Depth, DocState, Document, Fill, Layer, LayerKind, SmartObject};
    use resident::{ResidentRenderer, SmartQuality};
    if std::env::var_os("CI").is_some() {
        return;
    }
    let gpu = gpu::GpuCompositor::new().unwrap();
    let ce = Extent::new(254, 8);
    let pe = Extent::new(260, 8);
    let mut child = DocState::new(ce, Depth::F32);
    child.root.push(std::sync::Arc::new(Layer::new(
        "fill",
        LayerKind::Fill(Fill::Solid {
            color: [0.7, 0.2, 0.4],
        }),
    )));
    let t = Affine::scale_translate(1.0, 1.0, 0.37, 0.0);
    let mut parent = DocState::new(pe, Depth::F32);
    parent.root.push(std::sync::Arc::new(Layer::new(
        "smart",
        LayerKind::SmartObject(SmartObject::new(child, t)),
    )));
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    renderer.set_smart_quality(SmartQuality::Lanczos3).unwrap();
    renderer.render(&Document::new(parent), 0).unwrap();
    let got = renderer.read_level(0, true).unwrap().1;
    let reference = lanczos_reference(
        &vec![[0.7, 0.2, 0.4, 1.0]; (ce.width * ce.height) as usize],
        ce,
        pe,
        t,
        TileCoord::new(0, 1, 0),
    );
    let expected_alpha = reference[3 * 4 * 8 + 3 * 4];
    assert!(expected_alpha > 0.0);
    assert!(
        (got[(3 * 260 + 256) * 4 + 3] - expected_alpha).abs() < 2e-5,
        "Lanczos halo lost at object/tile boundary"
    );
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
