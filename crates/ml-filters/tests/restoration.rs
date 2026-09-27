use compositor::{Depth, Raster, Rect};
use engine_api::tile::Extent;
use ml_filters::{Cancel, NeuralFilter, Params, PhotoRestoration};

#[test]
fn missing_restoration_weights_are_actionable_and_leave_input_unchanged() {
    let mut input = Raster::new(Extent::new(8, 8), 4, Depth::F32, 0.0);
    input
        .edit_region(Rect::of_extent(input.extent()), 1, |x, y, p| {
            *p = [x as f32 / 8.0, y as f32 / 8.0, 0.3, 0.5];
        })
        .unwrap();
    let before = input.clone();
    let error = PhotoRestoration::unloaded()
        .apply(&input, &Params::default(), &Cancel::new())
        .unwrap_err()
        .to_string();
    assert!(error.contains("Photo Restoration"), "{error}");
    assert!(
        error.contains("DRUNet model weights are not loaded"),
        "{error}"
    );
    assert!(error.contains("never downloads weights"), "{error}");
    assert!(input.shares_all_tiles_with(&before));
    assert_eq!(input.max_rev(), before.max_rev());
}
