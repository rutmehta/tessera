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
        // LR-8c: an online original keeps main's unrotated resource extent.
        // Catalog orientation is a proxy-only override, not an original edit.
        image.orientation = Some("BC".into());
        let mut spool = import.spool.reopen().unwrap();
        let bytes = serde_json::to_vec(&image).unwrap();
        let offset = spool.seek(SeekFrom::End(0)).unwrap();
        spool.write_all(&bytes).unwrap();
        import.records[row.index] = (offset, bytes.len());
    }
    let (w, h) = image::image_dimensions(&row.path).unwrap();
    assert_ne!(w, h, "non-square extent must detect an accidental swap");
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
    let mask_key = ml_segment::MaskRaster::new(w, h, vec![128. / 255.; (w * h) as usize])
        .unwrap()
        .content_key();
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
    // M7: AI mask pins are u16 samples; LR-6 depth pins stay f32.
    for (root, sample_bytes) in [
        (support.join("imported-masks"), 2),
        (support.join("previews/depth-cache"), 4),
    ] {
        let files: Vec<_> = std::fs::read_dir(root.join("pinned"))
            .unwrap()
            .map(|e| e.unwrap())
            .collect();
        assert_eq!(files.len(), 1);
        let bytes: u64 = files.iter().map(|e| e.metadata().unwrap().len()).sum();
        assert_eq!(bytes, u64::from(w) * u64::from(h) * sample_bytes + 48);
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
    // The depth slot is replaced in place. The mask raster is immutable content
    // that the image no longer owns, reclaimed by explicit pruning (M3, M6).
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
    assert_eq!(engine.prune_missing(false).unwrap(), 0);
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

/// INT-1: the proxy admission, LR-11 schema, and LR-10 CPU dispatch coexist.
#[test]
fn int1_offline_proxy_nested_locals_adobe_render_and_orientation() {
    use engine_api::recipe::{Author, ProcessFamily, required_schema_version};
    use image_core::{PixelRect, RawImage, RenderOutput, Renderer, RendererConfig};
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
    import_lrcat::fixture::write_smart_previews(&fixture).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    let row = r#"s={CameraProfile='Adobe Color',Sharpness=0,ColorNoiseReduction=0,ConvertToGrayscale=false,
        PerspectiveUpright=1,UprightTransform_1='1,0,0,0,1,0,0.02,0,1',
        MaskGroupBasedCorrections={{MainCurve={0,0,255,127.5},
          LocalPointColors={'0,0.5,0.5,0.5,0,0,0.5,0,0.25,0.75,1,0,0.25,0.75,1,0,0.25,0.75,1'},
          CorrectionMasks={{What='Mask/Group',Masks={{What='Mask/Gradient',
            FullX=0,FullY=0,ZeroX=1,ZeroY=0}}}}}}}"#;
    db.execute(
        "INSERT INTO Adobe_imageDevelopSettings(image,text,processVersion) VALUES(35,?1,'15.4')",
        [row],
    )
    .unwrap();
    db.execute(
        "UPDATE Adobe_images SET orientation='BC' WHERE id_local=35",
        [],
    )
    .unwrap();
    drop(db);
    let engine = Engine::open(temp.path().join("support").to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    options.relocations[0].to = fixture.photos.to_string_lossy().into_owned();
    options.library_folder = fixture.photos.to_string_lossy().into_owned();
    options.import_smart_previews = true;
    options.copy_proxies = true;
    let resolved = resolve(&import.plan, &options).unwrap();
    assert_eq!(
        resolved
            .iter()
            .filter(|r| r.outcome == Outcome::OfflineProxy)
            .count(),
        1
    );
    assert_eq!(
        import
            .plan(options.clone())
            .unwrap()
            .offline_with_smart_preview,
        1
    );
    let report = import.apply(options, None).unwrap();
    assert_eq!((report.imported, report.indexed), (6, 6));
    let images = engine.list_images(crate::ImageQuery::default()).unwrap();
    let proxy = images.iter().find(|r| r.lightroom_smart_preview).unwrap();
    let recipe: Recipe =
        serde_json::from_str(&engine.get_recipe(proxy.id.clone()).unwrap()).unwrap();
    recipe.validate().unwrap();
    assert_eq!(recipe.history.entries.len(), 1);
    assert!(matches!(
        recipe.history.entries[0].meta.author,
        Author::Import { .. }
    ));
    assert_eq!(required_schema_version(&recipe), 4);
    let json = serde_json::to_value(&recipe).unwrap();
    assert_eq!(json["schema_version"], 4);
    assert_eq!(recipe.process_version.family, ProcessFamily::Adobe);
    assert!(
        !recipe
            .settings
            .color
            .monochrome
            .as_ref()
            .is_some_and(|m| m.enabled)
    );
    assert!(recipe.settings.geometry.upright.homography.is_some());
    let group = &recipe.settings.locals.adjustments[0];
    assert!(group.params.curves.is_some());
    assert_eq!(group.params.point_colors.as_ref().unwrap().len(), 1);
    assert_eq!(group.components[0].group.as_ref().unwrap().len(), 1);
    let notes = import_lrcat::diagnostics::entries(&recipe);
    for lane in ["LR-4", "LR-7", "LR-11"] {
        assert!(
            notes.values().flatten().any(|n| n.lane == lane),
            "{notes:?}"
        );
    }
    for note in notes.values().flatten() {
        if let Some(field) = &note.field {
            assert!(json.pointer(field).is_some(), "{note:?}");
        }
    }
    assert_eq!(recipe.unknown["lightroom_orientation"], 6);
    let path = Path::new(&proxy.path);
    let id = recipe.image_id.unwrap();
    let source = crate::catalog::open_image(id, path).unwrap();
    assert!(source.camera_linear_proxy().is_some());
    assert_eq!(source.metadata().catalog_orientation, Some(6));
    assert_eq!(source.metadata().orientation, 1);
    let unrotated = RawImage::open(id, path).unwrap();
    let extent = source.active_extent();
    let raw_extent = unrotated.active_extent();
    assert_ne!(raw_extent.width, raw_extent.height);
    assert_eq!(
        (extent.width, extent.height),
        (raw_extent.height, raw_extent.width)
    );
    let config = RendererConfig {
        process_version: recipe.process_version,
        ..Default::default()
    };
    let render = |renderer: &Renderer, settings: &engine_api::recipe::DevelopSettings| {
        let extent = Renderer::output_extent(&source, settings, 0).unwrap();
        renderer
            .render_region_as(
                &source,
                settings,
                0,
                PixelRect::full(extent),
                RenderOutput::SceneLinear,
            )
            .unwrap()
            .iter()
            .flat_map(|t| t.samples::<f32>().unwrap().to_vec())
            .collect::<Vec<_>>()
    };
    let renderer = Renderer::new(config.clone());
    let pixels = render(&renderer, &recipe.settings);
    assert!(pixels.iter().all(|v| v.is_finite()));
    assert!(pixels.iter().any(|v| *v > 0.));
    // Compare the eligible embedded substitution with an explicitly parsed
    // embedded profile. Installed DCPs retain their separate main behaviour.
    let embedded =
        image_core::pipeline_adobe::dcp::DcpProfile::parse_embedded(&std::fs::read(path).unwrap())
            .unwrap();
    let explicit = image_core::pipeline_adobe::render_linear_scaled_with_profile(
        &recipe.settings,
        &pipeline_cpu::RenderSource::CameraLinear(source.camera_linear_proxy().unwrap()),
        1,
        Some(&embedded),
    )
    .unwrap();
    assert_eq!(
        pixels,
        explicit
            .planes()
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
    );
    for remove_curve in [false, true] {
        let mut settings = recipe.settings.clone();
        if remove_curve {
            settings.locals.adjustments[0].params.curves = None;
        } else {
            settings.locals.adjustments[0].params.point_colors = None;
        }
        let without = render(&renderer, &settings);
        assert!(
            pixels
                .iter()
                .zip(without)
                .any(|(a, b)| (a - b).abs() > 1e-6),
            "local operator must affect pixels: curve={remove_curve}"
        );
    }
    let mut without_upright = recipe.settings.clone();
    without_upright.geometry.upright = Default::default();
    assert_ne!(pixels, render(&renderer, &without_upright));
}

/// LR-8R: LR-5b validates injected masks in the proxy's oriented active frame.
#[test]
fn lr8r_oriented_offline_proxy_accepts_matching_imported_ai_raster() {
    let temp = tempfile::tempdir().unwrap();
    let fixture = import_lrcat::fixture::write(&temp.path().join("fixture")).unwrap();
    import_lrcat::fixture::write_smart_previews(&fixture).unwrap();
    let db = rusqlite::Connection::open(&fixture.catalog).unwrap();
    db.execute(
        "INSERT INTO Adobe_imageDevelopSettings(image,text,processVersion) VALUES(35,?1,'15.4')",
        ["s={MaskGroupBasedCorrections={{LocalExposure2012=1,CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskDigest='mask'}}}}}"],
    ).unwrap();
    db.execute(
        "UPDATE Adobe_images SET orientation='BC' WHERE id_local=35",
        [],
    )
    .unwrap();
    drop(db);
    let support = temp.path().join("support");
    let engine = Engine::open(support.to_string_lossy().into_owned()).unwrap();
    let import = engine
        .clone()
        .open_lrcat(fixture.catalog.to_string_lossy().into_owned())
        .unwrap();
    let mut options = import.default_options().unwrap();
    options.relocations[0].to = fixture.photos.to_string_lossy().into_owned();
    options.library_folder = fixture.photos.to_string_lossy().into_owned();
    options.copy_proxies = true;
    let resolved = resolve(&import.plan, &options).unwrap();
    let row = resolved
        .iter()
        .find(|r| r.outcome == Outcome::OfflineProxy)
        .unwrap();
    let metadata =
        raw_decode::lossy_dng::read_metadata(&mut std::fs::File::open(&row.path).unwrap())
            .unwrap()
            .unwrap();
    assert_eq!(metadata.default_crop[2..], [12, 10]);
    let (w, h) = (10, 12);
    let mut png = Cursor::new(Vec::new());
    image::GrayImage::from_pixel(w, h, image::Luma([128]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let resource = Arc::new(Resource {
        id: 35,
        bytes: png.into_inner(),
        calls: std::sync::atomic::AtomicUsize::new(0),
    });
    let report = import
        .apply_with_resolvers(options, None, Some(resource.clone()), None)
        .unwrap();
    assert_eq!((report.imported, report.indexed), (6, 6));
    assert_eq!(resource.calls.load(Ordering::SeqCst), 1);
    let images = engine.list_images(crate::ImageQuery::default()).unwrap();
    let proxy = images.iter().find(|r| r.lightroom_smart_preview).unwrap();
    let recipe: Recipe =
        serde_json::from_str(&engine.get_recipe(proxy.id.clone()).unwrap()).unwrap();
    let key = recipe.settings.locals.adjustments[0].components[0]
        .adobe_ai
        .as_ref()
        .unwrap()
        .mask_key
        .expect("matching oriented proxy mask must remain available under LR-5b");
    let plane = export::mask_ai::imported_plane(&support, &key).unwrap();
    assert_eq!((plane.width, plane.height), (w, h));
}
