//! Run the deadlock probe in a child process so a regression cannot strand the
//! test runner (or leave blocked Rayon workers behind in other tests).
use compositor::document::{Fill, SmartFilter, SmartObject};
use compositor::render::smart_filters::{FilterContext, SmartFilterEvaluator};
use compositor::{Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster};
use engine_api::{EngineResult, tile::Extent, tile::TileCoord};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, OnceLock, Weak};
use std::time::{Duration, Instant};

fn document(name: &str) -> Document {
    let extent = Extent::new(257, 1); // Two tiles, including a partial edge tile.
    let mut child = DocState::new(extent, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "source",
        LayerKind::Fill(Fill::Solid {
            color: [0.25, 0.5, 0.75],
        }),
    )));
    let mut smart = SmartObject::new(child, Affine::IDENTITY);
    smart.filters.push(SmartFilter {
        name: name.into(),
        enabled: true,
        ..Default::default()
    });
    let mut outer = DocState::new(extent, Depth::F32);
    outer
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(smart))));
    Document::new(outer)
}

struct Reentrant {
    compositor: OnceLock<Weak<Compositor>>,
    nested: Document,
}

impl SmartFilterEvaluator for Reentrant {
    fn evaluate(
        &self,
        input: &Raster,
        filter: &SmartFilter,
        _: &FilterContext,
    ) -> EngineResult<Raster> {
        if filter.name == "outer" {
            let compositor = self.compositor.get().unwrap().upgrade().unwrap();
            // Re-enter the SAME filter cache from a stage, on the SAME Rayon
            // pool. The nested document has a different key and a leaf stage,
            // so there is no cyclic document/evaluator dependency.
            let (a, b) = rayon::join(
                || compositor.render_level_rgba(&self.nested, 0),
                || compositor.render_level_rgba(&self.nested, 0),
            );
            assert_pixels(&a?.1);
            assert_pixels(&b?.1);
        }
        Ok(input.clone())
    }
}

fn assert_pixels(pixels: &[f32]) {
    assert_eq!(pixels.len(), 257 * 4);
    for pixel in pixels.as_chunks::<4>().0 {
        assert_eq!(pixel, &[0.25, 0.5, 0.75, 1.0]);
    }
}

// Both tiles must enter evaluation before either can publish. This also rejects
// a blocking per-key OnceLock: a cold waiter must not park a Rayon worker.
fn same_key_cold_race() {
    struct Concurrent {
        barrier: Barrier,
        calls: AtomicUsize,
    }
    impl SmartFilterEvaluator for Concurrent {
        fn evaluate(
            &self,
            input: &Raster,
            _: &SmartFilter,
            _: &FilterContext,
        ) -> EngineResult<Raster> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.barrier.wait();
            Ok(input.clone())
        }
    }
    let evaluator = Arc::new(Concurrent {
        barrier: Barrier::new(2),
        calls: AtomicUsize::new(0),
    });
    let mut compositor = Compositor::new(1 << 20);
    compositor.set_filter_evaluator(evaluator.clone());
    let doc = document("race");
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    pool.install(|| {
        let (a, b) = rayon::join(
            || compositor.render_tile(&doc, TileCoord::new(0, 0, 0)),
            || compositor.render_tile(&doc, TileCoord::new(0, 1, 0)),
        );
        let pixels =
            compositor::render::interleave(doc.state().canvas, &[a.unwrap(), b.unwrap()]).unwrap();
        assert_pixels(&pixels);
    });
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 2);
    assert_eq!(compositor.filter_evaluations(), 1);
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 2);
    assert_eq!(stats.attempted_stages, 2);
    assert_eq!(stats.duplicate_stacks, 1);
    assert_eq!(stats.active_stacks, 0);
    assert_eq!(stats.peak_active_stacks, 2);
    compositor.clear_composites();
    assert_pixels(&compositor.render_level_rgba(&doc, 0).unwrap().1);
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 2);
    assert_eq!(compositor.filter_evaluations(), 1);
    assert_eq!(compositor.filter_evaluation_stats().attempted_stacks, 2);
}

/// Red regression: a result just above the persistent cache budget is currently
/// recomputed for every tile. The persistent cache budget is not an admission
/// limit: one successful frame must evaluate this whole-image stack once.
#[test]
fn oversized_filtered_source_is_evaluated_once_per_frame() {
    struct Counting(AtomicUsize);
    impl SmartFilterEvaluator for Counting {
        fn evaluate(
            &self,
            input: &Raster,
            _: &SmartFilter,
            _: &FilterContext,
        ) -> EngineResult<Raster> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(input.clone())
        }
    }
    let evaluator = Arc::new(Counting(AtomicUsize::new(0)));
    // 257 pixels × 32 bytes (source + result) = 8,224 bytes.
    let mut compositor = Compositor::new(8_223);
    compositor.set_filter_evaluator(evaluator.clone());
    let doc = document("count");
    let pool = rayon::ThreadPoolBuilder::new().num_threads(2).build().unwrap();
    let rendered = pool.install(|| compositor.render_level_rgba(&doc, 0));
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.active_stacks, 0);
    assert_pixels(&rendered.unwrap().1);
    assert_eq!(stats.attempted_stacks, 1, "one whole-image stack per frame");
    assert_eq!(stats.attempted_stages, 1);
    assert_eq!(stats.oversized_stacks, 1);
    assert_eq!(evaluator.0.load(Ordering::Relaxed), 1);
}

#[test]
fn smart_filter_parallel_reentry_does_not_deadlock() {
    const CHILD: &str = "TESSERA_SMART_FILTER_DEADLOCK_CHILD";
    if std::env::var_os(CHILD).is_some() {
        same_key_cold_race();
        for workers in [1, 2, 4] {
            for budget in [0, 1 << 20] {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .build()
                    .unwrap();
                let evaluator = Arc::new(Reentrant {
                    compositor: OnceLock::new(),
                    nested: document("leaf"),
                });
                let mut compositor = Compositor::new(budget);
                compositor.set_filter_evaluator(evaluator.clone());
                let compositor = Arc::new(compositor);
                evaluator
                    .compositor
                    .set(Arc::downgrade(&compositor))
                    .ok()
                    .unwrap();
                let doc = document("outer");
                pool.install(|| {
                    let cold = compositor.render_level_rgba(&doc, 0).unwrap().1;
                    assert_pixels(&cold);
                    let evaluations = compositor.filter_evaluations();
                    compositor.clear_composites();
                    let warm = compositor.render_level_rgba(&doc, 0).unwrap().1;
                    assert_eq!(cold, warm);
                    if budget != 0 {
                        assert_eq!(compositor.filter_evaluations(), evaluations);
                    }
                });
            }
        }
        return;
    }
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "smart_filter_parallel_reentry_does_not_deadlock",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(status.success(), "deadlock probe failed: {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("smart-filter stage re-entry timed out (cache/Rayon deadlock)");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}
