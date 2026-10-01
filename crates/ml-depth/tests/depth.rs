use image::{Rgb, RgbImage};
use ml_depth::{DepthMap, DepthStore, cache_key};
#[test]
fn refinement_cache_and_far_plane() {
    let image = RgbImage::from_fn(32, 16, |x, _| Rgb([if x < 16 { 240 } else { 10 }; 3]));
    let low = DepthMap::from_prediction(2, 1, vec![1., 0.]).unwrap();
    let depth = low.refined(&image).unwrap();
    assert_eq!((depth.width(), depth.height()), (32, 16));
    assert!(depth.inverse_depth()[15] > 0.99);
    assert!(depth.inverse_depth()[16] < 0.01);
    let key = cache_key(&image, "v1");
    assert_ne!(key, cache_key(&image, "v2"));
    assert_ne!(key, cache_key(&RgbImage::new(16, 32), "v1"));
    let dir = tempfile::tempdir().unwrap();
    let store = DepthStore::new(dir.path(), 10000).unwrap();
    depth.store(&store, &key).unwrap();
    assert_eq!(DepthMap::cached(&store, &key).unwrap(), depth);
    use engine_api::recipe::mask::*;
    let group = LocalAdjustment {
        components: vec![MaskComponent {
            enabled: true,
            group: None,
            kind: MaskKind::Depth {
                range: [0.9, 1.],
                feather: 0.,
                model: None,
            },
            combine: MaskCombine::Add,
            invert: false,
        }],
        ..Default::default()
    };
    let guide = pipeline_cpu::Image::new(32, 16, vec![vec![0.; 512]; 3]).unwrap();
    let plane = depth.near_to_far();
    let mask = pipeline_cpu::masks::rasterize(
        &guide,
        &group,
        pipeline_cpu::masks::MaskOptions {
            depth: Some(&plane),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(mask[15], 0.);
    assert_eq!(mask[16], 1.);
}

#[test]
fn cached_depth_is_available_without_model_weights_and_helpers_use_recipe_direction() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ml_runtime::ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        dir.path().join("weights"),
    )
    .unwrap();
    let store = DepthStore::new(dir.path().join("depth"), 10000).unwrap();
    let image = RgbImage::new(4, 1);
    let depth = DepthMap::from_prediction(4, 1, vec![0., 1., 2., 3.]).unwrap();
    depth
        .store(&store, &cache_key(&image, ml_depth::MODEL_VERSION))
        .unwrap();
    let mut estimator = ml_depth::CachedDepthEstimator::new(registry, Default::default(), store);
    assert_eq!(estimator.estimate(&image).unwrap(), depth);
    let missing = estimator
        .estimate(&RgbImage::new(2, 2))
        .unwrap_err()
        .to_string();
    assert!(
        missing.contains("Lens Blur depth model is not cached"),
        "{missing}"
    );
    assert!(
        estimator
            .estimate(&RgbImage::new(2, 2))
            .unwrap_err()
            .to_string()
            .contains("Lens Blur depth model is not cached")
    );
    assert_eq!(
        std::fs::read_dir(dir.path().join("weights"))
            .unwrap()
            .count(),
        0
    );
    assert_eq!(depth.visualisation().get_pixel(0, 0).0, [0; 3]);
    assert_eq!(depth.visualisation().get_pixel(3, 0).0, [255; 3]);
    let subject = ml_segment::MaskRaster::new(4, 1, vec![0., 0., 1., 1.]).unwrap();
    let range = depth.focus_range_for_subject(&subject).unwrap();
    assert_eq!(range, [0., 1. - 2. / 3.]);
    assert!(
        depth
            .focus_range_for_subject(&ml_segment::MaskRaster::new(4, 1, vec![0.; 4]).unwrap())
            .is_err()
    );
    assert!(
        depth
            .focus_range_for_subject(&ml_segment::MaskRaster::new(1, 1, vec![1.]).unwrap())
            .is_err()
    );
}

#[test]
fn support_loader_reuses_subject_mask_without_weights() {
    let dir = tempfile::tempdir().unwrap();
    let image = RgbImage::new(2, 1);
    let store = DepthStore::new(dir.path().join("previews/depth-cache"), 10000).unwrap();
    DepthMap::from_prediction(2, 1, vec![0., 1.])
        .unwrap()
        .store(&store, &cache_key(&image, ml_depth::MODEL_VERSION))
        .unwrap();
    let mask_store = ml_segment::MaskStore::new(dir.path().join("mask-cache"), 10000).unwrap();
    let key = ml_segment::cache_key(
        &image,
        "subject",
        ml_segment::SUBJECT_VERSION,
        &Default::default(),
        0,
    )
    .unwrap();
    mask_store
        .put(
            &key,
            &ml_segment::MaskRaster::new(2, 1, vec![0., 1.]).unwrap(),
        )
        .unwrap();
    let mut estimator = ml_depth::CachedDepthEstimator::from_support(dir.path()).unwrap();
    assert_eq!(estimator.subject_focus(&image).unwrap(), [0., 0.]);
}

#[test]
fn corrupt_depth_cache_is_a_miss_and_failed_load_is_retryable() {
    let dir = tempfile::tempdir().unwrap();
    let registry = ml_runtime::ModelRegistry::open(
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
        dir.path().join("weights"),
    )
    .unwrap();
    let path = dir.path().join("depth");
    let store = DepthStore::new(&path, 10000).unwrap();
    let image = RgbImage::new(2, 1);
    let key = cache_key(&image, ml_depth::MODEL_VERSION);
    let depth = DepthMap::from_prediction(2, 1, vec![0., 1.]).unwrap();
    depth.store(&store, &key).unwrap();
    let entry = std::fs::read_dir(&path)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    std::fs::write(entry, b"truncated cache").unwrap();
    let mut estimator = ml_depth::CachedDepthEstimator::new(registry, Default::default(), store);
    assert!(
        estimator
            .estimate(&image)
            .unwrap_err()
            .to_string()
            .contains("not cached")
    );
    let store = DepthStore::new(&path, 10000).unwrap();
    depth.store(&store, &key).unwrap();
    assert_eq!(estimator.estimate(&image).unwrap(), depth);
    assert_eq!(
        std::fs::read_dir(dir.path().join("weights"))
            .unwrap()
            .count(),
        0
    );
}
