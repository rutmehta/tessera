use super::*;
#[test]
fn lanczos_preserves_constants_and_suppresses_aliasing() {
    let token = CancellationToken::new();
    let flat = Rgb32FImage::from_pixel(63, 47, image::Rgb([0.3, 0.5, 0.8]));
    let out = resize(flat, Resize::Fit(17, 11), &token).unwrap();
    for p in out.pixels() {
        for (a, b) in p.0.into_iter().zip([0.3, 0.5, 0.8]) {
            assert!((a - b).abs() < 1e-5);
        }
    }
    let checker = Rgb32FImage::from_fn(128, 128, |x, y| image::Rgb([((x + y) % 2) as f32; 3]));
    let out = resize(checker, Resize::LongEdge(16), &token).unwrap();
    for y in 2..14 {
        for x in 2..14 {
            assert!((out.get_pixel(x, y)[0] - 0.5).abs() < 0.005);
        }
    }
}
#[test]
fn lanczos_matches_independent_reference() {
    let src = Rgb32FImage::from_fn(73, 41, |x, y| {
        image::Rgb([((x * 17 + y * 3) % 101) as f32 / 100.0; 3])
    });
    let expected = image::imageops::resize(&src, 23, 13, image::imageops::FilterType::Lanczos3);
    let actual = resize(src, Resize::LongEdge(23), &CancellationToken::new()).unwrap();
    assert_eq!(actual.dimensions(), expected.dimensions());
    // Both are separable, but pass ordering and edge extension differ.
    for y in 3..10 {
        for x in 3..20 {
            assert!((actual.get_pixel(x, y)[0] - expected.get_pixel(x, y)[0]).abs() < 0.002);
        }
    }
}
