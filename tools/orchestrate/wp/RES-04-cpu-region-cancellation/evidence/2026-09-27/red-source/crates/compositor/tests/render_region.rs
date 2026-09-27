use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use compositor::document::{Fill, SmartFilter, SmartObject};
use compositor::render::smart_filters::{FilterContext, SmartFilterEvaluator};
use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Mask, Raster, Rect,
};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    tile::{Extent, TileCoord},
};

fn filled_document(extent: Extent) -> Document {
    let mut state = DocState::new(extent, Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "source",
        LayerKind::Fill(Fill::Solid {
            color: [0.25, 0.5, 0.75],
        }),
    )));
    Document::new(state)
}

fn filtered_document(masked: bool) -> Document {
    let extent = Extent::new(257, 1);
    let child = filled_document(extent).state().as_ref().clone();
    let mut smart = SmartObject::new(child, Affine::IDENTITY);
    smart.filters.push(SmartFilter {
        name: "count".into(),
        enabled: true,
        ..Default::default()
    });
    if masked {
        smart.filter_mask = Some(Mask::hide_all(extent, Depth::F32));
    }
    let mut outer = DocState::new(extent, Depth::F32);
    outer
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(smart))));
    Document::new(outer)
}

#[test]
fn region_clips_signed_level_pixels_and_returns_full_edge_tiles_in_raster_order() {
    let doc = filled_document(Extent::new(1025, 513));
    let renderer = Compositor::new(1 << 20);
    let reference = Compositor::new(1 << 20);
    let cancel = CancellationToken::new();
    let tiles = renderer
        .render_region(&doc, 1, Rect::new(255, -10, 600, 300), &cancel)
        .unwrap();
    let expected = [
        TileCoord::new(1, 0, 0),
        TileCoord::new(1, 1, 0),
        TileCoord::new(1, 2, 0),
        TileCoord::new(1, 0, 1),
        TileCoord::new(1, 1, 1),
        TileCoord::new(1, 2, 1),
    ];
    assert_eq!(
        tiles.iter().map(|t| t.coord()).collect::<Vec<_>>(),
        expected
    );
    assert_eq!(tiles[2].layout().extent, Extent::new(1, 256));
    assert_eq!(tiles[5].layout().extent, Extent::new(1, 1));
    for (tile, coord) in tiles.iter().zip(expected) {
        let direct = reference.render_tile(&doc, coord).unwrap();
        assert_eq!(tile.layout(), direct.layout());
        assert_eq!(
            tile.samples::<f32>().unwrap(),
            direct.samples::<f32>().unwrap()
        );
    }
}

#[test]
fn empty_outside_and_extreme_signed_regions_are_bounded() {
    let doc = filled_document(Extent::new(257, 1));
    let renderer = Compositor::new(1 << 20);
    let cancel = CancellationToken::new();
    for region in [
        Rect::new(-100, -100, -1, -1),
        Rect::new(300, 0, 400, 1),
        Rect::new(1, 0, 1, 1),
        Rect::new(i64::MAX, i64::MIN, i64::MAX, i64::MAX),
    ] {
        assert!(
            renderer
                .render_region(&doc, 0, region, &cancel)
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(renderer.stats().root_full, 0);
    let all = renderer
        .render_region(
            &doc,
            0,
            Rect::new(i64::MIN, i64::MIN, i64::MAX, i64::MAX),
            &cancel,
        )
        .unwrap();
    assert_eq!(
        all.iter().map(|t| t.coord()).collect::<Vec<_>>(),
        [TileCoord::new(0, 0, 0), TileCoord::new(0, 1, 0)]
    );
    assert!(matches!(
        renderer.render_region(
            &doc,
            compositor::render::MAX_LEVEL,
            Rect::new(0, 0, 0, 0),
            &cancel
        ),
        Err(EngineError::InvalidArgument { .. })
    ));
}

#[test]
fn region_does_not_render_cold_root_tiles_outside_the_request() {
    let doc = filled_document(Extent::new(769, 1));
    let renderer = Compositor::new(1 << 20);
    let tiles = renderer
        .render_region(
            &doc,
            0,
            Rect::new(300, 0, 301, 1),
            &CancellationToken::new(),
        )
        .unwrap();
    assert_eq!(
        tiles.iter().map(|t| t.coord()).collect::<Vec<_>>(),
        [TileCoord::new(0, 1, 0)]
    );
    assert_eq!(renderer.stats().root_full, 1);
}

struct Counting(AtomicUsize);

impl SmartFilterEvaluator for Counting {
    fn evaluate(&self, input: &Raster, _: &SmartFilter, _: &FilterContext) -> EngineResult<Raster> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(input.clone())
    }
}

#[test]
fn two_region_tiles_share_one_oversized_masked_filter_pass() {
    let evaluator = Arc::new(Counting(AtomicUsize::new(0)));
    let mut renderer = Compositor::new(8_223);
    renderer.set_filter_evaluator(evaluator.clone());
    let doc = filtered_document(true);
    let tiles = renderer
        .render_region(&doc, 0, Rect::new(0, 0, 257, 1), &CancellationToken::new())
        .unwrap();
    assert_eq!(tiles.len(), 2);
    let stats = renderer.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 1);
    assert_eq!(stats.mask_compositions, 1);
    assert_eq!(stats.active_stacks, 0);
    assert_eq!(evaluator.0.load(Ordering::Relaxed), 1);
}

struct CancelOnce {
    calls: AtomicUsize,
    first: AtomicBool,
}

impl SmartFilterEvaluator for CancelOnce {
    fn evaluate(&self, input: &Raster, _: &SmartFilter, _: &FilterContext) -> EngineResult<Raster> {
        Ok(input.clone())
    }

    fn evaluate_with_cancel(
        &self,
        input: &Raster,
        _: &SmartFilter,
        _: &FilterContext,
        cancel: &CancellationToken,
    ) -> EngineResult<Raster> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        if self.first.swap(false, Ordering::Relaxed) {
            cancel.cancel();
        }
        Ok(input.clone())
    }
}

#[test]
fn caller_token_cancels_region_stack_and_fresh_request_retries() {
    let evaluator = Arc::new(CancelOnce {
        calls: AtomicUsize::new(0),
        first: AtomicBool::new(true),
    });
    let mut renderer = Compositor::new(8_223);
    renderer.set_filter_evaluator(evaluator.clone());
    let doc = filtered_document(false);
    let region = Rect::new(0, 0, 257, 1);
    let cancelled = CancellationToken::new();
    cancelled.cancel();
    assert!(matches!(
        renderer.render_region(&doc, 0, region, &cancelled),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 0);
    assert_eq!(renderer.filter_evaluation_stats().attempted_stacks, 0);

    let cancel_during_stack = CancellationToken::new();
    assert!(matches!(
        renderer.render_region(&doc, 0, region, &cancel_during_stack),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 1);
    assert_eq!(renderer.filter_evaluations(), 0);
    assert_eq!(renderer.filter_evaluation_stats().active_stacks, 0);

    let tiles = renderer
        .render_region(&doc, 0, region, &CancellationToken::new())
        .unwrap();
    assert_eq!(tiles.len(), 2);
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 2);
    assert_eq!(renderer.filter_evaluation_stats().attempted_stacks, 2);
    assert_eq!(renderer.filter_evaluation_stats().active_stacks, 0);
}
