use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::tile::Extent;
use filters::{
    liquify::{Interpolation, Mesh},
    liquify_gpu::GpuLiquify,
};
use std::sync::atomic::AtomicBool;

fn fixture(w: u32, h: u32) -> Raster {
    let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.);
    r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
        *p = [
            ((x * 13 + y * 7) % 31) as f32 / 7. - 1.,
            ((x + y) % 11) as f32 / 3.,
            -0.3,
            ((x * 3 + y) % 17) as f32 / 16.,
        ];
    })
    .unwrap();
    r
}
#[test]
#[cfg(target_os = "macos")]
fn nonidentity_bilinear_matches_cpu_hdr_alpha_edges() {
    parity(Interpolation::Bilinear);
}
#[test]
#[cfg(target_os = "macos")]
fn nonidentity_bicubic_matches_cpu_hdr_alpha_edges() {
    parity(Interpolation::Bicubic);
}
fn parity(mode: Interpolation) {
    let gpu = GpuLiquify::new().expect("Metal required on macOS");
    let cancel = AtomicBool::new(false);
    let r = fixture(37, 29);
    let mut mesh = Mesh::new(37, 29, 7).unwrap();
    for (i, d) in mesh.displacement.iter_mut().enumerate() {
        *d = [(i % 5) as f32 * 1.7 - 3.2, (i % 7) as f32 * 0.9 - 2.7];
    }
    let cpu = mesh.render(&r, mode, &cancel).unwrap();
    let out = gpu.render(&mesh, &r, mode, &cancel).unwrap();
    let (profiled, timing) = gpu.render_profiled(&mesh, &r, mode, &cancel).unwrap();
    assert!(timing.is_none_or(|t| !t.is_zero()));
    let mut changed = false;
    for y in 0..29 {
        for x in 0..37 {
            for c in 0..4 {
                assert!(
                    (cpu.pixel(x, y)[c] - out.pixel(x, y)[c]).abs() <= 1e-4,
                    "at {x},{y},{c}: {:?} != {:?}",
                    cpu.pixel(x, y),
                    out.pixel(x, y)
                );
                assert!((cpu.pixel(x, y)[c] - profiled.pixel(x, y)[c]).abs() <= 1e-4);
                changed |= out.pixel(x, y)[c] != r.pixel(x, y)[c];
            }
        }
    }
    assert!(changed);
}

#[test]
#[cfg(target_os = "macos")]
fn rejects_corruption_cancel_and_preserves_identity() {
    let gpu = GpuLiquify::new().unwrap();
    let r = fixture(9, 7);
    let cancel = AtomicBool::new(false);
    let mesh = Mesh::new(9, 7, 4).unwrap();
    let out = gpu
        .render(&mesh, &r, Interpolation::Bicubic, &cancel)
        .unwrap();
    assert!(out.shares_all_tiles_with(&r));
    assert_eq!(out.max_rev(), r.max_rev());
    for case in 0..8 {
        let mut bad = mesh.clone();
        match case {
            0 => bad.displacement.pop().map(|_| ()).unwrap(),
            1 => bad.freeze.clear(),
            2 => bad.version += 1,
            3 => bad.cell_size = 0,
            4 => bad.displacement[0][0] = f32::NAN,
            5 => bad.freeze[0] = f32::INFINITY,
            6 => bad.freeze[0] = -0.1,
            _ => bad.width = 0,
        }
        assert!(
            gpu.render(&bad, &r, Interpolation::Bilinear, &cancel)
                .is_err(),
            "case {case}"
        );
    }
    let wrong = Mesh::new(10, 7, 4).unwrap();
    assert!(
        gpu.render(&wrong, &r, Interpolation::Bilinear, &cancel)
            .is_err()
    );
    assert!(matches!(
        gpu.render(&mesh, &r, Interpolation::Bilinear, &AtomicBool::new(true)),
        Err(engine_api::EngineError::Cancelled)
    ));
    let bad = Raster::new(Extent::new(9, 7), 4, Depth::F32, f32::NAN);
    assert!(
        gpu.render(&mesh, &bad, Interpolation::Bilinear, &cancel)
            .is_err()
    );
}

#[test]
#[cfg(target_os = "macos")]
fn thin_images_and_tile_boundaries() {
    let gpu = GpuLiquify::new().unwrap();
    let cancel = AtomicBool::new(false);
    for (w, h) in [(1, 1), (1, 33), (33, 1), (263, 259)] {
        let r = fixture(w, h);
        let mut mesh = Mesh::new(w, h, 16).unwrap();
        mesh.displacement.fill([0.37, -0.83]);
        for mode in [Interpolation::Bilinear, Interpolation::Bicubic] {
            let cpu = mesh.render(&r, mode, &cancel).unwrap();
            let out = gpu.render(&mesh, &r, mode, &cancel).unwrap();
            for y in 0..h {
                for x in 0..w {
                    for c in 0..4 {
                        assert!((cpu.pixel(x, y)[c] - out.pixel(x, y)[c]).abs() <= 1e-4);
                    }
                }
            }
        }
    }
}

#[test]
#[cfg(target_os = "macos")]
fn resident_changed_mesh_rerenders_without_reupload() {
    let gpu = GpuLiquify::new().unwrap();
    let cancel = AtomicBool::new(false);
    let r = fixture(263, 259);
    for mode in [Interpolation::Bilinear, Interpolation::Bicubic] {
        let mut mesh = Mesh::new(263, 259, 16).unwrap();
        let mut job = gpu.prepare(&r, &mesh, mode, &cancel).unwrap();
        assert!(job.readback(&cancel).is_err(), "must dispatch first");
        let output = job.output_buffer().clone();
        assert_eq!(output.size(), 263 * 259 * 16);
        assert!(output.usage().contains(wgpu::BufferUsages::STORAGE));
        job.submit_wait(&cancel).unwrap();
        let first = job.readback(&cancel).unwrap();
        for (i, d) in mesh.displacement.iter_mut().enumerate() {
            *d = [(i % 5) as f32 * 1.7 - 3.2, (i % 7) as f32 * 0.9 - 2.7];
        }
        job.update_mesh(&mesh, &cancel).unwrap();
        assert!(
            job.readback(&cancel).is_err(),
            "updated mesh needs dispatch"
        );
        job.submit_wait(&cancel).unwrap();
        assert_eq!(
            &output,
            job.output_buffer(),
            "output allocation is retained"
        );
        let second = job.readback(&cancel).unwrap();
        let cpu = mesh.render(&r, mode, &cancel).unwrap();
        let mut changed = false;
        for y in 0..259 {
            for x in 0..263 {
                for c in 0..4 {
                    assert!((first.pixel(x, y)[c] - r.pixel(x, y)[c]).abs() <= 1e-4);
                    assert!((second.pixel(x, y)[c] - cpu.pixel(x, y)[c]).abs() <= 1e-4);
                    changed |= first.pixel(x, y)[c] != second.pixel(x, y)[c];
                }
            }
        }
        assert!(changed);
    }
}

#[test]
#[cfg(target_os = "macos")]
fn resident_validation_cancellation_and_downstream_stage() {
    let gpu = GpuLiquify::new().unwrap();
    let cancel = AtomicBool::new(false);
    let stopped = AtomicBool::new(true);
    let r = fixture(37, 29);
    let mut mesh = Mesh::new(37, 29, 7).unwrap();
    mesh.displacement.fill([0.375, -0.625]);
    assert!(matches!(
        gpu.prepare(&r, &mesh, Interpolation::Bilinear, &stopped),
        Err(engine_api::EngineError::Cancelled)
    ));
    assert!(
        gpu.prepare(&fixture(38, 29), &mesh, Interpolation::Bilinear, &cancel)
            .is_err()
    );
    let invalid_raster = Raster::new(r.extent(), 4, Depth::F32, f32::NAN);
    assert!(
        gpu.prepare(&invalid_raster, &mesh, Interpolation::Bilinear, &cancel)
            .is_err()
    );
    let mut job = gpu
        .prepare(&r, &mesh, Interpolation::Bilinear, &cancel)
        .unwrap();
    assert!(matches!(
        job.submit_wait(&stopped),
        Err(engine_api::EngineError::Cancelled)
    ));
    job.submit_wait(&cancel).unwrap();
    for bad in [
        Mesh::new(38, 29, 7).unwrap(),
        Mesh::new(37, 30, 7).unwrap(),
        Mesh::new(37, 29, 8).unwrap(),
    ] {
        assert!(job.update_mesh(&bad, &cancel).is_err());
    }
    for case in 0..4 {
        let mut bad = mesh.clone();
        match case {
            0 => bad.displacement[0][0] = f32::NAN,
            1 => {
                bad.displacement.pop();
            }
            2 => bad.freeze[0] = -1.,
            _ => bad.version += 1,
        }
        assert!(job.update_mesh(&bad, &cancel).is_err());
    }
    assert!(matches!(
        job.update_mesh(&mesh, &stopped),
        Err(engine_api::EngineError::Cancelled)
    ));
    assert!(matches!(
        job.readback(&stopped),
        Err(engine_api::EngineError::Cancelled)
    ));
    let cpu = mesh.render(&r, Interpolation::Bilinear, &cancel).unwrap();
    // Consume resident output in a separate real compute stage, with no Raster
    // roundtrip. This also verifies the exposed device/queue are the correct ones.
    let device = gpu.device();
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("downstream test"),
        source: wgpu::ShaderSource::Wgsl("@group(0) @binding(0) var<storage, read> src: array<vec4<f32>>; @group(0) @binding(1) var<storage, read_write> dst: array<vec4<f32>>; @compute @workgroup_size(1) fn main() { dst[0] = src[42] * 2.; }".into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &shader,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: job.output_buffer().as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &staging, 0, 16);
    gpu.queue().submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    rx.recv().unwrap().unwrap();
    let view = staging.slice(..).get_mapped_range().unwrap();
    let pixel: &[f32] = bytemuck::cast_slice(&view);
    for (c, value) in pixel.iter().enumerate() {
        assert!((*value - 2. * cpu.pixel(5, 1)[c]).abs() < 1e-4);
    }
    drop(view);
    staging.unmap();
    // Failed updates/cancellation did not replace the valid output, nor poison
    // subsequent readbacks or reuse of the persistent staging buffer.
    for _ in 0..2 {
        let out = job.readback(&cancel).unwrap();
        for c in 0..4 {
            assert!((out.pixel(5, 1)[c] - cpu.pixel(5, 1)[c]).abs() < 1e-4);
        }
    }
}

/// Run with --release --ignored --nocapture; asserts CPU and resident wall budgets.
#[test]
#[ignore = "24MP real CPU/Metal performance measurement"]
#[cfg(target_os = "macos")]
fn benchmark_24mp() {
    use std::time::Instant;
    let gpu = GpuLiquify::new().unwrap();
    let cancel = AtomicBool::new(false);
    let r = fixture(6000, 4000);
    let mut mesh = Mesh::new(6000, 4000, 32).unwrap();
    mesh.displacement.fill([0.375, -0.625]);
    for mode in [Interpolation::Bilinear, Interpolation::Bicubic] {
        let start = Instant::now();
        let mut resident = gpu.prepare(&r, &mesh, mode, &cancel).unwrap();
        let upload_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        resident.submit_wait(&cancel).unwrap();
        let first_dispatch_ms = start.elapsed().as_secs_f64() * 1000.;
        let mut resident_ms = Vec::new();
        // Change the mesh every frame. The measured wall includes encoding,
        // submission, mesh-upload flush, and completion wait, not GPU timestamps.
        for frame in 0..10 {
            mesh.displacement
                .fill([0.375 + frame as f32 * 0.03125, -0.625]);
            resident.update_mesh(&mesh, &cancel).unwrap();
            let start = Instant::now();
            resident.submit_wait(&cancel).unwrap();
            resident_ms.push(start.elapsed().as_secs_f64() * 1000.);
        }
        let start = Instant::now();
        let downloaded = resident.readback(&cancel).unwrap();
        let download_ms = start.elapsed().as_secs_f64() * 1000.;
        drop(resident);
        // Warm the existing Raster roundtrip independently, not identity.
        drop(gpu.render(&mesh, &r, mode, &cancel).unwrap());
        let start = Instant::now();
        let cpu = mesh.render(&r, mode, &cancel).unwrap();
        let cpu_ms = start.elapsed().as_secs_f64() * 1000.;
        let start = Instant::now();
        let (out, dispatch) = gpu.render_profiled(&mesh, &r, mode, &cancel).unwrap();
        let gpu_ms = start.elapsed().as_secs_f64() * 1000.;
        for (x, y) in [(0, 0), (17, 31), (255, 256), (3021, 2087), (5999, 3999)] {
            for c in 0..4 {
                assert!((cpu.pixel(x, y)[c] - out.pixel(x, y)[c]).abs() <= 1e-4);
            }
        }
        resident_ms.sort_by(f64::total_cmp);
        let max_ms = resident_ms[resident_ms.len() - 1];
        eprintln!(
            "24MP {mode:?}: cold prepare/upload {upload_ms:.3} ms; first resident submit+wait {first_dispatch_ms:.3} ms; resident render WALL min/median/max {:.3}/{:.3}/{max_ms:.3} ms (10 changed-mesh frames, target <25); download+Raster {download_ms:.3} ms; CPU end-to-end {cpu_ms:.3} ms (target <300); existing Raster GPU roundtrip {gpu_ms:.3} ms (NOT a <25 ms path); timestamp dispatch {:?} ms",
            resident_ms[0],
            resident_ms[resident_ms.len() / 2],
            dispatch.map(|d| d.as_secs_f64() * 1000.)
        );
        // Emit every measured stage even when a budget fails, so CPU
        // contention does not hide the independent GPU timing evidence.
        assert!(
            cpu_ms < 300.,
            "{mode:?} CPU end-to-end {cpu_ms:.3} ms exceeds 300 ms"
        );
        for (x, y) in [(0, 0), (17, 31), (255, 256), (3021, 2087), (5999, 3999)] {
            for c in 0..4 {
                assert!((cpu.pixel(x, y)[c] - downloaded.pixel(x, y)[c]).abs() <= 1e-4);
            }
        }
        assert!(
            first_dispatch_ms < 25.,
            "{mode:?} first resident submit+wait {first_dispatch_ms:.3} ms exceeds 25 ms"
        );
        assert!(
            max_ms < 25.,
            "{mode:?} resident submit+wait max {max_ms:.3} ms exceeds 25 ms"
        );
    }
}
