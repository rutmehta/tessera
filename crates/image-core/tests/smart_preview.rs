mod common;
use common::*;
use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{
        DevelopSettings, LocalAdjustment, LocalParams, MaskComponent, MaskKind, ProcessVersion,
        settings::{DemosaicMethod, WhiteBalanceMode},
    },
    tile::{Extent, Tile, TileCoord},
};
use image_core::{
    CountingStageOp, CpuStageOp, Headroom, PixelRect, RawImage, RenderOutput, Renderer,
    RendererConfig, TileCache, Viewport,
};
use pipeline_cpu::{CameraLinearProxy, LensContext, RenderSource};
use std::sync::Arc;

fn fixture(w: u32, h: u32) -> (RawImage, RawImage, DevelopSettings) {
    let raw = synthetic(901, w, h, RGGB, [1, 1, w - 2, h - 2]);
    let mut metadata = raw.metadata().clone();
    metadata.orientation = 6;
    let raw = raw.with_metadata(ImageId(902), Arc::new(metadata)).unwrap();
    let settings = DevelopSettings::default();
    let proxy = CameraLinearProxy::generate(
        raw.cfa(),
        raw.metadata(),
        &settings,
        ProcessVersion::NATIVE_CURRENT,
        [7; 32],
        &LensContext::default(),
    )
    .unwrap();
    let preview =
        RawImage::from_camera_linear_proxy(raw.id(), ImageId(903), Arc::new(proxy)).unwrap();
    (raw, preview, settings)
}
fn reference(preview: &RawImage, s: &DevelopSettings, level: u8) -> pipeline_cpu::Image {
    pipeline_cpu::render_linear_scaled(
        s,
        &RenderSource::CameraLinear(preview.camera_linear_proxy().unwrap()),
        1 << level,
    )
    .unwrap()
}
fn assert_tiles(
    tiles: &[Tile],
    expected: &pipeline_cpu::Image,
    output: RenderOutput,
    s: &DevelopSettings,
) {
    for tile in tiles {
        let coord = tile.coord();
        let expected = expected
            .tile(TileCoord::new(0, coord.x, coord.y), 0, 1)
            .unwrap();
        if output == RenderOutput::Display {
            let expected =
                pipeline_cpu::display(&expected, Default::default(), s.output.gamut_mapping)
                    .unwrap();
            assert_eq!(
                tile.samples::<u8>().unwrap(),
                expected.samples::<u8>().unwrap()
            );
        } else {
            let expected = match output {
                RenderOutput::DisplayLinear(h) => {
                    pipeline_cpu::display_linear(&expected, s.output.gamut_mapping, h.get())
                        .unwrap()
                }
                _ => expected,
            };
            assert_eq!(tile.layout(), expected.layout());
            for (a, b) in tile
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(expected.samples::<f32>().unwrap())
            {
                assert!((a - b).abs() < 2e-5, "{a} != {b}");
            }
        }
    }
}
#[test]
fn source_preserves_physical_metadata_and_separates_owner_and_render_identity() {
    let (raw, proxy, _) = fixture(2563, 9);
    assert_eq!(proxy.source_kind(), "raw");
    assert!(proxy.rgb().is_none());
    assert_eq!(proxy.recipe_owner(), raw.id());
    assert_ne!(proxy.id(), raw.id());
    assert_eq!(proxy.sensor_extent(), raw.sensor_extent());
    assert_eq!(proxy.metadata().default_crop, raw.metadata().default_crop);
    assert_eq!(proxy.metadata().orientation, 6);
    assert_eq!(proxy.active_extent(), Extent::new(1281, 4));
    let p = Arc::new(proxy.camera_linear_proxy().unwrap().clone());
    assert!(RawImage::from_camera_linear_proxy(raw.id(), raw.id(), p).is_err());
}
#[test]
fn ordinary_and_m2_wb_region_tiles_and_progressive_are_scalar_camera_linear() {
    let (raw, proxy, mut s) = fixture(269, 19);
    let ops = Arc::new(CountingStageOp::new(CpuStageOp));
    let renderer = Renderer::with_ops(
        ops.clone(),
        Arc::new(TileCache::new(1 << 20)),
        RendererConfig::default(),
    );
    for custom in [false, true] {
        if custom {
            s.white_balance.mode = WhiteBalanceMode::Custom;
            s.white_balance.temperature = 4200.;
            s.white_balance.tint = 13.;
            s.tone.exposure = 0.75;
            s.tone.contrast = 21.;
            s.locals.adjustments.push(LocalAdjustment {
                components: vec![MaskComponent::new(MaskKind::Linear {
                    start: [0., 0.],
                    end: [1., 0.],
                })],
                params: LocalParams {
                    exposure: 0.6,
                    ..Default::default()
                },
                ..Default::default()
            });
            s.geometry.crop.rect.right = 0.91;
        }
        let expected = reference(&proxy, &s, 0);
        let original = pipeline_cpu::render_linear_scaled(
            &s,
            &RenderSource::Cfa {
                image: raw.cfa(),
                metadata: raw.metadata(),
            },
            1,
        )
        .unwrap();
        assert_eq!(
            (original.width(), original.height()),
            (expected.width(), expected.height())
        );
        for (a, b) in original
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
        {
            assert!((a - b).abs() < 2e-5);
        }
        let wrong = pipeline_cpu::render_linear_scaled(
            &s,
            &RenderSource::Rgb(proxy.camera_linear_proxy().unwrap().pixels()),
            1,
        )
        .unwrap();
        assert_ne!(wrong.planes(), expected.planes());
        for output in [
            RenderOutput::SceneLinear,
            RenderOutput::Display,
            RenderOutput::DisplayLinear(Headroom::new(3.)),
        ] {
            let e = Renderer::output_extent(&proxy, &s, 0).unwrap();
            let tiles = renderer
                .render_region_as(&proxy, &s, 0, PixelRect::full(e), output)
                .unwrap();
            assert_tiles(&tiles, &expected, output, &s);
        }
        let cancel = CancellationToken::new();
        let mut tiles = Vec::new();
        renderer
            .render_tiles(
                &proxy,
                &s,
                &[TileCoord::new(0, 0, 0), TileCoord::new(0, 0, 0)],
                RenderOutput::SceneLinear,
                &cancel,
                &mut |t| tiles.push(t),
            )
            .unwrap();
        assert_eq!(tiles.len(), 1);
        assert_tiles(&tiles, &expected, RenderOutput::SceneLinear, &s);
        let mut tiles = Vec::new();
        renderer
            .render_progressive(
                &proxy,
                &s,
                &Viewport {
                    rect: PixelRect::full(proxy.active_extent()),
                    finest_level: 0,
                    coarsest_level: 2,
                },
                RenderOutput::SceneLinear,
                &cancel,
                &mut |t| tiles.push(t),
            )
            .unwrap();
        for level in 0..=2 {
            let expected = reference(&proxy, &s, level);
            assert_eq!(
                Renderer::output_extent(&proxy, &s, level).unwrap(),
                Extent::new(expected.width(), expected.height())
            );
            let at_level: Vec<_> = tiles
                .iter()
                .filter(|t| t.coord().level == level)
                .cloned()
                .collect();
            assert!(!at_level.is_empty());
            assert_tiles(&at_level, &expected, RenderOutput::SceneLinear, &s);
        }
    }
    // Neither injected GPU/backend operators nor original/mask memo caches enter.
    assert!(ops.counts().iter().all(|n| *n == 0));
    assert!(renderer.cache().is_empty());
    assert_eq!(renderer.mask_cache().stats().inserts, 0);
}
#[test]
fn unsupported_prefix_process_and_external_masks_fail_before_delivery() {
    let (_, proxy, s) = fixture(32, 24);
    let renderer = Renderer::new(Default::default());
    let rect = PixelRect::full(proxy.active_extent());
    let mut bad = s.clone();
    bad.demosaic.method = DemosaicMethod::Bilinear;
    let mut lens = s.clone();
    lens.lens.manual_distortion = 12.;
    let mut ai = s.clone();
    ai.locals.adjustments.push(LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
        ..Default::default()
    });
    for settings in [&bad, &lens, &ai] {
        let err = renderer
            .render_region(&proxy, settings, 0, rect)
            .unwrap_err();
        assert!(err.to_string().contains("original required"));
    }
    let adobe = renderer.for_process_version(ProcessVersion::adobe(6));
    assert!(
        adobe
            .render_region(&proxy, &s, 0, rect)
            .unwrap_err()
            .to_string()
            .contains("original required")
    );
    let cancel = CancellationToken::new();
    cancel.cancel();
    let mut count = 0;
    assert!(
        renderer
            .render_tiles(
                &proxy,
                &s,
                &[TileCoord::new(0, 0, 0)],
                RenderOutput::SceneLinear,
                &cancel,
                &mut |_| count += 1
            )
            .is_err()
    );
    assert_eq!(count, 0);
}
#[test]
fn resident_metrics_surfaces_and_export_rows_decline_without_touching_buffers() {
    let (_, proxy, s) = fixture(32, 24);
    let renderer = Renderer::new(Default::default());
    let cancel = CancellationToken::new();
    let rect = PixelRect::full(proxy.active_extent());
    assert!(!renderer.can_render_resident(&proxy, &s).unwrap());
    assert!(
        renderer
            .render_output_metrics(&proxy, &s, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        renderer
            .render_resident_region(&proxy, &s, 0, rect, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        renderer
            .render_resident_lens(&proxy, &s, 0, rect, None, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        !renderer
            .render_to_surface(&proxy, &s, 0, 0, &cancel)
            .unwrap()
    );
    assert!(
        renderer
            .render_surface(&proxy, &s, 0, 0, &cancel)
            .unwrap()
            .is_none()
    );
    assert!(
        renderer
            .render_rgb_linear(&proxy, 0, &s, &cancel)
            .unwrap_err()
            .to_string()
            .contains("original required")
    );
    let mut dst = vec![123.; 90];
    assert!(
        !renderer
            .render_export_rows(&proxy, &s, 0, 0..1, None, &mut dst, &cancel)
            .unwrap()
    );
    assert!(dst.iter().all(|v| *v == 123.));
}
#[test]
fn cancellation_from_sink_prevents_later_tile_delivery() {
    let (_, proxy, s) = fixture(269, 19);
    let renderer = Renderer::new(Default::default());
    let cancel = CancellationToken::new();
    let mut count = 0;
    let coords = Renderer::tiles_for(&proxy, 0, PixelRect::full(proxy.active_extent()));
    assert!(coords.len() > 1);
    let result = renderer.render_tiles(
        &proxy,
        &s,
        &coords,
        RenderOutput::SceneLinear,
        &cancel,
        &mut |_| {
            count += 1;
            cancel.cancel();
        },
    );
    assert!(result.is_err());
    assert_eq!(count, 1);
}

#[test]
fn persisted_proxy_reopens_into_the_same_camera_linear_render_route() {
    let (raw, preview, mut settings) = fixture(269, 19);
    let bytes = preview
        .camera_linear_proxy()
        .unwrap()
        .encode_persistent(123456)
        .unwrap();
    let decoded = CameraLinearProxy::decode_persistent(&bytes).unwrap();
    let reopened = RawImage::from_camera_linear_proxy(
        raw.recipe_owner(),
        ImageId(904),
        Arc::new(decoded.proxy),
    )
    .unwrap();
    settings.white_balance.mode = WhiteBalanceMode::Custom;
    settings.white_balance.temperature = 4200.;
    settings.white_balance.tint = 13.;
    settings.tone.exposure = 0.7;
    let renderer = Renderer::new(Default::default());
    for level in [0, 1, 2] {
        let expected = reference(&reopened, &settings, level);
        let tiles = renderer
            .render_region_as(
                &reopened,
                &settings,
                level,
                PixelRect::full(Extent::new(expected.width(), expected.height())),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        assert!(!tiles.is_empty());
        assert_tiles(&tiles, &expected, RenderOutput::SceneLinear, &settings);
    }
    assert_eq!(reopened.recipe_owner(), raw.id());
    assert_eq!(reopened.metadata().orientation, 6);
    assert!(reopened.rgb().is_none());
}
