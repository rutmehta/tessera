//! Synthetic catalog/XMP import -> CPU pixels. Tolerance is 2e-6 scene-linear.
use engine_api::recipe::Recipe;
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

fn fixture(name: &str, extension: &str) -> String {
    std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(format!(
            "../import-lrcat/tests/data/lr11/{name}.{extension}"
        )),
    )
    .unwrap()
}
fn catalog_recipe(name: &str) -> Recipe {
    let dir = tempfile::tempdir().unwrap();
    let f = import_lrcat::fixture::write(dir.path()).unwrap();
    let db = rusqlite::Connection::open(&f.catalog).unwrap();
    let id: i64 = db
        .query_row(
            "SELECT image FROM Adobe_imageDevelopSettings ORDER BY image LIMIT 1",
            [],
            |r| r.get(0),
        )
        .unwrap();
    db.execute(
        "UPDATE Adobe_imageDevelopSettings SET text=?1, processVersion='15.4' WHERE image=?2",
        rusqlite::params![fixture(name, "lua"), id],
    )
    .unwrap();
    drop(db);
    let plan = import_lrcat::import(&f.catalog).unwrap();
    let r = plan
        .images
        .iter()
        .find(|i| i.catalog_id == id)
        .unwrap()
        .recipe
        .clone();
    assert_eq!(r.history.entries.len(), 1);
    r
}
fn lin(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn image(pixels: &[[f32; 3]]) -> Image {
    Image::new(
        pixels.len() as u32,
        1,
        (0..3)
            .map(|c| pixels.iter().map(|p| p[c]).collect())
            .collect(),
    )
    .unwrap()
}
fn close(actual: &Image, expected: &[[f32; 3]]) {
    for (i, p) in expected.iter().enumerate() {
        for (c, v) in p.iter().enumerate() {
            assert!(
                (actual.planes()[c][i] - v).abs() < 2e-6,
                "pixel {i} channel {c}: {} != {v}",
                actual.planes()[c][i]
            );
        }
    }
}
#[test]
fn synthetic_local_imports_render_masked_reference_pixels() {
    for name in ["curve", "point-color", "overlay", "defringe"] {
        let lua = catalog_recipe(name);
        let (xmp, w) = import_lrcat::xmp::parse(&fixture(name, "xmp"), "15.4").unwrap();
        assert!(w.is_empty(), "{w:?}");
        let pixels = match name {
            "point-color" => [[0.75, 0.25, 0.25].map(lin), [0.25, 0.75, 0.75].map(lin)],
            "defringe" => [[1., 0., 1.], [0.01; 3]],
            _ => [[0.2, 0.3, 0.4], [0.2, 0.3, 0.4]],
        };
        let input = image(&pixels);
        let mut expected = pixels;
        for i in 0..2 {
            let a = if i == 0 { 0.75 } else { 0.25 };
            let y = 0.2627 * pixels[i][0] + 0.678 * pixels[i][1] + 0.0593 * pixels[i][2];
            let adjusted = match name {
                // Global point-curve coordinate is log1p(v/0.18), normalized at white.
                "curve" => pixels[i].map(|v| 0.18 * ((1. + v / 0.18).sqrt() - 1.)),
                "point-color" => {
                    if i == 0 {
                        [0.75, 0.5, 0.25].map(lin)
                    } else {
                        pixels[i]
                    }
                }
                "overlay" => std::array::from_fn(|c| {
                    pixels[i][c] + 0.5 * ((if c == 1 { y / 0.678 } else { 0. }) - pixels[i][c])
                }),
                "defringe" => {
                    if i == 0 {
                        [y; 3]
                    } else {
                        pixels[i]
                    }
                }
                _ => unreachable!(),
            };
            expected[i] = std::array::from_fn(|c| pixels[i][c] + a * (adjusted[c] - pixels[i][c]));
        }
        for r in [lua, xmp] {
            let out = pipeline_cpu::locals_image(
                &input,
                &r.settings.locals.adjustments,
                Default::default(),
            )
            .unwrap();
            close(&out, &expected);
            let full = render_linear_scaled(&r.settings, &RenderSource::Rgb(&input), 1).unwrap();
            close(&full, &expected);
            let mut groups = r.settings.locals.adjustments.clone();
            groups[0].enabled = false;
            assert_eq!(
                pipeline_cpu::locals_image(&input, &groups, Default::default())
                    .unwrap()
                    .planes(),
                input.planes()
            );
            groups[0].enabled = true;
            groups[0].amount = 0.;
            assert_eq!(
                pipeline_cpu::locals_image(&input, &groups, Default::default())
                    .unwrap()
                    .planes(),
                input.planes()
            );
            // Shift the gradient to leave the right half completely unselected.
            groups[0].amount = 100.;
            groups[0].components[0].kind = engine_api::recipe::MaskKind::Linear {
                start: [0., 0.],
                end: [0.5, 0.],
            };
            let outside = pipeline_cpu::locals_image(&input, &groups, Default::default()).unwrap();
            for c in 0..3 {
                assert_eq!(outside.planes()[c][1], input.planes()[c][1]);
            }
        }
    }
}

#[test]
fn local_point_color_precedes_bw_and_cached_edits_invalidate_tone() {
    let recipe = catalog_recipe("point-color");
    let mut settings = recipe.settings.clone();
    settings.color.monochrome = Some(engine_api::recipe::settings::MonochromeSettings {
        enabled: true,
        ..Default::default()
    });
    let input = image(&[[0.75, 0.25, 0.25].map(lin); 2]);
    let pre = pipeline_cpu::locals_image(&input, &settings.locals.adjustments, Default::default())
        .unwrap();
    let mut expected = pre.clone();
    for coord in expected.coords() {
        let mut tile = expected.tile(coord, 0, 1).unwrap();
        pipeline_cpu::color(&mut tile, &settings.color).unwrap();
        expected.put(&tile).unwrap();
    }
    let actual = render_linear_scaled(&settings, &RenderSource::Rgb(&input), 1).unwrap();
    for (a, b) in actual
        .planes()
        .iter()
        .flatten()
        .zip(expected.planes().iter().flatten())
    {
        assert!((a - b).abs() < 2e-6);
    }
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::LinearRec2020)
        .unwrap();
    let src = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(111),
        image_core::RgbSource::from_raster(input.clone(), &profile).unwrap(),
    )
    .unwrap();
    let renderer = image_core::Renderer::new(image_core::RendererConfig::default());
    let cancel = engine_api::jobs::CancellationToken::new();
    let first = renderer
        .render_rgb_linear(&src, 0, &settings, &cancel)
        .unwrap();
    let mut changed = settings.clone();
    changed.locals.adjustments[0]
        .params
        .point_colors
        .as_mut()
        .unwrap()[0]
        .hue_shift = -30.;
    assert_ne!(
        settings.stage_hashes()[engine_api::stage::StageId::Tone.index()],
        changed.stage_hashes()[engine_api::stage::StageId::Tone.index()]
    );
    let second = renderer
        .render_rgb_linear(&src, 0, &changed, &cancel)
        .unwrap();
    assert_ne!(first.planes(), second.planes());
    assert_eq!(
        second.planes(),
        render_linear_scaled(&changed, &RenderSource::Rgb(src.rgb().unwrap().pixels()), 1)
            .unwrap()
            .planes()
    );
}

#[test]
fn radial_conflict_retains_source_and_renders_no_local_change() {
    for ext in ["lua", "xmp"] {
        let (r, w) = if ext == "lua" {
            import_lrcat::lua_develop::parse(&fixture("radial-conflict", ext), "15.4")
        } else {
            import_lrcat::xmp::parse(&fixture("radial-conflict", ext), "15.4")
        }
        .unwrap();
        assert!(
            w.iter()
                .any(|w| w.contains("radial mask inversion flags conflict"))
        );
        assert!(r.settings.locals.adjustments.is_empty());
        let input = image(&[[0.2; 3]; 2]);
        assert_eq!(
            pipeline_cpu::locals_image(&input, &r.settings.locals.adjustments, Default::default())
                .unwrap()
                .planes(),
            input.planes()
        );
    }
}

#[test]
fn object_instance_uses_existing_host_mask_seam_and_preserves_hint() {
    use engine_api::{
        EngineResult,
        id::ImageId,
        recipe::{LocalAdjustment, MaskKind},
    };
    use image_core::{
        PixelRect, RawImage, RenderOutput, Renderer, RendererConfig, RgbSource,
        mask_cache::MaskHooks,
    };
    struct Selection;
    impl MaskHooks for Selection {
        fn revision(&self) -> u64 {
            1
        }
        fn rasterize(
            &self,
            input: &Image,
            group: &LocalAdjustment,
            _: u8,
        ) -> EngineResult<Vec<f32>> {
            assert!(matches!(group.components[0].kind, MaskKind::Object { .. }));
            let hint = group.components[0]
                .adobe_ai
                .as_ref()
                .unwrap()
                .instance_hint
                .as_ref()
                .unwrap();
            assert_eq!(hint["InstanceIDs"][0]["InstanceID"], 2.);
            Ok((0..input.width() * input.height())
                .map(|i| if i % input.width() == 0 { 1. } else { 0. })
                .collect())
        }
    }
    for recipe in [
        catalog_recipe("instance"),
        import_lrcat::xmp::parse(&fixture("instance", "xmp"), "15.4")
            .unwrap()
            .0,
    ] {
        let profile = color_mgmt::Registry::new()
            .builtin(color_mgmt::Builtin::LinearRec2020)
            .unwrap();
        let src = RawImage::from_rgb(
            ImageId(112),
            RgbSource::from_raster(image(&[[0.2; 3]; 2]), &profile).unwrap(),
        )
        .unwrap();
        let renderer = Renderer::new(RendererConfig {
            cache_budget_bytes: 0,
            ..Default::default()
        });
        renderer
            .mask_cache()
            .set_hooks(Some(std::sync::Arc::new(Selection)));
        let tiles = renderer
            .render_region_as(
                &src,
                &recipe.settings,
                0,
                PixelRect::full(src.active_extent()),
                RenderOutput::SceneLinear,
            )
            .unwrap();
        let samples = tiles[0].samples::<f32>().unwrap();
        for c in 0..3 {
            assert!((samples[c * 2] - 0.4).abs() < 2e-6);
            assert!((samples[c * 2 + 1] - 0.2).abs() < 2e-6);
        }
    }
}
