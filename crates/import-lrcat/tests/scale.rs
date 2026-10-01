//! B5-29c scale: on a synthetic catalog generated in-test, streaming import
//! keeps peak heap flat as the catalog grows (bounded by a batch of images,
//! not the catalog) and finishes 20k images in bounded time. The test binary
//! counts every Rust heap allocation with its own global allocator; SQLite's
//! own page cache is outside it. Single test so nothing else allocates
//! concurrently.
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

const MB: usize = 1 << 20;

#[test]
#[cfg_attr(
    debug_assertions,
    ignore = "timing and allocation bounds are for release builds"
)]
fn streaming_import_memory_is_flat_and_time_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let small = common::write(&dir.path().join("small"), 2_000);
    let large = common::write(&dir.path().join("large"), 20_000);

    let (summary, small_peak, _) = measure(|| import_lrcat::inspect(&small).unwrap());
    assert_eq!(summary.images, 2_000);
    let (summary, inspect_peak, inspect_time) = measure(|| import_lrcat::inspect(&large).unwrap());
    assert_eq!(summary.images, 20_000);
    assert_eq!(summary.virtual_copies, 400);

    // The streamed import with a consumer that serializes and drops each image
    // (what `tessera import lrcat --apply` does).
    let (count, apply_peak, apply_time) = measure(|| {
        let mut sink = std::io::sink();
        let json = std::cell::RefCell::new(None);
        let mut images = 0;
        let plan = import_lrcat::import_each(
            &large,
            |plan| {
                *json.borrow_mut() = Some(import_lrcat::PlanJson::begin(&mut sink, plan)?);
                Ok(())
            },
            |image| {
                images += 1;
                serde_json::to_vec_pretty(&image.recipe)?;
                json.borrow_mut().as_mut().unwrap().image(&image)
            },
        )
        .unwrap();
        json.into_inner().unwrap().finish(&plan).unwrap();
        images
    });
    assert_eq!(count, 20_000);
    eprintln!(
        "peak heap: inspect 2k {} MB, inspect 20k {} MB ({inspect_time:?}), stream 20k {} MB ({apply_time:?})",
        small_peak / MB,
        inspect_peak / MB,
        apply_peak / MB
    );
    // 10x the images must not mean more memory: bounded by the batch.
    assert!(
        inspect_peak < small_peak * 3 / 2 + 16 * MB,
        "inspect peak grew with the catalog: 2k {small_peak} B, 20k {inspect_peak} B"
    );
    assert!(inspect_peak < 256 * MB, "inspect peak {inspect_peak} B");
    assert!(apply_peak < 256 * MB, "streamed import peak {apply_peak} B");
    // Loose: the B5-29b importer was quadratic (minutes at this size).
    assert!(inspect_time < Duration::from_secs(120), "{inspect_time:?}");
    assert!(apply_time < Duration::from_secs(180), "{apply_time:?}");
}
