//! PSD/PSB channel compression. Buffers exclude the compression selector.
//!
//! `rows` includes stacked channel planes. Raw/RLE/ZIP support depths 1, 8,
//! 16, and 32; ZIP prediction supports 8, 16, and 32. Decoded buffers and RLE
//! row tables are limited to 256 MiB. Lengths are exact: framing/padding bytes
//! belong to the caller, not these buffers. ZIP uses a single zlib stream.
//! PSD RLE row lengths must fit u16; PSB uses u32.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Compression {
    Raw = 0,
    Rle = 1,
    Zip = 2,
    ZipPrediction = 3,
}

impl TryFrom<u16> for Compression {
    type Error = crate::Error;
    fn try_from(value: u16) -> crate::Result<Self> {
        match value {
            0 => Ok(Self::Raw),
            1 => Ok(Self::Rle),
            2 => Ok(Self::Zip),
            3 => Ok(Self::ZipPrediction),
            _ => Err(error("unknown compression selector")),
        }
    }
}

const MAX_DECODED: usize = 256 * 1024 * 1024;
fn error(message: &str) -> crate::Error {
    crate::Error(message.into())
}
fn layout(width: usize, rows: usize, depth: u16) -> crate::Result<(usize, usize)> {
    if !matches!(depth, 1 | 8 | 16 | 32) {
        return Err(error("unsupported bit depth"));
    }
    let row = width
        .checked_mul(depth as usize)
        .and_then(|n| n.checked_add(7))
        .map(|n| n / 8)
        .ok_or_else(|| error("row size overflow"))?;
    let total = row
        .checked_mul(rows)
        .ok_or_else(|| error("image size overflow"))?;
    if total > MAX_DECODED || row > MAX_DECODED {
        return Err(error("decoded size exceeds 256 MiB"));
    }
    Ok((row, total))
}

fn table_size(rows: usize, psb: bool) -> crate::Result<usize> {
    rows.checked_mul(if psb { 4 } else { 2 })
        .filter(|&n| n <= MAX_DECODED)
        .ok_or_else(|| error("RLE table too large"))
}

pub fn decode(
    data: &[u8],
    compression: Compression,
    width: usize,
    rows: usize,
    depth: u16,
    psb: bool,
) -> crate::Result<Vec<u8>> {
    let (row, total) = layout(width, rows, depth)?;
    match compression {
        Compression::Raw => {
            if data.len() != total {
                return Err(error("raw size mismatch"));
            }
            Ok(data.to_vec())
        }
        Compression::Rle => {
            let table = table_size(rows, psb)?;
            if data.len() < table {
                return Err(error("truncated RLE row table"));
            }
            let mut output = Vec::with_capacity(total);
            let mut offset = table;
            for count in data[..table].chunks_exact(if psb { 4 } else { 2 }) {
                let len = if psb {
                    u32::from_be_bytes(count.try_into().unwrap()) as usize
                } else {
                    u16::from_be_bytes(count.try_into().unwrap()) as usize
                };
                let end = offset
                    .checked_add(len)
                    .filter(|&n| n <= data.len())
                    .ok_or_else(|| error("truncated RLE row"))?;
                output.extend_from_slice(&decode_rle(&data[offset..end], row)?);
                offset = end;
            }
            if offset != data.len() {
                return Err(error("trailing RLE data"));
            }
            Ok(output)
        }
        Compression::Zip => inflate(data, total),
        Compression::ZipPrediction => {
            if depth == 1 {
                return Err(error("prediction requires 8, 16, or 32-bit depth"));
            }
            let mut output = inflate(data, total)?;
            predict(&mut output, row, depth, false);
            Ok(output)
        }
    }
}
pub fn encode(
    data: &[u8],
    compression: Compression,
    width: usize,
    rows: usize,
    depth: u16,
    psb: bool,
) -> crate::Result<Vec<u8>> {
    let (row, total) = layout(width, rows, depth)?;
    if data.len() != total {
        return Err(error("raw size mismatch"));
    }
    match compression {
        Compression::Raw => Ok(data.to_vec()),
        Compression::Rle => {
            let table = table_size(rows, psb)?;
            let mut output = vec![0; table];
            for i in 0..rows {
                let encoded = encode_rle(&data[i * row..(i + 1) * row]);
                if psb {
                    let len =
                        u32::try_from(encoded.len()).map_err(|_| error("PSB RLE row too large"))?;
                    output[i * 4..i * 4 + 4].copy_from_slice(&len.to_be_bytes());
                } else {
                    let len = u16::try_from(encoded.len())
                        .map_err(|_| error("PSD RLE row exceeds 65535 bytes"))?;
                    output[i * 2..i * 2 + 2].copy_from_slice(&len.to_be_bytes());
                }
                output.extend_from_slice(&encoded);
            }
            Ok(output)
        }
        Compression::Zip => deflate(data),
        Compression::ZipPrediction => {
            if depth == 1 {
                return Err(error("prediction requires 8, 16, or 32-bit depth"));
            }
            let mut predicted = data.to_vec();
            predict(&mut predicted, row, depth, true);
            deflate(&predicted)
        }
    }
}
// Prediction resets at every scanline (including stacked channel planes).
// 16-bit prediction operates on big-endian words, not individual bytes.
// 32-bit prediction first transposes each row into four byte planes, then
// differences the entire shuffled row, WITHOUT resetting at plane boundaries.
fn predict(data: &mut [u8], row_bytes: usize, depth: u16, encoding: bool) {
    if row_bytes == 0 || data.is_empty() {
        return;
    }
    let mut scratch = if depth == 32 {
        vec![0; row_bytes]
    } else {
        Vec::new()
    };
    for row in data.chunks_exact_mut(row_bytes) {
        if depth == 16 {
            let mut previous = 0u16;
            for word in row.as_chunks_mut::<2>().0 {
                let current = u16::from_be_bytes([word[0], word[1]]);
                let value = if encoding {
                    current.wrapping_sub(previous)
                } else {
                    current.wrapping_add(previous)
                };
                word.copy_from_slice(&value.to_be_bytes());
                previous = if encoding { current } else { value };
            }
        } else {
            let width = row_bytes / 4;
            if depth == 32 && encoding {
                for (i, &value) in row.iter().enumerate() {
                    scratch[(i % 4) * width + i / 4] = value;
                }
                row.copy_from_slice(&scratch);
            }
            let mut previous = 0u8;
            for byte in row.iter_mut() {
                let current = *byte;
                *byte = if encoding {
                    current.wrapping_sub(previous)
                } else {
                    current.wrapping_add(previous)
                };
                previous = if encoding { current } else { *byte };
            }
            if depth == 32 && !encoding {
                scratch.copy_from_slice(row);
                for (i, byte) in row.iter_mut().enumerate() {
                    *byte = scratch[(i % 4) * width + i / 4];
                }
            }
        }
    }
}

fn inflate(data: &[u8], expected: usize) -> crate::Result<Vec<u8>> {
    let mut decoder = flate2::Decompress::new(true);
    let mut output = Vec::new();
    let mut scratch = [0; 8192];
    loop {
        let before_in = decoder.total_in();
        let before_out = decoder.total_out();
        let status = decoder
            .decompress(
                &data[before_in as usize..],
                &mut scratch,
                flate2::FlushDecompress::None,
            )
            .map_err(|e| error(&format!("invalid ZIP data: {e}")))?;
        let produced = (decoder.total_out() - before_out) as usize;
        if produced > expected - output.len() {
            return Err(error("ZIP expands beyond expected size"));
        }
        output.extend_from_slice(&scratch[..produced]);
        if status == flate2::Status::StreamEnd {
            if output.len() != expected || decoder.total_in() as usize != data.len() {
                return Err(error("ZIP size mismatch or trailing data"));
            }
            return Ok(output);
        }
        if produced == 0 && decoder.total_in() == before_in {
            return Err(error("truncated ZIP stream"));
        }
    }
}

fn deflate(data: &[u8]) -> crate::Result<Vec<u8>> {
    use std::io::Write;
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(data)
        .map_err(|e| error(&format!("ZIP encoding failed: {e}")))?;
    encoder
        .finish()
        .map_err(|e| error(&format!("ZIP encoding failed: {e}")))
}

/// Decode one PackBits row, rejecting both underflow and over-expansion.
pub fn decode_rle(data: &[u8], expected: usize) -> crate::Result<Vec<u8>> {
    if expected > MAX_DECODED {
        return Err(error("decoded size exceeds 256 MiB"));
    }
    let mut output = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let control = data[pos];
        pos += 1;
        if control == 128 {
            continue;
        }
        let count = if control < 128 {
            control as usize + 1
        } else {
            257 - control as usize
        };
        if count > expected - output.len() {
            return Err(error("RLE row expands beyond expected size"));
        }
        if control < 128 {
            let end = pos
                .checked_add(count)
                .filter(|&n| n <= data.len())
                .ok_or_else(|| error("truncated RLE literal"))?;
            output.extend_from_slice(&data[pos..end]);
            pos = end;
        } else {
            let value = *data.get(pos).ok_or_else(|| error("truncated RLE repeat"))?;
            pos += 1;
            output.resize(output.len() + count, value);
        }
    }
    if output.len() != expected {
        return Err(error("RLE row size mismatch"));
    }
    Ok(output)
}

/// Encode a single row using PackBits packets of at most 128 bytes.
pub fn encode_rle(data: &[u8]) -> Vec<u8> {
    let mut output = Vec::new();
    let mut pos = 0;
    while pos < data.len() {
        let mut run = 1;
        while run < 128 && pos + run < data.len() && data[pos + run] == data[pos] {
            run += 1;
        }
        if run >= 3 {
            output.push((257 - run) as u8);
            output.push(data[pos]);
            pos += run;
        } else {
            let start = pos;
            pos += run;
            while pos < data.len() && pos - start < 128 {
                if pos + 2 < data.len() && data[pos] == data[pos + 1] && data[pos] == data[pos + 2]
                {
                    break;
                }
                pos += 1;
            }
            output.push((pos - start - 1) as u8);
            output.extend_from_slice(&data[start..pos]);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psd_rejects_oversized_rle_row_psb_accepts_it() {
        let raw: Vec<u8> = (0..65536).map(|n| n as u8).collect();
        assert!(encode(&raw, Compression::Rle, raw.len(), 1, 8, false).is_err());
        let bytes = encode(&raw, Compression::Rle, raw.len(), 1, 8, true).unwrap();
        assert_eq!(
            decode(&bytes, Compression::Rle, raw.len(), 1, 8, true).unwrap(),
            raw
        );
    }

    #[test]
    fn large_zip_streams_cross_scratch_boundaries() {
        let raw: Vec<u8> = (0..32768).map(|n| (n ^ (n >> 8)) as u8).collect();
        for mode in [Compression::Zip, Compression::ZipPrediction] {
            let bytes = encode(&raw, mode, 4096, 2, 32, false).unwrap();
            assert_eq!(decode(&bytes, mode, 4096, 2, 32, false).unwrap(), raw);
            assert!(decode(&bytes, mode, 1, 1, 32, false).is_err());
        }
    }

    #[test]
    fn malformed_random_inputs_never_panic() {
        let mut state = 0x9e3779b9u32;
        for len in 0..256 {
            let bytes: Vec<u8> = (0..len)
                .map(|_| {
                    state ^= state << 13;
                    state ^= state >> 17;
                    state ^= state << 5;
                    state as u8
                })
                .collect();
            let _ = decode_rle(&bytes, 32);
            for mode in [
                Compression::Raw,
                Compression::Rle,
                Compression::Zip,
                Compression::ZipPrediction,
            ] {
                for depth in [1, 8, 16, 32] {
                    for psb in [false, true] {
                        let _ = decode(&bytes, mode, 4, 2, depth, psb);
                    }
                }
            }
        }
    }

    #[test]
    fn prediction_known_external_vectors() {
        // Manually differenced rows, compressed independently with Python zlib.
        type PredictionCase = (u16, usize, usize, Vec<u8>, Vec<u8>, Vec<u8>);
        let cases: Vec<PredictionCase> = vec![
            (
                8,
                3,
                2,
                vec![250, 5, 4, 9, 7, 5],
                vec![250, 11, 255, 9, 254, 254],
                vec![120, 156, 251, 197, 253, 159, 243, 223, 63, 0, 13, 42, 4, 10],
            ),
            (
                16,
                3,
                2,
                vec![0, 255, 0, 2, 1, 4, 128, 0, 0, 1, 127, 255],
                vec![0, 255, 255, 3, 1, 2, 128, 0, 128, 1, 127, 254],
                vec![
                    120, 156, 99, 248, 255, 159, 153, 145, 169, 129, 161, 129, 177, 254, 31, 0, 28,
                    39, 4, 131,
                ],
            ),
            (
                32,
                2,
                1,
                vec![0x3f, 0x80, 0, 0xc0, 0x40, 0x81, 0, 0x3f],
                vec![0x3f, 1, 0x40, 1, 0x7f, 0, 0xc0, 0x7f],
                vec![
                    120, 156, 179, 103, 116, 96, 172, 103, 56, 80, 15, 0, 7, 135, 2, 64,
                ],
            ),
        ];
        for (depth, width, rows, raw, predicted, zipped) in cases {
            assert_eq!(
                decode(
                    &zipped,
                    Compression::ZipPrediction,
                    width,
                    rows,
                    depth,
                    false
                )
                .unwrap(),
                raw
            );
            let encoded =
                encode(&raw, Compression::ZipPrediction, width, rows, depth, false).unwrap();
            // Inspect pre-prediction bytes, so encoder/decoder bugs cannot cancel.
            assert_eq!(
                decode(&encoded, Compression::Zip, width, rows, depth, false).unwrap(),
                predicted
            );
        }
        assert!(encode(&[0], Compression::ZipPrediction, 1, 1, 1, false).is_err());
        assert!(decode(&[], Compression::ZipPrediction, 0, 0, 1, false).is_err());
    }
    #[test]
    fn compression_modes_roundtrip_depths_and_empty_shapes() {
        for mode in [
            Compression::Raw,
            Compression::Rle,
            Compression::Zip,
            Compression::ZipPrediction,
        ] {
            for depth in [1, 8, 16, 32] {
                if mode == Compression::ZipPrediction && depth == 1 {
                    continue;
                }
                for width in [0, 1, 2, 127, 128, 129, 257] {
                    for rows in [0, 1, 3] {
                        let len = (width * depth as usize).div_ceil(8) * rows;
                        let raw: Vec<u8> = (0..len)
                            .map(|n| (n.wrapping_mul(73) ^ (n >> 3)) as u8)
                            .collect();
                        for psb in [false, true] {
                            let encoded = encode(&raw, mode, width, rows, depth, psb).unwrap();
                            assert_eq!(
                                decode(&encoded, mode, width, rows, depth, psb).unwrap(),
                                raw
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn zip_known_external_zlib_stream() {
        // Python zlib.compress(bytes([1, 2, 3, 4])), not our encoder.
        let bytes = [120, 156, 99, 100, 98, 102, 1, 0, 0, 24, 0, 11];
        assert_eq!(
            decode(&bytes, Compression::Zip, 2, 2, 8, false).unwrap(),
            [1, 2, 3, 4]
        );
        let encoded = encode(&[1, 2, 3, 4], Compression::Zip, 2, 2, 8, false).unwrap();
        assert_eq!(
            decode(&encoded, Compression::Zip, 2, 2, 8, false).unwrap(),
            [1, 2, 3, 4]
        );
        for end in 0..bytes.len() {
            assert!(decode(&bytes[..end], Compression::Zip, 2, 2, 8, false).is_err());
        }
        assert!(decode(&bytes, Compression::Zip, 1, 1, 8, false).is_err());
        assert!(decode(&bytes, Compression::Zip, 5, 1, 8, false).is_err());
        let mut corrupt = bytes;
        corrupt[11] ^= 1;
        assert!(decode(&corrupt, Compression::Zip, 2, 2, 8, false).is_err());
        let mut trailing = bytes.to_vec();
        trailing.push(0);
        assert!(decode(&trailing, Compression::Zip, 2, 2, 8, false).is_err());
    }

    #[test]
    fn packbits_known_packets_and_errors() {
        let packed = [128, 2, 10, 20, 30, 254, 40, 255, 50, 128];
        assert_eq!(
            decode_rle(&packed, 8).unwrap(),
            [10, 20, 30, 40, 40, 40, 50, 50]
        );
        assert_eq!(encode_rle(&[7; 128]), [129, 7]);
        assert_eq!(encode_rle(&[1, 2, 3]), [2, 1, 2, 3]);
        for (bytes, expected) in [
            (vec![2, 1], 3),
            (vec![255], 2),
            (vec![129, 7], 127),
            (vec![], 1),
            (vec![0, 1], 0),
        ] {
            assert!(decode_rle(&bytes, expected).is_err());
        }
        assert!(decode_rle(&[], MAX_DECODED + 1).is_err());
        assert_eq!(decode_rle(&[128], 0).unwrap(), []);
    }
    #[test]
    fn rle_row_tables_known_psd_and_psb() {
        let raw = [7, 7, 7, 1, 2, 3];
        for (psb, bytes) in [
            (false, vec![0, 2, 0, 4, 254, 7, 2, 1, 2, 3]),
            (true, vec![0, 0, 0, 2, 0, 0, 0, 4, 254, 7, 2, 1, 2, 3]),
        ] {
            assert_eq!(decode(&bytes, Compression::Rle, 3, 2, 8, psb).unwrap(), raw);
            assert_eq!(encode(&raw, Compression::Rle, 3, 2, 8, psb).unwrap(), bytes);
            for end in 0..bytes.len() {
                assert!(decode(&bytes[..end], Compression::Rle, 3, 2, 8, psb).is_err());
            }
            let mut extra = bytes;
            extra.push(0);
            assert!(decode(&extra, Compression::Rle, 3, 2, 8, psb).is_err());
        }
        assert!(decode(&[], Compression::Rle, 0, usize::MAX, 8, true).is_err());
        assert!(encode(&[], Compression::Rle, 0, usize::MAX, 8, true).is_err());
    }
    #[test]
    fn randomized_packbits_roundtrips() {
        let mut state = 0x12345678u32;
        for len in (0..260).chain([511, 1024, 4096]) {
            let mut raw = Vec::new();
            for i in 0..len {
                state ^= state << 13;
                state ^= state >> 17;
                state ^= state << 5;
                raw.push(if i % 131 < 80 {
                    (i / 131) as u8
                } else {
                    state as u8
                });
            }
            assert_eq!(decode_rle(&encode_rle(&raw), len).unwrap(), raw);
        }
    }
    #[test]
    fn raw_known_bytes_and_validation() {
        for (value, mode) in [
            Compression::Raw,
            Compression::Rle,
            Compression::Zip,
            Compression::ZipPrediction,
        ]
        .into_iter()
        .enumerate()
        {
            assert_eq!(Compression::try_from(value as u16).unwrap(), mode);
        }
        assert!(Compression::try_from(4).is_err());
        let raw = [0x12, 0x34, 0xff, 0x00];
        assert_eq!(
            decode(&raw, Compression::Raw, 2, 1, 16, false).unwrap(),
            raw
        );
        assert_eq!(
            encode(&raw, Compression::Raw, 2, 1, 16, false).unwrap(),
            raw
        );
        for mode in [
            Compression::Raw,
            Compression::Rle,
            Compression::Zip,
            Compression::ZipPrediction,
        ] {
            assert!(decode(&[], mode, usize::MAX, 2, 32, false).is_err());
            assert!(decode(&[], mode, 268_435_457, 1, 8, false).is_err());
            assert!(decode(&[], mode, 1, 1, 7, false).is_err());
            assert!(encode(&[], mode, 1, 1, 8, false).is_err());
        }
        assert!(decode(&raw[..3], Compression::Raw, 2, 1, 16, false).is_err());
        assert_eq!(
            decode(&[0x80, 0], Compression::Raw, 9, 1, 1, false).unwrap(),
            [0x80, 0]
        );
    }
}
