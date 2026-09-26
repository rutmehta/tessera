use compositor::document::{SmartFilter, SmartObject};
use compositor::geom::Affine;
use compositor::{Depth, DocOp, DocState, Document, Layer, LayerKind};
use engine_api::tile::Extent;
use transform::{Kernel, Operation, TransformOp, free::FreeTransform};

fn transform() -> TransformOp {
    TransformOp {
        version: 1,
        kernel: Kernel::Bilinear,
        operation: Operation::Free(FreeTransform::identity()),
    }
}

#[test]
fn all_transform_edit_paths_respect_locks_atomically() {
    for lock_all in [false, true] {
        let canvas = Extent::new(4, 4);
        let mut doc = Document::new(DocState::new(canvas, Depth::F32));
        let mut smart = SmartObject::new(DocState::new(canvas, Depth::F32), Affine::IDENTITY);
        smart
            .filters
            .push(SmartFilter::transform(transform()).unwrap());
        let mut layer = Layer::new("locked", LayerKind::SmartObject(smart));
        layer.props.locks.all = lock_all;
        layer.props.locks.position = !lock_all;
        let id = doc
            .apply(DocOp::AddLayer {
                parent: None,
                index: 0,
                layer,
            })
            .unwrap()
            .created[0];
        let mut disabled = SmartFilter::transform(transform()).unwrap();
        disabled.enabled = false;
        for edit in [
            DocOp::AddTransform {
                id,
                index: 0,
                transform: transform(),
            },
            DocOp::SetTransform {
                id,
                index: 0,
                transform: transform(),
            },
            DocOp::SetSmartFilters {
                id,
                filters: vec![disabled],
                mask: None,
            },
            DocOp::SetSmartFilters {
                id,
                filters: vec![],
                mask: None,
            },
            DocOp::SetSmartTransform {
                id,
                transform: Affine::IDENTITY,
            },
            DocOp::SetSmartFilters {
                id,
                filters: vec![
                    SmartFilter {
                        name: "invert".into(),
                        enabled: true,
                        ..Default::default()
                    },
                    SmartFilter::transform(transform()).unwrap(),
                ],
                mask: None,
            },
        ] {
            let before = compositor::format::to_bytes(doc.state()).unwrap();
            let label = format!("{edit:?}");
            assert!(doc.apply(edit).is_err(), "lock_all={lock_all}: {label}");
            assert_eq!(before, compositor::format::to_bytes(doc.state()).unwrap());
        }
    }
}

#[test]
fn position_lock_allows_color_filter_edits_without_moving_transform() {
    let canvas = Extent::new(4, 4);
    let mut doc = Document::new(DocState::new(canvas, Depth::F32));
    let transform = SmartFilter::transform(transform()).unwrap();
    let mut smart = SmartObject::new(DocState::new(canvas, Depth::F32), Affine::IDENTITY);
    smart.filters.push(transform.clone());
    let mut layer = Layer::new("position locked", LayerKind::SmartObject(smart));
    layer.props.locks.position = true;
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer,
        })
        .unwrap()
        .created[0];
    let filters = vec![
        transform,
        SmartFilter {
            name: "invert".into(),
            enabled: true,
            ..Default::default()
        },
    ];
    doc.apply(DocOp::SetSmartFilters {
        id,
        filters: filters.clone(),
        mask: None,
    })
    .unwrap();
    let LayerKind::SmartObject(smart) = &doc.state().find(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(smart.filters, filters);
    assert!(doc.undo());
    let LayerKind::SmartObject(smart) = &doc.state().find(id).unwrap().kind else {
        panic!()
    };
    assert_eq!(smart.filters, filters[..1]);
}
