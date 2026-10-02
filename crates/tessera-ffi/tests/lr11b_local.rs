//! LR-11b: synthetic catalog/XMP import -> CPU pixels. Scene-linear tolerance 2e-6.
use engine_api::recipe::Recipe;
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

fn fixture(name: &str, extension: &str) -> String {
    let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../import-lrcat/tests/data");
    ["lr11", "lr11b"]
        .iter()
        .find_map(|dir| {
            std::fs::read_to_string(data.join(format!("{dir}/{name}.{extension}"))).ok()
        })
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
fn xmp_recipe(name: &str) -> Recipe {
    let (r, w) = import_lrcat::xmp::parse(&fixture(name, "xmp"), "15.4").unwrap();
    assert!(w.is_empty(), "{w:?}");
    r
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
fn max_difference(actual: &Image, expected: &[[f32; 3]]) -> f32 {
    let mut worst = 0f32;
    for (i, p) in expected.iter().enumerate() {
        for (c, v) in p.iter().enumerate() {
            worst = worst.max((actual.planes()[c][i] - v).abs());
        }
    }
    worst
}

/// B3: on an SDR image the extended local curve must not replace the ordinary
/// one. The reference is the ordinary curve (halved log1p(v/0.18) coordinate)
/// blended through the 0.75/0.25 gradient alpha.
#[test]
fn b3_sdr_extended_local_curve_renders_the_ordinary_curve() {
    let pixels = [[0.2f32, 0.3, 0.4]; 2];
    let input = image(&pixels);
    let mut expected = pixels;
    for (i, alpha) in [0.75f32, 0.25].into_iter().enumerate() {
        expected[i] = pixels[i].map(|v| v + alpha * (0.18 * ((1. + v / 0.18).sqrt() - 1.) - v));
    }
    for r in [
        catalog_recipe("curve-extended-sdr"),
        xmp_recipe("curve-extended-sdr"),
    ] {
        let local =
            pipeline_cpu::locals_image(&input, &r.settings.locals.adjustments, Default::default())
                .unwrap();
        assert!(max_difference(&local, &expected) < 2e-6);
        let full = render_linear_scaled(&r.settings, &RenderSource::Rgb(&input), 1).unwrap();
        assert!(max_difference(&full, &expected) < 2e-6);
        // The ordinary-only fixture renders the same pixels.
        let ordinary = catalog_recipe("curve");
        assert_eq!(
            pipeline_cpu::locals_image(
                &input,
                &ordinary.settings.locals.adjustments,
                Default::default()
            )
            .unwrap()
            .planes(),
            local.planes()
        );
    }
    // Control: with HDR output the extended curve is honoured and differs.
    for r in [
        catalog_recipe("curve-extended-hdr"),
        xmp_recipe("curve-extended-hdr"),
    ] {
        let local =
            pipeline_cpu::locals_image(&input, &r.settings.locals.adjustments, Default::default())
                .unwrap();
        assert!(max_difference(&local, &expected) > 1e-3);
    }
}

use engine_api::recipe::{DevelopSettings, LocalAdjustment};

fn global_curve() -> engine_api::recipe::settings::ToneCurves {
    serde_json::from_value(serde_json::json!({
        "rgb":[{"x":0,"y":0},{"x":0.4,"y":0.7},{"x":1,"y":1}]
    }))
    .unwrap()
}

fn per_tile(image: &mut Image, f: impl Fn(&mut engine_api::tile::Tile)) {
    for coord in image.coords() {
        let mut tile = image.tile(coord, 0, 1).unwrap();
        f(&mut tile);
        image.put(&tile).unwrap();
    }
}

/// Split by hand (independent of the production helper): the Point Color
/// part of each group, and each group without it.
fn split(groups: &[LocalAdjustment]) -> (Vec<LocalAdjustment>, Vec<LocalAdjustment>) {
    let mut before = Vec::new();
    let mut after = groups.to_vec();
    for g in &mut after {
        if let Some(points) = g.params.point_colors.take() {
            let mut p = g.clone();
            p.params = engine_api::recipe::LocalParams {
                point_colors: Some(points),
                ..Default::default()
            };
            before.push(p);
        }
    }
    (before, after)
}

/// S7 reference: local Point Color is ONE stage, after basic Tone and before
/// monochrome conversion and the global point curves, whether B&W is on or off.
fn documented_order(s: &DevelopSettings, toned: &Image) -> Image {
    let (points, rest) = split(&s.locals.adjustments);
    let mut out = pipeline_cpu::locals_image(toned, &points, Default::default()).unwrap();
    if s.color.monochrome.as_ref().is_some_and(|m| m.enabled) {
        per_tile(&mut out, |t| {
            pipeline_cpu::color(t, &s.color_before_curves()).unwrap()
        });
    }
    let mut out = pipeline_cpu::tone_extra_image(&out, &s.tone).unwrap();
    per_tile(&mut out, |t| {
        pipeline_cpu::color(t, &s.color_after_curves()).unwrap()
    });
    pipeline_cpu::locals_image(&out, &rest, Default::default()).unwrap()
}

/// The pre-fix B&W-off order: Point Color with the other locals, after curves.
fn after_curves_order(s: &DevelopSettings, toned: &Image) -> Image {
    let mut out = pipeline_cpu::tone_extra_image(toned, &s.tone).unwrap();
    per_tile(&mut out, |t| {
        pipeline_cpu::color(t, &s.color_after_curves()).unwrap()
    });
    pipeline_cpu::locals_image(&out, &s.locals.adjustments, Default::default()).unwrap()
}

fn planes_difference(a: &Image, b: &Image) -> f32 {
    a.planes()
        .iter()
        .flatten()
        .zip(b.planes().iter().flatten())
        .map(|(a, b)| (a - b).abs())
        .fold(0., f32::max)
}

fn monochrome(on: bool) -> Option<engine_api::recipe::settings::MonochromeSettings> {
    on.then(|| engine_api::recipe::settings::MonochromeSettings {
        enabled: true,
        ..Default::default()
    })
}

fn tiles_image(tiles: &[engine_api::tile::Tile], width: u32, height: u32) -> Image {
    let mut out = Image::new(width, height, vec![vec![0.; (width * height) as usize]; 3]).unwrap();
    for t in tiles {
        out.put(t).unwrap();
    }
    out
}

/// S7, RGB sources: CPU reference, image-core RGB path and the GPU-selected
/// renderer (which must fall back to CPU) all use the one documented stage.
#[test]
fn s7_rgb_local_point_color_is_one_stage_before_curves_with_and_without_bw() {
    let lin = |v: f32| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let input = image(&[[0.75, 0.25, 0.25].map(lin), [0.6, 0.3, 0.2].map(lin)]);
    let recipe = catalog_recipe("point-color");
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::LinearRec2020)
        .unwrap();
    let src = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(113),
        image_core::RgbSource::from_raster(input.clone(), &profile).unwrap(),
    )
    .unwrap();
    let gpu = std::sync::Arc::new(pipeline_gpu::GpuContext::new().unwrap());
    for bw in [false, true] {
        let mut s = recipe.settings.clone();
        s.tone.curves = global_curve();
        s.color.monochrome = monochrome(bw);
        let expected = documented_order(&s, &input);
        if !bw {
            // The stage is observable: the after-curves order is different.
            assert!(planes_difference(&expected, &after_curves_order(&s, &input)) > 1e-4);
        }
        let reference = render_linear_scaled(&s, &RenderSource::Rgb(&input), 1).unwrap();
        assert!(planes_difference(&reference, &expected) < 2e-6, "bw={bw}");
        let renderer = image_core::Renderer::new(image_core::RendererConfig::default());
        let cancel = engine_api::jobs::CancellationToken::new();
        let rgb = renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
        assert!(planes_difference(&rgb, &expected) < 2e-6, "bw={bw}");
        // No tile cache: a cached f16 checkpoint would be compared with f32.
        let uncached = image_core::Renderer::new(image_core::RendererConfig {
            cache_budget_bytes: 0,
            ..Default::default()
        });
        let rect = image_core::PixelRect::full(src.active_extent());
        let cpu = uncached
            .render_region_as(&src, &s, 0, rect, image_core::RenderOutput::SceneLinear)
            .unwrap();
        let selected = uncached.for_backend(std::sync::Arc::new(pipeline_gpu::GpuStageOp::new(
            gpu.clone(),
        )));
        let fallback = selected
            .render_region_as(&src, &s, 0, rect, image_core::RenderOutput::SceneLinear)
            .unwrap();
        assert_eq!(
            cpu[0].samples::<f32>().unwrap(),
            fallback[0].samples::<f32>().unwrap(),
            "bw={bw}"
        );
        assert!(planes_difference(&tiles_image(&cpu, 2, 1), &expected) < 2e-6);
    }
}

/// S7: the Tone stage hash covers local Point Color in both modes, so a
/// cached render is invalidated when only the local point changes.
#[test]
fn s7_tone_cache_tracks_local_point_color_with_and_without_bw() {
    let recipe = catalog_recipe("point-color");
    let input = image(&[[0.5, 0.2, 0.2]; 2]);
    let profile = color_mgmt::Registry::new()
        .builtin(color_mgmt::Builtin::LinearRec2020)
        .unwrap();
    let src = image_core::RawImage::from_rgb(
        engine_api::id::ImageId(114),
        image_core::RgbSource::from_raster(input.clone(), &profile).unwrap(),
    )
    .unwrap();
    let tone = engine_api::stage::StageId::Tone.index();
    for bw in [false, true] {
        let mut s = recipe.settings.clone();
        s.tone.curves = global_curve();
        s.color.monochrome = monochrome(bw);
        let mut changed = s.clone();
        changed.locals.adjustments[0]
            .params
            .point_colors
            .as_mut()
            .unwrap()[0]
            .hue_shift = -30.;
        assert_ne!(
            s.stage_hashes()[tone],
            changed.stage_hashes()[tone],
            "bw={bw}"
        );
        let mut plain = s.clone();
        plain.locals.adjustments[0].params.point_colors = None;
        plain.locals.adjustments[0].params.exposure = 0.5;
        let mut plain_changed = plain.clone();
        plain_changed.locals.adjustments[0].params.exposure = 1.;
        // Groups without local Point Color keep the Tone hash out of locals.
        assert_eq!(
            plain.stage_hashes()[tone],
            plain_changed.stage_hashes()[tone],
            "bw={bw}"
        );
        let renderer = image_core::Renderer::new(image_core::RendererConfig::default());
        let cancel = engine_api::jobs::CancellationToken::new();
        let first = renderer.render_rgb_linear(&src, 0, &s, &cancel).unwrap();
        let second = renderer
            .render_rgb_linear(&src, 0, &changed, &cancel)
            .unwrap();
        assert_ne!(first.planes(), second.planes(), "bw={bw}");
        assert!(
            planes_difference(
                &second,
                &render_linear_scaled(&changed, &RenderSource::Rgb(&input), 1).unwrap()
            ) < 2e-6
        );
    }
}

fn synthetic_raw(id: u128, width: u32, height: u32) -> image_core::RawImage {
    let layout = raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]);
    let mut samples = Vec::with_capacity((width * height) as usize);
    for y in 0..height {
        for x in 0..width {
            let c = layout.channel_at(x, y);
            let gain = [0.9, 0.35, 0.3, 0.35][c];
            samples.push(
                gain * (0.1 + 0.5 * x as f32 / width as f32 + 0.2 * y as f32 / height as f32),
            );
        }
    }
    let metadata = raw_decode::RawMetadata {
        make: "Synthetic".into(),
        model: "Test".into(),
        lens: None,
        iso: 100.0,
        shutter_s: 0.01,
        aperture: 4.0,
        focal_mm: 50.0,
        capture_time: 0,
        orientation: 1,
        opcode_lists: [None, None, None],
        width,
        height,
        cfa_layout: layout,
        black_levels: [0.0; 4],
        white_level: 16383,
        as_shot_wb: [2.0, 1.0, 1.6, 1.0],
        camera_to_xyz: engine_api::color::ColorMatrix3([[0.0; 3]; 3]),
        cam_xyz: [
            [0.9, 0.2, -0.1],
            [-0.3, 1.2, 0.1],
            [0.0, 0.1, 0.8],
            [0.0; 3],
        ],
        rgb_cam: [[0.0; 4]; 3],
        default_crop: [0, 0, width, height],
        has_gain_map: false,
        has_opcode_list: false,
    };
    image_core::RawImage::new(
        engine_api::id::ImageId(id),
        std::sync::Arc::new(raw_decode::CfaImage::from_linear(width, height, samples).unwrap()),
        std::sync::Arc::new(metadata),
    )
    .unwrap()
}

/// S7, raw sources: the image-core raw path matches the documented order
/// (1e-5, the existing raw-path reference bound) and the GPU-selected renderer
/// falls back to identical CPU pixels.
#[test]
fn s7_raw_local_point_color_is_one_stage_before_curves_with_and_without_bw() {
    let (w, h) = (32, 24);
    let raw = synthetic_raw(115, w, h);
    let source = RenderSource::Cfa {
        image: raw.cfa(),
        metadata: raw.metadata(),
    };
    let mut neutral = DevelopSettings::default();
    neutral.detail.sharpening.amount = 0.;
    neutral.detail.noise_reduction.color = 0.;
    let base = render_linear_scaled(&neutral, &source, 1).unwrap();
    let group: LocalAdjustment = serde_json::from_value(serde_json::json!({
        "params":{"point_colors":[{"source_lch":[0.6,0.15,30],"hue_shift":60,"saturation_shift":-40,"luminance_shift":30,"range":100}]},
        "components":[{"kind":"radial","center":[0.5,0.5],"radii":[2,2],"angle":0,"feather":0}]
    }))
    .unwrap();
    let gpu = std::sync::Arc::new(pipeline_gpu::GpuContext::new().unwrap());
    for bw in [false, true] {
        let mut s = neutral.clone();
        s.tone.curves = global_curve();
        s.color.monochrome = monochrome(bw);
        s.locals.adjustments = vec![group.clone()];
        let expected = documented_order(&s, &base);
        let mut no_point = s.clone();
        no_point.locals.adjustments.clear();
        assert!(planes_difference(&expected, &documented_order(&no_point, &base)) > 1e-3);
        if !bw {
            assert!(planes_difference(&expected, &after_curves_order(&s, &base)) > 1e-4);
        }
        let reference = render_linear_scaled(&s, &source, 1).unwrap();
        assert!(planes_difference(&reference, &expected) < 2e-6, "bw={bw}");
        let renderer = image_core::Renderer::new(image_core::RendererConfig {
            cache_budget_bytes: 0,
            ..Default::default()
        });
        let rect = image_core::PixelRect::full(raw.level_extent(0));
        let cpu = renderer
            .render_region_as(&raw, &s, 0, rect, image_core::RenderOutput::SceneLinear)
            .unwrap();
        assert!(
            planes_difference(&tiles_image(&cpu, w, h), &expected) < 1e-5,
            "bw={bw}"
        );
        let selected = renderer.for_backend(std::sync::Arc::new(pipeline_gpu::GpuStageOp::new(
            gpu.clone(),
        )));
        let fallback = selected
            .render_region_as(&raw, &s, 0, rect, image_core::RenderOutput::SceneLinear)
            .unwrap();
        for (a, b) in cpu.iter().zip(&fallback) {
            assert_eq!(
                a.samples::<f32>().unwrap(),
                b.samples::<f32>().unwrap(),
                "bw={bw}"
            );
        }
    }
}
