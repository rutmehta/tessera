use compositor::document::{Fill, SmartFilter, SmartObject};
use compositor::geom::Affine;
use compositor::raster::Depth;
use compositor::{Compositor, DocState, Document, Layer, LayerKind};
use engine_api::tile::Extent;
use std::sync::Arc;

#[test]
fn smart_filter_is_evaluated_on_nested_composite() {
    let mut child = DocState::new(Extent::new(3, 3), Depth::F32);
    child.root.push(Arc::new(Layer::new(
        "blue",
        LayerKind::Fill(Fill::Solid {
            color: [0.0, 0.0, 1.0],
        }),
    )));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(
        serde_json::from_value::<SmartFilter>(serde_json::json!({"enabled":true,"name":"invert"}))
            .unwrap(),
    );
    let mut state = DocState::new(Extent::new(3, 3), Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(so))));
    let doc = Document::new(state);
    let c = Compositor::new(1 << 20);
    let output = c.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(&output[..4], &[1.0, 1.0, 0.0, 1.0]);
}

#[test]
fn cache_reuse_mask_params_source_and_undo() {
    use compositor::render::smart_filters::FilterBlend;
    use compositor::{DocOp, Mask};
    let extent = Extent::new(3, 3);
    let mut child = Document::new(DocState::new(extent, Depth::F32));
    let inner = child
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new(
                "blue",
                LayerKind::Fill(Fill::Solid {
                    color: [0.0, 0.0, 1.0],
                }),
            ),
        })
        .unwrap()
        .created[0];
    let so = SmartObject::new((**child.state()).clone(), Affine::IDENTITY);
    let mut doc = Document::new(DocState::new(extent, Depth::F32));
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new("smart", LayerKind::SmartObject(so)),
        })
        .unwrap()
        .created[0];
    let filter = SmartFilter {
        name: "invert".into(),
        enabled: true,
        ..Default::default()
    };
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![filter.clone()],
        mask: None,
    })
    .unwrap();
    let c = Compositor::new(1 << 20);
    let pixels = |doc: &Document| c.render_level_rgba(doc, 0).unwrap().1;
    assert_eq!(&pixels(&doc)[..4], &[1.0, 1.0, 0.0, 1.0]);
    assert_eq!(c.filter_evaluations(), 1);
    c.clear_composites();
    assert_eq!(&pixels(&doc)[..4], &[1.0, 1.0, 0.0, 1.0]);
    assert_eq!(c.filter_evaluations(), 1);
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![filter.clone()],
        mask: Some(Mask::hide_all(extent, Depth::F32)),
    })
    .unwrap();
    assert_eq!(&pixels(&doc)[..4], &[0.0, 0.0, 1.0, 1.0]);
    assert_eq!(c.filter_evaluations(), 1);
    let half = SmartFilter {
        blend: FilterBlend {
            opacity: 0.5,
            ..Default::default()
        },
        ..filter.clone()
    };
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![half],
        mask: None,
    })
    .unwrap();
    assert_eq!(&pixels(&doc)[..4], &[0.5, 0.5, 0.5, 1.0]);
    assert_eq!(c.filter_evaluations(), 2);
    doc.apply(DocOp::EditSmartObject {
        id,
        op: Box::new(DocOp::SetFill {
            id: inner,
            fill: Fill::Solid {
                color: [1.0, 1.0, 1.0],
            },
        }),
    })
    .unwrap();
    pixels(&doc);
    assert_eq!(c.filter_evaluations(), 3);
    assert!(doc.undo());
    assert_eq!(&pixels(&doc)[..4], &[0.5, 0.5, 0.5, 1.0]);
    assert_eq!(c.filter_evaluations(), 3);
    // The shared mask is applied after both nodes, not between them.
    let mut mask = Mask::hide_all(extent, Depth::F32);
    mask.density = 0.5;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![filter.clone(), filter],
        mask: Some(mask),
    })
    .unwrap();
    assert_eq!(&pixels(&doc)[..4], &[0.0, 0.0, 1.0, 1.0]);
    let loaded =
        compositor::format::from_bytes(&compositor::format::to_bytes(doc.state()).unwrap())
            .unwrap();
    assert_eq!(pixels(&Document::new(loaded)), pixels(&doc));
}
