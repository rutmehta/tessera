use compositor::{
    Affine, Depth, DocState, Document, Layer, LayerKind, Rect, SmartObject, VectorMask, psd, vector,
};
use engine_api::{EngineError, jobs::CancellationToken, tile::Extent};
use std::sync::Arc;

fn colored_state(extent: Extent, depth: Depth) -> DocState {
    let mut state = DocState::new(extent, depth);
    let mut layer = Layer::pixel("red", extent, depth);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(extent), 1, |_, _, pixel| {
            *pixel = [1.0, 0.0, 0.0, 0.5]
        })
        .unwrap();
    state.root.push(Arc::new(layer));
    state
}

fn sample(depth: Depth, value: f32) -> Vec<u8> {
    match depth {
        Depth::U8 => vec![(value * 255.0).round() as u8],
        Depth::U16 => ((value * 65535.0).round() as u16).to_be_bytes().to_vec(),
        Depth::F32 => value.to_be_bytes().to_vec(),
    }
}

#[test]
fn cancellable_and_generic_document_wrappers_agree_at_each_depth() {
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let doc = Document::new(colored_state(Extent::new(2, 3), depth));
        let expected = psd::to_psd(&doc).unwrap();
        let actual = psd::to_psd_with_cancel(&doc, &CancellationToken::new()).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(actual.channels, 4);
        assert!(actual.layer_section.merged_alpha);
        let composite: Vec<u8> = [1.0, 0.0, 0.0, 0.5]
            .into_iter()
            .flat_map(|v| sample(depth, v).repeat(6))
            .collect();
        assert_eq!(actual.composite, composite);
    }
}

#[test]
fn placed_embedded_document_keeps_pixels_and_linked_source() {
    let extent = Extent::new(2, 1);
    let child = colored_state(extent, Depth::U8);
    let mut parent = DocState::new(extent, Depth::U8);
    parent.root.push(Arc::new(Layer::new(
        "placed",
        LayerKind::SmartObject(SmartObject::new(child, Affine::IDENTITY)),
    )));
    let out = psd::to_psd_with_cancel(&Document::new(parent), &CancellationToken::new()).unwrap();
    assert_eq!(out.channels, 4);
    assert_eq!(out.composite[..2], [255, 255]);
    assert!(out.layer_section.layers[0].info(b"SoLd").is_some());
    assert!(
        out.layer_section
            .additional
            .iter()
            .any(|block| block.key == *b"lnk2")
    );
}

#[test]
fn cancellable_shape_vector_mask_exports_raster_bridge() {
    let extent = Extent::new(2, 2);
    let shape = vector::Shape::Rectangle {
        rect: vector::Rect::new(0.0, 0.0, 2.0, 2.0),
        radii: [0.0; 4],
    };
    let model = vector::ShapeModel::from_shape(
        shape,
        Some(vector::Fill::Solid([1.0, 0.0, 0.0, 1.0])),
        None,
    )
    .unwrap();
    let mut layer = Layer::new(
        "shape",
        LayerKind::Shape {
            model,
            transform: Affine::IDENTITY,
        },
    );
    layer.vector_mask = Some(VectorMask {
        path: vector::Shape::Rectangle {
            rect: vector::Rect::new(0.0, 0.0, 1.0, 2.0),
            radii: [0.0; 4],
        }
        .path()
        .unwrap(),
        ..Default::default()
    });
    let mut state = DocState::new(extent, Depth::U8);
    state.root.push(Arc::new(layer));
    let doc = Document::new(state);
    let out = psd::to_psd_with_cancel(&doc, &CancellationToken::new()).unwrap();
    let record = &out.layer_section.layers[0];
    assert!(record.info(b"tvMk").is_some());
    let mask = record.channels.iter().find(|c| c.id == -2).unwrap();
    assert_eq!(mask.data.len(), 4);
    assert!(mask.data[0] > mask.data[1]);
    assert_eq!(out, psd::to_psd(&doc).unwrap());
}

#[test]
fn pre_cancelled_psd_conversion_returns_cancelled_and_retry_succeeds() {
    let doc = Document::new(DocState::new(Extent::new(2, 3), Depth::U8));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        psd::to_psd_with_cancel(&doc, &cancel),
        Err(EngineError::Cancelled)
    );
    assert!(psd::to_psd_with_cancel(&doc, &CancellationToken::new()).is_ok());
}

#[test]
fn cancellable_psd_preserves_retained_resource_bytes() {
    let native = Document::new(DocState::new(Extent::new(2, 2), Depth::U8));
    let mut source = psd::to_psd(&native).unwrap();
    source
        .resources
        .push(::psd::ImageResource::new(4000, vec![9, 8, 7, 6]));
    let imported = Document::from_psd(source).unwrap();
    let expected = psd::to_psd(&imported).unwrap();
    let actual = psd::to_psd_with_cancel(&imported, &CancellationToken::new()).unwrap();
    assert_eq!(actual, expected);
    assert_eq!(
        actual.resources.iter().find(|r| r.id == 4000).unwrap().data,
        vec![9, 8, 7, 6]
    );
}
