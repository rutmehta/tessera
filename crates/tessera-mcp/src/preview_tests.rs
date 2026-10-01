use super::*;
use crate::pixels::Source;
use engine_api::{color::ColorMatrix3, stage::StageId};
use image_core::{CountingStageOp, CpuStageOp};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};

#[test]
fn rgb_icc_and_orientation_are_shared_by_export_preview_and_histogram() {
    use image::ImageEncoder;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("adobe.jpg");
    let mut bytes = Vec::new();
    let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 100);
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::AdobeRgb)
        .unwrap();
    encoder
        .set_icc_profile(profile.icc_bytes().to_vec())
        .unwrap();
    encoder
        .set_exif_metadata(vec![
            b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0,
        ])
        .unwrap();
    encoder
        .encode(
            &[120, 80, 40].repeat(24 * 16),
            24,
            16,
            image::ExtendedColorType::Rgb8,
        )
        .unwrap();
    std::fs::write(&path, bytes).unwrap();
    let expected = image_core::RgbSource::open(&path).unwrap().into_pixels();
    let Source::Rgb(export) = Source::open(&path).unwrap() else {
        panic!("RGB expected")
    };
    assert_eq!((export.width(), export.height()), (16, 24));
    assert_eq!(export.planes(), expected.planes());
    let cache = PreviewCache::default();
    let recipe = Recipe::default();
    let display = cache.display(ImageId(10), &path, &recipe, None).unwrap();
    assert_eq!(
        cache.source_dimensions(ImageId(10), &path).unwrap(),
        (16, 24)
    );
    assert_eq!(
        display,
        pipeline_cpu::render(&recipe.settings, &RenderSource::Rgb(&expected)).unwrap()
    );
    let linear = cache.linear(ImageId(10), &path, &recipe).unwrap();
    let expected =
        pipeline_cpu::render_linear_scaled(&recipe.settings, &RenderSource::Rgb(&expected), 1)
            .unwrap();
    assert_eq!(linear.planes(), expected.planes());
}

#[test]
#[cfg(target_os = "macos")]
fn heic_is_accepted_by_export_and_preview() {
    let path = Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../image-core/tests/fixtures/rgb.heic"
    ));
    assert!(pixels::is_rgb(path));
    assert!(matches!(Source::open(path).unwrap(), Source::Rgb(_)));
    let rgb = PreviewCache::default()
        .display(ImageId(11), path, &Recipe::default(), None)
        .unwrap();
    assert_eq!(rgb.dimensions(), (32, 24));
    assert!(rgb.pixels().any(|p| p[0] > 30));
}

#[test]
fn nondefault_histogram_clipping_does_not_saturate_at_f32_integer_limit() {
    let full = image::RgbImage::from_pixel(4097, 4097, image::Rgb([0, 255, 128]));
    let histogram = pixels::histogram(&full, 64).unwrap();
    assert_eq!(histogram.clipped_shadows, 1.);
    assert_eq!(histogram.clipped_highlights, 1.);
}

#[test]
fn raw_steps_reuse_upstream_operator_results() {
    let id = ImageId(1);
    let metadata = RawMetadata {
        make: "Synthetic".into(),
        model: "Test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation: 1,
        opcode_lists: [None, None, None],
        width: 64,
        height: 64,
        cfa_layout: CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 16383,
        as_shot_wb: [2., 1., 1.6, 1.],
        camera_to_xyz: ColorMatrix3([[0.; 3]; 3]),
        cam_xyz: [[0.9, 0.2, -0.1], [-0.3, 1.2, 0.1], [0., 0.1, 0.8], [0.; 3]],
        rgb_cam: [[0.; 4]; 3],
        default_crop: [0, 0, 64, 64],
        has_gain_map: false,
        has_opcode_list: false,
    };
    let raw = RawImage::new(
        id,
        Arc::new(CfaImage::from_linear(64, 64, vec![0.2; 64 * 64]).unwrap()),
        Arc::new(metadata),
    )
    .unwrap();
    let cache = PreviewCache::default();
    cache
        .sources
        .lock()
        .unwrap()
        .insert(id, Arc::new(Decoded::Raw(raw)));
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    assert!(
        cache
            .renderer
            .set(Renderer::with_ops(
                ops.clone(),
                Arc::new(TileCache::new(32 << 20)),
                RendererConfig::default()
            ))
            .is_ok()
    );
    let mut recipe = Recipe::default();
    let path = Path::new("not-opened.NEF");
    let before = cache.display(id, path, &recipe, Some(1024)).unwrap();
    let counts = ops.counts();
    assert!(ops.count(StageId::Demosaic) > 0);
    for exposure in [0.1, 0.2, 0.3] {
        recipe.settings.tone.exposure = exposure;
        let after = cache.display(id, path, &recipe, Some(1024)).unwrap();
        assert_ne!(before, after);
        for stage in [
            StageId::Demosaic,
            StageId::CameraProfile,
            StageId::WhiteBalance,
        ] {
            assert_eq!(ops.count(stage), counts[stage.index()], "{stage:?}");
        }
    }
    assert!(ops.count(StageId::Tone) > counts[StageId::Tone.index()]);
    assert!(ops.count(StageId::Output) > counts[StageId::Output.index()]);
}

#[test]
fn rgb_decode_is_shared_by_preview_histogram_and_final() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(2050, 10, image::Rgb([64; 3]))
        .save(&path)
        .unwrap();
    let cache = PreviewCache::default();
    let id = ImageId(2);
    let recipe = Recipe::default();
    assert_eq!(
        cache
            .display(id, &path, &recipe, Some(1024))
            .unwrap()
            .width(),
        513
    );
    std::fs::remove_file(&path).unwrap();
    assert_eq!(cache.linear(id, &path, &recipe).unwrap().width(), 513);
    assert_eq!(
        cache.display(id, &path, &recipe, None).unwrap().width(),
        2050
    );
    assert_eq!(cache.sources.lock().unwrap().len(), 1);
}

#[test]
fn metrics_count_full_resolution_and_face_crop_uses_native_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("sparse.png");
    image::RgbImage::from_fn(2050, 12, |x, _| {
        image::Rgb(if x % 4 == 0 { [255, 0, 0] } else { [64; 3] })
    })
    .save(&path)
    .unwrap();
    let cache = PreviewCache::default();
    let id = ImageId(3);
    let recipe = Recipe::default();
    let full = cache.display(id, &path, &recipe, None).unwrap();
    let preview = cache.display(id, &path, &recipe, Some(512)).unwrap();
    let reduced = cache.metrics(id, &path, &recipe).unwrap();
    let mut cpu = image_core::resident::OutputMetrics::default();
    for p in full.pixels() {
        cpu.add_pixel(p.0);
    }
    assert_eq!(reduced, cpu);
    assert!(reduced.highlight_fraction() > 0.2);
    assert!(preview.pixels().all(|p| !p.0.contains(&255)));
    let region = engine_api::recipe::settings::NormalizedRect {
        left: 0.1,
        top: 0.25,
        right: 0.2,
        bottom: 0.75,
    };
    let crop = cache.crop(id, &path, &recipe, region).unwrap();
    assert_eq!(
        crop,
        image::imageops::crop_imm(&full, 205, 3, 205, 6).to_image()
    );
}

#[test]
fn retouch_is_registered_for_mcp_rgb_and_graph_previews() {
    use engine_api::{
        id::RetouchId,
        recipe::{
            MaskComponent, MaskKind,
            mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
        },
    };
    let plane = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 0.8 } else { 0.1 }))
        .collect();
    let pixels = Image::new(128, 80, vec![plane; 3]).unwrap();
    let cache = PreviewCache::default();
    let rgb_id = ImageId(71);
    let graph_id = ImageId(72);
    cache.sources.lock().unwrap().insert(
        rgb_id,
        Arc::new(Decoded::Rgb {
            full: pixels.clone(),
            preview: pixels.clone(),
        }),
    );
    let raw = RawImage::from_rgb(
        graph_id,
        image_core::RgbSource::from_linear_rec2020(pixels).unwrap(),
    )
    .unwrap();
    cache
        .sources
        .lock()
        .unwrap()
        .insert(graph_id, Arc::new(Decoded::Raw(raw)));
    let mut recipe = Recipe::default();
    recipe.settings.locals.retouch.push(RetouchOperation {
        id: RetouchId(1),
        kind: RetouchKind::Clone {
            source_offset: [0.5, 0.0],
        },
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.5, 1.0]],
                    radius: 0.0625,
                    ..Default::default()
                }],
            })],
        },
        opacity: 50.0,
        feather: 0.0,
        enabled: true,
    });
    let path = Path::new("synthetic-cached.png");
    for id in [rgb_id, graph_id] {
        let before = cache.linear(id, path, &Recipe::default()).unwrap();
        let after = cache.linear(id, path, &recipe).unwrap();
        assert!(after.planes()[0][40 * 128 + 32] > before.planes()[0][40 * 128 + 32] + 0.1);
        let before = cache.display(id, path, &Recipe::default(), None).unwrap();
        let after = cache.display(id, path, &recipe, None).unwrap();
        assert!(after.get_pixel(32, 40)[0] > before.get_pixel(32, 40)[0] + 20);
        let source = cache.source(rgb_id, path).unwrap();
        let Decoded::Rgb { full, .. } = &*source else { panic!() };
        let expected = pipeline_cpu::render_scaled_with_context(
            &recipe.settings, &RenderSource::Rgb(full), 4, &retouch_context(),
        ).unwrap();
        let small = cache.display(id, path, &recipe, Some(32)).unwrap();
        assert_eq!(small.dimensions(),expected.dimensions());
        assert!(small.as_raw() == expected.as_raw(), "retouch must run at requested preview resolution for {id:?}");
    }
}
