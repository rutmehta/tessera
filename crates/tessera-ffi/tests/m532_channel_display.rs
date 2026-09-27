//! M5-32: explicit alpha display metadata reaches the Channels panel.
#![cfg(target_os = "macos")]

use compositor::channels::{ChannelId, ChannelKind, DocumentChannel};
use compositor::{Depth, DocOp, DocState, Document, Raster};
use engine_api::tile::Extent;
use tessera_ffi::{DocChannelKind, Engine};

#[test]
fn saved_channel_display_fields_reach_session_records() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    for selected in [false, true] {
        let mut doc = Document::new(DocState::new(Extent::new(8, 8), Depth::U8));
        for (name, kind) in [
            ("legacy", ChannelKind::Alpha),
            (
                "explicit",
                ChannelKind::AlphaDisplay {
                    color: [0.2, 0.4, 0.6],
                    opacity: 0.37,
                    selected,
                },
            ),
            (
                "spot",
                ChannelKind::Spot {
                    color: [0.1, 0.3, 0.5],
                    solidity: 0.75,
                },
            ),
        ] {
            doc.apply(DocOp::AddChannel {
                channel: DocumentChannel {
                    id: ChannelId(0),
                    name: name.into(),
                    kind,
                    raster: Raster::new(doc.state().canvas, 1, Depth::U8, 0.0),
                },
            })
            .unwrap();
        }
        let path = dir.path().join(format!("channels-{selected}.tessera-doc"));
        std::fs::write(&path, compositor::format::to_bytes(doc.state()).unwrap()).unwrap();
        let session = engine
            .clone()
            .open_document(path.to_string_lossy().into_owned())
            .unwrap();
        let channels = session.document_channels().unwrap();
        assert_eq!(channels.len(), 3);
        assert_eq!(channels[0].kind, DocChannelKind::Alpha);
        assert_eq!(channels[0].opacity, 0.5);
        assert!(!channels[0].selected_areas);
        assert_eq!(channels[1].kind, DocChannelKind::Alpha);
        assert_eq!(
            [
                channels[1].color.r,
                channels[1].color.g,
                channels[1].color.b
            ],
            [0.2, 0.4, 0.6]
        );
        assert_eq!(channels[1].opacity, 0.37);
        assert_eq!(channels[1].selected_areas, selected);
        assert_eq!(channels[2].kind, DocChannelKind::Spot);
        assert_eq!(channels[2].opacity, 0.75);
        assert!(!channels[2].selected_areas);
    }
}
