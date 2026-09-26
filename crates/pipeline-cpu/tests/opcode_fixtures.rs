//! Exercise staged corrections when an opcode-bearing RAW fixture is available.
use pipeline_cpu::{RenderSource, render_linear_scaled};
use raw_decode::RawSource;
use std::path::{Path, PathBuf};

#[test]
fn real_opcode_fixtures_when_available() {
    let root = std::env::var_os("RAW_DECODE_FIXTURES")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw"));
    if !root.exists() {
        eprintln!("opcode fixture coverage unavailable: fixtures/raw absent");
        return;
    }
    let mut inspected = 0;
    let mut rendered = 0;
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
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
        let mut source = RawSource::open(&path).unwrap();
        let metadata = source.metadata();
        inspected += 1;
        if !metadata.opcode_lists.iter().flatten().any(|b| b.len() > 4) {
            continue;
        }
        let cfa = source.decode_cfa().unwrap();
        let output = render_linear_scaled(
            &Default::default(),
            &RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            16,
        )
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert!(output.planes().iter().flatten().all(|v| v.is_finite()));
        rendered += 1;
    }
    eprintln!("opcode fixture coverage: inspected {inspected}, rendered {rendered}");
}
