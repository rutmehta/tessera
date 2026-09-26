use engine_api::tools::{DocumentToolRequest, DocumentToolResponse};
use tessera_mcp::Console;

#[test]
fn executor_pixels_equal_direct_brush() {
    use engine_api::document::{BrushParams, StrokePoint};
    use engine_api::id::LayerId;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("paint.png");
    image::RgbaImage::from_pixel(32, 24, image::Rgba([30, 70, 90, 150]))
        .save(&path)
        .unwrap();
    let mut c = Console::open(dir.path().join("app")).unwrap();
    let id = c.open_document(path).unwrap();
    let mut expected = c
        .documents()
        .session(id)
        .unwrap()
        .document()
        .state()
        .find(LayerId(1))
        .unwrap()
        .raster()
        .unwrap()
        .clone();
    let params = BrushParams {
        size: 8.0,
        hardness: 0.3,
        flow: 0.6,
        opacity: 0.7,
        color: [0.8, 0.1, 0.2],
        pressure_size: true,
        ..Default::default()
    };
    let points = vec![
        StrokePoint {
            x: 4.0,
            y: 5.0,
            pressure: 0.5,
        },
        StrokePoint {
            x: 25.0,
            y: 17.0,
            pressure: 0.9,
        },
    ];
    let (_, tiles) = brush::api::paint_stroke(&expected, None, &params, &points, 0)
        .unwrap()
        .unwrap();
    for (tx, ty, tile) in tiles {
        expected.set_slot(tx, ty, Some(tile), 1).unwrap();
    }
    let req = serde_json::from_value(serde_json::json!({"tool":"paint_stroke","document":id,"layer":1,"points":points,"brush":params})).unwrap();
    assert!(matches!(
        c.execute_document(req),
        DocumentToolResponse::Ok(_)
    ));
    let actual = c
        .documents()
        .session(id)
        .unwrap()
        .document()
        .state()
        .find(LayerId(1))
        .unwrap()
        .raster()
        .unwrap();
    for y in 0..24 {
        for x in 0..32 {
            assert_eq!(actual.pixel(x, y), expected.pixel(x, y));
        }
    }
}

#[test]
fn saved_selection_follows_undo_redo() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.png");
    image::RgbaImage::new(8, 8).save(&path).unwrap();
    let mut c = Console::open(dir.path().join("app")).unwrap();
    let id = c.open_document(path).unwrap();
    let req: DocumentToolRequest = serde_json::from_value(serde_json::json!({"tool":"set_pixel_selection","document":id,"shape":"all","save_as":"all"})).unwrap();
    assert!(matches!(
        c.execute_document(req),
        DocumentToolResponse::Ok(_)
    ));
    assert_eq!(
        c.documents().session(id).unwrap().saved_selections().len(),
        1
    );
    c.documents_mut().undo(id).unwrap();
    assert!(
        c.documents()
            .session(id)
            .unwrap()
            .document()
            .state()
            .channels
            .is_empty()
    );
    assert!(
        c.documents()
            .session(id)
            .unwrap()
            .saved_selections()
            .is_empty()
    );
    c.documents_mut().redo(id).unwrap();
    let state = c.documents().session(id).unwrap().document().state();
    assert_eq!(state.channels.len(), 1);
    let loaded =
        compositor::format::from_bytes(&compositor::format::to_bytes(state).unwrap()).unwrap();
    assert_eq!(loaded.channels[0].name, "all");
    let saved_path = dir.path().join("saved.tessera-doc");
    compositor::format::save(c.documents().session(id).unwrap().document(), &saved_path).unwrap();
    let reopened = c.open_document(saved_path).unwrap();
    let saved = c.documents().session(reopened).unwrap().saved_selections();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].name, "all");
    let load_request: DocumentToolRequest = serde_json::from_value(serde_json::json!({"tool":"set_pixel_selection","document":reopened,"shape":"saved","selection":saved[0].id})).unwrap();
    assert!(matches!(
        c.execute_document(load_request),
        DocumentToolResponse::Ok(_)
    ));
    c.documents_mut().undo(reopened).unwrap();
    assert_eq!(
        c.documents()
            .session(reopened)
            .unwrap()
            .saved_selections()
            .len(),
        1
    );
    assert_eq!(
        c.documents().session(id).unwrap().saved_selections().len(),
        1
    );
}
