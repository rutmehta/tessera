//! Interactive (resident renderer) per-operator frame times on every real RAW
//! fixture. Never runs in the normal test gate.
//!
//! cargo test -p pipeline-gpu --release --test interactive_performance -- \
//!   --ignored --nocapture --test-threads=1
//!
//! Each operator is dragged through `INTERACTIVE_BENCH_SAMPLES` values (default
//! 7 at L2, 2 at L0) after one untimed frame that warms the upstream WB cache.
//! A frame is `Renderer::render_surface` into an RGBA8 IOSurface (resident path,
//! 4 KiB histogram readback) exactly as Develop presents it. When the recipe is
//! not resident-capable the bench times the tile fallback (`render_region`)
//! instead and reports `path=tiles`: that is the path Develop takes as well.
//! `INTERACTIVE_BENCH_LEVELS` (default `2,0`) and `INTERACTIVE_BENCH_OPS`
//! (comma-separated names) narrow the run; `INTERACTIVE_BENCH_EXT` picks
//! fixtures by extension; `INTERACTIVE_BENCH_PREVIEW=1` enables the opt-in
//! preview-level approximations (off by default, as in `RendererConfig`);
//! `INTERACTIVE_BENCH_STATS=1` prints `GpuStats` after every frame.
#![cfg(target_os = "macos")]

use engine_api::{
    id::ImageId,
    jobs::CancellationToken,
    recipe::{DevelopSettings, settings::CurvePoint},
};
use image_core::{PixelRect, RawImage, Renderer, RendererConfig, TileCache};
use objc2_core_foundation::{CFDictionary, CFNumber, CFRetained, CFType};
use objc2_io_surface::{
    IOSurfaceRef, kIOSurfaceBytesPerElement, kIOSurfaceHeight, kIOSurfacePixelFormat,
    kIOSurfaceWidth,
};
use pipeline_gpu::{GpuContext, GpuStageOp};
use std::{path::PathBuf, sync::Arc, time::Instant};

fn surface(w: u32, h: u32) -> CFRetained<IOSurfaceRef> {
    let numbers = [
        CFNumber::new_i64(i64::from(w)),
        CFNumber::new_i64(i64::from(h)),
        CFNumber::new_i64(4),
        CFNumber::new_i64(i64::from(u32::from_be_bytes(*b"RGBA"))),
    ];
    // SAFETY: immutable CFString keys and CFNumber values; Copy-rule surface.
    unsafe {
        let keys: [&CFType; 4] = [
            kIOSurfaceWidth.as_ref(),
            kIOSurfaceHeight.as_ref(),
            kIOSurfaceBytesPerElement.as_ref(),
            kIOSurfacePixelFormat.as_ref(),
        ];
        let values: Vec<&CFType> = numbers.iter().map(|n| (**n).as_ref()).collect();
        let dict = CFDictionary::from_slices(&keys, &values);
        IOSurfaceRef::new(dict.as_opaque()).unwrap()
    }
}

fn fixtures() -> Vec<PathBuf> {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    let only = std::env::var("INTERACTIVE_BENCH_EXT").ok();
    let mut paths: Vec<_> = std::fs::read_dir(&root)
        .map(|d| d.map(|e| e.unwrap().path()).collect())
        .unwrap_or_default();
    paths.retain(|p| {
        p.is_file()
            && p.extension().is_some_and(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                ["nef", "cr3", "raf", "arw", "dng"].contains(&e.as_str())
                    && only.as_ref().is_none_or(|o| o.eq_ignore_ascii_case(&e))
            })
    });
    paths.sort();
    paths
}

type Patch = fn(&mut DevelopSettings, f32);

/// Operator name and a slider patch for value `v` in (0, 1].
pub fn operators() -> Vec<(&'static str, Patch)> {
    vec![
        ("basic_tone", |s, v| s.tone.exposure = v - 0.5),
        ("curves", |s, v| s.tone.curves.parametric.darks = 60. * v),
        ("point_curve", |s, v| {
            s.tone.curves.rgb.0 = vec![
                CurvePoint { x: 0., y: 0. },
                CurvePoint {
                    x: 0.5,
                    y: 0.5 + 0.2 * v,
                },
                CurvePoint { x: 1., y: 1. },
            ]
        }),
        ("texture", |s, v| s.tone.texture = 100. * v - 20.),
        ("clarity", |s, v| s.tone.clarity = 100. * v - 20.),
        ("dehaze", |s, v| s.tone.dehaze = 60. * v),
        ("sharpening", |s, v| {
            s.detail.sharpening.amount = 40. + 60. * v
        }),
        ("luminance_nr", |s, v| {
            s.detail.noise_reduction.luminance = 10. + 60. * v
        }),
        ("chroma_nr", |s, v| {
            s.detail.noise_reduction.color = 10. + 60. * v
        }),
        ("vibrance", |s, v| s.color.vibrance = 60. * v),
        ("hsl", |s, v| s.color.hsl.hue.orange = 60. * v),
        ("color_grading", |s, v| {
            s.color.grading.shadows.hue = 220.;
            s.color.grading.shadows.saturation = 40. * v;
        }),
        ("vignette", |s, v| s.effects.vignette.amount = -60. * v),
        ("grain", |s, v| s.effects.grain.amount = 10. + 60. * v),
    ]
}

fn percentile(sorted: &[f64], q: f64) -> f64 {
    sorted[((sorted.len() - 1) as f64 * q).round() as usize]
}

#[test]
#[ignore = "requires real RAW fixtures and Metal; prints interactive frame times"]
fn bench_interactive_per_operator() {
    let levels: Vec<u8> = std::env::var("INTERACTIVE_BENCH_LEVELS")
        .unwrap_or_else(|_| "2,0".into())
        .split(',')
        .map(|l| l.trim().parse().unwrap())
        .collect();
    let only_ops = std::env::var("INTERACTIVE_BENCH_OPS").ok();
    let label = std::env::var("INTERACTIVE_BENCH_LABEL")
        .unwrap_or_else(|_| "unlabelled-working-tree".into());
    let context = Arc::new(GpuContext::new().expect("Metal adapter required"));
    eprintln!(
        "INTERACTIVE_BENCH_RUN label={label:?} adapter={:?}",
        context.adapter_info.name
    );
    let gpu = Arc::new(GpuStageOp::new(context));
    let paths = fixtures();
    assert!(!paths.is_empty(), "no RAW fixtures found");
    let cancel = CancellationToken::new();
    for path in paths {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let image = match RawImage::open(ImageId(4242), &path) {
            Ok(image) => image,
            Err(e) => {
                eprintln!("INTERACTIVE_BENCH_SKIP fixture={name} error={e}");
                continue;
            }
        };
        for &level in &levels {
            let config = RendererConfig {
                preview_approximations: std::env::var_os("INTERACTIVE_BENCH_PREVIEW").is_some(),
                ..RendererConfig::default()
            };
            gpu.clear_cache();
            let renderer = Renderer::with_ops(
                gpu.clone(),
                Arc::new(TileCache::new(config.cache_budget_bytes)),
                config,
            );
            let e = image.level_extent(level);
            let target = surface(e.width, e.height);
            let samples: usize = std::env::var("INTERACTIVE_BENCH_SAMPLES")
                .map(|s| s.parse().unwrap())
                .unwrap_or(if level == 0 { 2 } else { 7 });
            let frame = |s: &DevelopSettings| -> (f64, &'static str) {
                let start = Instant::now();
                let path = match renderer
                    .render_surface(&image, s, level, target.id(), &cancel)
                    .unwrap()
                {
                    Some(h) => {
                        std::hint::black_box(h);
                        "surface"
                    }
                    None => {
                        let tiles = renderer
                            .render_region(&image, s, level, PixelRect::full(e))
                            .unwrap();
                        std::hint::black_box(tiles);
                        "tiles"
                    }
                };
                let ms = start.elapsed().as_secs_f64() * 1e3;
                if std::env::var_os("INTERACTIVE_BENCH_STATS").is_some() {
                    eprintln!("INTERACTIVE_BENCH_STATS {ms:.2} ms {:?}", gpu.stats());
                }
                (ms, path)
            };
            // Warm upstream caches (decode, demosaic, WB) exactly once.
            let (cold, _) = frame(&DevelopSettings::default());
            eprintln!(
                "INTERACTIVE_BENCH_LEVEL fixture={name} level={level} extent={}x{} cold_ms={cold:.1}",
                e.width, e.height
            );
            for (op, patch) in operators() {
                if only_ops
                    .as_ref()
                    .is_some_and(|o| !o.split(',').any(|n| n == op))
                {
                    continue;
                }
                let mut times = Vec::new();
                let mut path_used = "";
                // One untimed frame enters the operator's steady state.
                for i in 0..=samples {
                    let mut s = DevelopSettings::default();
                    patch(&mut s, (i + 1) as f32 / (samples + 1) as f32);
                    let (ms, path) = frame(&s);
                    path_used = path;
                    if i > 0 {
                        times.push(ms);
                    }
                }
                times.sort_by(f64::total_cmp);
                eprintln!(
                    "INTERACTIVE_BENCH fixture={name} level={level} op={op} path={path_used} p50_ms={:.2} p90_ms={:.2} samples={times:.2?}",
                    percentile(&times, 0.5),
                    percentile(&times, 0.9),
                );
            }
        }
    }
}

/// M2-17b keeps L2 under a 12 ms steady-state budget and drops resolution
/// only above 16 ms. M2-22 retains the 12 ms budget. Use the real 36 MP NEF
/// (approximately 1845x1231 at L2), never the small synthetic parity fixture.
///
/// cargo test -p pipeline-gpu --release --test interactive_performance \
///   bench_l2_composed_geometry_budget -- --ignored --nocapture --test-threads=1
#[test]
#[ignore = "requires a real >=30 MP NEF, Metal and release build; asserts geometry drag budget"]
// This ignored benchmark compiles in debug but its runtime contract requires release.
#[allow(clippy::assertions_on_constants)]
fn bench_l2_composed_geometry_budget() {
    use engine_api::recipe::settings::{GuideLine, LensProfileSource, NormalizedRect, UprightMode};
    assert!(!cfg!(debug_assertions), "latency gate requires --release");
    let path = std::env::var_os("PIPELINE_BENCH_NEF")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            fixtures().into_iter().find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("nef")))
                .expect("real NEF required in PIPELINE_RAW_FIXTURES or fixtures/raw; no synthetic fallback")
        });
    let raw = RawImage::open(ImageId(249), &path).expect("decode real NEF");
    assert!(
        raw.active_extent().area() >= 30_000_000,
        "use the full-resolution >=30 MP NEF fixture"
    );
    let context = Arc::new(GpuContext::new().expect("Metal adapter required"));
    eprintln!(
        "GEOMETRY_BENCH fixture={} adapter={:?} sensor={}x{} level=2",
        path.display(),
        context.adapter_info.name,
        raw.active_extent().width,
        raw.active_extent().height
    );
    let gpu = Arc::new(GpuStageOp::new(context));
    let config = RendererConfig::default();
    let renderer = Renderer::with_ops(
        gpu.clone(),
        Arc::new(TileCache::new(config.cache_budget_bytes)),
        config,
    );
    let cancel = CancellationToken::new();
    let mut settings = DevelopSettings::default();
    settings.lens.profile = LensProfileSource::None;
    settings.lens.remove_chromatic_aberration = false;
    settings.lens.manual_distortion = 18.;
    settings.lens.manual_vignetting = 35.;
    settings.geometry.crop.rect = NormalizedRect {
        left: 0.05,
        top: 0.05,
        right: 0.95,
        bottom: 0.95,
    };
    let extent = Renderer::output_extent(&raw, &settings, 2).unwrap();
    let target = surface(extent.width, extent.height);
    let samples: usize = std::env::var("INTERACTIVE_BENCH_SAMPLES")
        .map(|v| v.parse().unwrap())
        .unwrap_or(40);
    assert!(
        samples >= 30,
        "at least 30 measured frames required for p90"
    );
    let patches: [(&str, Patch); 4] = [
        ("vertical", |s, v| {
            s.geometry.transform.vertical = -20. + 40. * v
        }),
        ("horizontal", |s, v| {
            s.geometry.transform.horizontal = -20. + 40. * v
        }),
        ("rotate", |s, v| s.geometry.transform.rotate = -4. + 8. * v),
        ("scale", |s, v| s.geometry.transform.scale = 95. + 10. * v),
    ];
    let mut latency_failures = Vec::new();
    for mode in [UprightMode::Guided, UprightMode::Auto] {
        settings.geometry.upright.mode = mode;
        settings.geometry.upright.guides = if mode == UprightMode::Guided {
            vec![
                GuideLine {
                    start: [0.2, 0.1],
                    end: [0.3, 0.9],
                },
                GuideLine {
                    start: [0.8, 0.1],
                    end: [0.7, 0.9],
                },
            ]
        } else {
            Vec::new()
        };
        let cold = Instant::now();
        assert!(
            renderer
                .render_surface(&raw, &settings, 2, target.id(), &cancel)
                .unwrap()
                .is_some(),
            "composed geometry must present resident; tile fallback is not this benchmark"
        );
        eprintln!(
            "GEOMETRY_BENCH_COLD mode={mode:?} extent={}x{} ms={:.2}",
            extent.width,
            extent.height,
            cold.elapsed().as_secs_f64() * 1e3
        );
        for (name, patch) in patches {
            let mut times = Vec::with_capacity(samples);
            let mut steady = None;
            for i in 0..samples + 3 {
                let mut frame_settings = settings.clone();
                patch(
                    &mut frame_settings,
                    (i % samples) as f32 / (samples - 1) as f32,
                );
                let start = Instant::now();
                let histogram = renderer
                    .render_surface(&raw, &frame_settings, 2, target.id(), &cancel)
                    .unwrap();
                let elapsed = start.elapsed().as_secs_f64() * 1e3;
                assert!(
                    histogram.is_some(),
                    "geometry surface path disappeared during drag"
                );
                std::hint::black_box(histogram);
                if i == 2 {
                    steady = Some(gpu.stats());
                }
                if i >= 3 {
                    times.push(elapsed);
                }
            }
            let before = steady.unwrap();
            let after = gpu.stats();
            assert_eq!(
                after.uploads, before.uploads,
                "warm transform re-uploaded upstream pixels"
            );
            assert_eq!(
                after.pixel_readback_bytes, before.pixel_readback_bytes,
                "warm transform read back full pixels"
            );
            times.sort_by(f64::total_cmp);
            let p50 = percentile(&times, 0.5);
            let p90 = percentile(&times, 0.9);
            let max = *times.last().unwrap();
            eprintln!(
                "GEOMETRY_BENCH mode={mode:?} op={name} path=surface level=2 extent={}x{} p50_ms={p50:.2} p90_ms={p90:.2} max_ms={max:.2} budget_ms=12 drop_threshold_ms=16 samples={times:.2?}",
                extent.width, extent.height
            );
            if p90 >= 12. {
                latency_failures.push(format!(
                    "{mode:?}/{name}: L2 p90 {p90:.2} ms exceeds the existing 12 ms budget"
                ));
            }
            if max >= 16. {
                latency_failures.push(format!(
                    "{mode:?}/{name}: frame {max:.2} ms crosses the 16 ms adaptive downgrade threshold"
                ));
            }
        }
    }
    assert!(
        latency_failures.is_empty(),
        "geometry drag latency failures:\n{}",
        latency_failures.join("\n")
    );
}
