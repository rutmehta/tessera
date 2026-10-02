#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, ProcessVersion},
    tile::TileCoord,
};
use image_core::{RawImage, RenderOutput, Renderer};
#[test]
fn classic_linear_dng_enters_camera_develop_for_native_and_adobe() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    let image = RawImage::open(ImageId(818), &path).unwrap();
    assert_eq!(image.metadata().orientation, 6);
    assert_eq!(image.metadata().as_shot_wb[..3], [2., 1., 1.5]);
    let settings = DevelopSettings::default();
    for process in [
        ProcessVersion::NATIVE_CURRENT,
        ProcessVersion {
            family: engine_api::recipe::ProcessFamily::Adobe,
            revision: 6,
        },
    ] {
        let renderer = Renderer::new(Default::default()).for_process_version(process);
        let mut tiles = Vec::new();
        renderer
            .render_tiles(
                &image,
                &settings,
                &[TileCoord::new(0, 0, 0)],
                RenderOutput::SceneLinear,
                &CancellationToken::new(),
                &mut |tile| tiles.push(tile),
            )
            .unwrap();
        assert!(!tiles.is_empty());
    }
}

#[test]
fn crop_radial_and_saved_upright_land_in_same_normalized_region() {
    use engine_api::recipe::{
        LocalAdjustment, LocalParams, MaskComponent, MaskKind, settings::UprightMode,
    };
    let source = |w: usize, h: usize| {
        let mut dng = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(
            false, false,
        )))
        .unwrap()
        .unwrap();
        dng.width = w;
        dng.height = h;
        dng.pixels = vec![[0.12; 3]; w * h];
        dng.metadata.width = w as u32;
        dng.metadata.height = h as u32;
        dng.metadata.default_crop = [0, 0, w as u32, h as u32];
        pipeline_cpu::CameraLinearProxy::from_dng(dng).unwrap()
    };
    let original = source(512, 384);
    let proxy = source(128, 96);
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.geometry.crop.rect.left = 0.125;
    s.geometry.crop.rect.right = 0.875;
    s.geometry.crop.rect.top = 0.125;
    s.geometry.crop.rect.bottom = 0.875;
    s.geometry.upright.mode = UprightMode::Full;
    s.geometry.upright.homography = Some([[1., 0.02, 0.01], [0., 1., 0.02], [0.015, 0., 1.]]);
    s.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.6, 0.4],
            radii: [0.25, 0.3],
            angle: 20.,
            feather: 60.,
        })],
        params: LocalParams {
            exposure: 1.,
            ..Default::default()
        },
        ..Default::default()
    });
    let render = |p: &pipeline_cpu::CameraLinearProxy, scale| {
        pipeline_cpu::render_linear_scaled(&s, &pipeline_cpu::RenderSource::CameraLinear(p), scale)
            .unwrap()
    };
    let full = render(&original, 4);
    let reduced = render(&proxy, 1);
    assert_eq!((full.width(), full.height()), (96, 72));
    assert_eq!(
        (full.width(), full.height()),
        (reduced.width(), reduced.height())
    );
    let max_error = full
        .planes()
        .iter()
        .flatten()
        .zip(reduced.planes().iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    assert!(max_error < 0.02, "normalized geometry error {max_error}");
}

#[test]
fn imported_camera_masks_use_host_hooks_in_each_catalog_frame() {
    use engine_api::recipe::{LocalAdjustment, LocalParams, MaskComponent, MaskKind};
    struct LeftHalf;
    impl image_core::mask_cache::MaskHooks for LeftHalf {
        fn revision(&self) -> u64 {
            1
        }
        fn rasterize(
            &self,
            input: &pipeline_cpu::Image,
            _: &LocalAdjustment,
            _: u8,
        ) -> engine_api::EngineResult<Vec<f32>> {
            Ok((0..input.width() * input.height())
                .map(|i| {
                    if i % input.width() < input.width() / 2 {
                        1.
                    } else {
                        0.
                    }
                })
                .collect())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, support::lossy_dng(false, false)).unwrap();
    for orientation in 1..=8 {
        let source = RawImage::open_with_catalog_orientation(
            ImageId(900 + u128::from(orientation)),
            &path,
            Some(orientation),
        )
        .unwrap();
        for process in [
            ProcessVersion::NATIVE_CURRENT,
            ProcessVersion {
                family: engine_api::recipe::ProcessFamily::Adobe,
                revision: 6,
            },
        ] {
            let renderer = Renderer::new(Default::default()).for_process_version(process);
            renderer
                .mask_cache()
                .set_hooks(Some(std::sync::Arc::new(LeftHalf)));
            let mut s = DevelopSettings::default();
            s.detail.sharpening.amount = 0.;
            s.detail.noise_reduction.color = 0.;
            let run = |s: &DevelopSettings| {
                let mut result = Vec::new();
                renderer
                    .render_tiles(
                        &source,
                        s,
                        &[TileCoord::new(0, 0, 0)],
                        RenderOutput::SceneLinear,
                        &CancellationToken::new(),
                        &mut |t| result.push(t),
                    )
                    .unwrap();
                result.pop().unwrap()
            };
            let base = run(&s);
            s.locals.adjustments.push(LocalAdjustment {
                components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
                params: LocalParams {
                    exposure: 1.,
                    ..Default::default()
                },
                ..Default::default()
            });
            let edited = run(&s);
            let width = edited.layout().extent.width as usize;
            assert_eq!(width, if orientation >= 5 { 10 } else { 12 });
            for (i, (a, b)) in base
                .plane::<f32>(0)
                .unwrap()
                .iter()
                .zip(edited.plane::<f32>(0).unwrap())
                .enumerate()
            {
                let expected = if i % width < width / 2 { 2. * a } else { *a };
                assert!((b - expected).abs() < 0.0001, "orientation {orientation}");
            }
        }
    }
}

#[test]
fn lr13_adobe_linearraw_output_does_not_apply_a_second_native_tone_curve() {
    let dir = tempfile::tempdir().unwrap();
    let bytes = support::lossy_dng(false, false);
    let path = dir.path().join("adobe.dng");
    std::fs::write(&path, &bytes).unwrap();
    let raw = RawImage::open(ImageId(1818), &path).unwrap();
    let profile = image_core::pipeline_adobe::dcp::DcpProfile::parse(&bytes).unwrap();
    let settings = DevelopSettings::default();
    let expected = image_core::pipeline_adobe::render_scaled_with_profile(
        &settings,
        &pipeline_cpu::RenderSource::CameraLinear(raw.camera_linear_proxy().unwrap()),
        1,
        Some(&profile),
    )
    .unwrap();
    let renderer = Renderer::new(Default::default()).for_process_version(ProcessVersion::adobe(6));
    let extent = Renderer::output_extent(&raw, &settings, 0).unwrap();
    let tiles = renderer
        .render_region(&raw, &settings, 0, image_core::PixelRect::full(extent))
        .unwrap();
    for tile in tiles {
        let l = tile.layout();
        let (ox, oy) = tile.coord().pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                for c in 0..3 {
                    let value = tile.plane::<u8>(c).unwrap()[(y as usize + l.halo as usize)
                        * l.stride()
                        + x as usize
                        + l.halo as usize];
                    assert_eq!(value, expected.get_pixel(ox + x, oy + y)[c as usize]);
                }
            }
        }
    }
}
