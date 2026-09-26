//! Reproduce with --release --test resident_filter_bench -- --ignored --nocapture.
use compositor::document::{Fill, SmartFilter};
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Rect, SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;
use std::time::Instant;

#[test]
#[ignore = "100-layer 20 MP interactive filter benchmark"]
fn filtered_100_layers_l2() {
    let canvas = Extent::new(5472, 3648);
    let mut doc = Document::new(DocState::new(canvas, Depth::F32));
    for i in 0..100 {
        let color = [i as f32 / 101.0, 0.3, 0.7];
        let mut layer = if i % 10 == 0 {
            let mut child = DocState::new(Extent::new(512, 512), Depth::F32);
            child.root.push(Arc::new(Layer::new(
                "source",
                LayerKind::Fill(Fill::Solid { color }),
            )));
            let mut so = SmartObject::new(
                child,
                Affine::scale_translate(1.0, 1.0, (i * 37) as f64, (i * 21) as f64),
            );
            so.filters.push(SmartFilter {
                name: "gaussian".into(),
                params: serde_json::json!({"radius":2.0}),
                enabled: true,
                ..Default::default()
            });
            Layer::new("filtered", LayerKind::SmartObject(so))
        } else {
            Layer::new("fill", LayerKind::Fill(Fill::Solid { color }))
        };
        layer.props.opacity = 0.1;
        doc.apply(compositor::DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer,
        })
        .unwrap();
    }
    let viewport = Rect::of_extent(canvas.at_level(2));
    let mut cpu = Compositor::new(256 << 20);
    cpu.set_filter_evaluator(Arc::new(filters::CompositorFilters));
    let gpu = GpuCompositor::new().unwrap();
    let mut resident = ResidentRenderer::with_budget(&gpu, 256 << 20).unwrap();
    resident
        .set_filter_evaluator(Arc::new(filters::CompositorFilters))
        .unwrap();
    let cold_cpu = Instant::now();
    let expected = cpu.render_level_rgba(&doc, 2).unwrap().1;
    let cold_cpu = cold_cpu.elapsed();
    let cold_gpu = Instant::now();
    resident.render_viewport(&doc, 2, viewport, 0).unwrap();
    resident.wait().unwrap();
    let cold_gpu = cold_gpu.elapsed();
    let actual = resident.read_level(2, false).unwrap().1;
    let error = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);

    assert!(error <= 1e-4, "{error}");
    assert_eq!(resident.filter_evaluations(), 10);
    resident.wait_for_specializations();
    let mut before = Vec::new();
    let mut after = Vec::new();
    for _ in 0..7 {
        cpu.clear_composites();
        let start = Instant::now();
        cpu.render_level_rgba(&doc, 2).unwrap();
        before.push(start.elapsed().as_secs_f64() * 1000.0);
        resident.invalidate();
        let start = Instant::now();
        resident.render_viewport(&doc, 2, viewport, 0).unwrap();
        resident.wait().unwrap();
        after.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    before.sort_by(f64::total_cmp);
    after.sort_by(f64::total_cmp);
    eprintln!(
        "M5-23 100 layers (10 filtered 512² children), 20 MP canvas, full L2 viewport: CPU before median {:.3} ms; resident after median {:.3} ms; cold CPU {:?}; cold GPU {:?}; max error {error}; stack cache {} bytes; 16ms target met={}",
        before[3],
        after[3],
        cold_cpu,
        cold_gpu,
        resident.filter_cache_bytes(),
        after[3] < 16.0
    );
}
