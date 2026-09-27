use compositor::channels::{ChannelId, ChannelKind, DocumentChannel};
use compositor::{Depth, DocOp, DocState, Document, Raster};
use engine_api::tile::Extent;

#[test]
fn channel_paint_is_copy_on_write_undoable_and_not_rgb_damage() {
    use compositor::{LayerId, PaintTarget, Rect, paint_op};
    for kind in [
        ChannelKind::Alpha,
        ChannelKind::Spot {
            color: [0.0, 1.0, 0.0],
            solidity: 0.75,
        },
    ] {
        let mut doc = Document::new(DocState::new(Extent::new(257, 2), Depth::F32));
        doc.apply(DocOp::AddChannel {
            channel: DocumentChannel {
                id: ChannelId(0),
                name: "paint".into(),
                kind,
                raster: Raster::new(doc.state().canvas, 1, Depth::F32, 0.25),
            },
        })
        .unwrap();
        let before = doc.state().clone();
        let id = before.channels[0].id;
        let rect = Rect::new(255, 0, 257, 1);
        let op = paint_op(
            doc.state(),
            LayerId(0),
            PaintTarget::Channel(id),
            rect,
            |_, _, p| p[0] = 0.75,
        )
        .unwrap();
        doc.apply(op).unwrap();
        assert_eq!(before.channels[0].raster.pixel(256, 0)[0], 0.25);
        assert_eq!(doc.state().channels[0].raster.pixel(256, 0)[0], 0.75);
        assert_eq!(doc.state().channels[0].raster.pixel(254, 0)[0], 0.25);
        assert!(doc.undo());
        assert_eq!(doc.state().channels[0].raster.pixel(256, 0)[0], 0.25);
        assert!(doc.redo());
        assert_eq!(doc.state().channels[0].raster.pixel(256, 0)[0], 0.75);
        assert!(
            paint_op(
                doc.state(),
                LayerId(0),
                PaintTarget::Channel(ChannelId(999)),
                rect,
                |_, _, _| {}
            )
            .is_err()
        );
    }
}

#[test]
fn invalid_alpha_display_is_atomic() {
    let mut doc = Document::new(DocState::new(Extent::new(2, 1), Depth::U8));
    for v in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
        for color_invalid in [false, true] {
            let head = doc.history().current();
            let kind = ChannelKind::AlphaDisplay {
                color: if color_invalid {
                    [v, 0.0, 0.0]
                } else {
                    [1.0, 0.0, 0.0]
                },
                opacity: if color_invalid { 0.5 } else { v },
                selected: false,
            };
            assert!(
                doc.apply(DocOp::AddChannel {
                    channel: DocumentChannel {
                        id: ChannelId(0),
                        name: "invalid".into(),
                        kind,
                        raster: Raster::new(doc.state().canvas, 1, Depth::U8, 0.0),
                    }
                })
                .is_err()
            );
            assert_eq!(doc.history().current(), head);
        }
    }
}

#[test]
fn alpha_display_roundtrips_native_and_both_psd_resources() {
    for selected in [false, true] {
        let kind: ChannelKind = serde_json::from_value(serde_json::json!({
            "AlphaDisplay": {"color": [0.0, 1.0, 0.0], "opacity": 0.37, "selected": selected}
        }))
        .expect("explicit alpha display metadata must be supported");
        let mut doc = Document::new(DocState::new(Extent::new(2, 1), Depth::U8));
        doc.apply(DocOp::AddChannel {
            channel: DocumentChannel {
                id: ChannelId(0),
                name: "Alpha λ".into(),
                kind: kind.clone(),
                raster: Raster::new(doc.state().canvas, 1, Depth::U8, 0.25),
            },
        })
        .unwrap();
        let native =
            compositor::format::from_bytes(&compositor::format::to_bytes(doc.state()).unwrap())
                .unwrap();
        assert_eq!(native.channels[0].kind, kind);
        let imported = compositor::psd::ImportedPsd::from_state(native).unwrap();
        let output = compositor::psd::to_psd(&imported).unwrap();
        for resource in [1007, 1077] {
            let mut output = output.clone();
            output
                .resources
                .retain(|r| !matches!(r.id, 1007 | 1077) || r.id == resource);
            let info = output.channel_display_info().unwrap().unwrap()[0];
            assert_eq!(info.mode, u8::from(selected));
            assert_eq!(info.color, [0, 65535, 0, 0]);
            assert_eq!(info.opacity, 37);
            let parsed = psd::PsdDocument::read(&output.write().unwrap()).unwrap();
            assert_eq!(
                compositor::psd::from_psd(&parsed).unwrap().channels[0].kind,
                kind
            );
        }
    }
}
