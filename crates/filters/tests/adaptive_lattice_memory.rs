//! B5-20b: the coarse-lattice render of a 24 MP layer allocates well below
//! what a dense solve would need. Its own test binary: the counting
//! allocator is global, so nothing else may run concurrently.
mod awa_scene;

use awa_scene::{Scene, context, node};
use compositor::{
    raster::{Depth, Raster},
    render::smart_filters::SmartFilterEvaluator,
};
use engine_api::tile::Extent;
use filters::CompositorFilters;
use std::{
    alloc::{GlobalAlloc, Layout, System},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Counting;
static CURRENT: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            let now = CURRENT.fetch_add(l.size(), Ordering::SeqCst) + l.size();
            PEAK.fetch_max(now, Ordering::SeqCst);
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        unsafe { System.dealloc(p, l) };
        CURRENT.fetch_sub(l.size(), Ordering::SeqCst);
    }
}

#[global_allocator]
static A: Counting = Counting;

#[test]
fn a_24_megapixel_render_peaks_well_below_the_dense_estimate() {
    let (w, h) = (6000u32, 4000u32);
    let scene = Scene::new(w, h);
    let recipe = scene.recipe();
    // An untiled (uniform) layer: the input costs nothing up front, so the
    // measured peak is the filter's own working set.
    let input = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.5);
    let base = CURRENT.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = CompositorFilters
        .evaluate(&input, &node(&recipe), &context(w, h))
        .expect("24 MP renders");
    let peak = PEAK.load(Ordering::SeqCst) - base;
    assert_eq!(out.extent(), Extent::new(w, h));
    let (wu, hu) = (w as usize, h as usize);
    let px = wu * hu;
    // Dense: the full (W+1)(H+1) Option<[f64; 2]> field, premultiplied input
    // and output planes, and the F32 output raster.
    let dense = (wu + 1) * (hu + 1) * std::mem::size_of::<Option<[f64; 2]>>() + 3 * 16 * px;
    // The output raster itself (16 B/px) is unavoidable in both.
    let output = 16 * px;
    eprintln!(
        "peak {:.0} MB (output raster {:.0} MB), dense estimate {:.0} MB",
        peak as f64 / 1e6,
        output as f64 / 1e6,
        dense as f64 / 1e6
    );
    assert!(
        peak * 10 <= dense * 6,
        "peak {peak} B is not well below the dense estimate {dense} B"
    );
}
