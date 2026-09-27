use engine_api::{
    id::LayerId,
    tools::{DocumentToolRequest, DocumentToolResponse},
};
use serde_json::json;
use tessera_mcp::Console;

#[test]
fn editable_text_dispatch_history_summaries_and_undo() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    image::RgbaImage::from_pixel(32, 32, image::Rgba([0, 0, 0, 0]))
        .save(&input)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let document = console.open_document(input).unwrap();
    let request: DocumentToolRequest = serde_json::from_value(json!({"tool":"add_text","document":document,"model":{"runs":[{"text":"Hello","family":"sans-serif"}]},"rationale":"caption"})).unwrap();
    let mut recorder = tessera_mcp::actions::Recorder::new("Caption");
    let result = console.execute_document(request.clone());
    if let DocumentToolResponse::Ok(output) = &result {
        recorder.record_document(&request, output);
    }
    assert!(matches!(result, DocumentToolResponse::Ok(_)), "{result:?}");
    let session = console.documents().session(document).unwrap();
    assert_eq!(session.history().entries.len(), 1);
    assert_eq!(session.history().entries[0].action.command, "add_text");
    assert!(matches!(
        session.document().state().find(LayerId(2)).unwrap().kind,
        compositor::LayerKind::Text { .. }
    ));
    let request: DocumentToolRequest = serde_json::from_value(json!({"tool":"edit_text_runs","document":document,"layer":2,"range":{"start":0,"end":1},"runs":[{"text":"Updated"}]})).unwrap();
    let result = console.execute_document(request.clone());
    let DocumentToolResponse::Ok(output) = result else {
        panic!("{result:?}")
    };
    recorder.record_document(&request, &output);
    let recorded = recorder.finish();
    assert_eq!(
        recorded.steps[1].action.params["layer"],
        json!({"$layer":0})
    );
    recorded.validate().unwrap();
    let request =
        serde_json::from_value(json!({"tool":"list_layers","document":document})).unwrap();
    let layers = serde_json::to_value(console.execute_document(request)).unwrap();
    assert_eq!(layers["ok"]["layers"][1]["text"]["preview"], "Updated");
    console.documents_mut().undo(document).unwrap();
    let session = console.documents().session(document).unwrap();
    let compositor::LayerKind::Text { model, .. } =
        &session.document().state().find(LayerId(2)).unwrap().kind
    else {
        panic!()
    };
    assert_eq!(model.runs[0].text, "Hello");
}

#[test]
fn shape_mask_conversion_and_failed_edit_are_atomic() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("input.png");
    image::RgbaImage::from_pixel(16, 16, image::Rgba([0, 0, 0, 0]))
        .save(&input)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let document = console.open_document(input).unwrap();
    let anchors: Vec<_> = [[2,2],[12,2],[12,12],[2,12]].into_iter().map(|[x,y]| json!({"point":{"x":x,"y":y},"incoming":{"x":x,"y":y},"outgoing":{"x":x,"y":y}})).collect();
    let path = json!({"fill_rule":"NonZero","subpaths":[{"anchors":anchors,"closed":true}]});
    let model = json!({"path":path,"fill":{"Solid":[1,0,0,1]}});
    for args in [
        json!({"tool":"add_shape","model":model}),
        json!({"tool":"edit_shape","layer":2,"model":model}),
        json!({"tool":"set_vector_mask","layer":2,"mask":{"path":path}}),
        json!({"tool":"remove_vector_mask","layer":2}),
        json!({"tool":"convert_to_pixels","layer":2}),
    ] {
        let mut args = args;
        args["document"] = json!(document);
        let request = serde_json::from_value(args).unwrap();
        let result = console.execute_document(request);
        assert!(matches!(result, DocumentToolResponse::Ok(_)), "{result:?}");
    }
    let session = console.documents().session(document).unwrap();
    assert_eq!(session.history().entries.len(), 5);
    let layer = session.document().state().find(LayerId(2)).unwrap();
    assert!(matches!(layer.kind, compositor::LayerKind::Pixel(_)));
    assert!(layer.raster().unwrap().pixel(5, 5)[3] > 0.9);
    let bad = serde_json::from_value(json!({"tool":"edit_text_runs","document":document,"layer":2,"range":{"start":0,"end":3},"runs":[]})).unwrap();
    assert!(matches!(
        console.execute_document(bad),
        DocumentToolResponse::Error(_)
    ));
    assert_eq!(
        console
            .documents()
            .session(document)
            .unwrap()
            .history()
            .entries
            .len(),
        5
    );
    console.documents_mut().undo(document).unwrap();
    assert!(matches!(
        console
            .documents()
            .session(document)
            .unwrap()
            .document()
            .state()
            .find(LayerId(2))
            .unwrap()
            .kind,
        compositor::LayerKind::Shape { .. }
    ));
}

#[test]
fn actions_can_reference_new_text_and_shape_layers() {
    use tessera_mcp::actions::ActionFile;
    for producer in ["add_text", "add_shape"] {
        let value = json!({"format":"tessera-action","version":1,"name":"editable","inputs":1,"steps":[
            {"command":producer,"params":{"document":{"$input":0},"model":{}}},
            {"command":"convert_to_pixels","params":{"document":{"$input":0},"layer":{"$layer":0}}}
        ]});
        ActionFile::from_json(&value.to_string()).unwrap();
    }
}
