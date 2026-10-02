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
fn jpeg_xl_tag_does_not_accept_classic_jpeg_payload() {
    let mut bytes = support::lossy_dng(false, false);
    let ifd = 38;
    let n = u16::from_le_bytes(bytes[ifd..ifd + 2].try_into().unwrap()) as usize;
    for entry in bytes[ifd + 2..ifd + 2 + n * 12].as_chunks_mut::<12>().0 {
        if entry[..2] == 259_u16.to_le_bytes() {
            entry[8..10].copy_from_slice(&52546_u16.to_le_bytes());
        }
    }
    let error = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
        .err()
        .expect("invalid JPEG XL payload");
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
}

#[test]
fn adobe_marker_and_rgb_component_ids_do_not_transform_camera_channels() {
    let original = include_bytes!("fixtures/linear-gradient.jpg");
    let baseline =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    for rgb_ids in [false, true] {
        let mut jpeg = original.to_vec();
        if rgb_ids {
            let mut pos = 2;
            loop {
                let marker = jpeg[pos + 1];
                let length = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
                if marker == 0xc0 {
                    for (c, id) in b"RGB".iter().enumerate() {
                        jpeg[pos + 10 + c * 3] = *id;
                    }
                }
                if marker == 0xda {
                    for (c, id) in b"RGB".iter().enumerate() {
                        jpeg[pos + 5 + c * 2] = *id;
                    }
                    break;
                }
                pos += 2 + length;
            }
        }
        jpeg.splice(
            2..2,
            [
                255, 238, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, 0,
            ],
        );
        let dng = support::lossy_dng_with_jpeg(false, false, &jpeg);
        let decoded = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(dng))
            .unwrap()
            .unwrap();
        assert_eq!(decoded.pixels, baseline.pixels);
    }
}

#[test]
fn required_polynomial_maps_only_selected_area_and_plane_before_crop() {
    let mut payload = Vec::new();
    for v in [4u32, 3, 8, 9, 1, 1, 1, 1, 2] {
        payload.extend(v.to_be_bytes());
    }
    for v in [0.1_f64, 0.5, 0.25] {
        payload.extend(v.to_be_bytes());
    }
    let mut list = Vec::new();
    for v in [1u32, 8, 0x01030000, 0, payload.len() as u32] {
        list.extend(v.to_be_bytes());
    }
    list.extend(payload);
    let base =
        raw_decode::lossy_dng::read(&mut std::io::Cursor::new(support::lossy_dng(false, false)))
            .unwrap()
            .unwrap();
    let bytes = support::lossy_dng_with_opcodes(
        false,
        false,
        include_bytes!("fixtures/linear-gradient.jpg"),
        &list,
    );
    let mapped = raw_decode::lossy_dng::read(&mut std::io::Cursor::new(bytes))
        .unwrap()
        .unwrap();
    for y in 0..10 {
        for x in 0..12 {
            for c in 0..3 {
                let v = base.pixels[y * 12 + x][c];
                let expected = if (4..8).contains(&(y + 3)) && (3..9).contains(&(x + 2)) && c == 1 {
                    0.1 + 0.5 * v + 0.25 * v * v
                } else {
                    v
                };
                assert!((mapped.pixels[y * 12 + x][c] - expected).abs() < 1e-6);
            }
        }
    }
    assert!(
        mapped.metadata.opcode_lists[1].is_none(),
        "consumed exactly once"
    );
}

#[test]
fn metadata_projection_does_not_decode_or_read_jpeg_tiles() {
    let mut bytes = support::lossy_dng(false, false);
    // Metadata remains readable even when the image payload is corrupt.
    let last = bytes.len() - 100;
    bytes[last..].fill(0);
    let metadata = raw_decode::lossy_dng::read_metadata(&mut std::io::Cursor::new(&bytes))
        .unwrap()
        .unwrap();
    assert_eq!(metadata.orientation, 6);
    assert_eq!((metadata.width, metadata.height), (12, 10));
    assert!(raw_decode::lossy_dng::read(&mut std::io::Cursor::new(&bytes)).is_err());
}
