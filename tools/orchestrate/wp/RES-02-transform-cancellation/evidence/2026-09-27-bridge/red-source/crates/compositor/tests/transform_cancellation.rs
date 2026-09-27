use compositor::document::{Fill, SmartFilter, SmartObject};
use compositor::render::smart_filters::{FilterContext, SmartFilterEvaluator};
use compositor::{Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster};
use engine_api::{EngineError, EngineResult, jobs::CancellationToken, tile::Extent};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use transform::{Kernel, Operation, TransformOp, seam::ContentAwareScale};

fn document_with_stages() -> Document {
    let extent = Extent::new(3, 2);
    let mut child = DocState::new(extent, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "source",
        LayerKind::Fill(Fill::Solid {
            color: [0.25, 0.5, 0.75],
        }),
    )));
    let mut smart = SmartObject::new(child, Affine::IDENTITY);
    smart.filters.push(SmartFilter {
        name: "trip".into(),
        enabled: true,
        ..Default::default()
    });
    smart.filters.push(
        SmartFilter::transform(TransformOp {
            version: 1,
            kernel: Kernel::Bilinear,
            operation: Operation::ContentAwareScale(ContentAwareScale {
                target_width: 2,
                target_height: 2,
                amount: 1.0,
                protect: None,
            }),
        })
        .unwrap(),
    );
    let mut outer = DocState::new(extent, Depth::F32);
    outer
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(smart))));
    Document::new(outer)
}

struct CancelFirstStage {
    cancel_once: AtomicBool,
    calls: AtomicUsize,
}

impl SmartFilterEvaluator for CancelFirstStage {
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
        if self.cancel_once.swap(false, Ordering::Relaxed) {
            cancel.cancel();
        }
        Ok(input.clone())
    }
}

#[test]
fn cancellation_between_stages_does_not_publish_a_filter_result() {
    let evaluator = Arc::new(CancelFirstStage {
        cancel_once: AtomicBool::new(true),
        calls: AtomicUsize::new(0),
    });
    let mut compositor = Compositor::new(1 << 20);
    compositor.set_filter_evaluator(evaluator.clone());
    let doc = document_with_stages();
    let cancel = CancellationToken::new();
    assert!(matches!(
        compositor.render_level(&doc, 0, &cancel),
        Err(EngineError::Cancelled)
    ));
    let stats = compositor.filter_evaluation_stats();
    assert_eq!(stats.attempted_stacks, 1);
    assert_eq!(stats.attempted_stages, 1);
    assert_eq!(stats.active_stacks, 0);
    assert_eq!(compositor.filter_evaluations(), 0);

    let (extent, pixels) = compositor
        .render_level_rgba_with_cancel(&doc, 0, &CancellationToken::new())
        .unwrap();
    assert_eq!(extent, Extent::new(3, 2));
    assert_eq!(pixels.len(), 3 * 2 * 4);
    assert_eq!(evaluator.calls.load(Ordering::Relaxed), 2);
    assert_eq!(compositor.filter_evaluation_stats().attempted_stages, 3);
}

#[test]
fn rgba_render_observes_a_precancelled_caller_token() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(matches!(
        Compositor::new(1 << 20).render_level_rgba_with_cancel(&document_with_stages(), 0, &cancel),
        Err(EngineError::Cancelled)
    ));
}
