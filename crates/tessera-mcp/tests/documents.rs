//! Layered-document tools and Actions through the console.
use compositor::Compositor;
use engine_api::EngineError;
use engine_api::action::Action;
use engine_api::document::*;
use engine_api::id::{DocumentId, HistoryEntryId, LayerId};
use engine_api::recipe::Author;
use engine_api::tools::{
    DocumentToolCall as Call, DocumentToolOutput as Out, DocumentToolRequest, DocumentToolResponse,
    ExportFormat,
};
use tessera_mcp::Console;
use tessera_mcp::actions::ActionFile;

fn photo(dir: &std::path::Path, name: &str, w: u32, h: u32, seed: u32) -> std::path::PathBuf {
    let path = dir.join(name);
    image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([
            ((x * 7 + seed * 13) % 256) as u8,
            ((y * 5 + seed * 3) % 256) as u8,
            ((x + y + seed) % 256) as u8,
        ])
    })
    .save(&path)
    .unwrap();
    path
}

fn req(call: Call, rationale: &str) -> DocumentToolRequest {
    DocumentToolRequest {
        call,
        rationale: Some(rationale.into()),
        group: None,
        expect_head: None,
    }
}

fn ok(console: &mut Console, request: DocumentToolRequest) -> Out {
    match console.execute_document(request) {
        DocumentToolResponse::Ok(o) => o,
        DocumentToolResponse::Error(e) => panic!("{e:?}"),
    }
}

fn err(console: &mut Console, request: DocumentToolRequest) -> EngineError {
    match console.execute_document(request) {
        DocumentToolResponse::Ok(o) => panic!("unexpected success {o:?}"),
        DocumentToolResponse::Error(e) => e,
    }
}

fn edited(o: Out) -> (HistoryEntryId, Option<LayerId>) {
    let Out::DocumentEdited { entry, layer, .. } = o else {
        panic!("not an edit: {o:?}")
    };
    (entry.unwrap(), layer)
}

fn composite(console: &Console, doc: DocumentId) -> Vec<f32> {
    let d = console.documents().session(doc).unwrap().document();
    Compositor::new(64 << 20).render_level_rgba(d, 0).unwrap().1
}

fn stroke(doc: DocumentId, layer: LayerId, color: [f32; 3]) -> Call {
    Call::PaintStroke {
        document: doc,
        layer,
        points: vec![
            StrokePoint {
                x: 4.0,
                y: 6.0,
                pressure: 1.0,
            },
            StrokePoint {
                x: 40.0,
                y: 30.0,
                pressure: 0.5,
            },
        ],
        brush: BrushParams {
            size: 6.0,
            color,
            pressure_size: true,
            ..Default::default()
        },
        target: StrokeTarget::Pixels,
    }
}

#[test]
fn every_edit_is_one_agent_entry_with_its_action_and_rationale() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let path = photo(dir.path(), "a.png", 48, 32, 1);
    let doc = console.open_document(&path).unwrap();
    let calls = vec![
        Call::ApplyAdjustmentLayer {
            document: doc,
            adjustment: AdjustmentSpec::Invert,
            name: None,
            parent: None,
            above: None,
            clipped: false,
            mask_from_selection: true,
        },
        Call::AddLayer {
            document: doc,
            layer: NewLayer::Pixel,
            name: Some("Paint".into()),
            parent: None,
            above: None,
            props: LayerPropsUpdate::default(),
        },
        stroke(doc, LayerId(3), [1.0, 0.0, 0.0]),
        Call::SetLayerProps {
            document: doc,
            layer: LayerId(3),
            props: LayerPropsUpdate {
                opacity: Some(0.5),
                blend_mode: Some(BlendMode::Multiply),
                ..Default::default()
            },
        },
        // No-op updates still record their entry (one per call).
        Call::SetLayerProps {
            document: doc,
            layer: LayerId(3),
            props: LayerPropsUpdate::default(),
        },
    ];
    for (i, call) in calls.iter().enumerate() {
        let (entry, _) = edited(ok(&mut console, req(call.clone(), &format!("step {i}"))));
        assert_eq!(entry, HistoryEntryId(i as u64 + 1));
    }
    let session = console.documents().session(doc).unwrap();
    let history = session.history();
    history.validate().unwrap();
    assert_eq!(history.entries.len(), calls.len());
    assert_eq!(history.head, Some(HistoryEntryId(calls.len() as u64)));
    for (i, (entry, call)) in history.entries.iter().zip(&calls).enumerate() {
        assert_eq!(entry.action, Action::from_document_tool(call).unwrap());
        assert_eq!(
            entry.meta.rationale.as_deref(),
            Some(format!("step {i}").as_str())
        );
        assert!(matches!(entry.meta.author, Author::Agent { .. }));
        assert_eq!(entry.meta.label, call.name());
    }
    // Read-only and effect calls record nothing.
    let Out::LayerList { layers, .. } =
        ok(&mut console, req(Call::ListLayers { document: doc }, "q"))
    else {
        panic!()
    };
    assert_eq!(
        layers.iter().map(|l| (l.id.0, l.kind)).collect::<Vec<_>>(),
        vec![
            (1, LayerKindTag::Pixel),
            (2, LayerKindTag::Adjustment),
            (3, LayerKindTag::Pixel)
        ]
    );
    assert_eq!(layers[2].blend_mode, BlendMode::Multiply);
    assert!(layers[2].bounds.is_some());
    let out = dir.path().join("out.png");
    ok(
        &mut console,
        req(
            Call::ExportDocument {
                document: doc,
                settings: DocumentExportSettings {
                    path: out.to_str().unwrap().into(),
                    format: DocumentFormat::Image {
                        encoding: ExportFormat::Png { bit_depth: 8 },
                        resize: None,
                        profile: None,
                    },
                },
            },
            "export",
        ),
    );
    assert_eq!(
        console
            .documents()
            .session(doc)
            .unwrap()
            .history()
            .entries
            .len(),
        calls.len()
    );
    // Exported pixels are the composite: an unpainted corner is inverted.
    let png = image::open(&out).unwrap().to_rgba8();
    let src = image::open(&path).unwrap().to_rgb8();
    let (p, s) = (png.get_pixel(47, 0), src.get_pixel(47, 0));
    for c in 0..3 {
        assert!((i32::from(p[c]) - (255 - i32::from(s[c]))).abs() <= 1);
    }
}

#[test]
fn failures_and_expect_head_conflicts_leave_history_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let doc = console
        .open_document(photo(dir.path(), "a.png", 16, 16, 2))
        .unwrap();
    let add = Call::AddLayer {
        document: doc,
        layer: NewLayer::Pixel,
        name: None,
        parent: None,
        above: None,
        props: LayerPropsUpdate::default(),
    };
    // `null` = the state as opened.
    let mut r = req(add.clone(), "first");
    r.expect_head = Some(None);
    let (first, _) = edited(ok(&mut console, r));
    let before = composite(&console, doc);
    for head in [None, Some(HistoryEntryId(7))] {
        let mut r = req(add.clone(), "stale");
        r.expect_head = Some(head);
        assert!(matches!(err(&mut console, r), EngineError::Conflict { .. }));
    }
    let mut r = req(add.clone(), "current");
    r.expect_head = Some(Some(first));
    edited(ok(&mut console, r));
    // Invalid calls: nothing recorded.
    for bad in [
        stroke(doc, LayerId(99), [0.0; 3]),
        Call::SetLayerProps {
            document: doc,
            layer: LayerId(2),
            props: LayerPropsUpdate {
                opacity: Some(1.5),
                ..Default::default()
            },
        },
        Call::MergeDown {
            document: doc,
            layer: LayerId(1),
        },
        Call::TransformLayer {
            document: doc,
            layer: LayerId(2),
            transform: AffineTransform([0.0; 6]),
            interpolation: Interpolation::Bilinear,
        },
    ] {
        err(&mut console, req(bad, "bad"));
    }
    let session = console.documents().session(doc).unwrap();
    assert_eq!(session.history().entries.len(), 2);
    // Undo returns to the first state; an edit then branches.
    console.documents_mut().undo(doc).unwrap();
    assert_eq!(composite(&console, doc), before);
    let (branch, _) = edited(ok(&mut console, req(add, "branch")));
    let h = console.documents().session(doc).unwrap().history();
    assert_eq!(h.entry(branch).unwrap().parent, Some(first));
    h.validate().unwrap();
    assert!(matches!(
        err(
            &mut console,
            req(
                Call::ListLayers {
                    document: DocumentId(42)
                },
                "x"
            )
        ),
        EngineError::NotFound { .. }
    ));
}

#[test]
fn transform_merge_and_selection_behave_like_their_definitions() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let doc = console
        .open_document(photo(dir.path(), "a.png", 64, 40, 3))
        .unwrap();
    let (_, layer) = edited(ok(
        &mut console,
        req(
            Call::AddLayer {
                document: doc,
                layer: NewLayer::Pixel,
                name: None,
                parent: None,
                above: None,
                props: LayerPropsUpdate {
                    blend_mode: Some(BlendMode::Screen),
                    opacity: Some(0.6),
                    ..Default::default()
                },
            },
            "layer",
        ),
    ));
    let layer = layer.unwrap();
    // Selection limits the stroke to the rectangle.
    edited(ok(
        &mut console,
        req(
            Call::SetPixelSelection {
                document: doc,
                shape: SelectionShape::Rect {
                    rect: CanvasRect {
                        x0: 0,
                        y0: 0,
                        x1: 20,
                        y1: 40,
                    },
                },
                mode: SelectionMode::Replace,
                feather: 0.0,
                save_as: Some("left".into()),
            },
            "select",
        ),
    ));
    edited(ok(
        &mut console,
        req(stroke(doc, layer, [0.0, 0.2, 1.0]), "paint"),
    ));
    let state = console
        .documents()
        .session(doc)
        .unwrap()
        .document()
        .state()
        .clone();
    let raster = state.find(layer).unwrap().raster().unwrap();
    assert!(raster.pixel(8, 10)[3] > 0.9, "inside selection painted");
    assert_eq!(raster.pixel(30, 21)[3], 0.0, "outside selection untouched");
    // Translate by (5, 3) with nearest sampling moves pixels exactly.
    let before = raster.clone();
    edited(ok(
        &mut console,
        req(
            Call::TransformLayer {
                document: doc,
                layer,
                transform: AffineTransform([1.0, 0.0, 5.0, 0.0, 1.0, 3.0]),
                interpolation: Interpolation::Nearest,
            },
            "move",
        ),
    ));
    let state = console
        .documents()
        .session(doc)
        .unwrap()
        .document()
        .state()
        .clone();
    let moved = state.find(layer).unwrap().raster().unwrap();
    for (x, y) in [(8, 10), (12, 13), (3, 6), (0, 0)] {
        assert_eq!(moved.pixel(x + 5, y + 3), before.pixel(x, y));
    }
    // Merging keeps the composite (within 8-bit rounding).
    let pre = composite(&console, doc);
    let (_, lower) = edited(ok(
        &mut console,
        req(
            Call::MergeDown {
                document: doc,
                layer,
            },
            "merge",
        ),
    ));
    assert_eq!(lower, Some(LayerId(1)));
    let post = composite(&console, doc);
    let worst = pre
        .iter()
        .zip(&post)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f32::max);
    assert!(
        worst <= 1.5 / 255.0,
        "merge changed the composite by {worst}"
    );
    assert_eq!(
        console
            .documents()
            .session(doc)
            .unwrap()
            .document()
            .state()
            .layer_ids(),
        vec![LayerId(1)]
    );
}

#[test]
fn native_and_psd_exports_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let doc = console
        .open_document(photo(dir.path(), "a.jpg", 40, 30, 4))
        .unwrap();
    let (_, layer) = edited(ok(
        &mut console,
        req(
            Call::AddLayer {
                document: doc,
                layer: NewLayer::Pixel,
                name: Some("Paint".into()),
                parent: None,
                above: None,
                props: LayerPropsUpdate {
                    opacity: Some(0.5),
                    ..Default::default()
                },
            },
            "layer",
        ),
    ));
    edited(ok(
        &mut console,
        req(stroke(doc, layer.unwrap(), [0.2, 0.4, 0.6]), "paint"),
    ));
    let want = composite(&console, doc);
    for (name, format) in [
        ("x.tessera-doc", DocumentFormat::TesseraDoc),
        (
            "x.psd",
            DocumentFormat::Psd {
                maximize_compatibility: true,
            },
        ),
    ] {
        let path = dir.path().join(name);
        ok(
            &mut console,
            req(
                Call::ExportDocument {
                    document: doc,
                    settings: DocumentExportSettings {
                        path: path.to_str().unwrap().into(),
                        format: format.clone(),
                    },
                },
                "save",
            ),
        );
        let again = console.open_document(&path).unwrap();
        let got = composite(&console, again);
        let worst = want
            .iter()
            .zip(&got)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f32::max);
        assert!(worst <= 1.0 / 255.0, "{name}: {worst}");
    }
}

#[test]
fn recorded_action_replays_identical_pixels_on_another_document() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let a_path = photo(dir.path(), "a.png", 72, 48, 5);
    let b_path = dir.path().join("b.png");
    std::fs::copy(&a_path, &b_path).unwrap();
    let a = console.open_document(&a_path).unwrap();
    let b = console.open_document(&b_path).unwrap();
    // B already has an extra layer, so fresh layer ids differ between A and B.
    edited(ok(
        &mut console,
        req(
            Call::AddLayer {
                document: b,
                layer: NewLayer::Group {
                    mode: GroupMode::PassThrough,
                },
                name: None,
                parent: None,
                above: None,
                props: LayerPropsUpdate::default(),
            },
            "unrelated",
        ),
    ));
    console.start_recording("Grade").unwrap();
    let e = |c| req(c, "recorded");
    edited(ok(
        &mut console,
        e(Call::SetPixelSelection {
            document: a,
            shape: SelectionShape::Ellipse {
                rect: CanvasRect {
                    x0: 10,
                    y0: 5,
                    x1: 60,
                    y1: 40,
                },
            },
            mode: SelectionMode::Replace,
            feather: 3.0,
            save_as: Some("oval".into()),
        }),
    ));
    let (_, curves) = edited(ok(
        &mut console,
        e(Call::ApplyAdjustmentLayer {
            document: a,
            adjustment: AdjustmentSpec::Curves {
                master: vec![[0.0, 0.1], [0.5, 0.6], [1.0, 0.95]],
                rgb: Default::default(),
            },
            name: None,
            parent: None,
            above: None,
            clipped: false,
            mask_from_selection: true,
        }),
    ));
    edited(ok(
        &mut console,
        e(Call::SetPixelSelection {
            document: a,
            shape: SelectionShape::None,
            mode: SelectionMode::Replace,
            feather: 0.0,
            save_as: None,
        }),
    ));
    let (_, paint) = edited(ok(
        &mut console,
        e(Call::AddLayer {
            document: a,
            layer: NewLayer::Pixel,
            name: None,
            parent: None,
            above: curves,
            props: LayerPropsUpdate::default(),
        }),
    ));
    let paint = paint.unwrap();
    edited(ok(&mut console, e(stroke(a, paint, [0.9, 0.8, 0.1]))));
    edited(ok(
        &mut console,
        e(Call::TransformLayer {
            document: a,
            layer: paint,
            transform: AffineTransform([0.9, 0.1, 4.0, -0.1, 0.9, 6.0]),
            interpolation: Interpolation::Bicubic,
        }),
    ));
    edited(ok(
        &mut console,
        e(Call::SetLayerProps {
            document: a,
            layer: paint,
            props: LayerPropsUpdate {
                blend_mode: Some(BlendMode::Overlay),
                ..Default::default()
            },
        }),
    ));
    edited(ok(
        &mut console,
        e(Call::SetPixelSelection {
            document: a,
            shape: SelectionShape::Saved {
                selection: engine_api::id::SelectionId(1),
            },
            mode: SelectionMode::Replace,
            feather: 0.0,
            save_as: None,
        }),
    ));
    edited(ok(
        &mut console,
        e(Call::PaintStroke {
            document: a,
            layer: curves.unwrap(),
            points: vec![StrokePoint {
                x: 30.0,
                y: 20.0,
                pressure: 1.0,
            }],
            brush: BrushParams {
                size: 15.0,
                hardness: 0.3,
                mode: BrushMode::Erase,
                ..Default::default()
            },
            target: StrokeTarget::Mask,
        }),
    ));
    let action = console.stop_recording().unwrap();
    assert_eq!(action.inputs, 1);
    assert_eq!(action.steps.len(), 9);
    // Created layers and saved selections are symbolic.
    assert_eq!(
        action.steps[3].action.params["above"],
        serde_json::json!({"$layer": 1})
    );
    assert_eq!(
        action.steps[7].action.params["selection"],
        serde_json::json!({"$selection": 0})
    );
    let file = dir.path().join("grade.tessera-action");
    action.write(&file).unwrap();
    let action = ActionFile::read(&file).unwrap();
    let before_b = console
        .documents()
        .session(b)
        .unwrap()
        .history()
        .entries
        .len();
    let report = console.play_action(&action, &[b]).unwrap();
    assert!(report.ok(), "{report:?}");
    let hb = console.documents().session(b).unwrap().history();
    assert_eq!(hb.entries.len(), before_b + 9);
    assert!(
        hb.entries
            .last()
            .unwrap()
            .meta
            .rationale
            .as_deref()
            .unwrap()
            .contains("action `Grade`, step 9")
    );
    // B's layer ids differ from A's, yet the pixels are identical.
    assert_ne!(
        console
            .documents()
            .session(a)
            .unwrap()
            .document()
            .state()
            .layer_ids(),
        console
            .documents()
            .session(b)
            .unwrap()
            .document()
            .state()
            .layer_ids()
    );
    assert_eq!(composite(&console, a), composite(&console, b));
    // Wrong input count is rejected before any step runs.
    assert!(console.play_action(&action, &[]).is_err());
    // A different document (other size) replays too.
    let c = console
        .open_document(photo(dir.path(), "c.png", 30, 90, 6))
        .unwrap();
    assert!(console.play_action(&action, &[c]).unwrap().ok());
}

#[test]
fn action_files_are_validated() {
    let bad = [
        r#"{"format":"x","version":1,"name":"a","steps":[]}"#,
        r#"{"format":"tessera-action","version":9,"name":"a","steps":[]}"#,
        r#"{"format":"tessera-action","version":1,"name":"a","steps":[{"command":"nope","params":{}}]}"#,
        r#"{"format":"tessera-action","version":1,"name":"a","inputs":1,"steps":[{"command":"merge_down","params":{"document":{"$input":1},"layer":2}}]}"#,
        r#"{"format":"tessera-action","version":1,"name":"a","inputs":1,"steps":[{"command":"merge_down","params":{"document":{"$input":0},"layer":{"$layer":0}}}]}"#,
        r#"{"format":"tessera-action","version":1,"name":"a","inputs":1,"steps":[{"command":"list_layers","params":{"document":{"$input":0}}},{"command":"merge_down","params":{"document":{"$input":0},"layer":{"$layer":0}}}]}"#,
    ];
    for text in bad {
        assert!(ActionFile::from_json(text).is_err(), "{text}");
    }
    ActionFile::from_json(
        r#"{"format":"tessera-action","version":1,"name":"a","inputs":1,"steps":[{"command":"add_layer","params":{"document":{"$input":0},"layer":{"kind":"pixel"}}},{"command":"merge_down","params":{"document":{"$input":0},"layer":{"$layer":0}},"enabled":false}]}"#,
    )
    .unwrap();
}

#[test]
fn describe_and_resident_preview() {
    let dir = tempfile::tempdir().unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let doc = console
        .open_document(photo(dir.path(), "a.png", 300, 200, 7))
        .unwrap();
    edited(ok(
        &mut console,
        req(
            Call::ApplyAdjustmentLayer {
                document: doc,
                adjustment: AdjustmentSpec::Invert,
                name: None,
                parent: None,
                above: None,
                clipped: false,
                mask_from_selection: true,
            },
            "invert for review",
        ),
    ));
    let preview = console.render_document_preview(doc, 100).unwrap();
    if compositor::gpu::GpuCompositor::new().is_ok() {
        assert!(console.documents().preview_is_resident(doc).unwrap());
    }
    assert!(
        preview.width() <= 100 && preview.width() >= 50,
        "{}",
        preview.width()
    );
    // The resident (or fallback) preview agrees with the CPU reference.
    let d = console.documents().session(doc).unwrap().document();
    let (e, cpu) = Compositor::new(16 << 20).render_level_rgba(d, 2).unwrap();
    assert_eq!((e.width, e.height), (preview.width(), preview.height()));
    for (p, c) in preview.pixels().zip(cpu.as_chunks::<4>().0) {
        for k in 0..4 {
            assert!((i32::from(p[k]) - (c[k].clamp(0.0, 1.0) * 255.0).round() as i32).abs() <= 1);
        }
    }
    let d = console.describe_document(doc, 128, Some(32)).unwrap();
    assert_eq!(d.summary["layers"].as_array().unwrap().len(), 2);
    assert_eq!(d.summary["layers"][1]["kind"], "adjustment");
    assert_eq!(
        d.summary["history"]["recent"][0]["rationale"],
        "invert for review"
    );
    assert_eq!(d.thumbnails.len(), 1, "adjustments have no thumbnail");
    assert!(d.thumbnails[0].image.width() <= 32);
}
