use super::*;
use image_core::{
    depth::imported_depth_key,
    ml_depth::{DepthMap, DepthStore},
};
use std::io::{Cursor, Seek, SeekFrom, Write};

struct DepthFixture {
    catalog_id: i64,
    bytes: Vec<u8>,
}
impl LrcatDepthResolver for DepthFixture {
    fn resolve(&self, catalog_image_id: i64, resource_id: String) -> Option<Vec<u8>> {
        assert_eq!(catalog_image_id, self.catalog_id);
        assert_eq!(resource_id, "opaque-depth-id");
        Some(self.bytes.clone())
    }
}

#[test]
fn lr6e_apply_pins_depth_before_user_edit_reimports_and_deletes_with_image() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fx")).unwrap();
    let support = temp.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let mut import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    let photos = fixture.photos.canonicalize().unwrap();
    options.relocations[0].to = photos.to_string_lossy().into_owned();
    options.library_folder = photos.to_string_lossy().into_owned();
    let row = resolve(&import.plan, &options)
        .unwrap()
        .into_iter()
        .find(|r| r.outcome == Outcome::Import)
        .unwrap();
    let id = app_image_id(&row.path).unwrap();
    let catalog_id;
    {
        let import = Arc::get_mut(&mut import).unwrap();
        let mut image = import.read_image(row.index).unwrap();
        catalog_id = image.catalog_id;
        image.recipe = import_lrcat::develop(catalog_id, "s = { LensBlur = { Active = true, BlurAmount = 37 }, DepthMapInfo = { BaseRawDepthTable = 'opaque-depth-id' } }", "15.4").unwrap().0;
        let mut spool = import.spool.reopen().unwrap();
        let bytes = serde_json::to_vec(&image).unwrap();
        let offset = spool.seek(SeekFrom::End(0)).unwrap();
        spool.write_all(&bytes).unwrap();
        import.records[row.index] = (offset, bytes.len());
    }
    let (width, height) = image::image_dimensions(&row.path).unwrap();
    let make_resolver = |value| {
        let mut bytes = Cursor::new(Vec::new());
        image::GrayImage::from_pixel(width, height, image::Luma([value]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        Arc::new(DepthFixture {
            catalog_id,
            bytes: bytes.into_inner(),
        }) as Arc<dyn LrcatDepthResolver>
    };
    let key = imported_depth_key(id);
    for (n, value) in [64, 191].into_iter().enumerate() {
        if n == 1 {
            options.overwrite_existing_edits = true;
        }
        let report = import
            .apply_with_depth_resolver(options.clone(), None, Some(make_resolver(value)))
            .unwrap();
        assert!(report.imported > 0, "{report:?}");
        let doc = Sidecar::read_recipe(Sidecar::paths(&row.path).recipe).unwrap();
        let mut recipe = doc.recipe;
        let state = recipe
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap();
        assert_eq!(state.mask_key, Some(key));
        assert!(!state.regenerate);
        assert!(
            !import_lrcat::diagnostics::entries(&recipe)
                .values()
                .flatten()
                .any(|d| d.reason.starts_with("regenerated depth:"))
        );
        assert_eq!(recipe.history.entries.len(), 1);
        recipe
            .edit(engine_api::recipe::EditMeta::user("exposure", 1), |s| {
                s.tone.exposure = 1.
            })
            .unwrap();
        recipe.validate().unwrap();
        let restored = Recipe::from_json(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(
            restored
                .settings
                .effects
                .lens_blur
                .as_ref()
                .unwrap()
                .depth
                .as_ref()
                .unwrap()
                .mask_key,
            Some(key)
        );
        let store = DepthStore::new(support.join("previews/depth-cache"), 1).unwrap();
        let depth = DepthMap::cached(&store, &key).unwrap();
        assert!((depth.inverse_depth()[0] - value as f32 / 255.).abs() < 1e-6);
        assert_eq!(
            std::fs::read_dir(support.join("previews/depth-cache/pinned"))
                .unwrap()
                .count(),
            1
        );
    }
    // A failure before recipe publication rolls back that image's depth slot.
    let xmp_path = Sidecar::paths(&row.path).xmp;
    let saved_xmp = std::fs::read(&xmp_path).unwrap();
    std::fs::remove_file(&xmp_path).unwrap();
    std::fs::create_dir(&xmp_path).unwrap();
    options.overwrite_existing_edits = false;
    let failed = import.apply_with_depth_resolver(options.clone(), None, Some(make_resolver(255)));
    // The per-image write is rolled back; the final catalog scan also rejects
    // this deliberately invalid XMP destination and propagates an I/O error.
    let BridgeError::Failure { message } = failed.unwrap_err();
    assert!(
        message.starts_with("i/o error"),
        "expected I/O failure, got {message}"
    );
    let store = DepthStore::new(support.join("previews/depth-cache"), 1).unwrap();
    assert!(
        (DepthMap::cached(&store, &key).unwrap().inverse_depth()[0] - 191. / 255.).abs() < 1e-6
    );
    std::fs::remove_dir(&xmp_path).unwrap();
    std::fs::write(&xmp_path, saved_xmp).unwrap();
    options.overwrite_existing_edits = true;
    import
        .apply_with_depth_resolver(options.clone(), None, Some(make_resolver(191)))
        .unwrap();
    let resumed = import.apply(options, None).unwrap();
    assert!(resumed.resumed > 0);
    assert!(
        !resumed
            .approximate
            .iter()
            .any(|issue| issue.reason.starts_with("regenerated depth:"))
    );
    // A still-existing original must retain both the catalog record and its pin.
    assert_eq!(engine.forget_missing(vec![id.to_string()]).unwrap(), 0);
    assert!(
        DepthMap::cached(
            &DepthStore::new(support.join("previews/depth-cache"), 1).unwrap(),
            &key
        )
        .is_some()
    );
    std::fs::remove_file(&row.path).unwrap();
    assert_eq!(engine.forget_missing(vec![id.to_string()]).unwrap(), 1);
    assert!(
        DepthMap::cached(
            &DepthStore::new(support.join("previews/depth-cache"), 1).unwrap(),
            &key
        )
        .is_none()
    );
}
