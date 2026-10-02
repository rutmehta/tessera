use super::*;
use std::io::{Cursor, Seek, SeekFrom, Write};

struct MaskFixture {
    catalog_id: i64,
    bytes: Vec<u8>,
}
impl LrcatMaskResolver for MaskFixture {
    fn resolve(&self, catalog_image_id: i64, resource_id: String) -> Option<Vec<u8>> {
        assert_eq!(catalog_image_id, self.catalog_id);
        assert_eq!(resource_id, "opaque-mask-id");
        Some(self.bytes.clone())
    }
}

#[test]
fn lr5_apply_pins_replaces_rolls_back_and_removes_masks_with_image() {
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
        image.recipe = import_lrcat::develop(catalog_id, "s = { MaskGroupBasedCorrections = { { LocalExposure2012=1, CorrectionMasks={ { What='Mask/Image', MaskSubType=2, MaskDigest='opaque-mask-id' } } } } }", "15.4").unwrap().0;
        let mut spool = import.spool.reopen().unwrap();
        let bytes = serde_json::to_vec(&image).unwrap();
        let offset = spool.seek(SeekFrom::End(0)).unwrap();
        spool.write_all(&bytes).unwrap();
        import.records[row.index] = (offset, bytes.len());
    }
    let (width, height) = image::image_dimensions(&row.path).unwrap();
    let resolver = |value| {
        let mut bytes = Cursor::new(Vec::new());
        image::GrayImage::from_pixel(width, height, image::Luma([value]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        Arc::new(MaskFixture {
            catalog_id,
            bytes: bytes.into_inner(),
        }) as Arc<dyn LrcatMaskResolver>
    };
    let key = crate::lrcat_masks::key(id, 0);
    let store = ml_segment::MaskStore::new(support.join("imported-masks"), 0).unwrap();
    for (n, value) in [64, 191].into_iter().enumerate() {
        options.overwrite_existing_edits = n > 0;
        let report = import
            .apply_with_mask_resolver(options.clone(), None, Some(resolver(value)))
            .unwrap();
        assert!(report.imported > 0, "{report:?}");
        let mut recipe = Sidecar::read_recipe(Sidecar::paths(&row.path).recipe)
            .unwrap()
            .recipe;
        let state = recipe.settings.locals.adjustments[0].components[0]
            .adobe_ai
            .as_ref()
            .unwrap();
        assert_eq!(state.mask_key, Some(key));
        assert!(!state.regenerate);
        assert!(
            !import_lrcat::diagnostics::entries(&recipe)
                .values()
                .flatten()
                .any(|d| d.reason.starts_with("regenerated:"))
        );
        assert_eq!(recipe.history.entries.len(), 1);
        recipe
            .edit(engine_api::recipe::EditMeta::user("exposure", 1), |s| {
                s.tone.exposure = 0.5
            })
            .unwrap();
        recipe.validate().unwrap();
        let recipe = Recipe::from_json(&recipe.to_json().unwrap()).unwrap();
        assert_eq!(
            recipe.settings.locals.adjustments[0].components[0]
                .adobe_ai
                .as_ref()
                .unwrap()
                .mask_key,
            Some(key)
        );
        assert!((store.get(&key).unwrap().data()[0] - value as f32 / 255.).abs() < 1e-6);
        assert_eq!(
            std::fs::read_dir(support.join("imported-masks/pinned"))
                .unwrap()
                .count(),
            1
        );
        // Export reads the explicit engine support root without consulting a model.
        let pixels = pipeline_cpu::Image::new(
            width,
            height,
            vec![vec![0.18; (width * height) as usize]; 3],
        )
        .unwrap();
        let input = export::ExportImage {
            source: pipeline_cpu::RenderSource::Rgb(&pixels),
            name: "synthetic",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let out = export::export_one(
            &input,
            &recipe,
            &export::ExportSettings {
                mask_support: Some(support.clone()),
                output_dir: temp.path().join(format!("export-{value}")),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(out.is_file());
        assert!(!support.join("models").exists());
    }
    let xmp = Sidecar::paths(&row.path).xmp;
    let prior = std::fs::read(&xmp).unwrap();
    std::fs::remove_file(&xmp).unwrap();
    std::fs::create_dir(&xmp).unwrap();
    options.overwrite_existing_edits = false;
    assert!(
        import
            .apply_with_mask_resolver(options.clone(), None, Some(resolver(255)))
            .is_err()
    );
    assert!((store.get(&key).unwrap().data()[0] - 191. / 255.).abs() < 1e-6);
    std::fs::remove_dir(&xmp).unwrap();
    std::fs::write(&xmp, prior).unwrap();
    options.overwrite_existing_edits = true;
    import
        .apply_with_mask_resolver(options.clone(), None, Some(resolver(191)))
        .unwrap();
    let resumed = import.apply(options.clone(), None).unwrap();
    assert!(resumed.resumed > 0);
    assert!(
        !resumed
            .approximate
            .iter()
            .any(|r| r.reason.starts_with("regenerated:"))
    );
    assert_eq!(engine.forget_missing(vec![id.to_string()]).unwrap(), 0);
    assert!(store.get(&key).is_some());
    // No resolver on a fresh re-import resets reference and clears the slot.
    // A new import bundle requests re-apply rather than resuming the completed bundle.
    options.library_folder = photos.join("new-library").to_string_lossy().into_owned();
    import.apply(options.clone(), None).unwrap();
    assert!(store.get(&key).is_none());
    let recipe = Sidecar::read_recipe(Sidecar::paths(&row.path).recipe)
        .unwrap()
        .recipe;
    assert!(
        recipe.settings.locals.adjustments[0].components[0]
            .adobe_ai
            .as_ref()
            .unwrap()
            .regenerate
    );
    options.library_folder = photos.to_string_lossy().into_owned();
    options.overwrite_existing_edits = false;
    import
        .apply_with_mask_resolver(options, None, Some(resolver(64)))
        .unwrap();
    assert!(store.get(&key).is_some());
    std::fs::remove_file(&row.path).unwrap();
    assert_eq!(engine.forget_missing(vec![id.to_string()]).unwrap(), 1);
    assert!(store.get(&key).is_none());
}

#[test]
fn lr5_regeneration_uses_existing_injected_segmenter_and_changes_pixels() {
    struct Segmenter {
        expected: export::mask_ai::SegmentRequest,
        calls: usize,
    }
    impl export::mask_ai::MaskSegmenter for Segmenter {
        fn segment(
            &mut self,
            image: &image::RgbImage,
            request: &export::mask_ai::SegmentRequest,
        ) -> anyhow::Result<Vec<f32>> {
            assert_eq!(request, &self.expected);
            self.calls += 1;
            Ok((0..image.width() * image.height())
                .map(|i| {
                    if i % image.width() < image.width() / 2 {
                        1.
                    } else {
                        0.
                    }
                })
                .collect())
        }
    }
    for (category, expected) in [
        ("Subject", export::mask_ai::SegmentRequest::Subject),
        ("Sky", export::mask_ai::SegmentRequest::Sky),
        ("Background", export::mask_ai::SegmentRequest::Background),
        ("Hair", export::mask_ai::SegmentRequest::Subject),
        (
            "Object",
            export::mask_ai::SegmentRequest::Prompts {
                clicks: vec![],
                boxes: vec![[0.25, 0.25, 0.75, 0.75]],
            },
        ),
    ] {
        let source = format!(
            "s = {{ MaskGroupBasedCorrections = {{ {{ LocalExposure2012=1, CorrectionMasks={{ {{ What='Mask/Image', MaskType='{category}', Left=0.25,Top=0.25,Right=0.75,Bottom=0.75 }} }} }} }} }}"
        );
        let (recipe, warnings) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert!(warnings.is_empty());
        let before = recipe.to_json().unwrap();
        let image = pipeline_cpu::Image::new(8, 4, vec![vec![0.18; 32]; 3]).unwrap();
        let input = export::ExportImage {
            source: pipeline_cpu::RenderSource::Rgb(&image),
            name: "synthetic",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let mut segmenter = Segmenter { expected, calls: 0 };
        let rgb = export::render_pixels(
            &input,
            &recipe,
            &export::RenderRequest {
                color_space: export::ColorSpace::Srgb,
                resize: export::Resize::None,
                sharpen_for: export::SharpenFor::None,
                scale: 1,
            },
            &engine_api::jobs::CancellationToken::new(),
            Some(&mut segmenter),
        )
        .unwrap();
        assert_eq!(segmenter.calls, 1);
        assert!(
            rgb.get_pixel(0, 0)[0] > rgb.get_pixel(7, 0)[0] + 0.05,
            "{category}"
        );
        assert_eq!(
            recipe.to_json().unwrap(),
            before,
            "render must not rewrite import history or diagnostics"
        );
        assert_eq!(recipe.history.entries.len(), 1);
        assert!(
            import_lrcat::diagnostics::entries(&recipe)
                .values()
                .flatten()
                .any(|e| e.reason.contains("regenerated"))
        );
    }
}
