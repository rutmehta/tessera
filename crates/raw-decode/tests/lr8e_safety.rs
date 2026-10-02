//! All malformed containers are assembled in memory; no external inputs.
use std::io::Cursor;

fn dng(fields: &[(u16, u16, u32, u32)]) -> Vec<u8> {
    let mut b = b"II*\0\x08\0\0\0".to_vec();
    b.extend((fields.len() as u16).to_le_bytes());
    for &(id, kind, count, value) in fields {
        b.extend(id.to_le_bytes());
        b.extend(kind.to_le_bytes());
        b.extend(count.to_le_bytes());
        b.extend(value.to_le_bytes());
    }
    b.extend(0u32.to_le_bytes());
    b
}
fn base() -> Vec<(u16, u16, u32, u32)> {
    vec![
        (256, 4, 1, 16),
        (257, 4, 1, 16),
        (258, 3, 1, 8),
        (259, 3, 1, 34892),
        (262, 3, 1, 34892),
        (277, 3, 1, 3),
        (322, 4, 1, 16),
        (323, 4, 1, 16),
        (324, 4, 1, 0),
        (325, 4, 1, 2),
    ]
}
fn rejected(fields: &[(u16, u16, u32, u32)], message: &str) {
    let result = raw_decode::lossy_dng::read_metadata(&mut Cursor::new(dng(fields)));
    let error = result.expect_err("identified malformed LinearRaw must fail closed");
    assert!(error.to_string().contains(message), "{error}");
}
#[test]
fn pre_identification_errors_fall_through() {
    for bytes in [
        vec![],
        vec![0; 7],
        dng(&[(262, 3, 1, 32803), (256, 99, 1, 16)]),
        dng(&[(262, 3, 1, 32803), (330, 4, 1, u32::MAX)]),
    ] {
        assert!(
            raw_decode::lossy_dng::read(&mut Cursor::new(&bytes))
                .unwrap()
                .is_none()
        );
        assert!(
            raw_decode::lossy_dng::read_metadata(&mut Cursor::new(bytes))
                .unwrap()
                .is_none()
        );
    }
}
#[test]
fn identified_linear_raw_rejects_bad_fields_even_before_photo_tag() {
    rejected(
        &[
            (256, 99, 1, 16),
            (262, 3, 1, 34892),
            (259, 3, 1, 34892),
            (277, 3, 1, 3),
        ],
        "unsupported TIFF field",
    );
}
#[test]
fn oversized_tiles_rejected_before_payload_or_calibration() {
    let mut f = base();
    f[6].3 = 65535;
    f[7].3 = 65535;
    rejected(&f, "tile dimensions");
}
#[test]
fn total_decoded_tile_budget_is_checked() {
    let mut f = base();
    f[0].3 = 8192;
    f[1].3 = 8192;
    f[6].3 = 8192;
    f[7].3 = 8192;
    rejected(&f, "decoded byte budget");
}
#[test]
fn huge_dimensions_and_tile_count_overflow_are_rejected() {
    let mut f = base();
    f[0].3 = u32::MAX;
    f[1].3 = u32::MAX;
    f[6].3 = 1;
    f[7].3 = 1;
    rejected(&f, "dimension");
    f = base();
    f[8].2 = u32::MAX;
    rejected(&f, "field range");
}
#[test]
fn cyclic_ifd_after_identification_fails_closed() {
    let mut b = dng(&base());
    let n = b.len();
    b[n - 4..].copy_from_slice(&8u32.to_le_bytes());
    assert_eq!(
        raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .err()
            .unwrap()
            .to_string(),
        "cyclic or excessive TIFF IFD graph"
    );
}
#[test]
fn bad_linearization_table_lengths() {
    for n in [0, 1, 2] {
        let mut f = base();
        f.push((50712, 3, n, 0));
        rejected(&f, "linearization");
    }
}
// A complete calibrated IFD with a supplied in-memory codec payload.
fn calibrated_tile(compression: u32, payload: &[u8]) -> Vec<u8> {
    calibrated_tile_with_fields(compression, payload, &[])
}
fn calibrated_tile_with_fields(
    compression: u32,
    payload: &[u8],
    extra: &[(u16, u16, u32, u32)],
) -> Vec<u8> {
    let mut f = base();
    f[3].3 = compression;
    f[9].3 = payload.len() as u32;
    f.extend([(50721, 10, 9, 0), (50728, 5, 3, 0)]);
    f.extend_from_slice(extra);
    let start = dng(&f).len() as u32;
    f[10].3 = start;
    f[11].3 = start + 72;
    f[8].3 = start + 96;
    let mut b = dng(&f);
    for i in 0..9 {
        b.extend(u32::from(i % 4 == 0).to_le_bytes());
        b.extend(1u32.to_le_bytes());
    }
    for _ in 0..3 {
        b.extend(1u32.to_le_bytes());
        b.extend(1u32.to_le_bytes());
    }
    b.extend(payload);
    b
}
#[test]
fn truncated_codecs_fail_closed() {
    for (compression, bytes) in [(34892, &[255, 216][..]), (52546, &[255, 10][..])] {
        let b = calibrated_tile(compression, bytes);
        // Prove failures reach the codec, rather than failing unrelated metadata validation.
        assert!(
            raw_decode::lossy_dng::read_metadata(&mut Cursor::new(&b))
                .unwrap()
                .is_some()
        );
        let error = raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .err()
            .unwrap();
        assert!(
            error
                .to_string()
                .contains(if compression == 34892 { "JPEG" } else { "JXL" }),
            "{error}"
        );
    }
}
#[test]
fn ycbcr_classic_jpeg_is_not_admitted_as_camera_channels() {
    // APP14 transform=1 identifies YCbCr. This IFD must not reach marker stripping.
    let mut b = calibrated_tile(
        34892,
        &[
            255, 216, 255, 238, 0, 14, b'A', b'd', b'o', b'b', b'e', 0, 100, 0, 0, 0, 0, 1, 255,
            217,
        ],
    );
    b[10 + 4 * 12 + 8..10 + 4 * 12 + 10].copy_from_slice(&6u16.to_le_bytes());
    assert!(
        raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .unwrap()
            .is_none()
    );
}
#[test]
fn invalid_crop_and_optional_numbers_fail_before_codec_reads() {
    for (id, kind, count, value, expected) in [
        (50719, 4, 1, 0, "crop"),
        (50730, 11, 1, f32::NAN.to_bits(), "nonfinite"),
        (274, 11, 1, 1.5f32.to_bits(), "integer"),
        (50964, 4, 1, 1, "matrix"),
    ] {
        let b = calibrated_tile_with_fields(34892, &[0, 0], &[(id, kind, count, value)]);
        // SOI is absent; if decoding starts first the test gets a JPEG error.
        let error = raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .err()
            .unwrap();
        assert!(error.to_string().contains(expected), "{error}");
    }
}
#[test]
fn small_mutation_corpus_never_panics() {
    let seed = dng(&base());
    for n in 0..seed.len() {
        let mut b = seed.clone();
        b[n] ^= 255;
        let _ = raw_decode::lossy_dng::read(&mut Cursor::new(b));
        let _ = raw_decode::lossy_dng::read(&mut Cursor::new(&seed[..n]));
    }
}

#[test]
fn ordinary_cfa_with_exotic_ifd_still_decodes_through_libraw() {
    let mut f = vec![
        (256, 4, 1, 32),
        (257, 4, 1, 32),
        (258, 3, 1, 16),
        (259, 3, 1, 1),
        (262, 3, 1, 32803),
        (273, 4, 1, 0),
        (277, 3, 1, 1),
        (278, 4, 1, 32),
        (279, 4, 1, 2048),
        (33421, 3, 2, 0x00020002),
        (33422, 1, 4, 0x02010100),
        (50706, 1, 4, 0x00000401),
        (50707, 1, 4, 0x00000101),
        (50717, 4, 1, 65535),
        (50721, 10, 9, 0),
        (50778, 3, 1, 21),
        // LibRaw ignores this optional field; the lossy parser used to reject its type.
        (50779, 99, 1, 0),
    ];
    let header = dng(&f).len() as u32;
    f[5].3 = header + 72;
    f[14].3 = header;
    let mut b = dng(&f);
    for i in 0..9 {
        b.extend(u32::from(i % 4 == 0).to_le_bytes());
        b.extend(1u32.to_le_bytes());
    }
    for _ in 0..1024 {
        b.extend(1024u16.to_le_bytes());
    }
    let file = tempfile::Builder::new().suffix(".dng").tempfile().unwrap();
    std::fs::write(file.path(), &b).unwrap();
    let mut raw = raw_decode::RawSource::open(file.path()).unwrap();
    raw.decode_cfa().unwrap();
    assert!(
        raw_decode::lossy_dng::read(&mut Cursor::new(b))
            .unwrap()
            .is_none()
    );
}

#[test]
fn jpeg_header_dimensions_are_limited_before_decode() {
    let jpeg = [
        255, 216, 255, 192, 0, 17, 8, 255, 255, 255, 255, 3, 1, 17, 0, 2, 17, 0, 3, 17, 0, 255, 218,
    ];
    let b = calibrated_tile(34892, &jpeg);
    let error = raw_decode::lossy_dng::read(&mut Cursor::new(b))
        .err()
        .unwrap();
    assert!(error.to_string().contains("limit"), "{error}");
}

#[test]
fn jxl_header_dimensions_are_limited_before_frame_data() {
    // Raw codestream: 65535-square SizeHeader, default 8-bit metadata, no frame.
    // LSB-first fields: signature, div8, height selector/value, square ratio,
    // all-default metadata, default opsin matrix, then zero byte padding.
    let mut bits = Vec::new();
    for (value, width) in [
        (0xaffu32, 16),
        (0, 1),
        (2, 2),
        (65534, 18),
        (1, 3),
        (1, 1),
        (1, 1),
    ] {
        bits.extend((0..width).map(|i| ((value >> i) & 1) as u8));
    }
    let payload: Vec<u8> = bits
        .chunks(8)
        .map(|chunk| {
            chunk
                .iter()
                .enumerate()
                .fold(0, |byte, (i, bit)| byte | (bit << i))
        })
        .collect();
    let bytes = calibrated_tile(52546, &payload);
    let error = raw_decode::lossy_dng::read(&mut Cursor::new(bytes))
        .err()
        .unwrap();
    assert!(
        error.to_string().contains("JXL tile size exceeds"),
        "{error}"
    );
}
