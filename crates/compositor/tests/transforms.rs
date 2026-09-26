use compositor::document::{SmartFilter, SmartObject};
use compositor::geom::Affine;
use compositor::{Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, Rect};
use engine_api::tile::Extent;
use std::sync::Arc;

fn stage() -> SmartFilter {
    SmartFilter {
        name: "transform".into(),
        enabled: true,
        params: serde_json::json!({"version":1,"operation":{"Free":{"matrix":[[1.,0.,1.],[0.,1.,0.],[0.,0.,1.]]}},"kernel":"Bilinear"}),
        ..Default::default()
    }
}
fn document(filter: SmartFilter) -> Document {
    let e = Extent::new(3, 2);
    let mut raster = Raster::new(e, 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::new(0, 0, 1, 2), 1, |_, _, p| {
            *p = [0.8, 0.2, 0.1, 0.5]
        })
        .unwrap();
    let mut child = DocState::new(e, Depth::F32);
    child
        .root
        .push(Arc::new(Layer::new("source", LayerKind::Pixel(raster))));
    let mut so = SmartObject::new(child, Affine::IDENTITY);
    so.filters.push(filter);
    let mut state = DocState::new(e, Depth::F32);
    state
        .root
        .push(Arc::new(Layer::new("smart", LayerKind::SmartObject(so))));
    Document::new(state)
}
#[test]
fn edits_validate_preserve_order_and_history() {
    use compositor::DocOp;
    let mut doc = document(stage());
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 1,
            layer: Layer::new("editable", doc.state().root[0].kind.clone()),
        })
        .unwrap()
        .created[0];
    let op = stage().transform_op().unwrap().unwrap();
    doc.apply(DocOp::AddTransform {
        id,
        index: 0,
        transform: op.clone(),
    })
    .unwrap();
    let count = |doc: &Document| match &doc.state().find(id).unwrap().kind {
        LayerKind::SmartObject(so) => so.filters.len(),
        _ => panic!(),
    };
    assert_eq!(count(&doc), 2);
    assert!(doc.undo());
    assert_eq!(count(&doc), 1);
    assert!(doc.redo());
    assert_eq!(count(&doc), 2);
    doc.apply(DocOp::SetTransform {
        id,
        index: 1,
        transform: op,
    })
    .unwrap();
    let mut invalid = stage();
    invalid.params["version"] = 999.into();
    assert!(
        doc.apply(DocOp::SetSmartFilters {
            id,
            filters: vec![invalid],
            mask: None
        })
        .is_err()
    );
    assert_eq!(count(&doc), 2);
}

#[test]
fn native_roundtrip_and_invalid_transform_rejection() {
    let doc = document(stage());
    let loaded =
        compositor::format::from_bytes(&compositor::format::to_bytes(doc.state()).unwrap())
            .unwrap();
    let c = Compositor::new(1 << 20);
    assert_eq!(
        c.render_level_rgba(&doc, 0).unwrap().1,
        c.render_level_rgba(&Document::new(loaded), 0).unwrap().1
    );
    for params in [serde_json::json!({}), {
        let mut p = stage().params;
        p["version"] = 999.into();
        p
    }] {
        let mut f = stage();
        f.params = params;
        f.enabled = false;
        let bad = document(f);
        let bytes = compositor::format::to_bytes(bad.state()).unwrap();
        assert!(compositor::format::from_bytes(&bytes).is_err());
    }
}

#[test]
fn custom_evaluator_cannot_override_transform() {
    struct Reject;
    impl compositor::render::smart_filters::SmartFilterEvaluator for Reject {
        fn evaluate(
            &self,
            _: &Raster,
            _: &SmartFilter,
            _: &compositor::render::smart_filters::FilterContext,
        ) -> engine_api::EngineResult<Raster> {
            panic!("reserved transform dispatched externally")
        }
    }
    let mut c = Compositor::new(1 << 20);
    c.set_filter_evaluator(Arc::new(Reject));
    let out = c.render_level_rgba(&document(stage()), 0).unwrap().1;
    assert_eq!(&out[..4], &[0.; 4]);
}

#[test]
fn transform_blend_mask_cache_and_undo_render() {
    use compositor::{DocOp, Mask};
    let mut doc = document(stage());
    // Assign a real edit identity rather than mutating the source snapshot.
    let source = doc.state().root[0].clone();
    doc = Document::new(DocState::new(doc.state().canvas, Depth::F32));
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: (*source).clone(),
        })
        .unwrap()
        .created[0];
    let c = Compositor::new(1 << 20);
    let pixels = |doc: &Document| c.render_level_rgba(doc, 0).unwrap().1;
    let translated = pixels(&doc);
    assert_eq!(c.filter_evaluations(), 1);
    assert_eq!(pixels(&doc), translated);
    assert_eq!(c.filter_evaluations(), 1);
    let mut op = stage().transform_op().unwrap().unwrap();
    if let transform::Operation::Free(f) = &mut op.operation {
        f.matrix[0][2] = 2.;
    }
    doc.apply(DocOp::SetTransform {
        id,
        index: 0,
        transform: op,
    })
    .unwrap();
    assert_eq!(&pixels(&doc)[..8], &[0.; 8]);
    assert_eq!(c.filter_evaluations(), 2);
    assert!(doc.undo());
    assert_eq!(pixels(&doc), translated);
    assert!(doc.redo());
    assert_eq!(&pixels(&doc)[..8], &[0.; 8]);
    let mut half = stage();
    half.blend.opacity = 0.5;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![half],
        mask: None,
    })
    .unwrap();
    let out = pixels(&doc);
    assert_eq!(out[3], 0.25);
    assert_eq!(out[7], 0.25);
    let mut mask = Mask::hide_all(doc.state().canvas, Depth::F32);
    mask.density = 1.;
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: vec![stage()],
        mask: Some(mask),
    })
    .unwrap();
    let out = pixels(&doc);
    assert_eq!(&out[..4], &[0.8, 0.2, 0.1, 0.5]);
    assert_eq!(&out[4..8], &[0.; 4]);
    let LayerKind::SmartObject(so) = &doc.state().find(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(
        so.state.root[0].raster().unwrap().pixel(0, 0),
        [0.8, 0.2, 0.1, 0.5]
    );
}

#[test]
fn transform_translation_preserves_straight_color_and_alpha() {
    let doc = document(stage());
    let c = Compositor::new(1 << 20);
    let out = c.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(&out[..4], &[0.; 4]);
    for (a, b) in out[4..8].iter().zip([0.8, 0.2, 0.1, 0.5]) {
        assert!((a - b).abs() < 1e-6);
    }
}
