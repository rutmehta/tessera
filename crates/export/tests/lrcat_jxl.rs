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
    // The CPU reference remains callable when the native RGB tail is GPU eligible.
    proxy.resident_tail_plan(&settings).unwrap();
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
fn external_dng_gpu_backend_accepts_native_rgb_tail_and_exports() {
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
            .is_some()
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

/// Justification for ab924590's contract change (A-LR13 item 8). The old
/// assertion "every external DNG declines the resident tail" encoded a
/// limitation, not a requirement: after the camera-profile stage a LinearRaw
/// proxy is ordinary linear RGB, and the Metal regression
/// `lr13_external_linearraw_identity_orientation_can_use_resident_rgb_tail`
/// (pipeline-gpu/tests/smart_preview.rs) checks GPU submissions and CPU pixel
/// agreement for that case. What the old assertion protected is kept here:
/// every case that needs caller-owned scalar resources or a rotated catalog
/// frame still declines the GPU tail, so it takes the explicit CPU route.
#[test]
fn external_dng_resident_tail_admits_only_identity_native_rgb() {
    use engine_api::recipe::{
        DevelopSettings, MaskComponent, MaskKind,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
        settings::LensBlur,
    };
    let bytes = support::lossy_dng(false, false);
    let proxy = |orientation: Option<u16>| {
        let dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes))
            .unwrap()
            .unwrap();
        let proxy = pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap();
        assert!(proxy.is_external_dng());
        match orientation {
            Some(o) => proxy.with_catalog_orientation(o).unwrap(),
            None => proxy,
        }
    };
    let plain = DevelopSettings::default();
    assert!(proxy(None).resident_tail_plan(&plain).unwrap().is_some());
    assert!(proxy(Some(1)).resident_tail_plan(&plain).unwrap().is_some());
    for rotated in [3, 6, 8] {
        assert!(
            proxy(Some(rotated))
                .resident_tail_plan(&plain)
                .unwrap()
                .is_none(),
            "rotated catalog frame {rotated} must keep the CPU route"
        );
    }
    let mut blur = plain.clone();
    blur.effects.lens_blur = Some(LensBlur {
        amount: 50.,
        ..Default::default()
    });
    assert!(proxy(None).resident_tail_plan(&blur).unwrap().is_none());
    let mut retouch = plain.clone();
    retouch.locals.retouch.push(RetouchOperation {
        id: engine_api::id::RetouchId(1),
        kind: RetouchKind::Heal {
            source_offset: [0.25, 0.],
        },
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.5, 0.5, 1.]],
                    radius: 0.1,
                    feather: 0.,
                    ..Default::default()
                }],
            })],
        },
        opacity: 100.,
        feather: 0.,
        enabled: true,
    });
    assert!(proxy(None).resident_tail_plan(&retouch).unwrap().is_none());
}
