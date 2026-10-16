//! Isolated allocator test: no other tests allocate concurrently in this binary.
#[path = "../../import-lrcat/tests/common/mod.rs"]
mod common;
use std::time::{Duration, Instant};

/// Peak heap above the starting level while `f` runs (the engine allocator
/// counts; see `tessera_ffi::alloc`), and its wall time.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, Duration) {
    let start = Instant::now();
    let (out, peak) = tessera_ffi::alloc::peak_during(f);
    (out, peak, start.elapsed())
}

use tessera_ffi::{Engine, inspect_lrcat};
const MB: usize = 1 << 20;

#[test]
#[cfg_attr(debug_assertions, ignore = "release memory and time gate")]
fn ffi_streaming_memory_is_bounded() {
    let temp = tempfile::tempdir().unwrap();
    let small = common::write(&temp.path().join("small"), 2_000);
    let large = common::write(&temp.path().join("large"), 20_000);
    let (_, small_peak, _) =
        measure(|| inspect_lrcat(small.to_string_lossy().into_owned()).unwrap());
    let (summary, peak, elapsed) =
        measure(|| inspect_lrcat(large.to_string_lossy().into_owned()).unwrap());
    assert_eq!(summary.images, 20_000);
    assert_eq!(summary.virtual_copies, 400);
    eprintln!(
        "FFI inspect: small={} large={} seconds={:.3}",
        small_peak,
        peak,
        elapsed.as_secs_f64()
    );
    assert!(
        peak < small_peak * 3 / 2 + 32 * MB,
        "FFI inspect grows with full recipes: {small_peak} -> {peak}"
    );
    assert!(peak < 256 * MB, "inspect peak {peak}");
    assert!(elapsed < Duration::from_secs(120));
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let ((import, report), apply_peak, _) = measure(|| {
        let import = engine
            .open_lrcat(large.to_string_lossy().into_owned())
            .unwrap();
        assert_eq!(import.summary(), summary);
        let mut options = import.default_options().unwrap();
        options.library_folder = temp.path().join("library").to_string_lossy().into_owned();
        for r in &mut options.relocations {
            r.to = temp.path().join("absent").to_string_lossy().into_owned();
        }
        let report = import.apply(options, None).unwrap();
        (import, report)
    });
    eprintln!("FFI open + apply peak={apply_peak}");
    assert!(apply_peak < 256 * MB, "open/apply peak {apply_peak}");
    assert_eq!(report.virtual_copies, 400);
    assert_eq!(report.skipped.len(), 19_600);
    drop(import);
}
