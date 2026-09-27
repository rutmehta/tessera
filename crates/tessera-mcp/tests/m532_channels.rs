use compositor::{Depth, Raster};
use engine_api::{
    document::{BrushParams, ChannelKind, StrokePoint, StrokeTarget},
    id::{DocumentId, LayerId},
    tile::Extent,
    tools::{DocumentToolCall as Call, DocumentToolRequest, DocumentToolResponse},
};
use tessera_mcp::Console;

fn run(c: &mut Console, call: Call) {
    let result = c.execute_document(DocumentToolRequest {
        call,
        rationale: None,
        group: None,
        expect_head: None,
    });
    assert!(matches!(result, DocumentToolResponse::Ok(_)), "{result:?}");
}
fn paint(c: &mut Console, document: DocumentId, channel: engine_api::id::ChannelId) {
    run(
        c,
        Call::PaintStroke {
            document,
            layer: LayerId(0),
            target: StrokeTarget::Channel(channel),
            points: vec![StrokePoint {
                x: 2.0,
                y: 2.0,
                pressure: 1.0,
            }],
            brush: BrushParams {
                size: 4.0,
                hardness: 1.0,
                color: [1.0; 3],
                ..Default::default()
            },
        },
    );
}
#[test]
fn channel_display_summary_paint_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.png");
    image::RgbImage::new(4, 4).save(&path).unwrap();
    let mut c = Console::open(dir.path().join("app")).unwrap();
    let doc = c.open_document(path).unwrap();
    for kind in [
        ChannelKind::AlphaDisplay {
            display_rgb: [0.0, 1.0, 0.0],
            opacity: 0.25,
            selected: true,
        },
        ChannelKind::Spot {
            display_rgb: [1.0, 0.0, 0.0],
            solidity: 0.5,
        },
    ] {
        let raster = c
            .documents_mut()
            .stage_channel_raster(Raster::new(Extent::new(4, 4), 1, Depth::F32, 0.0))
            .unwrap();
        run(
            &mut c,
            Call::AddChannel {
                document: doc,
                name: "display".into(),
                kind: kind.clone(),
                raster,
            },
        );
        let summary = c.describe_document(doc, 4, None).unwrap().summary;
        let row = summary["summary"]["channels"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(row["kind"], serde_json::to_value(&kind).unwrap());
        let channel = engine_api::id::ChannelId(row["id"].as_u64().unwrap());
        let before = c
            .documents()
            .session(doc)
            .unwrap()
            .document()
            .state()
            .clone();
        paint(&mut c, doc, channel);
        let state = c.documents().session(doc).unwrap().document().state();
        assert!(state.channels.last().unwrap().raster.pixel(2, 2)[0] > 0.9);
        assert_eq!(before.channels.last().unwrap().raster.pixel(2, 2)[0], 0.0);
        c.documents_mut().undo(doc).unwrap();
        assert_eq!(
            c.documents()
                .session(doc)
                .unwrap()
                .document()
                .state()
                .channels
                .last()
                .unwrap()
                .raster
                .pixel(2, 2)[0],
            0.0
        );
    }
}
