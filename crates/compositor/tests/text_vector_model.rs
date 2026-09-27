use compositor::{Affine, Depth, DocOp, DocState, Document, LayerKind, VectorMask};
use engine_api::tile::Extent;

#[test]
fn editable_models_history_and_native_roundtrip() {
    let mut doc = Document::new(DocState::new(Extent::new(32, 32), Depth::F32));
    let text = typography::TextModel::point("hello", "sans-serif", 12.);
    let id = doc
        .apply(DocOp::AddText {
            parent: None,
            index: 0,
            name: "Type".into(),
            model: text.clone(),
            transform: Affine::IDENTITY,
        })
        .unwrap()
        .created[0];
    doc.apply(DocOp::EditTextRuns {
        id,
        range: 0..1,
        runs: vec![typography::TextRun {
            text: "world".into(),
            ..text.runs[0].clone()
        }],
    })
    .unwrap();
    assert!(doc.undo());
    assert!(
        matches!(&doc.state().find(id).unwrap().kind, LayerKind::Text { model, .. } if model == &text)
    );
    assert!(doc.redo());
    let shape = vector::ShapeModel {
        path: vector::Path::polyline(
            &[
                vector::Point::new(1., 1.),
                vector::Point::new(20., 1.),
                vector::Point::new(20., 20.),
            ],
            true,
        ),
        fill: Some(vector::Fill::Solid([1., 0., 0., 1.])),
        ..Default::default()
    };
    let shape_id = doc
        .apply(DocOp::AddShape {
            parent: None,
            index: 1,
            name: "Shape".into(),
            model: shape.clone(),
            transform: Affine::IDENTITY,
        })
        .unwrap()
        .created[0];
    let mask = VectorMask {
        path: shape.path.clone(),
        enabled: true,
        feather: 0.5,
        density: 0.75,
    };
    doc.apply(DocOp::SetVectorMask {
        id: shape_id,
        mask: Some(mask.clone()),
    })
    .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("live.tessera-doc");
    compositor::format::save(&doc, &path).unwrap();
    let loaded = compositor::format::load(&path).unwrap();
    assert!(
        matches!(&loaded.state().find(shape_id).unwrap().kind, LayerKind::Shape { model, .. } if model == &shape)
    );
    assert_eq!(
        loaded.state().find(shape_id).unwrap().vector_mask,
        Some(mask)
    );
    let before = doc.history().len();
    assert!(
        doc.apply(DocOp::EditTextRuns {
            id,
            range: 5..6,
            runs: vec![]
        })
        .is_err()
    );
    assert_eq!(doc.history().len(), before);
}

#[test]
fn convert_shape_preserves_identity_masks_and_undo() {
    let mut doc = Document::new(DocState::new(Extent::new(8, 8), Depth::U16));
    let model = vector::ShapeModel::from_shape(
        vector::Shape::Rectangle {
            rect: vector::Rect::new(0., 0., 8., 8.),
            radii: [0.; 4],
        },
        Some(vector::Fill::Solid([1., 0., 0., 1.])),
        None,
    )
    .unwrap();
    let id = doc
        .apply(DocOp::AddShape {
            parent: None,
            index: 0,
            name: "shape".into(),
            model,
            transform: Affine::IDENTITY,
        })
        .unwrap()
        .created[0];
    let mask = VectorMask {
        density: 0.5,
        ..Default::default()
    };
    doc.apply(DocOp::SetVectorMask {
        id,
        mask: Some(mask.clone()),
    })
    .unwrap();
    doc.apply(DocOp::ConvertToPixels { id }).unwrap();
    let layer = doc.state().find(id).unwrap();
    assert_eq!(layer.props.name, "shape");
    assert_eq!(layer.vector_mask, Some(mask));
    let LayerKind::Pixel(raster) = &layer.kind else {
        panic!("must be pixels")
    };
    assert_eq!(raster.depth(), Depth::U16);
    assert_eq!(raster.pixel(3, 3), [1., 0., 0., 1.]);
    assert!(doc.undo());
    assert!(matches!(
        doc.state().find(id).unwrap().kind,
        LayerKind::Shape { .. }
    ));
}

#[test]
fn editable_validation_is_atomic_and_honors_locks() {
    let mut doc = Document::new(DocState::new(Extent::new(8, 8), Depth::F32));
    let model = typography::TextModel::point("a", "sans-serif", 12.);
    let id = doc
        .apply(DocOp::AddText {
            parent: None,
            index: 0,
            name: "type".into(),
            model: model.clone(),
            transform: Affine::IDENTITY,
        })
        .unwrap()
        .created[0];
    let before = doc.history().current();
    assert!(
        doc.apply(DocOp::Batch(vec![
            DocOp::EditTextRuns {
                id,
                range: 0..1,
                runs: vec![]
            },
            DocOp::SetVectorMask {
                id,
                mask: Some(VectorMask {
                    density: 2.,
                    ..Default::default()
                })
            }
        ]))
        .is_err()
    );
    assert_eq!(before, doc.history().current());
    assert!(
        matches!(&doc.state().find(id).unwrap().kind, LayerKind::Text { model: text, .. } if text == &model)
    );
    let mut props = doc.state().find(id).unwrap().props.clone();
    props.locks.position = true;
    doc.apply(DocOp::SetProps {
        id,
        props: props.clone(),
    })
    .unwrap();
    assert!(
        doc.apply(DocOp::EditText {
            id,
            model: model.clone(),
            transform: Affine::scale_translate(1., 1., 1., 0.)
        })
        .is_err()
    );
    doc.apply(DocOp::EditTextRuns {
        id,
        range: 1..1,
        runs: vec![],
    })
    .unwrap();
    props.locks.pixels = true;
    doc.apply(DocOp::SetProps { id, props }).unwrap();
    assert!(doc.apply(DocOp::ConvertToPixels { id }).is_err());
}

#[test]
fn legacy_native_text_and_polygon_mask_load() {
    let mut state = DocState::new(Extent::new(8, 8), Depth::F32);
    state.root.push(std::sync::Arc::new(compositor::Layer::new(
        "old",
        LayerKind::Text {
            model: typography::TextModel::point("new", "sans-serif", 12.),
            transform: Affine::IDENTITY,
        },
    )));
    let bytes = compositor::format::to_bytes(&state).unwrap();
    let footer = bytes.len() - 24;
    let offset = u64::from_le_bytes(bytes[footer..footer + 8].try_into().unwrap()) as usize;
    let mut manifest: serde_json::Value =
        serde_json::from_slice(&zstd::stream::decode_all(&bytes[offset..footer]).unwrap()).unwrap();
    let layer = &mut manifest["document"]["layers"][0];
    // The manifest's document key is asserted below so fixture mutation cannot silently miss it.
    assert!(layer.is_object());
    layer["kind"] = serde_json::json!({"type":"text", "text":"legacy", "font":"sans-serif", "size":18., "color":[1.,0.,0.]});
    layer["vector_mask"] = serde_json::json!({"enabled":true, "path":[[0,0],[8,0],[8,8]]});
    let compressed =
        zstd::stream::encode_all(serde_json::to_vec(&manifest).unwrap().as_slice(), 1).unwrap();
    let mut upgraded = bytes[..offset].to_vec();
    upgraded.extend_from_slice(&compressed);
    upgraded.extend_from_slice(&(offset as u64).to_le_bytes());
    upgraded.extend_from_slice(&(compressed.len() as u64).to_le_bytes());
    upgraded.extend_from_slice(b"TSRDEND\0");
    let loaded = compositor::format::from_bytes(&upgraded).unwrap();
    assert!(
        matches!(&loaded.root[0].kind, LayerKind::Text { model, .. } if model.runs[0].text == "legacy" && model.runs[0].color == [255,0,0,255])
    );
    assert_eq!(
        loaded.root[0].vector_mask.as_ref().unwrap().path.subpaths[0]
            .anchors
            .len(),
        3
    );
}

#[test]
fn full_shape_paint_and_geometry_serialization_validates() {
    let gradient = vector::Gradient::new(
        vector::GradientKind::Radial,
        vector::Point::new(0., 0.),
        vector::Point::new(5., 5.),
        vec![
            vector::Stop {
                position: 0.,
                color: [1., 0., 0., 1.],
            },
            vector::Stop {
                position: 1.,
                color: [0., 0., 1., 0.5],
            },
        ],
        true,
    )
    .unwrap();
    let model = vector::ShapeModel::from_shape(
        vector::Shape::Ellipse {
            center: vector::Point::new(3., 4.),
            radii: vector::Vec2::new(2., 3.),
        },
        Some(vector::Fill::Gradient(gradient)),
        Some((
            vector::Stroke {
                width: 2.,
                alignment: vector::Alignment::Outside,
                cap: vector::LineCap::Round,
                join: vector::LineJoin::Bevel,
                dashes: vec![1., 3.],
                dash_offset: 1.5,
                ..Default::default()
            },
            vector::Fill::Solid([0., 1., 0., 1.]),
        )),
    )
    .unwrap();
    let json = serde_json::to_value(&model).unwrap();
    let decoded: vector::ShapeModel = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(model, decoded);
    decoded.validate().unwrap();
    let mut malformed = json;
    malformed["fill"]["Gradient"]["stops"] = serde_json::json!([]);
    let decoded: vector::ShapeModel = serde_json::from_value(malformed).unwrap();
    assert!(decoded.validate().is_err());
}

#[test]
fn live_shape_parameter_edits_regenerate_geometry() {
    let mut doc = Document::new(DocState::new(Extent::new(16, 16), Depth::F32));
    let mut model = vector::ShapeModel {
        live_shape: Some(vector::Shape::Rectangle {
            rect: vector::Rect::new(1., 1., 5., 5.),
            radii: [0.; 4],
        }),
        fill: Some(vector::Fill::Solid([1., 0., 0., 1.])),
        ..Default::default()
    };
    let id = doc
        .apply(DocOp::AddShape {
            parent: None,
            index: 0,
            name: "Live rectangle".into(),
            model: model.clone(),
            transform: Affine::IDENTITY,
        })
        .unwrap()
        .created[0];
    let comp = compositor::Compositor::new(1 << 20);
    let p = comp.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(p[(3 * 16 + 3) * 4 + 3], 1.);
    model.live_shape = Some(vector::Shape::Rectangle {
        rect: vector::Rect::new(1., 1., 10., 10.),
        radii: [0.; 4],
    });
    doc.apply(DocOp::EditShape {
        id,
        model,
        transform: Affine::IDENTITY,
    })
    .unwrap();
    let p = comp.render_level_rgba(&doc, 0).unwrap().1;
    assert_eq!(p[(8 * 16 + 8) * 4 + 3], 1.);
    assert!(doc.undo());
    assert_eq!(
        comp.render_level_rgba(&doc, 0).unwrap().1[(8 * 16 + 8) * 4 + 3],
        0.
    );
}
