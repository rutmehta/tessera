use compositor::{Depth, psd::{PsdCopyEstimateInput, estimate_rasterized_psd_copy}};
use engine_api::tile::Extent;

fn input(depth: Depth) -> PsdCopyEstimateInput {
    PsdCopyEstimateInput {
        canvas: Extent::new(2, 2),
        depth,
        saved_channels: 0,
        retained_merged_alpha: false,
        rasterized_stacks: 1,
        emitted_raster_layers: 2,
    }
}

#[test]
fn modeled_pixel_payload_accounts_for_depth_stacks_leaves_and_saved_planes() {
    for (depth, expected) in [(Depth::U8, 196), (Depth::U16, 264), (Depth::F32, 400)] {
        let estimate = estimate_rasterized_psd_copy(input(depth)).unwrap();
        assert_eq!(estimate.modeled_pixel_bytes, expected);
        assert_eq!(estimate.guaranteed_composite_channels, 3);
        assert_eq!(estimate.possible_composite_channels, 4);
    }
    let baseline = estimate_rasterized_psd_copy(input(Depth::U8)).unwrap();
    let saved = estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        saved_channels: 1,
        ..input(Depth::U8)
    }).unwrap();
    assert_eq!(saved.modeled_pixel_bytes - baseline.modeled_pixel_bytes, 4);
    assert_eq!(saved.guaranteed_composite_channels, 4);
    let extra_stack = estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        rasterized_stacks: 2,
        ..input(Depth::U8)
    }).unwrap();
    assert_eq!(extra_stack.modeled_pixel_bytes - baseline.modeled_pixel_bytes, 16);
    let extra_layer = estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        emitted_raster_layers: 3,
        ..input(Depth::U8)
    }).unwrap();
    assert_eq!(extra_layer.modeled_pixel_bytes - baseline.modeled_pixel_bytes, 16);
}

#[test]
fn format_preflight_uses_guaranteed_channels_not_possible_alpha() {
    // 70 million U8 pixels: RGB is 210 MB; RGBA is 280 MB. The existing
    // PSD decoded-buffer limit lies between them. This test allocates no image.
    let large = PsdCopyEstimateInput {
        canvas: Extent::new(10_000, 7_000),
        depth: Depth::U8,
        saved_channels: 0,
        retained_merged_alpha: false,
        rasterized_stacks: 0,
        emitted_raster_layers: 0,
    };
    let opaque_eligible = estimate_rasterized_psd_copy(large).unwrap();
    assert_eq!(opaque_eligible.guaranteed_composite_bytes, 210_000_000);
    assert_eq!(opaque_eligible.possible_composite_bytes, 280_000_000);
    assert!(estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        retained_merged_alpha: true,
        ..large
    }).is_err());
    // 53 saved planes are valid only if the content stays opaque.
    assert!(estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        canvas: Extent::new(2, 2),
        saved_channels: 53,
        ..large
    }).is_ok());
    assert!(estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        canvas: Extent::new(2, 2),
        saved_channels: 54,
        ..large
    }).is_err());
}

#[test]
fn invalid_geometry_and_checked_weight_overflow_fail_without_allocation() {
    assert!(estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        canvas: Extent::new(0, 2),
        ..input(Depth::U8)
    }).is_err());
    assert!(estimate_rasterized_psd_copy(PsdCopyEstimateInput {
        rasterized_stacks: usize::MAX,
        ..input(Depth::F32)
    }).is_err());
}
