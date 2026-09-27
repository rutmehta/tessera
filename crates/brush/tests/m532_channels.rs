use compositor::channels::{ChannelId, ChannelKind, DocumentChannel};
use compositor::{Depth, DocOp, DocState, Document, Layer, LayerId, Mask, PaintTarget, Raster};
use engine_api::{
    document::{BrushParams, StrokePoint},
    tile::Extent,
};

#[test]
fn document_brush_destinations_paint_scalar_planes_and_undo() {
    for depth in [Depth::U8, Depth::U16, Depth::F32] {
        let mut doc = Document::new(DocState::new(Extent::new(8, 8), depth));
        let mut layer = Layer::pixel("pixels", doc.state().canvas, depth);
        layer.mask = Some(Mask::reveal_all(doc.state().canvas, depth));
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer,
        })
        .unwrap();
        let layer = doc.state().layer_ids()[0];
        for kind in [
            ChannelKind::Alpha,
            ChannelKind::Spot {
                color: [1.0, 0.0, 0.0],
                solidity: 0.8,
            },
        ] {
            doc.apply(DocOp::AddChannel {
                channel: DocumentChannel {
                    id: ChannelId(0),
                    name: "plane".into(),
                    kind,
                    raster: Raster::new(doc.state().canvas, 1, depth, 1.0),
                },
            })
            .unwrap();
        }
        for (id, target) in [
            (layer, PaintTarget::Mask),
            (LayerId(0), PaintTarget::Channel(ChannelId(1))),
            (LayerId(0), PaintTarget::Channel(ChannelId(2))),
        ] {
            let op = brush::api::paint_document_stroke(
                doc.state(),
                id,
                target,
                &BrushParams {
                    size: 6.0,
                    hardness: 1.0,
                    color: [0.0; 3],
                    ..Default::default()
                },
                &[StrokePoint {
                    x: 4.0,
                    y: 4.0,
                    pressure: 1.0,
                }],
                42,
            )
            .unwrap()
            .unwrap();
            doc.apply(op).unwrap();
            assert!(target.raster(doc.state(), id).unwrap().pixel(4, 4)[0] < 0.1);
            assert!(doc.undo());
            assert_eq!(target.raster(doc.state(), id).unwrap().pixel(4, 4)[0], 1.0);
            assert!(doc.redo());
            assert!(target.raster(doc.state(), id).unwrap().pixel(4, 4)[0] < 0.1);
        }
    }
}
