//! Bounded, uncompressed float32 LinearRaw DNG export.
use std::io::{self, Write};

/// Writes classic little-endian TIFF, one chunky RGB float strip, DNG 1.4.
/// Samples remain unscaled (including negative values and highlights above 1).
/// ColorMatrix1 is XYZ -> camera under D65. Rationals are rounded to 1e-6;
/// unrepresentable metadata is rejected. Limits: 64 Mi pixels and 1 MiB XMP.
/// Validation completes before writing; I/O failures can leave partial output.
pub fn write<W: Write>(writer: &mut W, image: &crate::LinearImage, xmp: &str) -> io::Result<()> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid linear DNG image or metadata",
        )
    };
    let count = image.width.checked_mul(image.height).ok_or_else(invalid)?;
    if count == 0
        || count > 64 * 1024 * 1024
        || count != image.pixels.len()
        || image.width > u32::MAX as usize
        || image.height > u32::MAX as usize
        || xmp.len() > 1024 * 1024
        || image.pixels.iter().flatten().any(|v| !v.is_finite())
    {
        return Err(invalid());
    }
    let mut matrix = Vec::new();
    for &v in image.color_matrix.iter().flatten() {
        let n = (v * 1_000_000.0).round();
        if !n.is_finite() || n < i32::MIN as f64 || n > i32::MAX as f64 {
            return Err(invalid());
        }
        matrix.extend_from_slice(&(n as i32).to_le_bytes());
        matrix.extend_from_slice(&1_000_000i32.to_le_bytes());
    }
    let mut neutral = Vec::new();
    for &v in &image.as_shot_neutral {
        let n = (v * 1_000_000.0).round();
        if !n.is_finite() || n < 1.0 || n > u32::MAX as f64 {
            return Err(invalid());
        }
        neutral.extend_from_slice(&(n as u32).to_le_bytes());
        neutral.extend_from_slice(&1_000_000u32.to_le_bytes());
    }
    // (tag, TIFF type, element count, encoded value), sorted below.
    let short = |v: u16| v.to_le_bytes().to_vec();
    let long = |v: u32| v.to_le_bytes().to_vec();
    let mut tags = vec![
        (254u16, 4u16, 1u32, long(0)),
        (256, 4, 1, long(image.width as u32)),
        (257, 4, 1, long(image.height as u32)),
        (258, 3, 3, [32u16.to_le_bytes(); 3].concat()),
        (259, 3, 1, short(1)),
        (262, 3, 1, short(34892)),
        (273, 4, 1, long(0)),
        (274, 3, 1, short(1)),
        (277, 3, 1, short(3)),
        (278, 4, 1, long(image.height as u32)),
        (279, 4, 1, long((count * 12) as u32)),
        (284, 3, 1, short(1)),
        (339, 3, 3, [3u16.to_le_bytes(); 3].concat()),
        (700, 1, xmp.len() as u32, xmp.as_bytes().to_vec()),
        (50706, 1, 4, vec![1, 4, 0, 0]),
        (50707, 1, 4, vec![1, 4, 0, 0]),
        (
            50708,
            2,
            b"Tessera Linear\0".len() as u32,
            b"Tessera Linear\0".to_vec(),
        ),
        (50717, 4, 3, [1u32.to_le_bytes(); 3].concat()),
        (50721, 10, 9, matrix),
        (50728, 5, 3, neutral),
        (50778, 3, 1, short(21)), // D65
    ];
    if xmp.is_empty() {
        tags.retain(|t| t.0 != 700);
    }
    tags.sort_by_key(|t| t.0);
    let table_end = 8 + 2 + tags.len() * 12 + 4;
    let mut payload = Vec::new();
    let mut entries = Vec::new();
    for (tag, typ, n, data) in &tags {
        entries.extend_from_slice(&tag.to_le_bytes());
        entries.extend_from_slice(&typ.to_le_bytes());
        entries.extend_from_slice(&n.to_le_bytes());
        if data.len() <= 4 {
            let mut value = [0; 4];
            value[..data.len()].copy_from_slice(data);
            entries.extend_from_slice(&value);
        } else {
            if (table_end + payload.len()) % 2 != 0 {
                payload.push(0);
            }
            entries.extend_from_slice(&((table_end + payload.len()) as u32).to_le_bytes());
            payload.extend_from_slice(data);
        }
    }
    while (table_end + payload.len()) % 4 != 0 {
        payload.push(0);
    }
    let strip = (table_end + payload.len()) as u32;
    let strip_entry = tags.iter().position(|t| t.0 == 273).unwrap() * 12 + 8;
    entries[strip_entry..strip_entry + 4].copy_from_slice(&strip.to_le_bytes());
    writer.write_all(b"II\x2a\x00\x08\x00\x00\x00")?;
    writer.write_all(&(tags.len() as u16).to_le_bytes())?;
    writer.write_all(&entries)?;
    writer.write_all(&0u32.to_le_bytes())?;
    writer.write_all(&payload)?;
    // Buffer samples without duplicating the full image.
    let mut buffer = Vec::with_capacity(12 * 4096);
    for chunk in image.pixels.chunks(4096) {
        buffer.clear();
        for v in chunk.iter().flatten() {
            buffer.extend_from_slice(&v.to_le_bytes());
        }
        writer.write_all(&buffer)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn image() -> crate::LinearImage {
        crate::LinearImage {
            width: 2,
            height: 1,
            pixels: vec![[0.0, 1.25, 8.5], [-0.125, 0.5, 2.0]],
            color_matrix: [[0.75, -0.25, 0.125], [-0.5, 1.0, 0.25], [0.0, -0.125, 1.5]],
            as_shot_neutral: [0.5, 1.0, 0.75],
        }
    }

    fn encoded() -> Vec<u8> {
        let mut b = Vec::new();
        write(&mut b, &image(), "<xmp/>").unwrap();
        b
    }

    fn entry(b: &[u8], tag: u16) -> usize {
        (0..u16::from_le_bytes([b[8], b[9]]) as usize)
            .map(|i| 10 + i * 12)
            .find(|&i| u16::from_le_bytes([b[i], b[i + 1]]) == tag)
            .unwrap()
    }

    #[test]
    fn rejects_malformed_files() {
        let original = encoded();
        for n in 0..original.len() {
            assert!(
                raw_decode::linear_dng::read(&mut Cursor::new(&original[..n])).is_err(),
                "truncation {n}"
            );
        }
        // Oversized dimensions, payloads, offsets, wrong counts and formats.
        for (tag, field, value) in [
            (256, 8, u32::MAX),
            (700, 4, u32::MAX),
            (273, 8, u32::MAX),
            (279, 8, 1),
            (50721, 4, 8),
            (259, 8, 8),
            (277, 8, 4),
            (262, 8, 2),
        ] {
            let mut b = original.clone();
            let i = entry(&b, tag) + field;
            b[i..i + 4].copy_from_slice(&value.to_le_bytes());
            assert!(
                raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err(),
                "tag {tag}"
            );
        }
        for tag in [50721, 50728] {
            let mut b = original.clone();
            let i = entry(&b, tag) + 8;
            let offset = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
            b[offset + 4..offset + 8].fill(0);
            assert!(raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err());
        }
        let mut b = original.clone();
        let i = entry(&b, 273) + 8;
        let offset = u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) as usize;
        b[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err());
        let mut b = original.clone();
        let i = entry(&b, 274);
        b[i..i + 2].copy_from_slice(&256u16.to_le_bytes());
        assert!(raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err());
        let mut b = original;
        let n = u16::from_le_bytes([b[8], b[9]]) as usize;
        b[10 + n * 12..14 + n * 12].copy_from_slice(&8u32.to_le_bytes());
        assert!(raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err());
    }

    #[test]
    fn rejects_unsupported_predictor_and_subifds() {
        for tag in [317u16, 330] {
            let mut b = encoded();
            let i = entry(&b, 274);
            b[i..i + 2].copy_from_slice(&tag.to_le_bytes());
            b[i + 8..i + 12].copy_from_slice(&2u32.to_le_bytes());
            assert!(
                raw_decode::linear_dng::read(&mut Cursor::new(b)).is_err(),
                "tag {tag}"
            );
        }
    }

    #[test]
    fn rejects_invalid_images_before_writing() {
        for case in 0..8 {
            let mut im = image();
            match case {
                0 => im.width = 0,
                1 => im.width = usize::MAX,
                2 => im.pixels.clear(),
                3 => im.pixels[0][0] = f32::INFINITY,
                4 => im.color_matrix[0][0] = f64::NAN,
                5 => im.color_matrix[0][0] = f64::MAX,
                6 => im.as_shot_neutral[0] = 0.0,
                _ => im.as_shot_neutral[0] = -1.0,
            }
            let mut b = Vec::new();
            assert_eq!(
                write(&mut b, &im, "").unwrap_err().kind(),
                io::ErrorKind::InvalidInput
            );
            assert!(b.is_empty());
        }
        assert!(write(&mut Vec::new(), &image(), &"x".repeat(1024 * 1024 + 1)).is_err());
    }

    #[test]
    fn empty_xmp_is_omitted() {
        let mut b = Vec::new();
        write(&mut b, &image(), "").unwrap();
        let n = u16::from_le_bytes([b[8], b[9]]) as usize;
        assert!(!(0..n).any(|i| u16::from_le_bytes([b[10 + i * 12], b[11 + i * 12]]) == 700));
        assert!(
            raw_decode::linear_dng::read(&mut Cursor::new(b))
                .unwrap()
                .xmp
                .is_empty()
        );
    }

    #[test]
    fn reads_big_endian_float_dng() {
        let original = encoded();
        let mut b = original.clone();
        b[..2].copy_from_slice(b"MM");
        b[2..4].reverse();
        b[4..8].reverse();
        b[8..10].reverse();
        let n = u16::from_le_bytes([original[8], original[9]]) as usize;
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
                b[i + 8..i + 12].reverse();
                u32::from_le_bytes(original[i + 8..i + 12].try_into().unwrap()) as usize
            } else {
                i + 8
            };
            if unit > 1 {
                let word = unit.min(4);
                for j in (offset..offset + count * unit).step_by(word) {
                    b[j..j + word].reverse();
                }
            }
            b[i..i + 2].reverse();
            b[i + 2..i + 4].reverse();
            b[i + 4..i + 8].reverse();
        }
        let i = entry(&original, 273) + 8;
        let strip = u32::from_le_bytes(original[i..i + 4].try_into().unwrap()) as usize;
        for j in (strip..b.len()).step_by(4) {
            b[j..j + 4].reverse();
        }
        let decoded = raw_decode::linear_dng::read(&mut Cursor::new(b)).unwrap();
        assert_eq!(decoded.pixels, image().pixels);
        assert_eq!(decoded.color_matrix, image().color_matrix);
        assert_eq!(decoded.as_shot_neutral, image().as_shot_neutral);
        assert_eq!(decoded.xmp, "<xmp/>");
    }

    #[test]
    fn linear_float_roundtrip() {
        let image = image();
        let xmp = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\">HDR ☀</x:xmpmeta>";
        let mut bytes = Vec::new();
        write(&mut bytes, &image, xmp).unwrap();
        let decoded = raw_decode::linear_dng::read(&mut Cursor::new(bytes)).unwrap();
        assert_eq!((decoded.width, decoded.height), (2, 1));
        assert_eq!(decoded.pixels, image.pixels);
        assert_eq!(decoded.color_matrix, image.color_matrix);
        assert_eq!(decoded.as_shot_neutral, image.as_shot_neutral);
        assert_eq!(decoded.xmp, xmp);
    }
}
