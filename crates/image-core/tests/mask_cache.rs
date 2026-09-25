use engine_api::{
    recipe::{LocalAdjustment, MaskComponent, MaskKind},
    stage::{ParamHash, StageId},
};
use image_core::MaskRasterCache;
use pipeline_cpu::{
    Image,
    masks::{GuidedRefinement, MaskOptions},
};
use std::sync::Arc;

#[test]
fn raster_cache_reuses_sliders_and_invalidates_every_raster_input() {
    let cache = MaskRasterCache::new(1 << 20);
    let image = Image::new(4, 3, vec![vec![0.2; 12]; 3]).unwrap();
    let mut group = LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::LuminanceRange {
            range: [0.1, 0.4],
            smoothness: 50.,
        })],
        ..Default::default()
    };
    let upstream = ParamHash::default();
    let options = MaskOptions::default();
    let first = cache
        .rasterize(&image, &group, 0, upstream, options)
        .unwrap();
    group.params.exposure = 2.;
    group.amount = 50.;
    group.name = "renamed".into();
    let again = cache
        .rasterize(&image, &group, 0, upstream, options)
        .unwrap();
    assert!(Arc::ptr_eq(&first, &again));
    assert_eq!(cache.stats().hits, 1);
    assert_eq!(cache.stats().inserts, 1);
    group.invert = true;
    assert!(!Arc::ptr_eq(
        &first,
        &cache
            .rasterize(&image, &group, 0, upstream, options)
            .unwrap()
    ));
    group.invert = false;
    cache
        .rasterize(&image, &group, 1, upstream, options)
        .unwrap();
    cache
        .rasterize(
            &image,
            &group,
            0,
            ParamHash::of(StageId::Color, &1),
            options,
        )
        .unwrap();
    let different = Image::new(4, 3, vec![vec![0.3; 12]; 3]).unwrap();
    cache
        .rasterize(&different, &group, 0, upstream, options)
        .unwrap();
    let dimensions = Image::new(3, 4, image.planes().to_vec()).unwrap();
    cache
        .rasterize(&dimensions, &group, 0, upstream, options)
        .unwrap();
    cache
        .rasterize(
            &image,
            &group,
            0,
            upstream,
            MaskOptions {
                depth: Some(&[0.2; 12]),
                ..options
            },
        )
        .unwrap();
    cache
        .rasterize(
            &image,
            &group,
            0,
            upstream,
            MaskOptions {
                depth: Some(&[0.3; 12]),
                ..options
            },
        )
        .unwrap();
    cache
        .rasterize(
            &image,
            &group,
            0,
            upstream,
            MaskOptions {
                refinement: Some(GuidedRefinement {
                    radius: 1,
                    epsilon: 0.01,
                }),
                ..options
            },
        )
        .unwrap();
    cache
        .rasterize(
            &image,
            &group,
            0,
            upstream,
            MaskOptions {
                color_smoothness: 25.,
                ..options
            },
        )
        .unwrap();
    assert_eq!(cache.stats().inserts, 10);
    assert_eq!(cache.bytes(), 10 * 12 * 4);
}

#[test]
fn raster_cache_is_byte_bounded_and_rejects_oversized_entries() {
    let cache = MaskRasterCache::new(12 * 4);
    let image = Image::new(4, 3, vec![vec![0.2; 12]; 3]).unwrap();
    let group = LocalAdjustment::default();
    for level in 0..3 {
        cache
            .rasterize(
                &image,
                &group,
                level,
                ParamHash::default(),
                MaskOptions::default(),
            )
            .unwrap();
    }
    assert_eq!(cache.bytes(), 48);
    assert_eq!(cache.stats().evictions, 2);
    let tiny = MaskRasterCache::new(47);
    tiny.rasterize(
        &image,
        &group,
        0,
        ParamHash::default(),
        MaskOptions::default(),
    )
    .unwrap();
    assert_eq!(tiny.bytes(), 0);
    assert_eq!(tiny.stats().rejected, 1);
    cache.clear();
    assert_eq!(cache.bytes(), 0);
    assert_eq!(cache.stats().inserts, 0);
}

#[test]
fn warm_geometric_cache_does_not_bypass_rgb_validation() {
    let cache = MaskRasterCache::new(1024);
    let group = LocalAdjustment::default();
    let image = Image::new(2, 1, vec![vec![0.1; 2]; 3]).unwrap();
    cache
        .rasterize(&image, &group, 0, ParamHash::default(), Default::default())
        .unwrap();
    let mono = Image::new(2, 1, vec![vec![0.1; 2]]).unwrap();
    assert!(
        cache
            .rasterize(&mono, &group, 0, ParamHash::default(), Default::default())
            .is_err()
    );
}
