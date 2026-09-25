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
