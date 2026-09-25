use engine_api::{
    id::HistoryGroupId,
    recipe::Author,
    tools::{ToolOutput, ToolRequest, ToolResponse},
};
use serde_json::json;
use tessera_mcp::Console;

fn request(value: serde_json::Value) -> ToolRequest {
    serde_json::from_value(value).unwrap()
}

#[test]
fn scene_linear_histogram_and_noop_history_are_explicit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    image::RgbImage::from_pixel(16, 16, image::Rgb([64; 3]))
        .save(&path)
        .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let image = console.open_image(&path).unwrap();
    let response = console.execute(request(
        json!({"tool":"get_histogram","image":image,"space":"scene_linear"}),
    ));
    let ToolResponse::Ok(ToolOutput::Histogram { histogram, .. }) = response else {
        panic!("{response:?}");
    };
    assert_eq!(histogram.red.len(), 256);
    assert_eq!(histogram.red.iter().sum::<u32>(), 256);
    for _ in 0..2 {
        assert!(matches!(
            console.execute(request(
                json!({"tool":"set_tone","image":image,"exposure":0})
            )),
            ToolResponse::Ok(_)
        ));
    }
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(path).recipe).unwrap();
    assert_eq!(doc.recipe.history.entries.len(), 2);
    assert!(
        doc.recipe
            .history
            .entries
            .iter()
            .all(|e| e.changes.is_empty())
    );
    doc.recipe.validate().unwrap();
}

#[test]
fn compare_catalog_scores_and_export_use_engine_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    for (name, value) in [("a.jpg", 40), ("b.jpg", 160)] {
        image::RgbImage::from_pixel(32, 24, image::Rgb([value; 3]))
            .save(photos.join(name))
            .unwrap();
    }
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let a = console.open_image(photos.join("a.jpg")).unwrap();
    let b = console.open_image(photos.join("b.jpg")).unwrap();
    assert_eq!(console.list_images(None).unwrap().len(), 2);
    assert_eq!(console.describe_image(a).unwrap()["id"], json!(a));
    let comparison = console
        .compare_images(a, b, engine_api::tools::CompareMetric::DeltaE2000)
        .unwrap();
    assert_eq!(comparison.images.len(), 2);
    assert!(comparison.metrics[0].mean < comparison.metrics[1].mean);
    assert!(comparison.distance > 0.);
    for metric in ["recipe_diff", "scores"] {
        assert!(matches!(
            console.execute(request(
                json!({"tool":"compare","image_a":a,"image_b":b,"metric":metric})
            )),
            ToolResponse::Ok(_)
        ));
    }
    assert!(matches!(
        console.execute(request(json!({"tool":"get_scores","image":a}))),
        ToolResponse::Ok(ToolOutput::Scores { .. })
    ));
    let out = dir.path().join("out");
    assert!(matches!(console.execute(request(json!({"tool":"export","images":[a],"settings":{"destination":out,"format":{"format":"png","bit_depth":8}}}))),ToolResponse::Ok(ToolOutput::ExportQueued {images:1,..})));
    assert!(out.join("a.png").is_file());
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(photos.join("a.jpg")).recipe)
        .unwrap();
    assert_eq!(doc.recipe.history.entries.len(), 1);
    assert_eq!(doc.recipe.history.entries[0].meta.label, "export");
}

#[test]
fn masks_crop_style_and_selection_record_one_entry_each() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jpg");
    image::RgbImage::from_pixel(16, 16, image::Rgb([100, 110, 120]))
        .save(&path)
        .unwrap();
    let app = dir.path().join("app");
    let mut console = Console::open(&app).unwrap();
    let id = console.open_image(&path).unwrap();
    std::fs::create_dir(app.join("styles")).unwrap();
    std::fs::write(app.join("styles/index.json"), r#"{"warm":"warm.json"}"#).unwrap();
    std::fs::write(app.join("styles/warm.json"), r#"{"tone":{"exposure":2.0}}"#).unwrap();
    let mask=match console.execute(request(json!({"tool":"create_mask","image":id,"name":"Gradient","components":[{"kind":"linear","start":[0,0],"end":[1,1]}],"params":{"exposure":0.5},"rationale":"Lift foreground","group":3}))) {
        ToolResponse::Ok(ToolOutput::MaskCreated {mask,coverage,..})=> {assert!(coverage>0. && coverage<1.);mask},
        other=>panic!("{other:?}"),
    };
    for call in [
        json!({"tool":"adjust_mask","image":id,"mask":mask,"amount":50}),
        json!({"tool":"crop","image":id,"rect":{"left":0.1,"right":0.9,"top":0.1,"bottom":0.9},"angle":2}),
        json!({"tool":"apply_style","image":id,"style":"warm","amount":50}),
        json!({"tool":"set_selection","images":[id,id],"grade":{"action":"set","value":3}}),
    ] {
        assert!(matches!(
            console.execute(request(call)),
            ToolResponse::Ok(_)
        ));
    }
    let doc = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&path).recipe).unwrap();
    assert_eq!(doc.recipe.history.entries.len(), 5);
    assert!(
        doc.recipe
            .history
            .entries
            .iter()
            .all(|e| matches!(e.meta.author, Author::Agent { .. }))
    );
    assert_eq!(doc.recipe.settings.tone.exposure, 1.);
    assert_eq!(doc.recipe.settings.locals.adjustments.len(), 1);
    assert_eq!(
        doc.recipe.selection.grade,
        Some(engine_api::recipe::Grade::Three)
    );
    doc.recipe.validate().unwrap();
    let bytes = std::fs::read(sidecar::Sidecar::paths(&path).recipe).unwrap();
    assert!(matches!(
        console.execute(request(
            json!({"tool":"apply_style","image":id,"style":"../../test","amount":100})
        )),
        ToolResponse::Error(_)
    ));
    assert_eq!(
        bytes,
        std::fs::read(sidecar::Sidecar::paths(path).recipe).unwrap()
    );
}

#[test]
fn rejected_mutations_preserve_history_and_pixels() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test.jpg");
    image::RgbImage::new(8, 8).save(&path).unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    for call in [
        json!({"tool":"set_tone","image":id,"exposure":99}),
        json!({"tool":"set_tone","image":id,"exposure":1,"expect_recipe":"0".repeat(64)}),
        json!({"tool":"remove_object","image":id,"mask":0}),
        json!({"tool":"retouch_skin","image":id,"person":0,"strength":50}),
    ] {
        assert!(matches!(
            console.execute(request(call)),
            ToolResponse::Error(_)
        ));
    }
    assert!(!sidecar::Sidecar::paths(path).recipe.exists());
}

#[test]
fn tone_is_one_persistent_agent_entry_and_histogram_tracks_render() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    let path = photos.join("test.jpg");
    image::RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([(20 + x * 3) as u8, (30 + y * 4) as u8, 70])
    })
    .save(&path)
    .unwrap();
    let mut console = Console::open(dir.path().join("app")).unwrap();
    let id = console.open_image(&path).unwrap();
    let before = console.render_preview(id, 16).unwrap();
    let response = console.execute(request(json!({"tool":"set_tone","image":id,"exposure":1.0,"rationale":"Lift the subject","group":7})));
    assert!(matches!(
        response,
        ToolResponse::Ok(ToolOutput::Edited { entry: Some(_), .. })
    ));
    let document = sidecar::Sidecar::read_recipe(sidecar::Sidecar::paths(&path).recipe).unwrap();
    assert_eq!(document.recipe.history.entries.len(), 1);
    let entry = &document.recipe.history.entries[0];
    assert!(matches!(entry.meta.author, Author::Agent { .. }));
    assert_eq!(entry.meta.rationale.as_deref(), Some("Lift the subject"));
    assert_eq!(entry.meta.group, Some(HistoryGroupId(7)));
    drop(console);
    let mut console = Console::open(dir.path().join("app")).unwrap();
    assert_ne!(before, console.render_preview(id, 16).unwrap());
    match console.execute(request(json!({"tool":"get_histogram","image":id}))) {
        ToolResponse::Ok(ToolOutput::Histogram { histogram, .. }) => {
            for channel in [
                &histogram.red,
                &histogram.green,
                &histogram.blue,
                &histogram.luminance,
            ] {
                assert_eq!(channel.len(), 256);
                assert_eq!(channel.iter().sum::<u32>(), 32 * 24);
            }
        }
        response => panic!("{response:?}"),
    }
}
