//! End-to-end resident viewport -> EDR presentation, not just the output kernel.
mod common;
use common::*;
use compositor::{
    gpu::GpuCompositor,
    resident::{ResidentRenderer, SourceColorPolicy},
    *,
};
use engine_api::tile::Extent;

#[test]
fn resident_viewport_presents_edr_and_rejects_unrendered_regions() {
    let g = GpuCompositor::new().expect("Metal required");
    let (device, queue) = g.handles();
    let e = Extent::new(32, 16);
    let mut d = doc(e, Depth::F32);
    add(
        &mut d,
        None,
        layer_fn("HDR", e, Depth::F32, |_, _| [3.0, -0.25, 0.5, 0.5]),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.render_viewport(&d, 0, Rect::new(0, 0, 16, 16), 0)
        .unwrap();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("resident EDR integration"),
        size: wgpu::Extent3d {
            width: 16,
            height: 16,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    assert!(
        r.present_managed(
            0,
            &texture,
            Rect::new(16, 0, 32, 16),
            (0, 0),
            None,
            SourceColorPolicy::DisplayLinear,
            None
        )
        .is_err()
    );
    assert!(
        r.present_managed(
            0,
            &texture,
            Rect::new(0, 0, 16, 16),
            (0, 0),
            None,
            SourceColorPolicy::DocumentEncoded,
            None
        )
        .is_err()
    );
    r.present_managed(
        0,
        &texture,
        Rect::new(0, 0, 16, 16),
        (0, 0),
        None,
        SourceColorPolicy::DisplayLinear,
        None,
    )
    .unwrap();
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * 16,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &out,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(16),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    let bytes = gpu_core::read_buffer(device, queue, &out, 0, out.size()).unwrap();
    for y in 0..16 {
        for x in 0..16 {
            let start = y * 256 + x * 8;
            let got: Vec<_> = bytes[start..start + 8]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|v| half::f16::from_bits(u16::from_le_bytes([v[0], v[1]])).to_f32())
                .collect();
            assert_eq!(got, [1.5, -0.125, 0.25, 0.5]);
        }
    }
}
