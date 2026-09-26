use engine_api::{id::ImageId, jobs::CancellationToken, recipe::DevelopSettings, stage::StageId};
use image_core::{RawImage, Renderer, RendererConfig, RgbSource};

fn source(id: u128) -> RawImage {
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::LinearRec2020)
        .unwrap();
    let pixels = pipeline_cpu::Image::new(
        37,
        23,
        (0..3)
            .map(|c| {
                (0..37 * 23)
                    .map(|i| ((i * 17 + c * 31) % 251) as f32 / 83. - 0.2)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    RawImage::from_rgb(
        ImageId(id),
        RgbSource::from_raster(pixels, &profile).unwrap(),
    )
    .unwrap()
}
#[test]
fn exact_rgb_memo_reuses_only_upstream_and_bounds_payload() {
    let renderer = Renderer::new(RendererConfig {
        cache_budget_bytes: 2 << 20,
        ..Default::default()
    });
    let src = source(900);
    let cancel = CancellationToken::new();
    let mut s = DevelopSettings::default();
    s.lens.manual_distortion = 8.;
    s.lens.manual_vignetting = -12.;
    s.lens.defringe_purple.amount = 10.;
    s.tone.exposure = 0.7;
    s.effects.grain.amount = 4.;
    for revision in [1, 1, 2] {
        for edit in 0..3 {
            s.color.saturation = edit as f32 * 12.;
            let actual = renderer
                .render_rgb_linear(&src, revision, &s, &cancel)
                .unwrap();
            let expected = pipeline_cpu::render_linear_scaled(
                &s,
                &pipeline_cpu::RenderSource::Rgb(src.rgb().unwrap().pixels()),
                1,
            )
            .unwrap();
            assert_eq!(actual.planes(), expected.planes());
            let warm = renderer
                .render_rgb_linear(&src, revision, &s, &cancel)
                .unwrap();
            assert_eq!(warm.planes(), actual.planes());
        }
    }
    let stats = renderer.rgb_cache_stats();
    assert_eq!(stats.executions[StageId::Detail.index()], 2);
    assert_eq!(stats.executions[StageId::Color.index()], 6);
    assert!(stats.bytes <= 2 << 20);
    let tiny = Renderer::new(RendererConfig {
        cache_budget_bytes: 1,
        ..Default::default()
    });
    tiny.render_rgb_linear(&src, 1, &s, &cancel).unwrap();
    assert_eq!(tiny.rgb_cache_stats().bytes, 0);
}

#[test]
fn white_balance_edit_reuses_lens_analysis_and_alignment() {
    let renderer = Renderer::new(RendererConfig::default());
    let src = source(901);
    let cancel = CancellationToken::new();
    let mut s = DevelopSettings::default();
    renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
    s.white_balance.mode = engine_api::recipe::settings::WhiteBalanceMode::Custom;
    s.white_balance.temperature = 7200.;
    let actual = renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
    let expected = pipeline_cpu::render_linear_scaled(
        &s,
        &pipeline_cpu::RenderSource::Rgb(src.rgb().unwrap().pixels()),
        1,
    )
    .unwrap();
    assert_eq!(actual.planes(), expected.planes());
    let stats = renderer.rgb_cache_stats();
    assert_eq!(stats.executions[StageId::Lens.index()], 1);
    assert_eq!(stats.executions[StageId::WhiteBalance.index()], 2);
}

#[test]
fn final_checkpoint_avoids_rebuilding_evicted_upstream() {
    let renderer = Renderer::new(RendererConfig {
        cache_budget_bytes: 37 * 23 * 12,
        ..Default::default()
    });
    let src = source(902);
    let cancel = CancellationToken::new();
    let settings = DevelopSettings::default();
    renderer
        .render_rgb_linear(&src, 1, &settings, &cancel)
        .unwrap();
    let before = renderer.rgb_cache_stats();
    renderer
        .render_rgb_linear(&src, 1, &settings, &cancel)
        .unwrap();
    assert_eq!(renderer.rgb_cache_stats().executions, before.executions);
}

#[test]
fn tone_crop_and_identity_invalidation_and_cancel() {
    let renderer = Renderer::new(RendererConfig::default());
    let src = source(903);
    let cancel = CancellationToken::new();
    let mut s = DevelopSettings::default();
    renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
    s.tone.exposure = 0.4;
    renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
    let stats = renderer.rgb_cache_stats();
    assert_eq!(stats.executions[StageId::Detail.index()], 1);
    assert_eq!(stats.executions[StageId::Tone.index()], 2);
    s.geometry.crop.rect.right = 0.8;
    let actual = renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
    let expected = pipeline_cpu::render_linear_scaled(
        &s,
        &pipeline_cpu::RenderSource::Rgb(src.rgb().unwrap().pixels()),
        1,
    )
    .unwrap();
    assert_eq!(actual.planes(), expected.planes());
    let stats = renderer.rgb_cache_stats();
    assert_eq!(stats.executions[StageId::Tone.index()], 2);
    assert_eq!(stats.executions[StageId::Effects.index()], 3);
    renderer
        .render_rgb_linear(&source(904), 0, &s, &cancel)
        .unwrap();
    assert_eq!(
        renderer.rgb_cache_stats().executions[StageId::Lens.index()],
        2
    );
    cancel.cancel();
    assert!(matches!(
        renderer.render_rgb_linear(&src, 0, &s, &cancel),
        Err(engine_api::EngineError::Cancelled)
    ));
}

#[test]
fn full_width_image_ids_never_collapse_to_json_null() {
    let renderer = Renderer::new(RendererConfig::default());
    let cancel = CancellationToken::new();
    let s = DevelopSettings::default();
    for id in [u128::MAX, u128::MAX - 1] {
        renderer
            .render_rgb_linear(&source(id), 0, &s, &cancel)
            .unwrap();
    }
    assert_eq!(
        renderer.rgb_cache_stats().executions[StageId::Lens.index()],
        2
    );
}

#[test]
fn linear_boundary_preserves_bits_and_rejects_nonfinite_mutation() {
    let planes = vec![vec![-0.0, -0.25, f32::MIN_POSITIVE, 4.125]; 3];
    let pixels = pipeline_cpu::Image::new(4, 1, planes.clone()).unwrap();
    let src = RgbSource::from_linear_rec2020(pixels.clone()).unwrap();
    for (actual, expected) in src
        .pixels()
        .planes()
        .iter()
        .flatten()
        .zip(planes.iter().flatten())
    {
        assert_eq!(actual.to_bits(), expected.to_bits());
    }
    let mut invalid = pixels;
    let coord = engine_api::tile::TileCoord::new(0, 0, 0);
    let tile = invalid.tile(coord, 0, 1).unwrap();
    let mut samples = tile.samples::<f32>().unwrap().to_vec();
    samples[0] = f32::NAN;
    let tile = engine_api::tile::Tile::from_samples(coord, tile.layout(), samples).unwrap();
    invalid.put(&tile).unwrap();
    assert!(RgbSource::from_linear_rec2020(invalid).is_err());
}

#[test]
fn automatic_rgb_ca_and_vignette_are_real_corrections() {
    for ca in [false, true] {
        let n = 80u32;
        let planes = (0..3)
            .map(|c| {
                (0..n * n)
                    .map(|i| {
                        let x = 2. * (i % n) as f64 / (n - 1) as f64 - 1.;
                        let y = 2. * (i / n) as f64 / (n - 1) as f64 - 1.;
                        if ca {
                            let scale = [1.012, 1., 0.989][c];
                            (0.5 + 0.2 * (19. * x / scale).sin() + 0.2 * (23. * y / scale).sin())
                                as f32
                        } else {
                            (0.5 * (1. - 0.2 * (x * x + y * y))) as f32
                        }
                    })
                    .collect()
            })
            .collect();
        let pixels = pipeline_cpu::Image::new(n, n, planes).unwrap();
        let src = RawImage::from_rgb(
            ImageId(905),
            RgbSource::from_linear_rec2020(pixels.clone()).unwrap(),
        )
        .unwrap();
        let renderer = Renderer::new(RendererConfig::default());
        let mut s = DevelopSettings::default();
        s.detail.sharpening.amount = 0.;
        s.detail.noise_reduction.color = 0.;
        if ca {
            s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
        }
        let cancel = CancellationToken::new();
        let actual = renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
        let expected =
            pipeline_cpu::render_linear_scaled(&s, &pipeline_cpu::RenderSource::Rgb(&pixels), 1)
                .unwrap();
        assert_eq!(actual.planes(), expected.planes());
        assert_ne!(actual.planes(), pixels.planes());
        if ca {
            let error = |im: &pipeline_cpu::Image| {
                (5..75)
                    .flat_map(|y| (5..75).map(move |x| (y * n + x) as usize))
                    .map(|i| {
                        (im.planes()[0][i] - im.planes()[1][i]).abs()
                            + (im.planes()[2][i] - im.planes()[1][i]).abs()
                    })
                    .sum::<f32>()
            };
            assert!(error(&actual) < error(&pixels) * 0.5);
        } else {
            assert!(actual.planes()[0][0] > pixels.planes()[0][0] + 0.1);
        }
        s.tone.exposure = 0.2;
        renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
        assert_eq!(
            renderer.rgb_cache_stats().executions[StageId::Lens.index()],
            1
        );
    }
}
