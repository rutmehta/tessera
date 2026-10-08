//! ENG-15 timing harness for GPU Web exports (REV-ENG-14 SHOULD-FIX 1).
//! Opt-in and public-API only, so the same file measures older commits too:
//!
//! `TESSERA_BENCH_FILE=<raw> [TESSERA_EXPORT_WEB_LEVEL=1] [TESSERA_EXPORT_TRACE=1]
//!  cargo test --release -p export --test web_export_timing -- --ignored --nocapture`
//!
//! One warm-up export, then `TESSERA_BENCH_RUNS` (default 8) timed Web
//! exports (long edge 2048, render only: no encode). Prints each time and
//! the median. `TESSERA_EXPORT_WEB_LEVEL=1` selects the pyramid level.
use std::{path::PathBuf, time::Instant};

#[test]
#[ignore = "real RAW fixture timing; run alone"]
fn web_export_timing() {
    let Some(path) = std::env::var_os("TESSERA_BENCH_FILE").map(PathBuf::from) else {
        eprintln!("SKIPPED: set TESSERA_BENCH_FILE");
        return;
    };
    let runs: usize = std::env::var("TESSERA_BENCH_RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let mut raw = raw_decode::RawSource::open(&path).unwrap();
    let cfa = raw.decode_cfa().unwrap();
    let metadata = raw.metadata();
    let recipe = engine_api::recipe::Recipe::default();
    let cancel = engine_api::jobs::CancellationToken::new();
    let dir = tempfile::tempdir().unwrap();
    let settings = export::ExportSettings {
        output_dir: dir.path().into(),
        format: export::Format::Jpeg { quality: 85 },
        resize: export::Resize::LongEdge(2048),
        sharpen_for: export::SharpenFor::None,
        ..Default::default()
    };
    let mut times = Vec::new();
    for run in 0..=runs {
        let started = Instant::now();
        let rendered = export::render_one_cancellable(
            &export::ExportImage {
                source: pipeline_cpu::RenderSource::Cfa {
                    image: &cfa,
                    metadata: &metadata,
                },
                name: "timing",
                sequence: 1,
                date: "",
                metadata: None,
            },
            &recipe,
            &settings,
            &cancel,
            None,
            None,
        )
        .unwrap();
        let ms = started.elapsed().as_secs_f64() * 1e3;
        assert!(rendered.used_gpu(), "the GPU export was declined");
        eprintln!("TIMING run={run} ms={ms:.1}");
        if run > 0 {
            times.push(ms);
        }
    }
    times.sort_by(f64::total_cmp);
    let median = (times[(runs - 1) / 2] + times[runs / 2]) / 2.;
    println!(
        "TIMING file={} web_level={} median_ms={median:.1} runs={times:.1?}",
        path.file_name().unwrap().to_string_lossy(),
        std::env::var("TESSERA_EXPORT_WEB_LEVEL").as_deref() == Ok("1"),
    );
}
