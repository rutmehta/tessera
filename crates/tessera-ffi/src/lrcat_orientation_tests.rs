//! Synthetic proxy -> original relink: catalog wins, no second EXIF transform.
use super::*;
#[allow(dead_code)]
#[path = "../../raw-decode/tests/support/mod.rs"]
mod support;

fn encoded(orientation: u16) -> Vec<u8> {
    let mut bytes = support::lossy_dng(false, false);
    let ifd = 38;
    let n = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    for entry in bytes[ifd + 2..ifd + 2 + n * 12].as_chunks_mut::<12>().0 {
        if entry[..2] == 274u16.to_le_bytes() {
            // Unknown tag simulates a proxy without Orientation.
            if orientation == 0 {
                entry[..2].copy_from_slice(&65000u16.to_le_bytes());
            } else {
                entry[8..10].copy_from_slice(&orientation.to_le_bytes());
            }
        }
    }
    bytes
}

#[test]
fn all_catalog_orientations_survive_relink_and_export_without_double_rotation() {
    use engine_api::recipe::DevelopSettings;
    use image_core::{PixelRect, Renderer};
    for orientation in 1..=8 {
        let dir = tempfile::tempdir().unwrap();
        let proxy = dir.path().join("proxy.dng");
        let original = dir.path().join("original.dng");
        std::fs::write(&proxy, encoded(0)).unwrap();
        let id = ImageId(87);
        let mut doc = sidecar::RecipeDocument::default();
        doc.recipe.image_id = Some(id);
        doc.recipe.unknown.insert(
            "lightroom_orientation".into(),
            serde_json::json!(orientation),
        );
        doc.recipe.unknown.insert(
            "lightroom_smart_preview".into(),
            serde_json::json!({"original_path":original,"proxy_path":proxy}),
        );
        Sidecar::write_recipe(Sidecar::paths(&proxy).recipe, &doc).unwrap();
        let render = || {
            let source = open_image(id, &proxy).unwrap();
            assert_eq!(source.metadata().orientation, 1);
            assert_eq!(source.recipe_owner(), id);
            let extent = source.active_extent();
            assert_eq!(
                (extent.width, extent.height),
                if orientation >= 5 { (10, 12) } else { (12, 10) }
            );
            let mut settings = DevelopSettings::default();
            settings.detail.sharpening.amount = 0.;
            settings.geometry.crop.rect.left = 0.25;
            settings.geometry.crop.rect.bottom = 0.75;
            let extent = Renderer::output_extent(&source, &settings, 0).unwrap();
            let tiles = Renderer::new(Default::default())
                .render_region(&source, &settings, 0, PixelRect::full(extent))
                .unwrap();
            let values: Vec<u8> = tiles
                .iter()
                .flat_map(|t| t.samples::<u8>().unwrap().to_vec())
                .collect();
            let export = crate::export::Source::open(&proxy, 1).unwrap();
            let export_pixels = pipeline_cpu::render(&settings, &export.render_source()).unwrap();
            (extent, values, export_pixels)
        };
        let before = render();
        // EXIF agrees with the catalog; composing them would rotate/refelect twice.
        std::fs::write(&original, encoded(orientation)).unwrap();
        assert!(!is_offline_proxy(&proxy));
        let after = render();
        assert_eq!(before, after, "orientation {orientation}");
        assert_eq!(
            Sidecar::read_recipe(Sidecar::paths(&proxy).recipe)
                .unwrap()
                .recipe,
            doc.recipe
        );
    }
}

#[test]
fn unavailable_named_lens_profile_is_retained_and_reported_without_blocking_proxy() {
    use engine_api::recipe::{
        DevelopSettings,
        settings::{LensProfileRef, LensProfileSource},
    };
    use image_core::{PixelRect, Renderer};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("synthetic.dng");
    std::fs::write(&path, encoded(0)).unwrap();
    let image =
        image_core::RawImage::open_with_catalog_orientation(ImageId(901), &path, Some(6)).unwrap();
    let mut settings = DevelopSettings::default();
    settings.lens.profile = LensProfileSource::Database {
        profile: LensProfileRef::named("synthetic unavailable profile"),
    };
    settings.lens.manual_distortion = 0.1;
    let retained = settings.clone();
    let drawn = crate::develop::session_renderable(&settings, true, false);
    assert_eq!(drawn.lens.profile, LensProfileSource::None);
    assert_eq!(
        drawn.lens.manual_distortion,
        settings.lens.manual_distortion
    );
    assert_eq!(settings, retained);
    assert!(
        crate::develop::ignored_settings(&settings)
            .iter()
            .any(|p| p.starts_with("/lens/profile"))
    );
    let extent = Renderer::output_extent(&image, &drawn, 0).unwrap();
    assert!(
        !Renderer::new(Default::default())
            .render_region(&image, &drawn, 0, PixelRect::full(extent))
            .unwrap()
            .is_empty()
    );
}
