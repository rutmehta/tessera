use compositor::channels::{ChannelId, ChannelKind, DocumentChannel};
use compositor::{Depth, DocOp, DocState, Document, Raster};
use engine_api::tile::Extent;

#[test]
fn channels_follow_history() {
    let mut doc = Document::new(DocState::new(Extent::new(2, 1), Depth::U8));
    doc.apply(DocOp::AddChannel {
        channel: DocumentChannel {
            id: ChannelId(0),
            name: "Saved".into(),
            kind: ChannelKind::Alpha,
            raster: Raster::new(Extent::new(2, 1), 1, Depth::F32, 0.5),
        },
    })
    .unwrap();
    let id = doc.state().channels[0].id;
    doc.apply(DocOp::RenameChannel {
        id,
        name: "New".into(),
    })
    .unwrap();
    assert!(doc.undo());
    assert_eq!(doc.state().channels[0].name, "Saved");
    assert!(doc.undo());
    assert!(doc.state().channels.is_empty());
    assert!(doc.redo());
    assert_eq!(doc.state().channels[0].id, id);
    let bytes = compositor::format::to_bytes(doc.state()).unwrap();
    let loaded = compositor::format::from_bytes(&bytes).unwrap();
    assert_eq!(loaded.channels.len(), 1);
    assert_eq!(loaded.channels[0].raster.default_value(), 0.5);
    assert_eq!(loaded.next_channel_id, doc.state().next_channel_id);
}

#[test]
fn edit_delete_spot_history_storage_and_composite() {
    let mut doc = Document::new(DocState::new(Extent::new(257, 2), Depth::U16));
    let renderer = compositor::Compositor::new(16 << 20);
    let before = renderer.render_level_rgba(&doc, 0).unwrap();
    let mut raster = Raster::new(doc.state().canvas, 1, Depth::F32, 0.25);
    raster
        .edit_region(compositor::Rect::new(255, 0, 257, 2), 1, |x, _, p| {
            p[0] = if x == 256 { 0.123456 } else { 1.0 }
        })
        .unwrap();
    doc.apply(DocOp::AddChannel {
        channel: DocumentChannel {
            id: ChannelId(0),
            name: "墨".into(),
            kind: ChannelKind::Spot {
                color: [0.1, 0.2, 0.3],
                solidity: 0.7,
            },
            raster,
        },
    })
    .unwrap();
    let original = doc.state().channels[0].clone();
    assert_eq!(renderer.render_level_rgba(&doc, 0).unwrap(), before);
    let loaded =
        compositor::format::from_bytes(&compositor::format::to_bytes(doc.state()).unwrap())
            .unwrap();
    assert_eq!(loaded.channels[0].kind, original.kind);
    assert_eq!(loaded.channels[0].name, original.name);
    for x in 0..257 {
        assert_eq!(
            loaded.channels[0].raster.pixel(x, 0),
            original.raster.pixel(x, 0)
        );
    }
    let mut edited = original.clone();
    edited
        .raster
        .edit_region(compositor::Rect::new(256, 0, 257, 1), 2, |_, _, p| {
            p[0] = 0.8
        })
        .unwrap();
    edited.kind = ChannelKind::Alpha;
    doc.apply(DocOp::EditChannel { channel: edited }).unwrap();
    assert_eq!(original.raster.pixel(256, 0)[0], 0.123456);
    doc.apply(DocOp::DeleteChannel { id: original.id }).unwrap();
    assert!(doc.state().channels.is_empty());
    assert!(doc.undo());
    assert_eq!(doc.state().channels[0].raster.pixel(256, 0)[0], 0.8);
    assert!(doc.undo());
    assert_eq!(doc.state().channels[0].kind, original.kind);
    assert_eq!(doc.state().channels[0].raster.pixel(256, 0)[0], 0.123456);
    assert!(doc.redo());
    assert!(doc.redo());
    assert!(doc.state().channels.is_empty());
}

#[test]
fn invalid_channel_batches_are_atomic() {
    let mut doc = Document::new(DocState::new(Extent::new(2, 1), Depth::U8));
    let channel = DocumentChannel {
        id: ChannelId(1),
        name: "a".into(),
        kind: ChannelKind::Alpha,
        raster: Raster::new(doc.state().canvas, 1, Depth::U8, 0.0),
    };
    let head = doc.history().current();
    assert!(
        doc.apply(DocOp::Batch(vec![
            DocOp::AddChannel {
                channel: channel.clone()
            },
            DocOp::AddChannel {
                channel: channel.clone()
            }
        ]))
        .is_err()
    );
    assert_eq!(doc.history().current(), head);
    assert!(doc.state().channels.is_empty());
    for raster in [
        Raster::new(Extent::new(1, 1), 1, Depth::U8, 0.0),
        Raster::new(doc.state().canvas, 4, Depth::U8, 0.0),
    ] {
        assert!(
            doc.apply(DocOp::AddChannel {
                channel: DocumentChannel {
                    raster,
                    ..channel.clone()
                }
            })
            .is_err()
        );
    }
    for solidity in [f32::NAN, -0.1, 1.1] {
        assert!(
            doc.apply(DocOp::AddChannel {
                channel: DocumentChannel {
                    kind: ChannelKind::Spot {
                        color: [0.0; 3],
                        solidity
                    },
                    ..channel.clone()
                }
            })
            .is_err()
        );
    }
    assert!(
        doc.apply(DocOp::DeleteChannel { id: ChannelId(99) })
            .is_err()
    );
    assert_eq!(doc.history().current(), head);
}
