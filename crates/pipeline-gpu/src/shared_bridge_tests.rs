use crate::{GpuContext, GpuStageOp};
use engine_api::{
    jobs::CancellationToken,
    recipe::settings::ToneSettings,
    tile::{Extent, TileCoord, TileLayout},
};
use image_core::{Op, StageOp};
use std::sync::Arc;
use wgpu::util::DeviceExt;

#[test]
fn bridge_rejects_invalid_imports_and_foreign_exports() {
    let owned = GpuContext::new().unwrap();
    assert_eq!(owned.shared().device, owned.device);
    assert!(owned.shared_device().is_some());
    // Keep both devices in one wgpu Instance: resource IDs from unrelated
    // Instances are not a reliable provenance check in wgpu 30.
    let mut desc = wgpu::InstanceDescriptor::new_without_display_handle();
    desc.backends = wgpu::Backends::METAL;
    let instance = wgpu::Instance::new(desc);
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let context = || {
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: gpu_core::limits(&adapter.limits()),
            ..Default::default()
        }))
        .unwrap();
        GpuContext::from_device(&device, &queue).unwrap()
    };
    let ctx = context();
    let other = context();
    let layout = TileLayout {
        extent: Extent::new(1, 1),
        halo: 0,
        channels: 3,
    };
    let coord = TileCoord::new(0, 0, 0);
    let make = |device: &wgpu::Device, size, usage| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage,
            mapped_at_creation: false,
        })
    };
    let usage = wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC;
    let good = make(&ctx.device, 12, usage);
    assert!(
        ctx.import_resident_buffer(coord, layout, &make(&ctx.device, 8, usage))
            .is_err()
    );
    assert!(
        ctx.import_resident_buffer(
            coord,
            layout,
            &make(&ctx.device, 12, wgpu::BufferUsages::COPY_SRC)
        )
        .is_err()
    );
    for bad in [
        TileLayout {
            channels: 0,
            ..layout
        },
        TileLayout {
            extent: Extent::new(0, 1),
            ..layout
        },
        TileLayout {
            extent: Extent::new(u32::MAX, u32::MAX),
            halo: u16::MAX,
            channels: u8::MAX,
        },
    ] {
        assert!(ctx.import_resident_buffer(coord, bad, &good).is_err());
    }
    assert!(
        ctx.import_resident_buffer(coord, layout, &make(&other.device, 12, usage))
            .is_err()
    );
    let foreign = image_core::resident::ResidentTile {
        coord,
        layout,
        storage: Arc::new(()),
    };
    assert!(ctx.resident_buffer(&foreign).is_err());
    let tile = ctx.import_resident_buffer(coord, layout, &good).unwrap();
    assert!(other.resident_buffer(&tile).is_err());
    let gpu = GpuStageOp::new(Arc::new(other));
    let mut batch = gpu.begin_resident().unwrap();
    assert!(
        batch
            .run(&Op::Tone(&ToneSettings::default()), &tile)
            .is_err()
    );
}

#[test]
fn exported_packed_view_reports_half_pairs_and_retains_storage() {
    use image_core::resident::ResidentBatch;
    let ctx = Arc::new(GpuContext::new().unwrap());
    let gpu = GpuStageOp::new(ctx.clone());
    let mut batch = super::Batch::new(&gpu);
    let layout = TileLayout {
        extent: Extent::new(3, 1),
        halo: 0,
        channels: 1,
    };
    let input = engine_api::tile::Tile::from_samples(
        TileCoord::new(0, 0, 0),
        layout,
        vec![0.25_f32, 0.5, 1.0],
    )
    .unwrap();
    let source = batch.upload(&input).unwrap();
    let packed = batch.convert(&source, true).unwrap();
    let view = ctx.resident_buffer(&packed).unwrap();
    assert!(view.packed());
    assert_eq!(view.layout(), layout);
    assert_eq!(view.buffer().size(), 8);
    drop(packed);
    assert_eq!(batch.pool.lock().unwrap().free.len(), 0);
    Box::new(batch)
        .finish(vec![], false, None, &CancellationToken::new())
        .unwrap();
    let bytes = gpu_core::read_buffer(&ctx.device, &ctx.queue, view.buffer(), 0, 8).unwrap();
    let words: &[u32] = bytemuck::cast_slice(&bytes);
    assert_eq!(words[0], 0x3800_3400);
    assert_eq!(words[1] & 0xffff, 0x3c00);
}

#[test]
fn shared_device_resident_bridge_runs_without_pixel_transfers() {
    let shared = gpu_core::GpuDevice::new().unwrap();
    let ctx = Arc::new(GpuContext::from_device(&shared.device, &shared.queue).unwrap());
    assert_eq!(ctx.device, shared.device);
    assert_eq!(ctx.queue, shared.queue);
    assert!(ctx.shared_device().is_none());
    let layout = TileLayout {
        extent: Extent::new(2, 1),
        halo: 0,
        channels: 3,
    };
    let input = [0.1_f32, 0.2, 0.3, 0.4, 0.5, 0.6];
    let buffer = shared
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&input),
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        });
    let tile = ctx
        .import_resident_buffer(TileCoord::new(0, 0, 0), layout, &buffer)
        .unwrap();
    let imported = ctx.resident_buffer(&tile).unwrap();
    assert_eq!(imported.buffer(), &buffer);
    assert_eq!(imported.layout(), layout);
    assert!(!imported.packed());
    let gpu = GpuStageOp::new(ctx.clone());
    let mut batch = gpu.begin_resident().unwrap();
    let tone = ToneSettings {
        exposure: 1.0,
        ..Default::default()
    };
    let output = batch.run(&Op::Tone(&tone), &tile).unwrap();
    let exported = ctx.resident_buffer(&output).unwrap();
    drop(output); // The exported guard must keep the pooled allocation alive.
    let _later = batch
        .run(
            &Op::Tone(&ToneSettings {
                exposure: 2.0,
                ..Default::default()
            }),
            &tile,
        )
        .unwrap();
    let completion = batch
        .finish(vec![], false, None, &CancellationToken::new())
        .unwrap();
    assert!(completion.tiles.is_empty());
    assert_eq!(gpu.stats().uploads, 0);
    assert_eq!(gpu.stats().readbacks, 0);
    assert_eq!(gpu.stats().pixel_readback_bytes, 0);
    assert_eq!(gpu.stats().submissions, 1);
    assert!(!exported.packed());
    // Explicit test-only readback, outside the resident bridge.
    let bytes =
        gpu_core::read_buffer(&shared.device, &shared.queue, exported.buffer(), 0, 24).unwrap();
    let actual: &[f32] = bytemuck::cast_slice(&bytes);
    for (a, b) in actual.iter().zip(input) {
        assert!((a - b * 2.0).abs() < 0.0001, "{a} vs {b}");
    }
}
