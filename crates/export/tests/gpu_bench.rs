//! Opt-in, isolated-process CPU/GPU comparison. Never mutates the test runner's
//! environment (other tests may render concurrently).
use std::{path::PathBuf, process::Command, time::Instant};

#[test]
#[ignore = "five real RAW fixtures; prints decode + render + encode wall time"]
fn five_fixture_export_benchmark() {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    for name in [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ] {
        if std::env::var("TESSERA_BENCH_ONLY").is_ok_and(|only| !name.contains(&only)) {
            continue;
        }
        let path = root.join(name);
        assert!(
            path.is_file(),
            "missing benchmark fixture {}",
            path.display()
        );
        let backends = std::env::var("TESSERA_BENCH_BACKENDS").unwrap_or_else(|_| "cpu,gpu".into());
        for preset in ["web", "full"] {
            for backend in backends.split(',') {
                let output = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--ignored",
                        "--exact",
                        "fixture_export_worker",
                        "--nocapture",
                    ])
                    .env("TESSERA_BENCH_FILE", &path)
                    .env("TESSERA_BENCH_PRESET", preset)
                    .env("TESSERA_EXPORT_BACKEND", backend)
                    .output()
                    .unwrap();
                print!("{}", String::from_utf8_lossy(&output.stdout));
                for line in String::from_utf8_lossy(&output.stderr).lines() {
                    if line.starts_with("EXPORT_TRACE") {
                        println!("  {line}");
                    }
                }
                assert!(
                    output.status.success(),
                    "{}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
        }
    }
}

#[test]
#[ignore = "subprocess worker of five_fixture_export_benchmark"]
fn fixture_export_worker() {
    let Some(path) = std::env::var_os("TESSERA_BENCH_FILE") else {
        return;
    };
    let path = PathBuf::from(path);
    let web = std::env::var("TESSERA_BENCH_PRESET").unwrap() == "web";
    let dir = tempfile::tempdir().unwrap();
    let start = Instant::now();
    let mut raw = raw_decode::RawSource::open(&path).unwrap();
    let cfa = raw.decode_cfa().unwrap();
    let metadata = raw.metadata();
    let mut scale = 1;
    let [_, _, w, h] = metadata.default_crop;
    if web {
        while scale < 8 && w.max(h) / (scale * 2) >= 2048 {
            scale *= 2;
        }
    }
    let mut recipe = engine_api::recipe::Recipe::default();
    let lens_off = std::env::var_os("TESSERA_BENCH_LENS_OFF").is_some();
    if lens_off {
        recipe
            .edit(
                engine_api::recipe::EditMeta::user("benchmark lens disabled", 0),
                |s| {
                    s.lens.profile = engine_api::recipe::settings::LensProfileSource::None;
                    s.lens.remove_chromatic_aberration = false;
                },
            )
            .unwrap();
    }
    let cancel = engine_api::jobs::CancellationToken::new();
    let rendered = export::render_one_cancellable(
        &export::ExportImage {
            source: pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            name: "bench",
            sequence: 1,
            date: "",
            metadata: None,
        },
        &recipe,
        &export::ExportSettings {
            output_dir: dir.path().into(),
            format: export::Format::Jpeg {
                quality: if web { 85 } else { 90 },
            },
            resize: if web {
                export::Resize::LongEdge(2048)
            } else {
                export::Resize::None
            },
            sharpen_for: if web {
                export::SharpenFor::Screen
            } else {
                export::SharpenFor::None
            },
            render_scale: scale,
            apply_orientation: true,
            ..Default::default()
        },
        &cancel,
        None,
        None,
    )
    .unwrap();
    let used_gpu = rendered.used_gpu();

    let output = rendered.finish(&cancel).unwrap();
    let elapsed = start.elapsed().as_secs_f64();
    assert!(std::fs::metadata(output).unwrap().len() > 100);
    println!(
        "BENCH file={} preset={} requested={} used_gpu={used_gpu} lens_off={lens_off} seconds={elapsed:.3}",
        path.file_name().unwrap().to_string_lossy(),
        if web { "web" } else { "full" },
        std::env::var("TESSERA_EXPORT_BACKEND").unwrap()
    );
}

/// docs/08 "Export 100 JPEGs < 40 s": 100 Web-preset exports (20 of each of
/// the five fixtures) through the pipelined batch (one render overlapping one
/// encode). Sources are decoded once per fixture; decode time is reported
/// separately so the full per-image cost can be projected.
#[test]
#[ignore = "five real RAW fixtures; 100 Web-preset JPEG exports"]
fn hundred_web_exports() {
    let root = std::env::var_os("PIPELINE_RAW_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    let recipe = engine_api::recipe::Recipe::default();
    let cancel = engine_api::jobs::CancellationToken::new();
    let dir = tempfile::tempdir().unwrap();
    let mut decode = 0.0;
    let mut render = 0.0;
    let mut used = 0;
    for name in [
        "canon-cr3.CR3",
        "sony-arw.ARW",
        "nikon-nef.NEF",
        "fuji-raf.RAF",
        "sample.dng",
    ] {
        let start = Instant::now();
        let mut raw = raw_decode::RawSource::open(root.join(name)).unwrap();
        let cfa = raw.decode_cfa().unwrap();
        let metadata = raw.metadata();
        decode += start.elapsed().as_secs_f64();
        let [_, _, w, h] = metadata.default_crop;
        let mut scale = 1;
        while scale < 8 && w.max(h) / (scale * 2) >= 2048 {
            scale *= 2;
        }
        let names: Vec<String> = (0..20).map(|i| format!("{name}-{i}")).collect();
        let items: Vec<_> = names
            .iter()
            .enumerate()
            .map(|(i, n)| export::ExportItem {
                image: export::ExportImage {
                    source: pipeline_cpu::RenderSource::Cfa {
                        image: &cfa,
                        metadata: &metadata,
                    },
                    name: n,
                    sequence: i + 1,
                    date: "",
                    metadata: None,
                },
                recipe: &recipe,
            })
            .collect();
        let settings = export::ExportSettings {
            output_dir: dir.path().into(),
            format: export::Format::Jpeg { quality: 85 },
            resize: export::Resize::LongEdge(2048),
            sharpen_for: export::SharpenFor::Screen,
            render_scale: scale,
            apply_orientation: true,
            ..Default::default()
        };
        let start = Instant::now();
        let report = export::export_batch_with_jobs(&items, &settings, |_| {}, &cancel, 2).unwrap();
        render += start.elapsed().as_secs_f64();
        used += report.results.iter().filter(|r| r.is_ok()).count();
    }
    assert_eq!(used, 100);
    println!(
        "BENCH100 web: 100 exports (render + encode, pipelined) {render:.2} s; \
         decode {:.3} s/image avg => projected with serial decode {:.1} s",
        decode / 5.0,
        render + decode / 5.0 * 100.0
    );
}
