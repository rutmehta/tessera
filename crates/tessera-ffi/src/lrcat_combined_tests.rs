//! LR-6f integration: invented catalog rows and caller-owned rasters only.
use super::*;
use std::io::{Cursor, Seek, SeekFrom, Write};

const SOURCE: &str = r#"s = {
    PointColors={{SrcHue=0,SrcSat=0.5,SrcLum=0.5,HueShift=0.2}},
    ConvertToGrayscale=true, GrayMixerRed=25,
    RetouchInfo={{centerX=0.25,centerY=0.5,radius=0.0625,sourceX=0.75,sourceY=0.5,spotType='heal',opacity=1}},
    PerspectiveUpright=1, UprightTransform_1='1,0,0,0,1,0,0.02,0,1',
    MaskGroupBasedCorrections={
        {LocalExposure2012=0.5,CorrectionMasks={{What='Mask/RangeMask',CorrectionRangeMask={Type=2,LumMin=0.1,LumMax=0.9}}}},
        {LocalExposure2012=1,CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskDigest='mask'}}}
    },
    LensBlur={Active=true,BlurAmount=37,FocalRange='0 5 15 20'},
    DepthMapInfo={BaseRawDepthTable='depth'}
}"#;

struct Resource {
    id: i64,
    bytes: Vec<u8>,
    calls: std::sync::atomic::AtomicUsize,
}
impl LrcatMaskResolver for Resource {
    fn resolve(&self, id: i64, resource: String) -> Option<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(id, self.id);
        assert_eq!(resource, "mask");
        Some(self.bytes.clone())
    }
}
impl LrcatDepthResolver for Resource {
    fn resolve(&self, id: i64, resource: String) -> Option<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(id, self.id);
        assert_eq!(resource, "depth");
        Some(self.bytes.clone())
    }
}
struct PinnedMasks(PathBuf);
impl image_core::mask_cache::MaskHooks for PinnedMasks {
    fn revision(&self) -> u64 {
        1
    }
    fn rasterize(
        &self,
        input: &pipeline_cpu::Image,
        group: &engine_api::recipe::LocalAdjustment,
        _: u8,
    ) -> engine_api::EngineResult<Vec<f32>> {
        export::mask_ai::compose_with_components(input, group, |component, w, h| {
            let key = component.adobe_ai.as_ref().unwrap().mask_key.unwrap();
            let plane = export::mask_ai::imported_plane(&self.0, &key)
                .map_err(|e| engine_api::EngineError::internal(e.to_string()))?;
            assert_eq!((plane.width, plane.height), (w, h));
            Ok(plane.data.into())
        })
    }
}

#[test]
fn lr6f_all_lanes_one_apply_both_resources_and_both_absent() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
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
        let (recipe, warnings) = import_lrcat::develop(catalog_id, SOURCE, "15.4").unwrap();
        assert!(warnings.is_empty(), "{warnings:?}");
        image.recipe = recipe;
        let mut spool = import.spool.reopen().unwrap();
        let bytes = serde_json::to_vec(&image).unwrap();
        let offset = spool.seek(SeekFrom::End(0)).unwrap();
        spool.write_all(&bytes).unwrap();
        import.records[row.index] = (offset, bytes.len());
    }
    let (w, h) = image::image_dimensions(&row.path).unwrap();
    let mut png = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(w, h, image::Luma([128]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let resource = Arc::new(Resource {
        id: catalog_id,
        bytes: png.into_inner(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let report = import
        .apply_with_resolvers(
            options.clone(),
            None,
            Some(resource.clone()),
            Some(resource.clone()),
        )
        .unwrap();
    assert!(report.imported > 0);
    assert_eq!(resource.calls.load(Ordering::SeqCst), 2);
    let recipe = Sidecar::read_recipe(Sidecar::paths(&row.path).recipe)
        .unwrap()
        .recipe;
    recipe.validate().unwrap();
    assert_eq!(recipe.history.entries.len(), 1);
    assert!(matches!(
        recipe.history.entries[0].meta.author,
        engine_api::recipe::Author::Import { .. }
    ));
    let value = serde_json::to_value(&recipe).unwrap();
    assert_eq!(value["schema_version"], 4);
    let entries = import_lrcat::diagnostics::entries(&recipe);
    for lane in ["LR-1", "LR-2", "LR-3", "LR-4", "LR-5", "LR-6", "LR-7"] {
        let notes: Vec<_> = entries
            .values()
            .flatten()
            .filter(|e| e.lane == lane)
            .collect();
        assert!(!notes.is_empty(), "missing {lane}: {entries:?}");
        for note in notes {
            assert_eq!(note.level, "info");
            assert_eq!(note.status, "approximate");
            assert!(
                note.field
                    .as_deref()
                    .is_some_and(|p| value.pointer(p).is_some())
            );
        }
    }
    assert!(
        !entries
            .values()
            .flatten()
            .any(|e| e.reason.starts_with("regenerated"))
    );
    assert_eq!(recipe.settings.color.point_colors.len(), 1);
    assert!(recipe.settings.color.monochrome.as_ref().unwrap().enabled);
    assert_eq!(recipe.settings.locals.retouch.len(), 1);
    assert!(recipe.settings.geometry.upright.homography.is_some());
    let mask_key = crate::lrcat_masks::key(id, 0);
    assert_eq!(
        recipe.settings.locals.adjustments[1].components[0]
            .adobe_ai
            .as_ref()
            .unwrap()
            .mask_key,
        Some(mask_key)
    );
    let depth_key = image_core::depth::imported_depth_key(id);
    assert_eq!(
        recipe
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap()
            .mask_key,
        Some(depth_key)
    );
    for root in [
        support.join("imported-masks"),
        support.join("previews/depth-cache"),
    ] {
        let files: Vec<_> = std::fs::read_dir(root.join("pinned"))
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(files.len(), 1);
        let bytes: u64 = files.iter().map(|e| e.metadata().unwrap().len()).sum();
        assert_eq!(bytes, u64::from(w) * u64::from(h) * 4 + 48);
        assert!(bytes <= ml_segment::MaskStore::MAX_PINNED_BYTES);
    }
    let before = recipe.to_json().unwrap();
    let mut replay = Recipe::from_json(&before).unwrap();
    assert!(replay.undo().unwrap());
    assert!(replay.redo().unwrap());
    assert_eq!(replay.settings, recipe.settings);
    // Full CPU graph with real brush retouch, composed pinned masks and stored depth.
    // Wrong-sized fallback proves depth lookup cannot silently invoke estimation.
    let depth = image_core::depth::DepthProvider::from_map(
        image_core::ml_depth::DepthMap::from_normalized_inverse(1, 1, vec![0.]).unwrap(),
    )
    .with_store(
        image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 0).unwrap(),
    );
    let renderer = image_core::Renderer::new(Default::default())
        .with_depth(Arc::new(depth))
        .with_retouch_renderer(Arc::new(brush::render_retouch));
    renderer
        .mask_cache()
        .set_hooks(Some(Arc::new(PinnedMasks(support.clone()))));
    let pixels = pipeline_cpu::Image::new(
        w,
        h,
        vec![
            (0..w * h)
                .map(|i| if i % 2 == 0 { 0.1 } else { 0.8 })
                .collect();
            3
        ],
    )
    .unwrap();
    let raw = image_core::RawImage::from_rgb(
        id,
        image_core::RgbSource::from_linear_rec2020(pixels).unwrap(),
    )
    .unwrap();
    // Use the production tiled CPU entry point, which owns the external-mask hooks.
    let rect = image_core::PixelRect::full(raw.active_extent());
    let rendered = renderer
        .render_region_as(
            &raw,
            &recipe.settings,
            0,
            rect,
            image_core::RenderOutput::SceneLinear,
        )
        .unwrap();
    assert!(!rendered.is_empty());
    assert!(
        rendered
            .iter()
            .all(|tile| tile.samples::<f32>().unwrap().iter().all(|v| v.is_finite()))
    );
    let mut sharp_settings = recipe.settings.clone();
    sharp_settings.effects.lens_blur = None;
    let sharp = renderer
        .render_region_as(
            &raw,
            &sharp_settings,
            0,
            rect,
            image_core::RenderOutput::SceneLinear,
        )
        .unwrap();
    assert_eq!(rendered.len(), sharp.len());
    assert!(
        rendered.iter().zip(&sharp).any(|(a, b)| a
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(b.samples::<f32>().unwrap())
            .any(|(blurred, sharp)| (blurred - sharp).abs() > 1e-5)),
        "pinned depth must produce visible blur"
    );
    assert_eq!(before, recipe.to_json().unwrap());
    assert!(!support.join("models").exists());
    // A joint reimport that cannot publish restores BOTH previous pin classes.
    let xmp_path = Sidecar::paths(&row.path).xmp;
    let saved_xmp = std::fs::read(&xmp_path).unwrap();
    std::fs::remove_file(&xmp_path).unwrap();
    std::fs::create_dir(&xmp_path).unwrap();
    let mut changed_png = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(w, h, image::Luma([191]))
        .write_to(&mut changed_png, image::ImageFormat::Png)
        .unwrap();
    let changed = Arc::new(Resource {
        id: catalog_id,
        bytes: changed_png.into_inner(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    options.overwrite_existing_edits = true;
    assert!(
        import
            .apply_with_resolvers(
                options.clone(),
                None,
                Some(changed.clone()),
                Some(changed.clone())
            )
            .is_err()
    );
    assert_eq!(
        changed.calls.load(Ordering::SeqCst),
        2,
        "both new rasters must reach the publication attempt"
    );
    let stored_mask = ml_segment::MaskStore::new(support.join("imported-masks"), 0)
        .unwrap()
        .get(&mask_key)
        .unwrap();
    assert!((stored_mask.data()[0] - 128. / 255.).abs() < 1e-6);
    let stored_depth = image_core::ml_depth::DepthMap::cached(
        &image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 0).unwrap(),
        &depth_key,
    )
    .unwrap();
    assert!((stored_depth.inverse_depth()[0] - 128. / 255.).abs() < 1e-6);
    std::fs::remove_dir(&xmp_path).unwrap();
    std::fs::write(&xmp_path, saved_xmp).unwrap();
    // Reimport with neither resolver: both old pin classes must be removed and
    // both pending diagnostics must survive publication under one Import entry.
    options.overwrite_existing_edits = true;
    import.apply(options, None).unwrap();
    let pending = Sidecar::read_recipe(Sidecar::paths(&row.path).recipe)
        .unwrap()
        .recipe;
    pending.validate().unwrap();
    assert_eq!(pending.history.entries.len(), 1);
    let entries = import_lrcat::diagnostics::entries(&pending);
    for (lane, prefix) in [("LR-5", "regenerated:"), ("LR-6", "regenerated depth:")] {
        assert_eq!(
            entries
                .values()
                .flatten()
                .filter(|e| e.lane == lane && e.reason.starts_with(prefix))
                .count(),
            1
        );
    }
    assert!(
        pending.settings.locals.adjustments[1].components[0]
            .adobe_ai
            .as_ref()
            .unwrap()
            .regenerate
    );
    assert!(
        pending
            .settings
            .effects
            .lens_blur
            .as_ref()
            .unwrap()
            .depth
            .as_ref()
            .unwrap()
            .regenerate
    );
    assert!(
        ml_segment::MaskStore::new(support.join("imported-masks"), 0)
            .unwrap()
            .get(&mask_key)
            .is_none()
    );
    assert!(
        image_core::ml_depth::DepthMap::cached(
            &image_core::ml_depth::DepthStore::new(support.join("previews/depth-cache"), 0)
                .unwrap(),
            &depth_key
        )
        .is_none()
    );
}
