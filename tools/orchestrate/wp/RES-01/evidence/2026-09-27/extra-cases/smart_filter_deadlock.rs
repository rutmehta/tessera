//! Run the deadlock probe in a child process so a regression cannot strand the
//! test runner (or leave blocked Rayon workers behind in other tests).
use compositor::document::{Fill, SmartFilter, SmartObject};
use compositor::render::smart_filters::{FilterContext, FilterPassLimits, SmartFilterEvaluator};
use compositor::{Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster};
use engine_api::{EngineError, EngineResult, tile::Extent, tile::TileCoord};
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
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
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
fn pass_limit_rejects_a_cold_result_before_source_work() {
    let mut compositor = Compositor::new(8_223);
    compositor.set_filter_pass_limits(FilterPassLimits {
        retained_bytes: 8_223,
        entries: 1,
    });
    assert!(matches!(
        compositor.render_level_rgba(&document("invert"), 0),
        Err(EngineError::ResourceExhausted { .. })
    ));
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 0);
    assert_eq!(stats.attempted_stages, 0);
}

#[test]
fn pass_limit_is_separate_from_persistent_cache_and_releases_nested_reservations() {
    let mut inner = document("invert").state().as_ref().clone();
    let inner_smart = inner.root.pop().unwrap();
    let mut outer_child = DocState::new(Extent::new(257, 1), Depth::F32);
    outer_child.root.push(inner_smart);
    let mut outer = SmartObject::new(outer_child, Affine::IDENTITY);
    outer.filters.push(SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    });
    let mut state = DocState::new(Extent::new(257, 1), Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("outer", LayerKind::SmartObject(outer))));
    let doc = Document::new(state);
    let mut compositor = Compositor::new(0);
    compositor.set_filter_pass_limits(FilterPassLimits {
        retained_bytes: 8_224,
        entries: 2,
    });
    assert!(matches!(
        compositor.render_level_rgba(&doc, 0),
        Err(EngineError::ResourceExhausted { .. })
    ));
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(
        stats.attempted_stacks, 1,
        "inner admission must fail before source work"
    );
    assert_eq!(stats.attempted_stages, 0);
    assert_eq!(stats.active_stacks, 0);
    // The failed frame's outer reservation cannot consume the next frame's cap.
    compositor.set_filter_pass_limits(FilterPassLimits {
        retained_bytes: 16_448,
        entries: 2,
    });
    let rendered = compositor.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(rendered.len(), 257 * 4);
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 3);
    assert_eq!(stats.attempted_stages, 2);
    assert_eq!(stats.active_stacks, 0);
}

#[test]
fn hidden_disabled_and_offscreen_filters_do_not_consume_pass_admission() {
    let extent = Extent::new(257, 1);
    let make_smart = |name: &str, enabled: bool, transform: Affine| {
        let mut child = DocState::new(extent, Depth::F32);
        child.root.push(Arc::new(Layer::new(
            "source",
            LayerKind::Fill(Fill::Solid {
                color: [0.25, 0.5, 0.75],
            }),
        )));
        let mut smart = SmartObject::new(child, transform);
        smart.filters.push(SmartFilter {
            name: name.into(),
            enabled,
            ..Default::default()
        });
        Layer::new(name, LayerKind::SmartObject(smart))
    };
    let mut hidden = make_smart("invalid", true, Affine::IDENTITY);
    hidden.props.visible = false;
    let disabled = make_smart("invalid", false, Affine::IDENTITY);
    let offscreen = make_smart("invalid", true, Affine::scale_translate(1., 1., 1000., 0.));
    let mut hidden_base = Layer::new(
        "hidden base",
        LayerKind::Fill(Fill::Solid {
            color: [1., 0., 0.],
        }),
    );
    hidden_base.props.visible = false;
    let mut clipped = make_smart("invalid", true, Affine::IDENTITY);
    clipped.props.clipped = true;
    let mut state = DocState::new(extent, Depth::F32);
    state
        .root
        .extend([hidden, disabled, offscreen, hidden_base, clipped].map(Arc::new));
    let mut compositor = Compositor::new(0);
    compositor.set_filter_pass_limits(FilterPassLimits {
        retained_bytes: 0,
        entries: 0,
    });
    assert_eq!(
        compositor
            .render_level_rgba(&Document::new(state), 0)
            .unwrap()
            .1
            .len(),
        257 * 4
    );
    assert_eq!(compositor.filter_evaluation_stats().attempted_stacks, 0);
}

#[test]
fn live_scene_and_styled_smart_keep_full_level_pixels_and_one_evaluation() {
    use compositor::render::styles::{Overlay, StyleEffect};
    let extent = Extent::new(257, 1);
    let path = vector::Shape::Rectangle {
        rect: vector::Rect::new(0., 0., 2., 1.),
        radii: [0.; 4],
    }
    .path()
    .unwrap();
    let shape = Layer::new(
        "live shape",
        LayerKind::Shape {
            model: vector::ShapeModel {
                path,
                fill: Some(vector::Fill::Solid([0.8, 0.2, 0.1, 0.7])),
                ..Default::default()
            },
            transform: Affine::IDENTITY,
        },
    );
    let mut child = DocState::new(extent, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "source",
        LayerKind::Fill(Fill::Solid {
            color: [0.25, 0.5, 0.75],
        }),
    )));
    let mut smart = SmartObject::new(child, Affine::IDENTITY);
    smart.filters.push(SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    });
    let mut styled = Layer::new("styled smart", LayerKind::SmartObject(smart));
    styled
        .props
        .styles
        .effects
        .push(StyleEffect::ColorOverlay(Overlay::default()));
    let mut state = DocState::new(extent, Depth::F32);
    state.root.extend([Arc::new(shape), Arc::new(styled)]);
    let doc = Document::new(state);
    let direct = Compositor::new(8_223);
    let tiles = [0, 1].map(|x| direct.render_tile(&doc, TileCoord::new(0, x, 0)).unwrap());
    let expected = compositor::render::interleave(extent, &tiles).unwrap();
    let compositor = Compositor::new(8_223);
    let actual = compositor.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(actual, expected);
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 1);
    assert_eq!(stats.attempted_stages, 1);
}

#[test]
fn same_key_mask_variants_share_unmasked_pass_entry_without_sharing_pixels() {
    use compositor::Mask;
    let child_extent = Extent::new(257, 1);
    let mut child = DocState::new(child_extent, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "blue",
        LayerKind::Fill(Fill::Solid {
            color: [0., 0., 1.],
        }),
    )));
    let mut left = SmartObject::new(child, Affine::IDENTITY);
    left.filters.push(SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    });
    let mut right = left.clone(); // Exact child/filter cache key, different mask and placement.
    left.filter_mask = Some(Mask::hide_all(child_extent, Depth::F32));
    right.transform = Affine::scale_translate(1., 1., 257., 0.);
    let extent = Extent::new(514, 1);
    let mut state = DocState::new(extent, Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "masked left",
        LayerKind::SmartObject(left),
    )));
    state.root.push(Arc::new(Layer::new(
        "unmasked right",
        LayerKind::SmartObject(right),
    )));
    let compositor = Compositor::new(8_223);
    let (_, pixels) = compositor
        .render_level_rgba(&Document::new(state), 0)
        .unwrap();
    for x in [0, 256] {
        assert_eq!(&pixels[x * 4..x * 4 + 4], &[0., 0., 1., 1.]);
    }
    for x in [257, 513] {
        assert_eq!(&pixels[x * 4..x * 4 + 4], &[1., 1., 0., 1.]);
    }
    assert_eq!(compositor.filter_evaluation_stats().attempted_stacks, 1);
}

#[test]
fn concurrent_full_level_calls_have_independent_passes() {
    let doc = document("invert");
    let compositor = Compositor::new(0);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    pool.install(|| {
        let (a, b) = rayon::join(
            || compositor.render_level_rgba(&doc, 0),
            || compositor.render_level_rgba(&doc, 0),
        );
        assert_eq!(a.unwrap(), b.unwrap());
    });
    assert_eq!(compositor.filter_evaluation_stats().attempted_stacks, 2);
    assert_eq!(compositor.filter_evaluation_stats().active_stacks, 0);
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
