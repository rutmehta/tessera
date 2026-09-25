use image::{Rgb, RgbImage};
use ml_segment::{MaskRaster, refine};

#[test]
fn guided_refinement_preserves_edge() {
    let guide = RgbImage::from_fn(64, 32, |x, _| Rgb([if x < 32 { 0 } else { 255 }; 3]));
    let low = MaskRaster::new(
        8,
        4,
        (0..32).map(|i| if i % 8 < 4 { 0. } else { 1. }).collect(),
    )
    .unwrap();
    let out = refine(&low, &guide, 8, 0.0001).unwrap();
    assert!(out.data()[16 * 64 + 31] < 0.15);
    assert!(out.data()[16 * 64 + 32] > 0.85);
    assert!(MaskRaster::new(2, 2, vec![f32::NAN; 4]).is_err());
}
