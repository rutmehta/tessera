use image::{Rgb, RgbImage};
use ml_embed::preprocess;

#[test]
fn siglip_rgb_nchw_normalization_and_batch() {
    let red = RgbImage::from_pixel(16, 20, Rgb([255, 0, 0]));
    let blue = RgbImage::from_pixel(20, 16, Rgb([0, 0, 255]));
    let pixels = preprocess(&[red, blue]).unwrap();
    assert_eq!(pixels.len(), 2 * 3 * 224 * 224);
    assert_eq!(&pixels[..224 * 224], vec![1.; 224 * 224]);
    assert_eq!(&pixels[224 * 224..3 * 224 * 224], vec![-1.; 2 * 224 * 224]);
    assert_eq!(&pixels[5 * 224 * 224..], vec![1.; 224 * 224]);
    assert!(preprocess(&[]).is_err());
    assert!(preprocess(&[RgbImage::new(0, 2)]).is_err());
}
