//! M5-32: restoration dispatch must fail without publishing pixels or history.
#![cfg(target_os = "macos")]

use tessera_ffi::*;

#[test]
fn restoration_missing_weights_is_atomic_on_pixels_and_smart_objects() {
    for smart in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        let path = dir.path().join("restoration.png");
        image::RgbaImage::from_fn(8, 8, |x, y| {
            image::Rgba([(x * 30) as u8, (y * 30) as u8, 80, 255])
        })
        .save(&path)
        .unwrap();
        let session = engine
            .open_document(path.to_string_lossy().into_owned())
            .unwrap();
        let id = session.layers().unwrap()[0].id;
        if smart {
            session.convert_for_smart_filters(id).unwrap();
        }
        let history = session.history_items().unwrap();
        let info = session.info().unwrap();
        let pixels = session.read_presented_level(0).unwrap();
        let error = session
            .apply_raster_filter(
                id,
                RasterFilterRequest {
                    operation: RasterFilterOperation::PhotoRestoration,
                    params_json: r#"{"photo_enhancement":0.5}"#.into(),
                },
            )
            .unwrap_err()
            .to_string();
        assert_eq!(session.history_items().unwrap(), history);
        assert_eq!(session.info().unwrap().history_head, info.history_head);
        assert_eq!(session.info().unwrap().dirty, info.dirty);
        assert_eq!(session.read_presented_level(0).unwrap(), pixels);
        if smart {
            assert!(session.smart_filters(id).unwrap().is_empty());
        }
        assert!(error.contains("weights"), "{error}");
    }
}
