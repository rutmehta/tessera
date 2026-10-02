mod support;

#[test]
fn lossy_linear_tiles_preserve_camera_channels_crop_and_metadata() {
    for big_endian in [false, true] {
        for strips in [false, true] {
            let bytes = support::lossy_dng(big_endian, strips);
            let decoded = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
                .unwrap()
                .expect("linear DNG");
            assert_eq!((decoded.width, decoded.height), (12, 10));
            assert_eq!(decoded.metadata.orientation, 6);
            assert_eq!(decoded.metadata.make, "Synthetic");
            assert_eq!(decoded.metadata.as_shot_wb[..3], [2., 1., 1.5]);
            for y in 0..10 {
                for x in 0..12 {
                    let codes = [32 + (x + 2) * 8, 48 + (y + 3) * 7, 64 + (x + y + 5) * 4];
                    for (got, code) in decoded.pixels[y * 12 + x].iter().zip(codes) {
                        let expected = (code as f32 * 257. - 257.) / (65535. - 257.);
                        // Quality-100 4:4:4 JPEG IDCT may differ by two code values.
                        assert!((got - expected).abs() <= 2.1 / 254., "{got} vs {expected}");
                    }
                }
            }
        }
    }
}

#[test]
fn malformed_ifd_and_tile_ranges_fail_without_panicking() {
    let bytes = support::lossy_dng(false, false);
    for length in [0, 7, 12, bytes.len() / 2, bytes.len() - 4] {
        assert!(raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes[..length])).is_err());
    }
}

#[test]
fn jpeg_xl_is_explicitly_unsupported_not_mistaken_for_classic_jpeg() {
    let mut bytes = support::lossy_dng(false, false);
    let ifd = 38;
    let n = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    for entry in bytes[ifd + 2..ifd + 2 + n * 12].chunks_exact_mut(12) {
        if entry[..2] == 259_u16.to_le_bytes() {
            entry[8..10].copy_from_slice(&52546_u16.to_le_bytes());
        }
    }
    let error = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
        .err()
        .expect("unsupported JPEG XL");
    assert_eq!(error.kind(), std::io::ErrorKind::Unsupported);
}
