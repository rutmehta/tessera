mod common;
use common::*;
use compositor::{
    gpu::GpuCompositor,
    resident::{DisplayDestination, Headroom, ResidentRenderer, SourceDomain},
    *,
};
use engine_api::tile::Extent;
use gpu_core::color_mgmt::{Builtin, Registry, TransformOptions};

#[test]
fn profiled_viewport_rebases_and_rejects_stale_or_foreign_document() {
    let g = GpuCompositor::new().expect("Metal required");
    let e = Extent::new(97, 65);
    let mut d = doc(e, Depth::F32);
    add(
        &mut d,
        None,
        layer_fn("pattern", e, Depth::F32, |x, y| {
            [(x % 19) as f32 / 18.0, (y % 11) as f32 / 10.0, 0.2, 0.7]
        }),
    );
    let mut full = ResidentRenderer::new(&g).unwrap();
    let mut compact = ResidentRenderer::new(&g).unwrap();
    full.render(&d, 0).unwrap();
    let rect = Rect::new(33, 17, 61, 40);
    compact.render_viewport(&d, 0, rect, 0).unwrap();
    let (device, queue) = g.handles();
    let mut registry = Registry::new();
    let dest = registry.builtin(Builtin::DisplayP3).unwrap();
    let t = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 28,
            height: 23,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let present = |r: &ResidentRenderer, document: &Document| {
        r.present_profiled(
            document,
            0,
            &t,
            rect,
            (0, 0),
            None,
            SourceDomain::EncodedUnit,
            DisplayDestination::Encoded(dest.clone()),
            TransformOptions::default(),
            Headroom::default(),
        )
    };
    let read = || {
        let b = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: 256 * 23,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            t.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &b,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(256),
                    rows_per_image: Some(23),
                },
            },
            t.size(),
        );
        queue.submit([encoder.finish()]);
        gpu_core::read_buffer(device, queue, &b, 0, b.size()).unwrap()
    };
    present(&full, &d).unwrap();
    let expected = read();
    present(&compact, &d).unwrap();
    assert_eq!(expected, read());
    present(&compact, &d).unwrap();
    assert_eq!(compact.output_cache_stats().preparations, 1);
    let foreign = Document::new((**d.state()).clone());
    assert!(present(&compact, &foreign).is_err());
    add(
        &mut d,
        None,
        layer_fn("new", e, Depth::F32, |_, _| [0.3, 0.2, 0.1, 1.0]),
    );
    assert!(present(&compact, &d).is_err());
    compact.render_viewport(&d, 0, rect, 0).unwrap();
    present(&compact, &d).unwrap();
}
