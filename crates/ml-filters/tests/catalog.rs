use compositor::raster::{Depth, Raster};
use engine_api::tile::Extent;
use ml_filters::{Cancel, NeuralFilter, Params, PhotoRestoration, catalog};
#[test]
fn catalog_is_offline_and_restoration_never_silently_ignores_unsupported_controls()
-> anyhow::Result<()> {
    let entries = catalog();
    assert_eq!(entries.len(), 3);
    assert_eq!(entries[0].name, "Skin Smoothing");
    assert!(!entries[0].requires_weights);
    assert_eq!(entries[1].name, "Colorize");
    assert!(entries[1].limitation.unwrap().contains("CPU"));
    assert_eq!(entries[2].name, "JPEG Artifact Removal");
    let filter = PhotoRestoration::unloaded();
    assert_eq!(filter.name(), "Photo Restoration (no face model)");
    let r = Raster::new(Extent::new(2, 2), 4, Depth::U8, 0.5);
    let p = Params {
        photo_enhancement: 0.0,
        enhance_face: 0.0,
        scratch_reduction: 0.0,
        ..Params::default()
    };
    assert!(
        filter
            .apply(&r, &p, &Cancel::new())?
            .shares_all_tiles_with(&r)
    );
    assert!(
        filter
            .apply(
                &r,
                &Params {
                    enhance_face: 1.0,
                    ..p.clone()
                },
                &Cancel::new()
            )
            .is_err()
    );
    assert!(
        filter
            .apply(
                &r,
                &Params {
                    scratch_reduction: 1.0,
                    ..p
                },
                &Cancel::new()
            )
            .is_err()
    );
    Ok(())
}
