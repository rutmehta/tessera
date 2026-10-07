//! Synthetic LinearRaw fixtures only. The thumbnail keeps the decoder's safety
//! checks and sample interpretation while bounding the assembled image.
use raw_decode::lossy_dng;
use std::io::Cursor;

#[test]
fn lr13c_thumbnail_is_small_and_preserves_camera_samples() {
    for bytes in [
        include_bytes!("fixtures/linear-gradient-jxl.dng").as_slice(),
        include_bytes!("fixtures/linear-gradient.dng").as_slice(),
    ] {
        let full = lossy_dng::read(&mut Cursor::new(bytes)).unwrap().unwrap();
        let small = lossy_dng::read_thumbnail(&mut Cursor::new(bytes), 8)
            .unwrap()
            .unwrap();
        assert!(small.width.max(small.height) <= 8);
        assert!(full.width.max(full.height) > 8);
        assert_eq!(small.pixels.len(), (small.width * small.height) as usize);
        assert_eq!(small.orientation, full.metadata.orientation);
        let step = full.width.max(full.height).div_ceil(8);
        for y in 0..small.height as usize {
            for x in 0..small.width as usize {
                assert_eq!(
                    small.pixels[y * small.width as usize + x],
                    full.pixels[y * step * full.width + x * step]
                );
            }
        }
    }
}

#[test]
fn lr13c_thumbnail_keeps_claim_and_corruption_rules() {
    assert!(
        lossy_dng::read_thumbnail(&mut Cursor::new(b"not a LinearRaw"), 8)
            .unwrap()
            .is_none()
    );
    let bytes = include_bytes!("fixtures/linear-gradient-jxl.dng");
    assert!(lossy_dng::read_thumbnail(&mut Cursor::new(bytes), 0).is_err());
    let truncated = &bytes[..bytes.len() - 16];
    assert!(lossy_dng::read(&mut Cursor::new(truncated)).is_err());
    assert!(lossy_dng::read_thumbnail(&mut Cursor::new(truncated), 8).is_err());
}
