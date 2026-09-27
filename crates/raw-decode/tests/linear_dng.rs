#[path = "../../image-core/tests/support/linear_dng.rs"]
mod support;
use raw_decode::linear_dng::{is_linear_dng, read};
use std::io::Cursor;

fn entry(bytes: &[u8], tag: u16) -> usize {
    let n = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    (10..10 + n * 12)
        .step_by(12)
        .find(|&i| u16::from_le_bytes(bytes[i..i + 2].try_into().unwrap()) == tag)
        .unwrap()
}

#[test]
fn uint16_big_endian_matches_little_endian() {
    let original = support::fixture(16, 6, &[[0.12345, 0.5, 1.], [0., 0.0001, 0.75]]);
    let mut bytes = original.clone();
    bytes[..2].copy_from_slice(b"MM");
    bytes[2..4].reverse();
    bytes[4..8].reverse();
    bytes[8..10].reverse();
    let n = u16::from_le_bytes(original[8..10].try_into().unwrap()) as usize;
    for i in (10..10 + n * 12).step_by(12) {
        let typ = u16::from_le_bytes(original[i + 2..i + 4].try_into().unwrap());
        let count = u32::from_le_bytes(original[i + 4..i + 8].try_into().unwrap()) as usize;
        let unit = match typ {
            3 => 2,
            4 => 4,
            5 | 10 => 8,
            _ => 1,
        };
        let offset = if count * unit > 4 {
            bytes[i + 8..i + 12].reverse();
            u32::from_le_bytes(original[i + 8..i + 12].try_into().unwrap()) as usize
        } else {
            i + 8
        };
        if unit > 1 {
            for j in (offset..offset + count * unit).step_by(unit.min(4)) {
                bytes[j..j + unit.min(4)].reverse();
            }
        }
        bytes[i..i + 2].reverse();
        bytes[i + 2..i + 4].reverse();
        bytes[i + 4..i + 8].reverse();
    }
    let i = entry(&original, 273) + 8;
    let strip = u32::from_le_bytes(original[i..i + 4].try_into().unwrap()) as usize;
    for j in (strip..bytes.len()).step_by(2) {
        bytes[j..j + 2].reverse();
    }
    assert!(is_linear_dng(&mut Cursor::new(&bytes)).unwrap());
    let big = read(&mut Cursor::new(bytes)).unwrap();
    let little = read(&mut Cursor::new(original)).unwrap();
    assert_eq!(big.pixels, little.pixels);
    assert_eq!(big.orientation, 6);
    assert_eq!(big.color_matrix, little.color_matrix);
    assert_eq!(big.as_shot_neutral, little.as_shot_neutral);
}

#[test]
fn uint16_rejects_truncations_invalid_orientation_and_white_level() {
    let original = support::fixture(16, 1, &[[0.2, 0.3, 0.4]; 2]);
    for n in 0..original.len() {
        assert!(
            read(&mut Cursor::new(&original[..n])).is_err(),
            "truncation {n}"
        );
    }
    let mut bytes = original.clone();
    let i = entry(&bytes, 274) + 8;
    bytes[i..i + 2].copy_from_slice(&9u16.to_le_bytes());
    assert!(read(&mut Cursor::new(bytes)).is_err());
    let mut bytes = original;
    let i = entry(&bytes, 50717) + 8;
    let offset = u32::from_le_bytes(bytes[i..i + 4].try_into().unwrap()) as usize;
    bytes[offset..offset + 4].fill(0);
    assert!(read(&mut Cursor::new(bytes)).is_err());
}

#[test]
fn classifier_does_not_treat_rgb_tiff_or_cfa_as_linear_raw() {
    for photometric in [2u16, 32803] {
        let mut bytes = support::fixture(16, 1, &[[0.2, 0.3, 0.4]; 2]);
        let i = entry(&bytes, 262) + 8;
        bytes[i..i + 2].copy_from_slice(&photometric.to_le_bytes());
        assert!(!is_linear_dng(&mut Cursor::new(bytes)).unwrap());
    }
    assert!(!is_linear_dng(&mut Cursor::new(b"not a TIFF file")).unwrap());
}
