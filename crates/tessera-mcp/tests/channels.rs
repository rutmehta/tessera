use compositor::{Depth, Raster};
use engine_api::{
    document::{ChannelKind, SelectionMode},
    id::{ChannelId, DocumentId},
    tile::Extent,
    tools::{
        DocumentToolCall as Call, DocumentToolOutput as Out, DocumentToolRequest,
        DocumentToolResponse,
    },
};
use tessera_mcp::Console;

fn run(c: &mut Console, call: Call) -> Out {
    match c.execute_document(DocumentToolRequest {
        call,
        rationale: Some("channel wiring".into()),
        group: None,
        expect_head: None,
    }) {
        DocumentToolResponse::Ok(o) => o,
        DocumentToolResponse::Error(e) => panic!("{e:?}"),
    }
}
fn fixture() -> (tempfile::TempDir, Console, DocumentId) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.png");
    image::RgbImage::new(4, 3).save(&path).unwrap();
    let mut c = Console::open(dir.path().join("app")).unwrap();
    let doc = c.open_document(path).unwrap();
    (dir, c, doc)
}

#[test]
fn add_summary_load_edit_rename_delete_and_undo() {
    let (_dir, mut c, doc) = fixture();
    let raster = c
        .documents_mut()
        .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, 0.5))
        .unwrap();
    let out = run(
        &mut c,
        Call::AddChannel {
            document: doc,
            name: "Mask".into(),
            kind: ChannelKind::Alpha,
            raster: raster.clone(),
        },
    );
    let Out::DocumentEdited {
        channel: Some(channel),
        selection,
        ..
    } = out
    else {
        panic!("{out:?}")
    };
    assert_eq!(selection.unwrap().0, channel.0);
    let summary = c.describe_document(doc, 4, None).unwrap().summary;
    assert_eq!(summary["summary"]["channels"][0]["id"], channel.0);
    assert_eq!(summary["selection"]["active"], false);
    run(
        &mut c,
        Call::LoadChannelAsSelection {
            document: doc,
            channel,
            mode: SelectionMode::Replace,
        },
    );
    let state = c.documents().session(doc).unwrap().document().state();
    assert_eq!(
        selection::Mask::from_raster(state.selection.as_ref().unwrap())
            .unwrap()
            .data(),
        &[0.5; 12]
    );
    run(
        &mut c,
        Call::RenameChannel {
            document: doc,
            channel,
            name: "Spot".into(),
        },
    );
    run(
        &mut c,
        Call::EditChannel {
            document: doc,
            channel,
            kind: Some(ChannelKind::Spot {
                display_rgb: [1., 0., 0.],
                solidity: 0.8,
            }),
            raster: None,
        },
    );
    assert!(
        c.documents()
            .session(doc)
            .unwrap()
            .saved_selections()
            .is_empty()
    );
    run(
        &mut c,
        Call::LoadChannelAsSelection {
            document: doc,
            channel,
            mode: SelectionMode::Intersect,
        },
    );
    run(
        &mut c,
        Call::DeleteChannel {
            document: doc,
            channel,
        },
    );
    assert!(
        c.describe_document(doc, 4, None).unwrap().summary["summary"]["channels"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    c.documents_mut().undo(doc).unwrap();
    let s = c.documents().session(doc).unwrap();
    assert_eq!(s.document().state().channels[0].name, "Spot");
    assert_eq!(s.history().entries.len(), 6);
    assert!(
        s.history()
            .entries
            .iter()
            .all(|e| e.meta.rationale.as_deref() == Some("channel wiring"))
    );
}

#[test]
fn channel_validation_is_atomic_and_staging_is_immutable() {
    let (_dir, mut c, doc) = fixture();
    assert!(
        c.documents_mut()
            .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, f32::NAN))
            .is_err()
    );
    let raster = c
        .documents_mut()
        .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, 0.5))
        .unwrap();
    assert_eq!(
        raster,
        c.documents_mut()
            .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, 0.5))
            .unwrap()
    );
    let mut bad = raster.clone();
    bad.extent = Extent::new(3, 4);
    for call in [
        Call::AddChannel {
            document: doc,
            name: "bad".into(),
            kind: ChannelKind::Alpha,
            raster: bad,
        },
        Call::AddChannel {
            document: doc,
            name: "bad".into(),
            kind: ChannelKind::Spot {
                display_rgb: [2., 0., 0.],
                solidity: 0.5,
            },
            raster,
        },
        Call::LoadChannelAsSelection {
            document: doc,
            channel: ChannelId(999),
            mode: SelectionMode::Replace,
        },
    ] {
        assert!(matches!(
            c.execute_document(DocumentToolRequest {
                call,
                rationale: None,
                group: None,
                expect_head: None
            }),
            DocumentToolResponse::Error(_)
        ));
    }
    let s = c.documents().session(doc).unwrap();
    assert!(s.history().entries.is_empty());
    assert!(s.document().state().channels.is_empty());
}

#[test]
fn channel_ids_are_not_reused_on_history_branches() {
    let (_dir, mut c, doc) = fixture();
    let raster = c
        .documents_mut()
        .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, 0.5))
        .unwrap();
    let call = Call::AddChannel {
        document: doc,
        name: "Mask".into(),
        kind: ChannelKind::Alpha,
        raster,
    };
    let Out::DocumentEdited {
        channel: Some(first),
        ..
    } = run(&mut c, call.clone())
    else {
        panic!()
    };
    c.documents_mut().undo(doc).unwrap();
    let Out::DocumentEdited {
        channel: Some(second),
        ..
    } = run(&mut c, call)
    else {
        panic!()
    };
    assert!(second.0 > first.0);
    c.documents_mut().undo(doc).unwrap();
    let Out::DocumentEdited {
        selection: Some(saved),
        ..
    } = run(
        &mut c,
        Call::SetPixelSelection {
            document: doc,
            shape: engine_api::document::SelectionShape::All,
            mode: SelectionMode::Replace,
            feather: 0.,
            save_as: Some("saved".into()),
        },
    )
    else {
        panic!()
    };
    assert!(saved.0 > second.0);
}

#[test]
fn recorded_channels_bind_to_newly_created_ids() {
    let (_dir, mut c, doc) = fixture();
    let raster = c
        .documents_mut()
        .stage_channel_raster(Raster::new(Extent::new(4, 3), 1, Depth::F32, 0.5))
        .unwrap();
    c.start_recording("channels").unwrap();
    let Out::DocumentEdited {
        channel: Some(channel),
        ..
    } = run(
        &mut c,
        Call::AddChannel {
            document: doc,
            name: "Mask".into(),
            kind: ChannelKind::Alpha,
            raster,
        },
    )
    else {
        panic!()
    };
    run(
        &mut c,
        Call::RenameChannel {
            document: doc,
            channel,
            name: "Renamed".into(),
        },
    );
    let action = c.stop_recording().unwrap();
    let report = c.play_action(&action, &[doc]).unwrap();
    assert!(report.ok(), "{report:?}");
    let state = c.documents().session(doc).unwrap().document().state();
    assert_eq!(state.channels[1].name, "Renamed");
}
