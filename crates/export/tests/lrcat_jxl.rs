//! Synthetic camera-channel fixture: no photo data.
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};
#[test]
fn jxl_linear_dng_preserves_16_bit_camera_codes() {
    let samples: Vec<u16> = (0..16)
        .flat_map(|y| {
            (0..16).flat_map(move |x| [1000 + x * 2000, 2000 + y * 1700, 3000 + (x + y) * 700])
        })
        .collect();
    let data: Vec<u8> = samples.iter().flat_map(|v| v.to_ne_bytes()).collect();
    let mut tile = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(
        &data,
        EncoderOptions::new(16, 16, ColorSpace::RGB, BitDepth::Sixteen),
    )
    .encode(&mut tile)
    .unwrap();
    for strips in [false, true] {
        let mut dng = support::lossy_dng_with_jpeg(false, strips, &tile);
        let n = u16::from_le_bytes(dng[38..40].try_into().unwrap()) as usize;
        for i in 0..n {
            let p = 40 + i * 12;
            let tag = u16::from_le_bytes(dng[p..p + 2].try_into().unwrap());
            if tag == 259 {
                dng[p + 8..p + 10].copy_from_slice(&52546_u16.to_le_bytes());
            }
            if tag == 258 {
                let offset = u32::from_le_bytes(dng[p + 8..p + 12].try_into().unwrap()) as usize;
                for c in 0..3 {
                    dng[offset + c * 2..offset + c * 2 + 2].copy_from_slice(&16_u16.to_le_bytes());
                }
            }
            // Omit the classic 8-bit lookup table; camera codes are already linear.
            if tag == 50712 {
                dng[p..p + 2].copy_from_slice(&65000_u16.to_le_bytes());
            }
        }
        let decoded = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(dng))
            .unwrap()
            .unwrap();
        assert_eq!((decoded.width, decoded.height), (12, 10));
        for y in 0..10 {
            for x in 0..12 {
                for c in 0..3 {
                    let expected =
                        (samples[((y + 3) * 16 + x + 2) * 3 + c] as f32 - 257.) / (65535. - 257.);
                    assert!((decoded.pixels[y * 12 + x][c] - expected).abs() < 2e-6);
                }
            }
        }
    }
}

/// Opt-in scratch output; no private image or metadata enters committed fixtures.
#[test]
fn private_sample_cpu_render() {
    let Some(path) = std::env::var_os("TESSERA_SMART_PREVIEW_SAMPLE") else {
        return;
    };
    let decoded = image_core::RawImage::open(engine_api::id::ImageId(830), path).unwrap();
    let proxy = decoded.camera_linear_proxy().expect("camera DNG");
    let settings = engine_api::recipe::DevelopSettings::default();
    assert!(
        proxy.resident_tail_plan(&settings).unwrap().is_none(),
        "DNG uses explicit CPU fallback"
    );
    let rgb = pipeline_cpu::render_scaled(
        &settings,
        &pipeline_cpu::RenderSource::CameraLinear(proxy),
        1,
    )
    .unwrap();
    assert_eq!((rgb.width(), rgb.height()), (2560, 1707));
    if let Some(output) = std::env::var_os("TESSERA_SMART_PREVIEW_RENDER") {
        image::save_buffer_with_format(
            output,
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            image::ColorType::Rgb8,
            image::ImageFormat::Png,
        )
        .unwrap();
    }
}

#[test]
fn external_dng_gpu_backend_uses_explicit_cpu_fallback_and_exports() {
    use engine_api::{id::ImageId, jobs::CancellationToken, recipe::Recipe, tile::TileCoord};
    use image_core::{RawImage, RenderOutput, Renderer, TileCache};
    use std::sync::Arc;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    let image = RawImage::open(ImageId(820), path).unwrap();
    let proxy = image.camera_linear_proxy().unwrap();
    assert!(proxy.is_external_dng());
    assert!(
        proxy
            .resident_tail_plan(&Default::default())
            .unwrap()
            .is_none()
    );
    let renderer = match pipeline_gpu::GpuContext::new() {
        Ok(context) => Renderer::with_ops(
            Arc::new(pipeline_gpu::GpuStageOp::new(Arc::new(context))),
            Arc::new(TileCache::new(1 << 20)),
            Default::default(),
        ),
        Err(_) => Renderer::new(Default::default()),
    };
    let mut tiles = Vec::new();
    renderer
        .render_tiles(
            &image,
            &Default::default(),
            &[TileCoord::new(0, 0, 0)],
            RenderOutput::Display,
            &CancellationToken::new(),
            &mut |tile| tiles.push(tile),
        )
        .unwrap();
    assert_eq!(tiles.len(), 1);
    let recipe = Recipe::default();
    let source = export::ExportImage {
        source: pipeline_cpu::RenderSource::CameraLinear(proxy),
        name: "synthetic",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let output = export::export_one(
        &source,
        &recipe,
        &export::ExportSettings {
            output_dir: dir.path().join("out"),
            apply_orientation: true,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(image::image_dimensions(output).unwrap(), (10, 12));
}
