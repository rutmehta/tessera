use image::{Rgb, RgbImage};
use ml_segment::{
    Click, MaskRaster, MaskStore, Prompts, cache_key, image_hash, person_box, sky_prior,
};
#[test]
fn lossless_cache_survives_reopen_and_corruption_is_miss() {
    let dir = tempfile::tempdir().unwrap();
    let mask = MaskRaster::new(2, 2, vec![0., 0.12345678, 0.7, 1.]).unwrap();
    let store = MaskStore::new(dir.path(), 1024).unwrap();
    store.put(&[1; 32], &mask).unwrap();
    drop(store);
    let store = MaskStore::new(dir.path(), 1024).unwrap();
    assert_eq!(store.get(&[1; 32]), Some(mask));
    assert!(store.get(&[2; 32]).is_none());
    let path = std::fs::read_dir(dir.path())
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[17] ^= 1;
    std::fs::write(path, bytes).unwrap();
    assert!(store.get(&[1; 32]).is_none());
}
#[test]
fn cache_budget_and_identity() {
    let dir = tempfile::tempdir().unwrap();
    let store = MaskStore::new(dir.path(), 64).unwrap();
    let mask = MaskRaster::new(2, 2, vec![0.5; 4]).unwrap();
    store.put(&[1; 32], &mask).unwrap();
    store.put(&[2; 32], &mask).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    let image = RgbImage::new(2, 2);
    let prompts = Prompts::default();
    let key = cache_key(&image, "subject", "v1", &prompts, 0).unwrap();
    for (kind, version, level) in [("sky", "v1", 0), ("subject", "v2", 0), ("subject", "v1", 1)] {
        assert_ne!(
            key,
            cache_key(&image, kind, version, &prompts, level).unwrap()
        );
    }
    assert_ne!(image_hash(&image), image_hash(&RgbImage::new(1, 4)));
    let mut changed = image.clone();
    changed.put_pixel(0, 0, Rgb([1, 0, 0]));
    assert_ne!(
        key,
        cache_key(&changed, "subject", "v1", &prompts, 0).unwrap()
    );
    let positive = Prompts {
        clicks: vec![Click {
            point: [0.5, 0.5],
            positive: true,
        }],
        boxes: vec![],
    };
    let mut negative = positive.clone();
    negative.clicks[0].positive = false;
    assert_ne!(
        cache_key(&image, "sam", "v1", &positive, 0).unwrap(),
        cache_key(&image, "sam", "v1", &negative, 0).unwrap()
    );
}
#[test]
fn validates_prompts_and_face_boxes() {
    assert!(Prompts::default().validate().is_err());
    for b in [[0., 0., 0., 1.], [0., 0., 2., 1.], [0., f32::NAN, 1., 1.]] {
        assert!(
            Prompts {
                clicks: vec![],
                boxes: vec![b]
            }
            .validate()
            .is_err()
        );
    }
    let image = RgbImage::new(100, 100);
    assert_eq!(
        person_box(&image, [40., 10., 20., 20.]).unwrap(),
        [0.2, 0., 0.8, 1.]
    );
    assert!(person_box(&image, [200., 10., 20., 20.]).is_err());
    assert!(person_box(&image, [0., 0., 0., 1.]).is_err());
}
#[test]
fn sky_prior_is_top_connected_and_excludes_subject() {
    let image = RgbImage::from_fn(20, 20, |_, y| {
        if !(8..=12).contains(&y) {
            Rgb([40, 120, 240])
        } else {
            Rgb([50, 50, 50])
        }
    });
    let subject = MaskRaster::new(
        20,
        20,
        (0..400).map(|i| if i % 20 < 3 { 1. } else { 0. }).collect(),
    )
    .unwrap();
    let sky = sky_prior(&image, &subject).unwrap();
    assert_eq!(sky.data()[4], 1.);
    assert_eq!(sky.data()[0], 0.);
    assert_eq!(sky.data()[19 * 20 + 4], 0.);
}
