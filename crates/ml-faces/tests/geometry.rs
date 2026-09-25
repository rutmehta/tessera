use image::{Rgb, RgbImage};
use ml_faces::{Face, align_crop, face_signals, letterbox, nms};

fn face() -> Face {
    Face {
        bbox: [10., 10., 90., 100.],
        landmarks5: [
            [38.2946, 51.6963],
            [73.5318, 51.5014],
            [56.0252, 71.7366],
            [41.5493, 92.3655],
            [70.7299, 92.2041],
        ],
        score: 0.9,
    }
}
#[test]
fn letterbox_preserves_aspect_ratio_and_bgr() {
    let image = RgbImage::from_pixel(200, 100, Rgb([10, 20, 30]));
    let (tensor, mapping) = letterbox(&image).unwrap();
    assert_eq!(tensor.shape(), [1, 3, 640, 640]);
    assert_eq!(mapping.unproject([320., 320.]), [100., 50.]);
    assert_eq!(tensor.data()[320 * 640 + 320], 30.);
    assert_eq!(tensor.data()[0], 0.);
    assert!(letterbox(&RgbImage::new(0, 2)).is_err());
}
#[test]
fn nms_keeps_highest_score_and_disjoint_boxes() {
    let a = face();
    let mut b = a.clone();
    b.score = 0.8;
    let mut c = a.clone();
    c.bbox[0] = 200.;
    let result = nms(vec![b, a, c], 0.3).unwrap();
    assert_eq!(result.len(), 2);
    assert_eq!(result[0].score, 0.9);
    assert!(nms(vec![], f32::NAN).is_err());
}
#[test]
fn alignment_is_identity_for_canonical_landmarks_and_rejects_degenerate() {
    let image = RgbImage::from_fn(112, 112, |x, y| Rgb([x as u8, y as u8, 20]));
    let aligned = align_crop(&image, &face()).unwrap();
    assert_eq!(aligned.dimensions(), (112, 112));
    for (a, b) in aligned.pixels().zip(image.pixels()) {
        for c in 0..3 {
            assert!(a[c].abs_diff(b[c]) <= 1);
        }
    }
    let mut bad = face();
    bad.landmarks5 = [[2., 2.]; 5];
    assert!(align_crop(&image, &bad).is_err());
}
#[test]
fn face_sharpness_separates_blur_and_geometry_proxy_is_bounded() {
    let image = RgbImage::from_fn(112, 112, |x, y| {
        Rgb([if (x / 4 + y / 4) % 2 == 0 { 30 } else { 220 }; 3])
    });
    let a = face_signals(&image, &face()).unwrap();
    let b = face_signals(&image::imageops::blur(&image, 2.), &face()).unwrap();
    assert!(a.sharpness > b.sharpness);
    assert!((0.0..=1.0).contains(&a.eyes_open.unwrap()));
    let mut bad = face();
    bad.bbox = [200., 0., 10., 10.];
    assert!(face_signals(&image, &bad).is_err());
}
