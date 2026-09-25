use std::sync::{Arc, Mutex};
use tessera_ffi::*;

#[test]
fn folder_filter_precedes_pagination_and_handles_glob_characters() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
    for folder in ["a", "b[1]"] {
        let path = dir.path().join(folder);
        std::fs::create_dir(&path).unwrap();
        for n in 0..3 {
            image::RgbImage::new(8, 8)
                .save(path.join(format!("{n}.jpg")))
                .unwrap();
        }
        engine
            .index_folder(path.to_string_lossy().into_owned())
            .unwrap();
    }
    let folder = dir
        .path()
        .join("b[1]")
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let all = engine
        .list_images(ImageQuery {
            folder: Some(folder.clone()),
            ..Default::default()
        })
        .unwrap();
    let page = engine
        .list_images(ImageQuery {
            folder: Some(folder),
            limit: 1,
            offset: 1,
            ..Default::default()
        })
        .unwrap();
    assert_eq!(all.len(), 3);
    assert_eq!(page.len(), 1);
    assert_eq!(page[0].id, all[1].id);
}

#[derive(Default)]
struct Events(Mutex<Vec<EngineEvent>>);

#[test]
fn recipe_updates_preserve_unknown_fields_and_history() {
    use engine_api::recipe::{EditMeta, Recipe};
    let dir = tempfile::tempdir().unwrap();
    image::RgbImage::new(8, 8)
        .save(dir.path().join("one.jpg"))
        .unwrap();
    let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
    engine
        .index_folder(dir.path().to_string_lossy().into_owned())
        .unwrap();
    let row = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    let mut recipe: Recipe =
        serde_json::from_str(&engine.get_recipe(row.id.clone()).unwrap()).unwrap();
    recipe
        .unknown
        .insert("future".into(), serde_json::json!({"preserve": true}));
    engine
        .set_recipe_json(row.id.clone(), serde_json::to_string(&recipe).unwrap())
        .unwrap();
    recipe.unknown.clear();
    recipe
        .edit(EditMeta::default(), |s| s.tone.exposure = 1.0)
        .unwrap();
    engine
        .set_recipe_json(row.id.clone(), serde_json::to_string(&recipe).unwrap())
        .unwrap();
    let saved: Recipe = serde_json::from_str(&engine.get_recipe(row.id.clone()).unwrap()).unwrap();
    assert_eq!(
        saved.unknown["future"],
        serde_json::json!({"preserve": true})
    );
    assert_ne!(saved.recipe_hash().to_string(), row.recipe_hash);
    assert_eq!(
        engine.list_images(ImageQuery::default()).unwrap()[0].recipe_hash,
        saved.recipe_hash().to_string()
    );
    assert!(
        engine
            .set_recipe_json(
                row.id.clone(),
                serde_json::to_string(&Recipe::new(saved.image_id.unwrap())).unwrap()
            )
            .is_err()
    );
    let mut bad = serde_json::to_value(&saved).unwrap();
    bad["settings"]["tone"]["exposure"] = 4.0.into();
    assert!(engine.set_recipe_json(row.id, bad.to_string()).is_err());
}
impl EngineEventListener for Events {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap().push(event);
    }
}

#[test]
fn recipe_preview_and_callback_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let photos = dir.path().join("photos");
    std::fs::create_dir(&photos).unwrap();
    image::RgbImage::new(64, 32)
        .save(photos.join("one.jpg"))
        .unwrap();
    let engine = Engine::open(dir.path().join("db").to_string_lossy().into_owned()).unwrap();
    let events = Arc::new(Events::default());
    engine.set_event_listener(Some(events.clone()));
    engine
        .index_folder(photos.to_string_lossy().into_owned())
        .unwrap();
    let row = engine.list_images(ImageQuery::default()).unwrap().remove(0);
    let json = engine.get_recipe(row.id.clone()).unwrap();
    engine
        .set_recipe_json(row.id.clone(), json.clone())
        .unwrap();
    assert_eq!(engine.get_recipe(row.id.clone()).unwrap(), json);
    assert!(engine.set_recipe_json(row.id.clone(), "{}".into()).is_err());
    let bytes = engine.embedded_preview(row.id.clone(), 16).unwrap();
    let preview = image::load_from_memory(&bytes).unwrap();
    assert_eq!((preview.width(), preview.height()), (16, 8));
    assert!(engine.embedded_preview(row.id, 0).is_err());
    let events = events.0.lock().unwrap();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::ScanProgress { finished: true, .. }))
    );
    assert!(
        events
            .iter()
            .any(|e| matches!(e, EngineEvent::PreviewReady { .. }))
    );
}
