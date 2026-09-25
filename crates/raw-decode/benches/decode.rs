use criterion::{Criterion, criterion_group, criterion_main};
use raw_decode::RawSource;
use std::{hint::black_box, path::Path};

fn benchmark_fixtures(c: &mut Criterion) {
    let root = std::env::var_os("RAW_DECODE_FIXTURES")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    if !root.exists() {
        eprintln!("skipping RAW benchmarks: fixtures/raw is absent");
        return;
    }
    for entry in std::fs::read_dir(root).expect("read fixtures") {
        let path = entry.expect("fixture entry").path();
        if !path.is_file()
            || !path.extension().is_some_and(|e| {
                matches!(
                    e.to_string_lossy().to_ascii_lowercase().as_str(),
                    "cr3" | "arw" | "nef" | "raf" | "dng"
                )
            })
        {
            continue;
        }
        c.bench_function(
            &format!("decode {}", path.file_name().unwrap().to_string_lossy()),
            |b| {
                b.iter(|| {
                    let mut source = RawSource::open(&path).expect("open RAW fixture");
                    black_box(source.decode_cfa().expect("decode RAW fixture"));
                });
            },
        );
    }
}

criterion_group!(benches, benchmark_fixtures);
criterion_main!(benches);
