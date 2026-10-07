//! ENG-10: opt-in release benchmark for Adobe-process (imported Lightroom)
//! exports and prints of real RAW originals.
//!
//! Every case runs in its own subprocess so that the reported peak resident
//! set size (`getrusage`, monotonic per process) belongs to that case alone.
//! The runner's environment is never mutated (other tests may run at once).
//!
//! ```text
//! cargo test --release -p export --test eng10_adobe_bench -- --ignored \
//!     --exact eng10_adobe_export_benchmark --nocapture
//! ```
//!
//! Knobs: `TESSERA_BENCH_FILE` (default `fixtures/raw/sony-arw.ARW`),
//! `ENG10_CASES` (comma list, default all), `ENG10_PROCESS=native` (compare
//! the same recipe in the current Native process), `TESSERA_EXPORT_BACKEND`
//! (inherited by the workers).
use engine_api::{
    jobs::CancellationToken,
    recipe::{EditMeta, ProcessVersion, Recipe},
    tile::Pyramid as _,
};
use std::{path::PathBuf, process::Command, time::Instant};

const CASES: [&str; 5] = [
    "export-s1",
    "export-s4",
    "print-8x10",
    "print-4x6",
    "batch20",
];

fn fixture() -> PathBuf {
    std::env::var_os("TESSERA_BENCH_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW")
        })
}

#[test]
#[ignore = "real RAW fixture; prints wall time and peak RSS per Adobe export case"]
fn eng10_adobe_export_benchmark() {
    let path = fixture();
    assert!(
        path.is_file(),
        "missing benchmark fixture {}",
        path.display()
    );
    let cases = std::env::var("ENG10_CASES").unwrap_or_else(|_| CASES.join(","));
    for case in cases.split(',') {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--ignored",
                "--exact",
                "eng10_adobe_bench_worker",
                "--nocapture",
            ])
            .env("ENG10_CASE", case)
            .output()
            .unwrap();
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            if line.starts_with("ENG10") {
                println!("{line}");
            }
        }
        assert!(
            output.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// Peak resident set size of this process in MiB (macOS reports bytes,
/// Linux kilobytes).
fn peak_rss_mib() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
    // SAFETY: getrusage writes a plain C struct into valid storage.
    let usage = unsafe {
        assert_eq!(libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()), 0);
        usage.assume_init()
    };
    let raw = usage.ru_maxrss as f64;
    if cfg!(target_os = "macos") {
        raw / (1024. * 1024.)
    } else {
        raw / 1024.
    }
}

/// A typical imported Lightroom edit: basic tone, presence and sharpening.
fn recipe() -> Recipe {
    let process = if std::env::var("ENG10_PROCESS").as_deref() == Ok("native") {
        ProcessVersion::NATIVE_CURRENT
    } else {
        ProcessVersion::adobe(6)
    };
    let mut recipe = Recipe {
        process_version: process,
        ..Default::default()
    };
    recipe
        .edit(EditMeta::user("eng10 benchmark", 1), |s| {
            s.tone.exposure = 0.3;
            s.tone.contrast = 20.;
            s.tone.highlights = -40.;
            s.tone.shadows = 30.;
            s.tone.whites = 10.;
            s.tone.blacks = -5.;
            s.color.vibrance = 15.;
            s.color.saturation = 5.;
        })
        .unwrap();
    recipe
}

#[test]
#[ignore = "subprocess worker of eng10_adobe_export_benchmark"]
fn eng10_adobe_bench_worker() {
    let Ok(case) = std::env::var("ENG10_CASE") else {
        return;
    };
    let path = fixture();
    let started = Instant::now();
    let mut raw = raw_decode::RawSource::open(&path).unwrap();
    let cfa = raw.decode_cfa().unwrap();
    let metadata = raw.metadata();
    let decode = started.elapsed().as_secs_f64();
    let decoded_rss = peak_rss_mib();
    let recipe = recipe();
    let cancel = CancellationToken::new();
    let dir = tempfile::tempdir().unwrap();
    let image = |name: &'static str, sequence| export::ExportImage {
        source: pipeline_cpu::RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        },
        name,
        sequence,
        date: "",
        metadata: None,
    };
    let [_, _, w, h] = metadata.default_crop;
    let started = Instant::now();
    let (width, height, renders) = match case.as_str() {
        "export-s1" | "export-s4" => {
            let scale = if case == "export-s1" { 1 } else { 4 };
            let rendered = export::render_one_cancellable(
                &image("bench", 1),
                &recipe,
                &export::ExportSettings {
                    output_dir: dir.path().into(),
                    format: export::Format::Jpeg { quality: 90 },
                    render_scale: scale,
                    apply_orientation: true,
                    ..Default::default()
                },
                &cancel,
                None,
                None,
            )
            .unwrap();
            let out = rendered.finish(&cancel).unwrap();
            let decoded = image::image_dimensions(out).unwrap();
            (decoded.0, decoded.1, 1)
        }
        // 300 dpi boxes; the render scale is the FFI's `print_scale` binning
        // (largest power of two that still covers the box).
        "print-8x10" | "print-4x6" => {
            let (bw, bh) = if case == "print-8x10" {
                (3000, 2400)
            } else {
                (1800, 1200)
            };
            let fit = (f64::from(bw) / f64::from(w)).min(f64::from(bh) / f64::from(h));
            let mut scale = 1;
            while scale < 8 && fit * f64::from(scale * 2) <= 1.0 {
                scale *= 2;
            }
            let (rgb, _) = export::render_pixels_with_notes(
                &image("print", 1),
                &recipe,
                &export::RenderRequest {
                    color_space: export::ColorSpace::DisplayP3,
                    resize: export::Resize::Fit(bw, bh),
                    sharpen_for: export::SharpenFor::Glossy,
                    scale,
                },
                &cancel,
                None,
                None,
                None,
            )
            .unwrap();
            (rgb.width(), rgb.height(), 1)
        }
        // Web preset (long edge 2048, the FFI's binning picks the scale) for
        // 20 exports of the decoded source, through the default batch path.
        "batch20" => {
            let fit = 2048. / f64::from(w.max(h));
            let mut scale = 1;
            while scale < 8 && fit * f64::from(scale * 2) <= 1.0 {
                scale *= 2;
            }
            let names: Vec<String> = (0..20).map(|i| format!("bench-{i}")).collect();
            let items: Vec<_> = names
                .iter()
                .enumerate()
                .map(|(i, name)| export::ExportItem {
                    image: export::ExportImage {
                        source: pipeline_cpu::RenderSource::Cfa {
                            image: &cfa,
                            metadata: &metadata,
                        },
                        name,
                        sequence: i + 1,
                        date: "",
                        metadata: None,
                    },
                    recipe: &recipe,
                })
                .collect();
            let report = export::export_batch(
                &items,
                &export::ExportSettings {
                    output_dir: dir.path().into(),
                    format: export::Format::Jpeg { quality: 85 },
                    resize: export::Resize::LongEdge(2048),
                    sharpen_for: export::SharpenFor::Screen,
                    render_scale: scale,
                    apply_orientation: true,
                    ..Default::default()
                },
                |_| {},
                &cancel,
            )
            .unwrap();
            assert!(report.results.iter().all(Result::is_ok));
            (2048, 0, 20)
        }
        // Develop's own renderer (what the loupe draws) on the whole frame at
        // a pyramid level, on the CPU operators or the Metal backend:
        // `develop-cpu-L0`, `develop-gpu-L2`, ...
        develop if develop.starts_with("develop-") => {
            let level: u8 = develop[develop.len() - 1..].parse().unwrap();
            let raw = image_core::RawImage::new(
                engine_api::id::ImageId(1),
                std::sync::Arc::new(
                    raw_decode::CfaImage::from_linear(
                        cfa.pyramid().extent().width,
                        cfa.pyramid().extent().height,
                        cfa.pyramid().pixels().to_vec(),
                    )
                    .unwrap(),
                ),
                std::sync::Arc::new(metadata.clone()),
            )
            .unwrap();
            let config = image_core::RendererConfig {
                process_version: recipe.process_version,
                cache_budget_bytes: 0,
                ..Default::default()
            };
            let renderer = if develop.contains("gpu") {
                image_core::Renderer::with_ops(
                    std::sync::Arc::new(pipeline_gpu::GpuStageOp::new(std::sync::Arc::new(
                        pipeline_gpu::GpuContext::new().unwrap(),
                    ))),
                    std::sync::Arc::new(image_core::TileCache::new(0)),
                    config,
                )
            } else {
                image_core::Renderer::new(config)
            };
            let extent =
                image_core::Renderer::output_extent(&raw, &recipe.settings, level).unwrap();
            let tiles = renderer
                .render_region_as(
                    &raw,
                    &recipe.settings,
                    level,
                    image_core::PixelRect::full(extent),
                    image_core::RenderOutput::SceneLinear,
                )
                .unwrap();
            assert!(!tiles.is_empty());
            (extent.width, extent.height, 1)
        }
        other => panic!("unknown ENG10_CASE {other}"),
    };
    let seconds = started.elapsed().as_secs_f64();
    println!(
        "ENG10 case={case} file={} process={} backend={} renders={renders} out={width}x{height} \
         seconds={seconds:.2} per_render={:.2} decode={decode:.2} rss_after_decode_mib={decoded_rss:.0} \
         peak_rss_mib={:.0}",
        path.file_name().unwrap().to_string_lossy(),
        format!("{:?}", recipe.process_version.family),
        std::env::var("TESSERA_EXPORT_BACKEND").unwrap_or_else(|_| "default".into()),
        seconds / f64::from(renders),
        peak_rss_mib(),
    );
}
