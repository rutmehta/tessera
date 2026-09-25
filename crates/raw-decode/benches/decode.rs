use criterion::{Criterion, criterion_group, criterion_main};
use raw_decode::RawSource;
use std::{hint::black_box, path::Path};

fn benchmark_fixtures(c: &mut Criterion) {
    let root = Path::new("fixtures/raw");
    if !root.exists() {
        eprintln!("skipping RAW benchmarks: fixtures/raw is absent");
        return;
    }
    for entry in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        c.bench_function(&format!("decode {}", path.display()), |b| {
            b.iter(|| {
                let mut source = RawSource::open(&path).expect("open RAW fixture");
                black_box(source.decode_cfa().expect("decode RAW fixture"));
            });
        });
    }
}

criterion_group!(benches, benchmark_fixtures);
criterion_main!(benches);
