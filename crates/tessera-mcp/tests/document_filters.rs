use engine_api::{
    id::DocumentId,
    tools::{DocumentToolRequest, DocumentToolResponse},
};
use serde_json::{Value, json};
use tessera_mcp::Console;

fn smart_document(dir: &std::path::Path) -> std::path::PathBuf {
    use compositor::{Depth, DocState, Document, Layer, LayerKind, Rect, SmartObject};
    use engine_api::tile::Extent;
    use std::sync::Arc;
    let extent = Extent::new(24, 24);
    let mut child = DocState::new(extent, Depth::F32);
    let mut pixel = Layer::pixel("source", extent, Depth::F32);
    pixel.id = engine_api::id::LayerId(1);
    pixel
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            *p = [((x * 7 + y * 11) % 23) as f32 / 24.0, 0.3, 0.4, 1.0]
        })
        .unwrap();
    child.root.push(Arc::new(pixel));
    child.next_id = 2;
    let mut outer = DocState::new(extent, Depth::F32);
    let mut layer = Layer::new(
        "smart",
        LayerKind::SmartObject(SmartObject::new(child, compositor::Affine::IDENTITY)),
    );
    layer.id = engine_api::id::LayerId(1);
    outer.root.push(Arc::new(layer));
    outer.next_id = 2;
    let path = dir.join("smart.tessera-doc");
    compositor::format::save(&Document::new(outer), &path).unwrap();
    path
}
#[test]
fn smart_filters_are_enabled_evaluated_and_rendered_without_baking() {
    let (dir, mut c, _) = setup();
    let d = c.open_document(smart_document(dir.path())).unwrap();
    let before = c.render_document_preview(d, 24).unwrap();
    let out = c.execute_document(request(
        d,
        "camera_raw_filter",
        json!({"settings":{"tone":{"exposure":1.0}}}),
        true,
    ));
    assert!(matches!(out, DocumentToolResponse::Ok(_)), "{out:?}");
    let session = c.documents().session(d).unwrap();
    let compositor::LayerKind::SmartObject(so) = &session.document().state().root[0].kind else {
        panic!("baked")
    };
    assert_eq!(so.filters.len(), 1);
    assert!(so.filters[0].enabled);
    assert_eq!(so.filters[0].name, "camera_raw");
    let after = c.render_document_preview(d, 24).unwrap();
    assert_ne!(before, after);
    let head = c.documents().session(d).unwrap().history().head;
    for tool in ["neural_colorize", "neural_jpeg_artifact_removal"] {
        let mut req = request(d, tool, json!({}), true);
        req.expect_head = None;
        let out = c.execute_document(req);
        assert!(
            matches!(
                out,
                DocumentToolResponse::Error(engine_api::EngineError::Unsupported { .. })
            ),
            "{out:?}"
        );
        assert_eq!(c.documents().session(d).unwrap().history().head, head);
    }
}

#[test]
fn distractions_return_masks_and_preserve_replayable_history() {
    let (_dir, mut c, d) = setup();
    let out = c.execute_document(request(d,"remove_distractions",json!({"faces":[[8.0,8.0,2.0,2.0]],"wires":false,"remove":{"dilation":0,"fill":{"patch_radius":1,"iterations":1}}}),false));
    assert!(matches!(out, DocumentToolResponse::Ok(_)), "{out:?}");
    let value = serde_json::to_value(out).unwrap();
    assert_eq!(value["ok"]["report"]["mask"].as_array().unwrap().len(), 576);
    assert!(
        value["ok"]["report"]["people"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.as_f64().unwrap() > 0.0)
    );
    assert!(
        value["ok"]["report"]["limitation"]
            .as_str()
            .unwrap()
            .contains("not semantic")
    );
    c.documents()
        .session(d)
        .unwrap()
        .history()
        .validate()
        .unwrap();
}

fn setup() -> (tempfile::TempDir, Console, DocumentId) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.png");
    image::RgbImage::from_fn(24, 24, |x, y| {
        image::Rgb([((x * 17 + y * 31) % 255) as u8, 100, 120])
    })
    .save(&path)
    .unwrap();
    let mut c = Console::open(dir.path().join("app")).unwrap();
    let d = c.open_document(path).unwrap();
    (dir, c, d)
}
#[test]
fn retouch_operations_dispatch_and_undo_with_strict_errors() {
    let mut mask = vec![0.0; 24 * 24];
    for y in 10..14 {
        for x in 10..14 {
            mask[y * 24 + x] = 1.0;
        }
    }
    let mut mesh = filters::liquify::Mesh::new(24, 24, 8).unwrap();
    mesh.displacement.fill([2.0, 0.0]);
    for (tool, params) in [
        (
            "document_remove_object",
            json!({"mask":mask,"remove":{"backend":"cpu","dilation":0}}),
        ),
        ("content_aware_fill", json!({"mask":mask})),
        ("content_aware_move", json!({"mask":mask,"offset":[4,0]})),
        ("liquify", json!({"mesh":mesh})),
        (
            "neural_skin_smoothing",
            json!({"faces":[[0,0,24,24]],"blur":8,"smoothness":1}),
        ),
    ] {
        let (_dir, mut c, d) = setup();
        let before = c.documents().session(d).unwrap().document().state().root[0]
            .raster()
            .unwrap()
            .clone();
        let out = c.execute_document(request(d, tool, params.clone(), false));
        assert!(
            matches!(out, DocumentToolResponse::Ok(_)),
            "{tool}: {out:?}"
        );
        let session = c.documents().session(d).unwrap();
        assert_eq!(session.history().entries.len(), 1);
        assert_eq!(session.history().entries[0].action.command, tool);
        let after = session.document().state().root[0].raster().unwrap();
        assert!(
            (0..24).any(|y| (0..24).any(|x| before.pixel(x, y) != after.pixel(x, y))),
            "{tool} did nothing"
        );
        c.documents_mut().undo(d).unwrap();
        let restored = c.documents().session(d).unwrap().document().state().root[0]
            .raster()
            .unwrap();
        for y in 0..24 {
            for x in 0..24 {
                assert_eq!(restored.pixel(x, y), before.pixel(x, y));
            }
        }
        let mut bad = params;
        bad["typo"] = json!(1);
        let out = c.execute_document(request(d, tool, bad, false));
        assert!(
            matches!(
                out,
                DocumentToolResponse::Error(engine_api::EngineError::InvalidArgument { .. })
            ),
            "{out:?}"
        );
        assert_eq!(c.documents().session(d).unwrap().history().head, None);
    }
}

fn request(d: DocumentId, tool: &str, params: Value, smart: bool) -> DocumentToolRequest {
    serde_json::from_value(json!({"tool":tool,"document":d,"layer":1,"params":params,"smart":smart,"rationale":"retouch","group":7,"expect_head":null})).unwrap()
}
#[test]
fn camera_raw_evaluates_before_one_history_commit_and_checks_head() {
    let (_dir, mut c, d) = setup();
    let before = c.documents().session(d).unwrap().document().state().root[0]
        .raster()
        .unwrap()
        .pixel(2, 2);
    let req = request(
        d,
        "camera_raw_filter",
        json!({"settings":{"tone":{"exposure":1.0}}}),
        false,
    );
    let out = c.execute_document(req.clone());
    assert!(matches!(out, DocumentToolResponse::Ok(_)), "{out:?}");
    let session = c.documents().session(d).unwrap();
    let after = session.document().state().root[0]
        .raster()
        .unwrap()
        .pixel(2, 2);
    assert_ne!(before, after);
    let history = session.history();
    assert_eq!(history.entries.len(), 1);
    assert_eq!(history.entries[0].action.command, "camera_raw_filter");
    assert_eq!(
        history.entries[0].meta.rationale.as_deref(),
        Some("retouch")
    );
    assert_eq!(history.entries[0].meta.group.unwrap().0, 7);
    history.validate().unwrap();
    assert!(matches!(
        c.execute_document(req),
        DocumentToolResponse::Error(engine_api::EngineError::Conflict { .. })
    ));
    c.documents_mut().undo(d).unwrap();
    assert_eq!(
        c.documents().session(d).unwrap().document().state().root[0]
            .raster()
            .unwrap()
            .pixel(2, 2),
        before
    );
}
