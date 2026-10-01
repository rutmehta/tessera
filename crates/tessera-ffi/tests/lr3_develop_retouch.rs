//! LR-3b: supported heal/clone recipes must reach the Develop CPU renderer.
use engine_api::{
    id::RetouchId,
    recipe::{
        DevelopSettings, MaskComponent, MaskKind,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
    },
};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};
use std::sync::Arc;

#[test]
fn heal_and_clone_spots_render_through_develop_cpu() {
    let plane: Vec<f32> = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 0.8 } else { 0.1 }))
        .collect();
    let image = Image::new(128, 80, vec![plane; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    let baseline = render_linear_scaled(&settings, &RenderSource::Rgb(&image), 1).unwrap();
    settings.locals.retouch = [
        RetouchKind::Clone {
            source_offset: [0.5, 0.0],
        },
        RetouchKind::Heal {
            source_offset: [0.5, 0.0],
        },
    ]
    .into_iter()
    .enumerate()
    .map(|(i, kind)| RetouchOperation {
        id: RetouchId(i as u32),
        kind,
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.3 + i as f32 * 0.4, 1.0]],
                    radius: 0.0625,
                    feather: 50.0,
                    ..BrushStroke::default()
                }],
            })],
        },
        opacity: 50.0,
        feather: 0.0,
        enabled: true,
    })
    .collect();
    let context = pipeline_cpu::LensContext {
        retouch: Some(Arc::new(brush::render_retouch)),
        ..Default::default()
    };
    let rendered = pipeline_cpu::render_linear_scaled_with_lens(
        &settings,
        &RenderSource::Rgb(&image),
        1,
        &context,
    )
    .expect("Develop CPU must render supported heal and clone spots");
    assert_eq!((rendered.width(), rendered.height()), (128, 80));
    // A successful return that silently drops retouch must also fail this test.
    assert!(rendered.planes()[0][24 * 128 + 32] > baseline.planes()[0][24 * 128 + 32] + 0.1);
}

fn settings(heal: bool) -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.locals.retouch = vec![RetouchOperation {
        id: RetouchId(1),
        kind: if heal {
            RetouchKind::Heal {
                source_offset: [0.5, 0.0],
            }
        } else {
            RetouchKind::Clone {
                source_offset: [0.5, 0.0],
            }
        },
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush {
                strokes: vec![BrushStroke {
                    points: vec![[0.25, 0.5, 1.0]],
                    radius: 0.0625,
                    feather: 50.0,
                    ..Default::default()
                }],
            })],
        },
        opacity: 50.0,
        feather: 0.0,
        enabled: true,
    }];
    s
}
fn image() -> Image {
    let plane: Vec<f32> = (0..80)
        .flat_map(|y| {
            (0..128).map(move |x| {
                if x >= 64 {
                    0.4 + ((x * y) % 17) as f32 * 0.025
                } else {
                    0.1
                }
            })
        })
        .collect();
    Image::new(
        128,
        80,
        (0..3)
            .map(|c| plane.iter().map(|v| v * (1.0 + c as f32 * 0.2)).collect())
            .collect(),
    )
    .unwrap()
}
fn direct_kernel(base: &Image, heal: bool) -> Image {
    direct_kernel_spots(base, heal, false)
}
fn direct_kernel_spots(base: &Image, heal: bool, second: bool) -> Image {
    use brush::{Brush, CloneSource, InputPoint, PaintMode, Stroke, Tip};
    use compositor::{Depth, Raster, Rect};
    let extent = engine_api::tile::Extent::new(128, 80);
    let rect = Rect::of_extent(extent);
    let mut raster = Raster::new(extent, 3, Depth::F32, 0.0);
    raster
        .edit_region(rect, 1, |x, y, p| {
            let i = y as usize * 128 + x as usize;
            *p = [
                base.planes()[0][i],
                base.planes()[1][i],
                base.planes()[2][i],
                1.0,
            ];
        })
        .unwrap();
    let source = CloneSource {
        offset: [64.0, 0.0],
        source: None,
    };
    let mut stroke = Stroke::new(
        Brush {
            size: 16.0,
            opacity: 0.5,
            flow: 1.0,
            tip: Tip::round(0.5),
            mode: if heal {
                PaintMode::Heal(source)
            } else {
                PaintMode::Clone(source)
            },
            ..Default::default()
        },
        &raster,
        1,
    )
    .unwrap();
    stroke
        .add_point(InputPoint::at(32.0, 40.0).pressure(1.0))
        .unwrap();
    stroke.finish().unwrap();
    stroke.apply(&mut raster, rect, 2).unwrap();
    if second {
        // Independent pixel-space reference for a second, softer, 25% spot.
        let source = CloneSource {
            offset: [64.0, 0.0],
            source: None,
        };
        let mut stroke = Stroke::new(
            Brush {
                size: 16.0,
                opacity: 0.25,
                flow: 1.0,
                tip: Tip::round(0.0),
                mode: if heal {
                    PaintMode::Heal(source)
                } else {
                    PaintMode::Clone(source)
                },
                ..Default::default()
            },
            &raster,
            1,
        )
        .unwrap();
        stroke
            .add_point(InputPoint::at(32.0, 16.0).pressure(1.0))
            .unwrap();
        stroke.finish().unwrap();
        stroke.apply(&mut raster, rect, 3).unwrap();
    }
    let planes = (0..3)
        .map(|c| {
            (0..80)
                .flat_map(|y| (0..128).map(move |x| (x, y)))
                .map(|(x, y)| raster.pixel(x, y)[c])
                .collect()
        })
        .collect();
    Image::new(128, 80, planes).unwrap()
}
fn assert_bits(a: &Image, b: &Image) {
    assert_eq!(
        (a.width(), a.height(), a.planes().len()),
        (b.width(), b.height(), b.planes().len())
    );
    for (a, b) in a.planes().iter().flatten().zip(b.planes().iter().flatten()) {
        assert_eq!(a.to_bits(), b.to_bits());
    }
}
#[test]
fn develop_matches_direct_clone_and_heal_kernels_bit_for_bit() {
    let image = image();
    let baseline =
        render_linear_scaled(&DevelopSettings::default(), &RenderSource::Rgb(&image), 1).unwrap();
    for heal in [false, true] {
        let mut settings = settings(heal);
        let mut second = settings.locals.retouch[0].clone();
        second.id = RetouchId(2);
        second.opacity = 25.0;
        let RetouchTarget::Area { components } = &mut second.target else {
            panic!()
        };
        let MaskKind::Brush { strokes } = &mut components[0].kind else {
            panic!()
        };
        strokes[0].points = vec![[0.25, 0.2, 1.0]];
        strokes[0].feather = 100.0;
        settings.locals.retouch.push(second);
        settings.tone.contrast = 17.0;
        settings.color.saturation = 12.0;
        let mut downstream = settings.clone();
        downstream.locals.retouch.clear();
        let corrected = direct_kernel_spots(&image, heal, true);
        let expected =
            render_linear_scaled(&downstream, &RenderSource::Rgb(&corrected), 1).unwrap();
        let context = pipeline_cpu::LensContext {
            retouch: Some(Arc::new(brush::render_retouch)),
            ..Default::default()
        };
        let actual = pipeline_cpu::render_linear_scaled_with_lens(
            &settings,
            &RenderSource::Rgb(&image),
            1,
            &context,
        )
        .unwrap();
        assert_bits(&actual, &expected);
        assert!(
            actual.planes()[0]
                .iter()
                .zip(&baseline.planes()[0])
                .any(|(a, b)| a.to_bits() != b.to_bits()),
            "kernel must change pixels, heal={heal}"
        );
    }
}
#[test]
fn develop_graph_registers_renderer_and_invalidates_retouch_memo() {
    use image_core::{RawImage, Renderer, RgbSource};
    let raw = RawImage::from_rgb(
        engine_api::id::ImageId(42),
        RgbSource::from_linear_rec2020(image()).unwrap(),
    )
    .unwrap();
    let renderer =
        Renderer::new(Default::default()).with_retouch_renderer(Arc::new(brush::render_retouch));
    let token = engine_api::jobs::CancellationToken::new();
    renderer
        .render_rgb_linear(&raw, 0, &DevelopSettings::default(), &token)
        .unwrap();
    for heal in [false, true] {
        let actual = renderer
            .render_rgb_linear(&raw, 0, &settings(heal), &token)
            .unwrap();
        let corrected = direct_kernel(&image(), heal);
        let expected = render_linear_scaled(
            &DevelopSettings::default(),
            &RenderSource::Rgb(&corrected),
            1,
        )
        .unwrap();
        assert_bits(&actual, &expected);
    }
    let error = Renderer::new(Default::default())
        .render_rgb_linear(&raw, 0, &settings(false), &token)
        .unwrap_err();
    assert!(matches!(error,engine_api::EngineError::InvalidArgument{name,..} if name=="retouch"));
}
#[test]
#[cfg(target_os = "macos")]
fn gpu_retouch_routes_through_cpu_stage() {
    use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RgbSource, TileCache};
    let raw = RawImage::from_rgb(
        engine_api::id::ImageId(43),
        RgbSource::from_linear_rec2020(image()).unwrap(),
    )
    .unwrap();
    let gpu = Arc::new(pipeline_gpu::GpuStageOp::new(Arc::new(
        pipeline_gpu::GpuContext::new().unwrap(),
    )));
    let renderer = Renderer::with_ops(gpu, Arc::new(TileCache::new(16 << 20)), Default::default())
        .with_retouch_renderer(Arc::new(brush::render_retouch));
    let mut s = settings(false);
    s.tone.contrast = 23.0;
    s.locals
        .adjustments
        .push(engine_api::recipe::mask::LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::Radial {
                center: [0.25, 0.5],
                radii: [0.2, 0.7],
                angle: 0.0,
                feather: 25.0,
            })],
            params: engine_api::recipe::mask::LocalParams {
                exposure: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
    assert!(!renderer.can_render_resident(&raw, &s).unwrap());
    let rect = PixelRect::full(raw.active_extent());
    let baseline = renderer
        .render_region_as(
            &raw,
            &DevelopSettings::default(),
            0,
            rect,
            RenderOutput::SceneLinear,
        )
        .unwrap();
    let got = renderer
        .render_region_as(&raw, &s, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    assert!(
        got.iter()
            .zip(&baseline)
            .any(|(a, b)| a.samples::<f32>().unwrap() != b.samples::<f32>().unwrap())
    );
    let cpu =
        Renderer::new(Default::default()).with_retouch_renderer(Arc::new(brush::render_retouch));
    let expected = cpu
        .render_region_as(&raw, &s, 0, rect, RenderOutput::SceneLinear)
        .unwrap();
    for (a, b) in got.iter().zip(&expected) {
        for (a, b) in a
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(b.samples::<f32>().unwrap())
        {
            assert_eq!(a.to_bits(), b.to_bits());
        }
    }
    let i = got[0].layout().index(0, 32, 40).unwrap();
    assert!(got[0].samples::<f32>().unwrap()[i] > baseline[0].samples::<f32>().unwrap()[i] + 0.1);
    fn reject(
        _: u32,
        _: u32,
        _: &mut [Vec<f32>],
        _: &[RetouchOperation],
    ) -> engine_api::EngineResult<()> {
        Err(engine_api::EngineError::invalid(
            "retouch-test",
            "injected kernel failure",
        ))
    }
    let renderer = renderer.with_retouch_renderer(Arc::new(reject));
    let error = renderer
        .render_region_as(&raw, &s, 0, rect, RenderOutput::SceneLinear)
        .unwrap_err();
    assert!(
        matches!(error, engine_api::EngineError::InvalidArgument { name, .. } if name == "retouch-test")
    );
}

#[test]
fn unsupported_retouch_is_an_error_and_disabled_spots_are_identity() {
    let image = image();
    let context = pipeline_cpu::LensContext {
        retouch: Some(Arc::new(brush::render_retouch)),
        ..Default::default()
    };
    let mut s = settings(false);
    s.locals.retouch[0].kind = RetouchKind::Remove { model: None };
    assert!(
        pipeline_cpu::render_linear_scaled_with_lens(&s, &RenderSource::Rgb(&image), 1, &context)
            .is_err()
    );
    s.locals.retouch[0].enabled = false;
    let actual =
        pipeline_cpu::render_linear_scaled_with_lens(&s, &RenderSource::Rgb(&image), 1, &context)
            .unwrap();
    let expected =
        render_linear_scaled(&DevelopSettings::default(), &RenderSource::Rgb(&image), 1).unwrap();
    assert_bits(&actual, &expected);
    assert!(render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).is_err());
}

#[test]
fn pixel_and_file_export_use_registered_retouch_or_error() {
    use engine_api::{
        id::ImageId,
        jobs::CancellationToken,
        recipe::{EditMeta, Recipe},
    };
    let pixels = image();
    let image = export::ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "synthetic",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let mut recipe = Recipe::new(ImageId(45));
    recipe
        .edit(EditMeta::user("synthetic clone", 0), |s| {
            *s = settings(false)
        })
        .unwrap();
    let request = export::RenderRequest {
        color_space: export::ColorSpace::Srgb,
        resize: export::Resize::None,
        sharpen_for: export::SharpenFor::None,
        scale: 1,
    };
    let cancel = CancellationToken::new();
    assert!(export::render_pixels(&image, &recipe, &request, &cancel, None).is_err());
    let rendered = export::render_pixels_with_retouch(
        &image,
        &recipe,
        &request,
        &cancel,
        None,
        Some(Arc::new(brush::render_retouch)),
    )
    .unwrap();
    let baseline =
        export::render_pixels(&image, &Recipe::new(ImageId(45)), &request, &cancel, None).unwrap();
    assert!(rendered.get_pixel(32, 40)[0] > baseline.get_pixel(32, 40)[0] + 0.1);
    let dir = tempfile::tempdir().unwrap();
    let mut options = export::ExportSettings {
        format: export::Format::Png,
        output_dir: dir.path().into(),
        ..Default::default()
    };
    assert!(
        export::render_one_cancellable(&image, &recipe, &options, &cancel, None, None).is_err()
    );
    options.retouch = Some(Arc::new(brush::render_retouch));
    let rendered =
        export::render_one_cancellable(&image, &recipe, &options, &cancel, None, None).unwrap();
    assert!(!rendered.used_gpu());
    let path = rendered.finish(&cancel).unwrap();
    let saved = image::open(path).unwrap().to_rgb8();
    assert!(saved.get_pixel(32, 40)[0] > (baseline.get_pixel(32, 40)[0] * 255.0) as u8 + 20);
    options.hdr = Some(export::HdrTransfer::Pq);
    options.color_space = export::ColorSpace::Rec2020;
    options.naming = "{name}-hdr-{seq}".into();
    options.retouch = None;
    assert!(
        export::render_one_cancellable(&image, &recipe, &options, &cancel, None, None).is_err()
    );
    options.retouch = Some(Arc::new(brush::render_retouch));
    let hdr =
        export::render_one_cancellable(&image, &recipe, &options, &cancel, None, None).unwrap();
    assert!(!hdr.used_gpu());
    let hdr_path = hdr.finish(&cancel).unwrap();
    let hdr_pixels = image::open(hdr_path).unwrap().to_rgb16();
    assert!(hdr_pixels.get_pixel(32, 40)[0] > hdr_pixels.get_pixel(10, 40)[0]);
}

#[test]
fn develop_admission_preserves_all_retouch_for_render_or_error() {
    for heal in [false, true] {
        let s = settings(heal);
        assert_eq!(tessera_ffi::renderable(&s).locals.retouch, s.locals.retouch);
    }
    let mut s = settings(false);
    s.locals.retouch[0].kind = RetouchKind::Remove { model: None };
    assert_eq!(tessera_ffi::renderable(&s).locals.retouch, s.locals.retouch);
    s.locals.retouch[0].enabled = false;
    assert_eq!(tessera_ffi::renderable(&s).locals.retouch, s.locals.retouch);
}

#[test]
fn library_preview_renders_retouch_and_cache_cannot_hide_missing_renderer() {
    use previews::{Codec, Jpeg, Level, PreviewStore};
    let dir = tempfile::tempdir().unwrap();
    let pixels = image();
    let source = export::ExportImage {
        source: RenderSource::Rgb(&pixels),
        name: "synthetic",
        sequence: 1,
        date: "",
        metadata: None,
    };
    let path = export::export_one(
        &source,
        &Default::default(),
        &export::ExportSettings {
            format: export::Format::Dng,
            output_dir: dir.path().into(),
            ..Default::default()
        },
    )
    .unwrap();
    let cache = dir.path().join("cache");
    let store = PreviewStore::new(&cache, 1 << 20)
        .unwrap()
        .with_retouch_renderer(Arc::new(brush::render_retouch));
    let baseline = store
        .from_raw_settings(&path, 128, &DevelopSettings::default(), [0; 32])
        .unwrap();
    let baseline = Jpeg
        .decode(&store.get(&baseline, Level::Full).unwrap())
        .unwrap();
    let edited = store
        .from_raw_settings(&path, 128, &settings(false), [1; 32])
        .unwrap();
    let edited = Jpeg
        .decode(&store.get(&edited, Level::Full).unwrap())
        .unwrap();
    assert!(edited.get_pixel(32, 40)[0] > baseline.get_pixel(32, 40)[0] + 20);
    let unregistered = PreviewStore::new(&cache, 1 << 20).unwrap();
    let error = unregistered
        .from_raw_settings(&path, 128, &settings(false), [1; 32])
        .unwrap_err();
    assert!(
        matches!(error, previews::PreviewError::Render(engine_api::EngineError::InvalidArgument { name, .. }) if name=="retouch")
    );
}

#[test]
#[cfg(target_os = "macos")]
fn session_detail_preview_keeps_clone_source_outside_the_window() {
    use tessera_ffi::{Engine, ImageQuery, surface::Surface};
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    image::RgbImage::from_fn(512, 80, |x, _| {
        image::Rgb([if x >= 256 { 220 } else { 30 }; 3])
    })
    .save(photos.join("synthetic.jpg"))
    .unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let id = engine.list_images(ImageQuery::default()).unwrap()[0]
        .id
        .clone();
    let session = engine.clone().open_develop_session(id).unwrap();
    // Keep both comparisons on the full-source retouch path. The legacy RGB
    // detail-window path otherwise expects CFA metadata.
    let mut disabled = settings(false).locals.retouch;
    disabled[0].enabled = false;
    session
        .set_settings(
            serde_json::json!({"locals":{"retouch":disabled}}).to_string(),
            false,
        )
        .unwrap();
    let surface =
        Surface::lookup(tessera_ffi::surface::testing::create_rgba8(16, 16), 16, 16).unwrap();
    session
        .render_detail_preview(surface.id(), 16, 16, 0.25, 0.5)
        .unwrap();
    let before = surface
        .with_pixels(|px, stride| px[8 * stride + 8 * 4])
        .unwrap();
    session
        .set_settings(
            serde_json::json!({"locals":{"retouch":settings(false).locals.retouch}}).to_string(),
            false,
        )
        .unwrap();
    session
        .render_detail_preview(surface.id(), 16, 16, 0.25, 0.5)
        .unwrap();
    let after = surface
        .with_pixels(|px, stride| px[8 * stride + 8 * 4])
        .unwrap();
    assert!(
        after > before + 20,
        "clone source outside the 32px margin must remain available: {before} -> {after}"
    );
    session.close().unwrap();
}

#[test]
fn lr3d_retouch_precedes_tone_and_local_adjustments() {
    use engine_api::recipe::mask::{LocalAdjustment, LocalParams};
    let input = image();
    let mut s = settings(false);
    s.detail.sharpening.amount = 0.0;
    s.detail.noise_reduction.color = 0.0;
    s.tone.contrast = 35.0;
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.25, 0.5],
            radii: [0.24, 1.0],
            angle: 0.0,
            feather: 0.0,
        })],
        params: LocalParams {
            exposure: 1.0,
            ..Default::default()
        },
        ..Default::default()
    });
    // Source is already scene linear and default WB is identity. Retouch first,
    // then run the independent public Develop pipeline without spots.
    let corrected = direct_kernel(&input, false);
    let mut no_spots = s.clone();
    no_spots.locals.retouch.clear();
    let expected = render_linear_scaled(&no_spots, &RenderSource::Rgb(&corrected), 1).unwrap();
    let context = pipeline_cpu::LensContext {
        retouch: Some(Arc::new(brush::render_retouch)),
        ..Default::default()
    };
    let actual =
        pipeline_cpu::render_linear_scaled_with_lens(&s, &RenderSource::Rgb(&input), 1, &context)
            .unwrap();
    assert_bits(&actual, &expected);
    let raw = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(88),
        image_core::RgbSource::from_linear_rec2020(input).unwrap(),
    )
    .unwrap();
    let renderer = image_core::Renderer::new(Default::default())
        .with_retouch_renderer(Arc::new(brush::render_retouch));
    let actual = renderer
        .render_rgb_linear(&raw, 0, &s, &engine_api::jobs::CancellationToken::new())
        .unwrap();
    assert_bits(&actual, &expected);
    let extent = raw.active_extent();
    let tiles = renderer.render_region_as(&raw, &s, 0, image_core::PixelRect::full(extent), image_core::RenderOutput::SceneLinear).unwrap();
    let mut m2 = Image::new(extent.width, extent.height, vec![vec![0.0; extent.area() as usize];3]).unwrap();
    for tile in tiles { m2.put(&tile).unwrap(); }
    assert_bits(&m2,&expected);
}

#[test]
fn lr3d_duplicate_strokes_form_one_union_mask() {
    let input = image();
    for heal in [false, true] {
        let mut s = settings(heal);
        let mut once = input.planes().to_vec();
        brush::render_retouch(128, 80, &mut once, &s.locals.retouch).unwrap();
        let RetouchTarget::Area { components } = &mut s.locals.retouch[0].target else {
            panic!()
        };
        let MaskKind::Brush { strokes } = &mut components[0].kind else {
            panic!()
        };
        strokes.push(strokes[0].clone());
        let mut twice = input.planes().to_vec();
        brush::render_retouch(128, 80, &mut twice, &s.locals.retouch).unwrap();
        for (a, b) in once.iter().flatten().zip(twice.iter().flatten()) {
            assert_eq!(a, b, "one spot must not compound 50% opacity into 75%");
        }
    }
}

#[test]
fn lr3d_scaled_render_invokes_retouch_at_target_resolution() {
    let context = pipeline_cpu::LensContext {
        retouch: Some(Arc::new(
            |w, h, p: &mut [Vec<f32>], spots: &[RetouchOperation]| {
                assert_eq!((w, h), (32, 20));
                brush::render_retouch(w, h, p, spots)
            },
        )),
        ..Default::default()
    };
    pipeline_cpu::render_linear_scaled_with_lens(
        &settings(false),
        &RenderSource::Rgb(&image()),
        4,
        &context,
    )
    .unwrap();
}

#[test]
fn lr3d_two_spots_have_independent_feather_and_opacity() {
    let plane = (0..80)
        .flat_map(|_| (0..128).map(|x| if x >= 64 { 1.0 } else { 0.0 }))
        .collect();
    let mut pixels = vec![plane; 3];
    let mut spots = settings(false).locals.retouch;
    let mut second = spots[0].clone();
    second.id = RetouchId(2);
    second.opacity = 25.0;
    let RetouchTarget::Area { components } = &mut second.target else {
        panic!()
    };
    let MaskKind::Brush { strokes } = &mut components[0].kind else {
        panic!()
    };
    strokes[0].points = vec![[0.25, 0.2, 1.0]];
    strokes[0].feather = 100.0;
    spots.push(second);
    brush::render_retouch(128, 80, &mut pixels, &spots).unwrap();
    // Independent analytic footprint: diameter 16px; sample center is (+.5,+.5).
    // The first spot's solid core is 50%; second has a 9px smoothstep ramp.
    assert_eq!(pixels[0][40 * 128 + 32], 0.5);
    let t = (8.5_f32 - 0.5_f32.sqrt()) / 9.0;
    let expected = 0.25 * t * t * (3.0 - 2.0 * t);
    assert!((pixels[0][16 * 128 + 32] - expected).abs() < 1e-6);
    assert_eq!(pixels[0][5 * 128 + 5], 0.0);
}

#[test]
fn lr3d_all_strokes_in_a_spot_read_the_same_source_snapshot() {
    let plane = (0..80)
        .flat_map(|_| (0..128).map(|x| (x / 32) as f32 * 0.4))
        .collect();
    let mut pixels = vec![plane; 3];
    let mut spots = settings(false).locals.retouch;
    spots[0].kind = RetouchKind::Clone {
        source_offset: [-0.25, 0.0],
    };
    let RetouchTarget::Area { components } = &mut spots[0].target else {
        panic!()
    };
    let MaskKind::Brush { strokes } = &mut components[0].kind else {
        panic!()
    };
    strokes[0].radius = 0.03;
    strokes[0].feather = 0.0;
    let mut second = strokes[0].clone();
    second.points[0][0] = 0.5;
    strokes.push(second);
    brush::render_retouch(128, 80, &mut pixels, &spots).unwrap();
    assert!((pixels[0][40 * 128 + 32] - 0.2).abs() < 1e-6);
    assert!(
        (pixels[0][40 * 128 + 64] - 0.6).abs() < 1e-6,
        "second stroke must clone original 0.4, not edited 0.2"
    );
}

fn lr3d_metadata(orientation: u16) -> raw_decode::RawMetadata {
    raw_decode::RawMetadata {
        make: "test".into(),
        model: "test".into(),
        lens: None,
        iso: 100.,
        shutter_s: 0.01,
        aperture: 4.,
        focal_mm: 50.,
        capture_time: 0,
        orientation,
        width: 32,
        height: 24,
        cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
        black_levels: [0.; 4],
        white_level: 65535,
        as_shot_wb: [1.; 4],
        camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
        cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
        rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
        default_crop: [3, 1, 26, 22],
        has_gain_map: false,
        has_opcode_list: false,
        opcode_lists: [None, None, None],
    }
}

#[test]
fn lr3d_exif_five_to_eight_orients_retouch_after_sensor_crop_and_distortion() {
    let cfa = raw_decode::CfaImage::from_linear(
        32,
        24,
        (0..768)
            .map(|i| if i % 32 < 16 { 0.1 } else { 0.8 })
            .collect(),
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut recipe = engine_api::recipe::Recipe::default();
    recipe
        .edit(engine_api::recipe::EditMeta::user("synthetic", 0), |s| {
            *s = settings(false);
            s.geometry.crop.rect.left = 0.1;
            s.geometry.crop.rect.right = 0.9;
            s.geometry.crop.angle = 7.0;
            s.lens.manual_distortion = 10.0;
        })
        .unwrap();
    for orientation in 5..=8 {
        let metadata = lr3d_metadata(orientation);
        let image = export::ExportImage {
            source: RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            name: "synthetic",
            sequence: orientation as usize,
            date: "",
            metadata: None,
        };
        let mut options = export::ExportSettings {
            format: export::Format::Png,
            output_dir: dir.path().into(),
            retouch: Some(Arc::new(brush::render_retouch)),
            ..Default::default()
        };
        let path = export::export_one(&image, &recipe, &options).unwrap();
        let sensor = image::open(path).unwrap().to_rgb8();
        options.apply_orientation = true;
        options.naming = "{name}-oriented-{seq}".into();
        let path = export::export_one(&image, &recipe, &options).unwrap();
        let oriented = image::open(path).unwrap().to_rgb8();
        let expected = match orientation {
            5 => image::imageops::rotate90(&image::imageops::flip_vertical(&sensor)),
            6 => image::imageops::rotate90(&sensor),
            7 => image::imageops::rotate90(&image::imageops::flip_horizontal(&sensor)),
            8 => image::imageops::rotate270(&sensor),
            _ => unreachable!(),
        };
        assert_eq!(oriented.dimensions(), expected.dimensions());
        assert_eq!(oriented.as_raw(), expected.as_raw(), "EXIF {orientation}");
    }
}

#[test]
fn lr3d_inactive_spots_are_identity_at_reduced_resolution() {
    let input = image();
    let context = pipeline_cpu::LensContext { retouch: Some(Arc::new(brush::render_retouch)), ..Default::default() };
    for zero_opacity in [false, true] {
        let mut s = settings(false);
        s.tone.contrast = 35.0;
        s.locals.retouch[0].enabled = zero_opacity;
        if zero_opacity { s.locals.retouch[0].opacity = 0.0; }
        let mut empty = s.clone();
        empty.locals.retouch.clear();
        let expected = render_linear_scaled(&empty, &RenderSource::Rgb(&input), 4).unwrap();
        let actual = pipeline_cpu::render_linear_scaled_with_lens(&s, &RenderSource::Rgb(&input), 4, &context).unwrap();
        assert_bits(&actual, &expected);
    }
}
