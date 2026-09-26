use compositor::document::SmartFilter;
use compositor::geom::Rect;
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;

fn filter(name: &str, params: serde_json::Value) -> SmartFilter {
    SmartFilter {
        name: name.into(),
        params,
        enabled: true,
        ..Default::default()
    }
}
#[test]
fn mask_edits_reuse_gpu_stages_and_cpu_only_is_layer_local() {
    use compositor::{DocOp, Mask};
    let gpu = GpuCompositor::new().unwrap();
    let stages = vec![filter("invert", serde_json::json!({})); 3];
    let mut doc = document(stages.clone());
    let id = doc.state().root[0].id;
    let cpu = Compositor::new(1 << 20);
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(renderer.filter_evaluations(), 3);
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: stages,
        mask: Some(Mask::hide_all(doc.state().canvas, Depth::F32)),
    })
    .unwrap();
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(renderer.filter_evaluations(), 3);
    assert_eq!(renderer.filter_fallbacks(), 0);
    assert!(doc.undo());
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(renderer.filter_evaluations(), 3);
    // Unsupported built-in Gaussian falls back only for this smart layer. The
    // unrelated resident pixel layer remains uploaded across the filter edit.
    let plain = Layer::new(
        "resident sibling",
        LayerKind::Pixel(Raster::new(doc.state().canvas, 4, Depth::F32, 0.2)),
    );
    doc.apply(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: plain,
    })
    .unwrap();
    renderer.render(&doc, 0).unwrap();
    let uploaded = renderer.stats().uploaded_pages;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![filter("gaussian", serde_json::json!({"radius":1.0}))],
        mask: None,
    })
    .unwrap();
    compare(&doc, &mut renderer, &cpu, 1e-4);
    assert!(renderer.filter_fallbacks() > 0);
    assert_eq!(renderer.stats().uploaded_pages, uploaded);
}

#[test]
fn adjustments_and_cpu_only_content_aware() {
    use serde_json::json;
    let gpu = GpuCompositor::new().unwrap();
    let mut cpu = Compositor::new(1 << 20);
    cpu.set_filter_evaluator(Arc::new(filters::CompositorFilters));
    let adjustments = vec![
        json!({"levels":{"input_black":([0.1;3]),"input_white":([0.9;3]),"gamma":([1.1;3]),"output_black":([0.0;3]),"output_white":([1.0;3])}}),
        json!({"curves":{"points":([[[0.0,0.0],[0.5,0.6],[1.0,1.0]];3])}}),
        json!({"brightness_contrast":{"brightness":10.0,"contrast":20.0,"legacy":false}}),
        json!({"exposure":{"stops":0.5,"offset":0.1,"gamma":1.2}}),
        json!({"threshold":{"level":0.4}}),
        json!({"posterize":{"levels":6}}),
        json!({"hsl":{"hue_degrees":15.0,"saturation":0.2,"lightness":0.1}}),
        json!({"vibrance":{"vibrance":0.3,"saturation":0.1}}),
        json!({"photo_filter":{"colour":[0.4,0.6,0.8],"density":0.3,"preserve_luminosity":true}}),
        json!({"channel_mixer":{"matrix":[[0.8,0.1,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]],"constant":[0.1,0.0,0.0]}}),
        json!({"gradient_map":{"stops":[[0.0,[0.2,0.1,0.0]],[1.0,[1.0,0.7,0.8]]],"reverse":false}}),
        json!({"selective_colour":{"corrections":([[0.1,0.0,0.0,0.0];9]),"relative":true}}),
        json!({"black_white":{"weights":([1.1;6]),"tint":[0.9,0.7,0.5]}}),
        json!("invert"),
        json!("desaturate"),
    ];
    for adjustment in adjustments {
        eprintln!("adjustment {adjustment}");
        let doc = document(vec![filter("adjust", json!({"adjust":adjustment}))]);
        let mut renderer = ResidentRenderer::new(&gpu).unwrap();
        renderer
            .set_filter_evaluator(Arc::new(filters::CompositorFilters))
            .unwrap();
        compare(&doc, &mut renderer, &cpu, 1e-4);
        assert_eq!(renderer.filter_evaluations(), 1);
        assert_eq!(renderer.filter_fallbacks(), 0);
    }
    let op = transform::TransformOp {
        version: 1,
        kernel: transform::Kernel::Bilinear,
        operation: transform::Operation::ContentAwareScale(transform::seam::ContentAwareScale {
            target_width: 15,
            target_height: 12,
            amount: 1.0,
            protect: None,
        }),
    };
    let doc = document(vec![SmartFilter::transform(op).unwrap()]);
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    compare(&doc, &mut renderer, &cpu, 1e-4);
    assert_eq!(renderer.filter_evaluations(), 0);
    assert_eq!(renderer.filter_fallbacks(), 2);
}

#[test]
fn lossless_opaque_nearest_chain_is_one_displacement_stage() {
    use compositor::document::Fill;
    use transform::{Kernel, Operation, TransformOp, free::FreeTransform};
    let e = Extent::new(17, 13);
    let mut child = DocState::new(e, Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "opaque pattern",
        LayerKind::Fill(Fill::Pattern {
            width: 2,
            height: 1,
            rgba: vec![0.1, 0.3, 0.5, 1.0, 0.7, 0.9, 0.2, 1.0],
            origin: [0.0, 0.0],
        }),
    )));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    for matrix in [
        [[-1.0, 0.0, 17.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        [[1.0, 0.0, 0.0], [0.0, -1.0, 13.0], [0.0, 0.0, 1.0]],
    ] {
        so.filters.push(
            SmartFilter::transform(TransformOp {
                version: 1,
                kernel: Kernel::Nearest,
                operation: Operation::Free(FreeTransform { matrix }),
            })
            .unwrap(),
        );
    }
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("chain", LayerKind::SmartObject(so))));
    let gpu = GpuCompositor::new().unwrap();
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    compare(
        &Document::new(state),
        &mut renderer,
        &Compositor::new(1 << 20),
        0.0,
    );
    assert_eq!(renderer.filter_evaluations(), 1);
}

#[test]
fn prefix_cache_revision_invalidation_and_budget() {
    use compositor::DocOp;
    let gpu = GpuCompositor::new().unwrap();
    let cpu = Compositor::new(1 << 20);
    let mut stages = vec![filter("invert", serde_json::json!({})); 3];
    let mut doc = document(stages.clone());
    let id = doc.state().root[0].id;
    let mut renderer = ResidentRenderer::with_budget(&gpu, 1 << 20).unwrap();
    compare(&doc, &mut renderer, &cpu, 0.0);
    stages[2].blend.opacity = 0.5;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: stages,
        mask: None,
    })
    .unwrap();
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(
        renderer.filter_evaluations(),
        4,
        "only changed suffix executes"
    );
    doc.apply(DocOp::EditSmartObject {
        id,
        op: Box::new(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: Layer::new(
                "new source",
                LayerKind::Fill(compositor::document::Fill::Solid {
                    color: [0.2, 0.4, 0.6],
                }),
            ),
        }),
    })
    .unwrap();
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(
        renderer.filter_evaluations(),
        7,
        "source revision invalidates every stage"
    );
    assert!(doc.undo());
    compare(&doc, &mut renderer, &cpu, 0.0);
    assert_eq!(
        renderer.filter_evaluations(),
        7,
        "undo reuses cached source revision"
    );
    let mut bounded = ResidentRenderer::with_budget(&gpu, 1024).unwrap();
    compare(&doc, &mut bounded, &cpu, 0.0);
    assert!(bounded.filter_cache_bytes() <= 1024);
}

#[test]
fn styled_smart_source_falls_back_without_omitting_effects() {
    let mut layer = Layer::new(
        "styled",
        LayerKind::Fill(compositor::document::Fill::Solid {
            color: [0.0, 0.0, 1.0],
        }),
    );
    layer.props = serde_json::from_value(
        serde_json::json!({"styles":{"effects":[{"kind":"color_overlay","settings":{}}]}}),
    )
    .unwrap();
    let mut child = DocState::new(Extent::new(17, 13), Depth::F32);
    child.root.push(Arc::new(layer));
    let so = SmartObject::new(child.clone(), Affine::IDENTITY);
    let mut state = DocState::new(child.canvas, Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "styled smart",
        LayerKind::SmartObject(so),
    )));
    let gpu = GpuCompositor::new().unwrap();
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    compare(
        &Document::new(state),
        &mut renderer,
        &Compositor::new(1 << 20),
        0.0,
    );
    assert!(renderer.filter_fallbacks() > 0);
    assert!(
        ResidentRenderer::new(&gpu)
            .unwrap()
            .render(&Document::new(child), 0)
            .is_err()
    );
}

#[test]
fn median_fallback_keeps_unrelated_uploaded_pages() {
    use compositor::DocOp;
    let mut doc = document(vec![filter("box", serde_json::json!({"radius":1.0}))]);
    let id = doc.state().root[0].id;
    let e = doc.state().canvas;
    let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(e), 1, |_, _, p| *p = [0.1, 0.2, 0.3, 1.0])
        .unwrap();
    doc.apply(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: Layer::new("resident sibling", LayerKind::Pixel(raster)),
    })
    .unwrap();
    let gpu = GpuCompositor::new().unwrap();
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    renderer
        .set_filter_evaluator(Arc::new(filters::CompositorFilters))
        .unwrap();
    let mut cpu = Compositor::new(1 << 20);
    cpu.set_filter_evaluator(Arc::new(filters::CompositorFilters));
    compare(&doc, &mut renderer, &cpu, 1e-4);
    let uploaded = renderer.stats().uploaded_pages;
    assert!(uploaded > 0);
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![filter("median", serde_json::json!({"radius":1.0}))],
        mask: None,
    })
    .unwrap();
    compare(&doc, &mut renderer, &cpu, 1e-4);
    assert_eq!(renderer.filter_fallbacks(), 2);
    assert_eq!(renderer.stats().uploaded_pages, uploaded);
    assert_eq!(renderer.render(&doc, 2).unwrap().blocks, 0);
    renderer
        .set_smart_quality(compositor::resident::SmartQuality::Lanczos3)
        .unwrap();
    assert!(matches!(
        renderer.render(&doc, 0),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

fn document(filters: Vec<SmartFilter>) -> Document {
    let e = Extent::new(17, 13);
    let mut raster = Raster::new(e, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(e), 1, |x, y, p| {
            *p = [
                x as f32 / 19.0,
                y as f32 / 17.0,
                0.25,
                [0.0, 0.25, 0.5, 1.0][((x + y) % 4) as usize],
            ]
        })
        .unwrap();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("pixels", LayerKind::Pixel(raster))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters = filters;
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(so))));
    Document::new(state)
}
fn compare(doc: &Document, renderer: &mut ResidentRenderer, cpu: &Compositor, tolerance: f32) {
    for level in [0, 2] {
        renderer.render(doc, level).unwrap();
        let actual = renderer.read_level(level, false).unwrap().1;
        let expected = cpu.render_level_rgba(doc, level).unwrap().1;
        let error = actual
            .iter()
            .zip(&expected)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert_eq!(actual.len(), expected.len());
        assert!(actual.iter().all(|x| x.is_finite()));
        assert!(
            error <= tolerance,
            "L{level}: max error {error} > {tolerance}"
        );
    }
}
#[test]
fn every_metal_filter_routes_and_matches_cpu() {
    let gpu = GpuCompositor::new().unwrap();
    let mut cpu = Compositor::new(1 << 20);
    cpu.set_filter_evaluator(Arc::new(filters::CompositorFilters));
    let names = [
        "gaussian",
        "box",
        "motion",
        "radial_spin",
        "radial_zoom",
        "lens_blur",
        "surface_blur",
        "unsharp_mask",
        "high_pass",
        "add_noise",
        "pinch",
        "spherize",
        "twirl",
        "wave",
        "ripple",
        "polar_to_rectangular",
        "rectangular_to_polar",
        "offset",
    ];
    for name in names {
        eprintln!("filter {name}");
        let mut params = serde_json::json!({"radius":1.2,"angle":17.0,"strength":0.25,"threshold":0.1,"seed":42,"distort":{"amount":0.2,"offset":[1.2,-0.3]}});
        if name == "lens_blur" {
            params["depth"] = serde_json::json!(vec![0.7f32; 17 * 13]);
        }
        let doc = document(vec![filter(name, params)]);
        let mut renderer = ResidentRenderer::new(&gpu).unwrap();
        renderer
            .set_filter_evaluator(Arc::new(filters::CompositorFilters))
            .unwrap();
        compare(&doc, &mut renderer, &cpu, 1e-4);
        assert_eq!(renderer.filter_evaluations(), 1);
        assert_eq!(renderer.filter_fallbacks(), 0);
    }
}
#[test]
fn automatic_transform_kinds_and_three_stage_nested_stack() {
    use transform::{
        Kernel, Operation, TransformOp,
        free::FreeTransform,
        perspective::PerspectiveWarp,
        puppet::{PuppetDensity, PuppetWarp},
        warp::{WarpMesh, WarpPreset},
    };
    let gpu = GpuCompositor::new().unwrap();
    let cpu = Compositor::new(1 << 20);
    let quad = [[0., 0.], [17., 0.], [17., 13.], [0., 13.]];
    let mut puppet =
        PuppetWarp::from_alpha(&[255; 17 * 13], 17, 13, PuppetDensity::Sparse, 0).unwrap();
    puppet.pins.push(transform::puppet::PuppetPin {
        vertex: 0,
        target: [0.2, 0.1],
        rotation: None,
    });
    let operations = vec![
        Operation::Free(FreeTransform::translate(0.3, -0.2).unwrap()),
        Operation::Warp(WarpMesh::preset(17., 13., WarpPreset::Wave, 0.2).unwrap()),
        Operation::Perspective(
            PerspectiveWarp::from_quads(
                vec![quad],
                vec![[[0.2, 0.1], [16.5, 0.3], [17., 12.7], [0., 13.]]],
            )
            .unwrap(),
        ),
        Operation::Puppet(puppet),
    ];
    for operation in operations {
        for kernel in [
            Kernel::Nearest,
            Kernel::Bilinear,
            Kernel::Bicubic,
            Kernel::Lanczos3,
            Kernel::Automatic,
        ] {
            let stage = SmartFilter::transform(TransformOp {
                version: 1,
                operation: operation.clone(),
                kernel,
            })
            .unwrap();
            let doc = document(vec![stage]);
            let mut renderer = ResidentRenderer::new(&gpu).unwrap();
            compare(
                &doc,
                &mut renderer,
                &cpu,
                if kernel == Kernel::Nearest { 0.0 } else { 1e-4 },
            );
            assert_eq!(renderer.filter_evaluations(), 1);
            assert_eq!(renderer.filter_fallbacks(), 0);
        }
    }
    let stages = vec![
        filter("invert", serde_json::json!({})),
        SmartFilter::transform(TransformOp {
            version: 1,
            operation: Operation::Free(FreeTransform::translate(0.4, 0.2).unwrap()),
            kernel: Kernel::Bilinear,
        })
        .unwrap(),
        filter("invert", serde_json::json!({})),
    ];
    let inner = document(stages.clone());
    let mut so = SmartObject::new((**inner.state()).clone(), Affine::IDENTITY);
    so.filters = stages;
    let mut state = DocState::new(inner.state().canvas, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("nested", LayerKind::SmartObject(so))));
    let mut renderer = ResidentRenderer::new(&gpu).unwrap();
    compare(&Document::new(state), &mut renderer, &cpu, 1e-4);
    assert_eq!(renderer.filter_evaluations(), 3);
    assert_eq!(renderer.filter_fallbacks(), 0);
}
