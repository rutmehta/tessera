//! Real RAW fixtures: every file in fixtures/raw runs by default.
//!
//! `IMAGE_CORE_FIXTURES=arw,cr3` narrows a run for speed (extension or file
//! name substring). Without fixtures each test prints an uncaptured SKIPPED
//! line; `TESSERA_REQUIRE_RAW_FIXTURES=1` makes that a failure.

mod common;
#[path = "common/preview.rs"]
mod preview;
#[path = "common/raw_fixtures.rs"]
mod raw_fixtures;

use std::path::PathBuf;
use std::time::Instant;

use common::*;
use engine_api::id::ImageId;
use engine_api::recipe::DevelopSettings;
use engine_api::stage::StageId;
use image_core::{CountingStageOp, CpuStageOp, PixelRect, RawImage, RenderOutput, Renderer};
use image_core::{RendererConfig, TileCache};
use pipeline_cpu::RenderSource;
use std::sync::Arc;

/// Narrows the fixture set for speed; correctness coverage never needs it.
const SELECT_ENV: &str = "IMAGE_CORE_FIXTURES";

fn selected(test: &str) -> Vec<PathBuf> {
    raw_fixtures::selected(test, SELECT_ENV)
}

/// Default settings with the explicit lens auto-calibration opt-in. Since
/// ENG-7 the default `Auto` applies no image-estimated distortion, so with
/// plain defaults no fixture would exercise the Geometry stage; the opt-in
/// keeps the estimated CR3/RAF distortion in these engine/reference checks.
fn geometry_settings() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.lens.profile = engine_api::recipe::settings::LensProfileSource::AutoCalibrated;
    s
}

/// Every selected camera is compared, and every mismatch is reported, before
/// the test fails: one camera's failure must not hide another's.
#[test]
fn fixture_level3_matches_pipeline_cpu() {
    let mut failures = Vec::new();
    for path in selected("fixture_level3_matches_pipeline_cpu") {
        let name = raw_fixtures::name(&path);
        let image = RawImage::open(ImageId(42), &path).unwrap();
        let s = geometry_settings();
        let source = RenderSource::Cfa {
            image: image.cfa(),
            metadata: image.metadata(),
        };
        let e = image.level_extent(3);
        let rect = PixelRect::full(e);
        let r = Renderer::new(RendererConfig::default());

        let t = Instant::now();
        let linear = r
            .render_region_as(&image, &s, 3, rect, RenderOutput::SceneLinear)
            .unwrap();
        let tiled_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let reference = preview::linear(&source, &s, 8);
        let reference_ms = t.elapsed().as_secs_f64() * 1e3;
        assert_eq!((reference.width(), reference.height()), (e.width, e.height));
        let diff = max_f32_diff(&assemble_f32(e, &linear), reference.planes());

        // Display output from a second cold renderer is identical as well.
        let display = Renderer::new(RendererConfig::default())
            .render_region(&image, &s, 3, rect)
            .unwrap();
        let expected = preview::display(&reference, &s);
        let d = max_u8_diff(&assemble_u8(e, &display), &expected);
        eprintln!(
            "{name}: {}x{} L3, max linear diff {diff:e}, display diff {d}, tiled {tiled_ms:.0} ms vs reference {reference_ms:.0} ms",
            e.width, e.height
        );
        if diff > 1e-5 || d != 0 {
            failures.push(format!(
                "{name}: scene-linear max diff {diff:e}, display max diff {d}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Level 0 is the full-resolution reference path itself (render.rs module
/// docs): bit-identical to `pipeline_cpu::render_linear_scaled(.., 1)` for
/// every camera, including the lens correction the reference resolves from
/// the whole demosaiced frame (the L3 preview model resolves from sparse
/// sensor patches, so this also pins those two resolutions together).
#[test]
fn fixture_level0_matches_pipeline_cpu_reference() {
    let mut failures = Vec::new();
    for path in selected("fixture_level0_matches_pipeline_cpu_reference") {
        let name = raw_fixtures::name(&path);
        let image = RawImage::open(ImageId(44), &path).unwrap();
        let s = geometry_settings();
        let e = image.level_extent(0);
        let t = Instant::now();
        let linear = Renderer::new(RendererConfig::default())
            .render_region_as(&image, &s, 0, PixelRect::full(e), RenderOutput::SceneLinear)
            .unwrap();
        let tiled_ms = t.elapsed().as_secs_f64() * 1e3;
        let t = Instant::now();
        let reference = pipeline_cpu::render_linear_scaled(
            &s,
            &RenderSource::Cfa {
                image: image.cfa(),
                metadata: image.metadata(),
            },
            1,
        )
        .unwrap();
        let reference_ms = t.elapsed().as_secs_f64() * 1e3;
        assert_eq!((reference.width(), reference.height()), (e.width, e.height));
        let diff = max_f32_diff(&assemble_f32(e, &linear), reference.planes());
        eprintln!(
            "{name}: {}x{} L0, max linear diff {diff:e}, tiled {tiled_ms:.0} ms vs reference {reference_ms:.0} ms",
            e.width, e.height
        );
        if diff != 0.0 {
            failures.push(format!("{name}: L0 scene-linear max diff {diff:e}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn fixture_level3_m2_extremes_are_finite() {
    for path in raw_fixtures::all("fixture_level3_m2_extremes_are_finite") {
        let image = RawImage::open(ImageId(43), &path).unwrap();
        let r = Renderer::new(RendererConfig::default());
        for sign in [-1., 1.] {
            let mut s = DevelopSettings::default();
            s.tone.texture = sign * 100.;
            s.tone.clarity = sign * 100.;
            s.tone.dehaze = sign * 100.;
            s.color.vibrance = sign * 100.;
            s.color.saturation = sign * 100.;
            s.tone.exposure = sign * 10.;
            s.tone.contrast = sign * 100.;
            s.tone.curves.parametric.shadows = sign * 100.;
            s.tone.curves.parametric.darks = -sign * 100.;
            s.tone.curves.parametric.lights = sign * 100.;
            s.tone.curves.parametric.highlights = -sign * 100.;
            s.color.hsl.hue.red = sign * 100.;
            s.color.hsl.saturation.blue = sign * 100.;
            s.color.hsl.luminance.green = sign * 100.;
            s.color.grading.shadows.saturation = 100.;
            s.color.grading.highlights.saturation = 100.;
            s.color.grading.highlights.hue = 270.;
            s.color.grading.balance = sign * 100.;
            s.color.grading.blending = if sign > 0. { 100. } else { 0. };
            s.detail.sharpening.amount = 150.;
            s.detail.sharpening.radius = 3.;
            s.detail.sharpening.detail = 100.;
            s.detail.noise_reduction.color = 100.;
            s.detail.noise_reduction.color_smoothness = 100.;
            s.detail.noise_reduction.luminance = 100.;
            s.effects.grain.amount = 100.;
            s.effects.vignette.amount = sign * 100.;
            s.geometry.crop.angle = sign * 45.;
            s.geometry.crop.rect.right = 0.9;
            let e = Renderer::output_extent(&image, &s, 3).unwrap();
            let tiles = r
                .render_region_as(
                    &image,
                    &s,
                    3,
                    PixelRect::full(image.level_extent(3)),
                    RenderOutput::SceneLinear,
                )
                .unwrap();
            assert_eq!(
                tiles.iter().map(|t| t.layout().extent.area()).sum::<u64>(),
                e.area()
            );
            assert!(
                tiles
                    .iter()
                    .all(|t| t.samples::<f32>().unwrap().iter().all(|v| v.is_finite())),
                "{}",
                path.display()
            );
            eprintln!(
                "M2 finite: {} L3 {}x{} sign={sign}",
                path.display(),
                e.width,
                e.height
            );
        }
    }
}

/// `cargo test -p image-core --release -- --ignored --nocapture bench`
#[test]
#[ignore]
fn bench_tone_only_change_at_level_2() {
    for path in selected("bench_tone_only_change_at_level_2") {
        let t = Instant::now();
        let image = RawImage::open(ImageId(7), &path).unwrap();
        let decode_ms = t.elapsed().as_secs_f64() * 1e3;
        let ops = Arc::new(CountingStageOp::new(CpuStageOp));
        let config = RendererConfig::default();
        let threads = config.threads;
        let r = Renderer::with_ops(
            ops.clone(),
            Arc::new(TileCache::new(config.cache_budget_bytes)),
            config,
        );
        let mut s = DevelopSettings::default();
        let e2 = image.level_extent(2);
        let full2 = PixelRect::full(e2);
        let t = Instant::now();
        let tiles = r.render_region(&image, &s, 2, full2).unwrap();
        let cold_ms = t.elapsed().as_secs_f64() * 1e3;

        let mut times = Vec::new();
        for i in 0..10 {
            s.tone.exposure = 0.1 * (i + 1) as f32;
            s.tone.contrast = 5.0 * i as f32;
            ops.reset();
            let t = Instant::now();
            r.render_region(&image, &s, 2, full2).unwrap();
            times.push(t.elapsed().as_secs_f64() * 1e3);
            assert_eq!(ops.count(StageId::Demosaic), 0);
            assert_eq!(ops.count(StageId::WhiteBalance), 0);
        }
        times.sort_by(f64::total_cmp);
        // A typical 1:1-at-L2 viewport: 2x2 output tiles (512x512 px).
        let view = PixelRect::new(e2.width / 2 - 256, e2.height / 2 - 256, 512, 512);
        let mut vtimes = Vec::new();
        for i in 0..10 {
            s.tone.shadows = 3.0 * i as f32;
            let t = Instant::now();
            r.render_region(&image, &s, 2, view).unwrap();
            vtimes.push(t.elapsed().as_secs_f64() * 1e3);
        }
        vtimes.sort_by(f64::total_cmp);
        let stats = r.cache().stats();
        eprintln!(
            "{}: sensor {:?}, L2 {}x{} ({} tiles), {threads} threads\n  decode {decode_ms:.0} ms, cold L2 {cold_ms:.0} ms\n  tone-only full L2: median {:.1} ms (min {:.1}, max {:.1})\n  tone-only 512x512 L2 viewport: median {:.1} ms (min {:.1})\n  cache {} MiB resident, peak {} MiB, hits {} misses {}",
            path.display(),
            image.sensor_extent(),
            e2.width,
            e2.height,
            tiles.len(),
            times[times.len() / 2],
            times[0],
            times[times.len() - 1],
            vtimes[vtimes.len() / 2],
            vtimes[0],
            r.cache().bytes() >> 20,
            stats.peak_bytes >> 20,
            stats.hits,
            stats.misses,
        );
    }
}
