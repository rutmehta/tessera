//! Isolated allocator test: no other tests allocate concurrently in this binary.
#[path = "../../import-lrcat/tests/common/mod.rs"]
mod common;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering::Relaxed},
    time::{Duration, Instant},
};

struct Counting;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            let now = LIVE.fetch_add(layout.size(), Relaxed) + layout.size();
            PEAK.fetch_max(now, Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size(), Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            if new_size >= layout.size() {
                let now =
                    LIVE.fetch_add(new_size - layout.size(), Relaxed) + new_size - layout.size();
                PEAK.fetch_max(now, Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Relaxed);
            }
        }
        p
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Peak heap above the starting level while `f` runs, and its wall time.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize, Duration) {
    let base = LIVE.load(Relaxed);
    PEAK.store(base, Relaxed);
    let start = Instant::now();
    let out = f();
    (out, PEAK.load(Relaxed) - base, start.elapsed())
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
