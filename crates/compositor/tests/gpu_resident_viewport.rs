mod common;
use common::*;
use compositor::{gpu::GpuCompositor, resident::ResidentRenderer, *};
use engine_api::tile::Extent;

fn present_bytes(g: &GpuCompositor, r: &ResidentRenderer, rect: Rect) -> Vec<u8> {
    present_level_bytes(g, r, 0, rect)
}

fn present_level_bytes(g: &GpuCompositor, r: &ResidentRenderer, level: u8, rect: Rect) -> Vec<u8> {
    let (device, queue) = g.handles();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: rect.width() as u32,
            height: rect.height() as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    r.present(level, &texture, rect, (0, 0), None).unwrap();
    let out = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256 * rect.height() as u64,
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
                rows_per_image: Some(rect.height() as u32),
            },
        },
        texture.size(),
    );
    queue.submit([encoder.finish()]);
    gpu_core::read_buffer(device, queue, &out, 0, out.size()).unwrap()
}

#[test]
fn nonzero_viewport_rgba8_matches_full_render_after_pan_and_specialization() {
    if std::env::var_os("CI").is_some() {
        return;
    }
    let g = GpuCompositor::new().expect("Metal required");
    let e = Extent::new(97, 65);
    let mut d = doc(e, Depth::U8);
    add(
        &mut d,
        None,
        layer_fn("pattern", e, Depth::U8, |x, y| {
            [(x % 19) as f32 / 18.0, (y % 11) as f32 / 10.0, 0.2, 0.7]
        }),
    );
    let mut full = ResidentRenderer::new(&g).unwrap();
    full.render(&d, 0).unwrap();
    let mut r = ResidentRenderer::new(&g).unwrap();
    for rect in [
        Rect::new(33, 17, 61, 40),
        Rect::new(49, 33, 77, 56),
        Rect::new(81, 49, 97, 65),
    ] {
        r.render_viewport(&d, 0, rect, 0).unwrap();
        assert_eq!(present_bytes(&g, &r, rect), present_bytes(&g, &full, rect));
        r.wait_for_specializations();
        r.invalidate();
        r.render_viewport(&d, 0, rect, 0).unwrap();
        assert_eq!(present_bytes(&g, &r, rect), present_bytes(&g, &full, rect));
    }
}

#[test]
fn huge_smart_child_renders_only_the_sampling_window() {
    if std::env::var_os("CI").is_some() {
        return;
    }
    let g = GpuCompositor::new().expect("Metal required");
    let child = DocState::new(Extent::new(16384, 16384), Depth::F32);
    let mut d = doc(Extent::new(1024, 1024), Depth::F32);
    add(
        &mut d,
        None,
        Layer::new(
            "large child",
            LayerKind::SmartObject(SmartObject::new(
                child,
                Affine::scale_translate(1.0, 1.0, -4096.25, -2048.5),
            )),
        ),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.render_viewport(&d, 0, Rect::new(512, 256, 544, 288), 0)
        .unwrap();
    r.wait().unwrap();
}

#[test]
fn compact_smart_windows_match_cpu_across_parent_tiles() {
    if std::env::var_os("CI").is_some() {
        return;
    }
    let g = GpuCompositor::new().expect("Metal required");
    let ce = Extent::new(16384, 16384);
    let mut child = DocState::new(ce, Depth::F32);
    let mut layer = Layer::pixel("sparse", ce, Depth::F32);
    layer.id = LayerId(1);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::new(4090, 2040, 4700, 2350), 1, |x, y, p| {
            *p = [(x % 31) as f32 / 30.0, (y % 17) as f32 / 16.0, 0.3, 0.7];
        })
        .unwrap();
    child.root.push(std::sync::Arc::new(layer));
    child.next_id = 2;
    let e = Extent::new(513, 271);
    let mut d = doc(e, Depth::F32);
    add(
        &mut d,
        None,
        Layer::new(
            "smart",
            LayerKind::SmartObject(SmartObject::new(
                child,
                Affine::scale_translate(1.0, 1.0, -4096.25, -2048.5),
            )),
        ),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.render_viewport(&d, 0, Rect::of_extent(e), 0).unwrap();
    let cpu = Compositor::new(64 << 20);
    for tile in r.read_tiles(0).unwrap() {
        let want = cpu.render_tile_premultiplied(&d, tile.coord()).unwrap();
        assert_close(
            tile.samples::<f32>().unwrap(),
            want.samples::<f32>().unwrap(),
            1e-4,
            "compact smart child",
        );
    }
}

#[test]
fn output_storage_tracks_viewport_not_canvas() {
    if std::env::var_os("CI").is_some() {
        return;
    }
    let g = GpuCompositor::new().expect("Metal required");
    let d = doc(Extent::new(4096, 4096), Depth::F32);
    let mut r = ResidentRenderer::new(&g).unwrap();
    for rect in [
        Rect::new(1024, 512, 1056, 544),
        Rect::new(2048, 2048, 2064, 2064),
    ] {
        r.render_viewport(&d, 0, rect, 0).unwrap();
        assert_eq!(
            r.stats().resident_bytes,
            rect.width() as u64 * rect.height() as u64 * 16
        );
        assert!(r.read_level(0, true).is_err());
    }
}

#[test]
fn oversized_full_render_does_not_poison_viewport_mips() {
    let g = GpuCompositor::new().expect("Metal required");
    let e = Extent::new(32768, 32768);
    let mut d = doc(e, Depth::U8);
    let mut layer = Layer::pixel("sparse red", e, Depth::U8);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::new(0, 0, 32, 32), 1, |_, _, p| {
            *p = [1.0, 0.0, 0.0, 1.0]
        })
        .unwrap();
    add(&mut d, None, layer);
    let mut r = ResidentRenderer::new(&g).unwrap();
    assert!(matches!(
        r.render(&d, 1),
        Err(engine_api::EngineError::ResourceExhausted { .. })
    ));
    let rect = Rect::new(0, 0, 16, 16);
    r.render_viewport(&d, 1, rect, 0).unwrap();
    let bytes = present_level_bytes(&g, &r, 1, rect);
    for y in 0..16 {
        for x in 0..16 {
            assert_eq!(
                &bytes[y * 256 + x * 4..y * 256 + x * 4 + 4],
                &[255, 0, 0, 255]
            );
        }
    }
}
