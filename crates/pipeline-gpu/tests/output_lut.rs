use engine_api::tile::{Extent, Tile, TileCoord, TileLayout};
use pipeline_gpu::{GpuContext, GpuOutputLut};
use std::sync::Arc;

fn nodes() -> Vec<[f32; 3]> {
    (0..33 * 33 * 33)
        .map(|i| {
            let r = (i % 33) as f32 / 32.;
            let g = ((i / 33) % 33) as f32 / 32.;
            let b = (i / (33 * 33)) as f32 / 32.;
            [r * r + g * 0.17, g * b - 0.2, b * b + r * g * 0.3]
        })
        .collect()
}

#[test]
fn rejects_wrong_size_or_nonfinite_lut_nodes() {
    let ctx = Arc::new(GpuContext::new().unwrap());
    for count in [0, 1, 33 * 33 * 33 - 1, 33 * 33 * 33 + 1] {
        assert!(
            GpuOutputLut::new(ctx.clone(), &vec![[0.; 3]; count]).is_err(),
            "count {count}"
        );
    }
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut values = nodes();
        values[123][1] = bad;
        assert!(GpuOutputLut::new(ctx.clone(), &values).is_err());
    }
}

#[test]
fn rejects_non_rgb_and_nonfinite_input() {
    let lut = GpuOutputLut::new(Arc::new(GpuContext::new().unwrap()), &nodes()).unwrap();
    for channels in [1, 4] {
        let layout = TileLayout {
            extent: Extent::new(1, 1),
            halo: 0,
            channels,
        };
        let input = Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![0.5f32; layout.len()])
            .unwrap();
        assert!(lut.apply(&input).is_err(), "channels {channels}");
    }
    let layout = TileLayout {
        extent: Extent::new(1, 1),
        halo: 0,
        channels: 3,
    };
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let input = Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![bad, 0., 1.]).unwrap();
        assert!(lut.apply(&input).is_err());
    }
    let input = Tile::from_samples(TileCoord::new(0, 0, 0), layout, vec![1u8; 3]).unwrap();
    assert!(lut.apply(&input).is_err());
}

#[test]
fn resident_buffers_chain_without_host_pixel_roundtrip() {
    use wgpu::util::DeviceExt;
    let ctx = Arc::new(GpuContext::new().unwrap());
    let values = nodes();
    let lut = GpuOutputLut::new(ctx.clone(), &values).unwrap();
    let rgb = [0.27f32, 0.59, 0.83];
    let source = ctx
        .device
        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: None,
            contents: bytemuck::cast_slice(&rgb),
            usage: wgpu::BufferUsages::STORAGE,
        });
    let mut encoder = ctx.device.create_command_encoder(&Default::default());
    let first = lut.encode(&mut encoder, &source).unwrap();
    let second = lut.encode(&mut encoder, &first).unwrap();
    let staging = ctx.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 12,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&second, 0, &staging, 0, 12);
    ctx.queue.submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |r| tx.send(r).unwrap());
    ctx.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    let mapped = staging.slice(..).get_mapped_range().unwrap();
    let actual: &[f32] = bytemuck::cast_slice(&mapped);
    let expected = reference(&values, reference(&values, rgb));
    for c in 0..3 {
        assert!((actual[c] - expected[c]).abs() < 2e-6);
    }
    drop(mapped);
    staging.unmap();
}

// Independent weighted-corner CPU reference (not the shader's nested lerps).
fn reference(nodes: &[[f32; 3]], rgb: [f32; 3]) -> [f32; 3] {
    let p = rgb.map(|v| v.clamp(0., 1.) * 32.);
    let lo = p.map(|v| (v.floor() as usize).min(31));
    let t = std::array::from_fn::<_, 3, _>(|c| p[c] - lo[c] as f32);
    let mut out = [0.; 3];
    for z in 0..2 {
        for y in 0..2 {
            for x in 0..2 {
                let w = [x, y, z]
                    .iter()
                    .enumerate()
                    .map(|(c, &d)| if d == 0 { 1. - t[c] } else { t[c] })
                    .product::<f32>();
                let node = nodes[(lo[2] + z) * 33 * 33 + (lo[1] + y) * 33 + lo[0] + x];
                for c in 0..3 {
                    out[c] += w * node[c];
                }
            }
        }
    }
    out
}

#[test]
fn gpu_output_lut_matches_cpu_trilinear_with_halos_and_clamping() {
    let ctx = Arc::new(GpuContext::new().unwrap());
    let nodes = nodes();
    let lut = GpuOutputLut::new(ctx, &nodes).unwrap();
    let layout = TileLayout {
        extent: Extent::new(17, 9),
        halo: 2,
        channels: 3,
    };
    let n = layout.plane_len();
    let mut samples: Vec<f32> = (0..layout.len())
        .map(|i| ((i * 73) % 1021) as f32 / 800. - 0.1)
        .collect();
    for (i, rgb) in [
        [0., 0., 0.],
        [1., 1., 1.],
        [1., 0., 0.],
        [0., 1., 0.],
        [0., 0., 1.],
        [-f32::MAX, 0.5, f32::MAX],
        [0.25, 0.75, 0.5],
    ]
    .iter()
    .enumerate()
    {
        for c in 0..3 {
            samples[c * n + i] = rgb[c];
        }
    }
    let tile = Tile::from_samples(TileCoord::new(2, 1, 3), layout, samples.clone()).unwrap();
    let actual = lut.apply(&tile).unwrap();
    assert_eq!(actual.coord(), tile.coord());
    assert_eq!(actual.layout(), layout);
    let actual = actual.samples::<f32>().unwrap();
    for i in 0..n {
        let expected = reference(&nodes, [samples[i], samples[n + i], samples[2 * n + i]]);
        for c in 0..3 {
            assert!(
                (actual[c * n + i] - expected[c]).abs() < 2e-6,
                "pixel {i} channel {c}: {} vs {}",
                actual[c * n + i],
                expected[c]
            );
        }
    }
}
