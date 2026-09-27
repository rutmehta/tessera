//! Bounded float32/uint16 LinearRaw DNG interchange, not a general RAW decoder.
use std::collections::BTreeMap;
use std::io::{self, BufReader, Read, Seek, SeekFrom};

#[derive(Debug)]
pub struct LinearDng {
    pub width: usize,
    pub height: usize,
    /// EXIF orientation, not yet applied to camera samples.
    pub orientation: u16,
    pub pixels: Vec<[f32; 3]>,
    /// DNG ColorMatrix1: XYZ -> camera, without white balance applied.
    pub color_matrix: [[f64; 3]; 3],
    pub as_shot_neutral: [f64; 3],
    pub xmp: String,
}

/// Bounded first-IFD classifier, not a decoder. CFA DNG remains on LibRaw.
/// Recognized LinearRaw files must subsequently pass `read`; do not fall back
/// to CFA decoding when their layout or calibration is unsupported.
pub fn is_linear_dng<R: Read + Seek>(reader: &mut R) -> io::Result<bool> {
    let size = reader.seek(SeekFrom::End(0))?;
    if size < 8 {
        return Ok(false);
    }
    let h = at(reader, 0, 8, size)?;
    let le = match &h[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(false),
    };
    let short = |b: &[u8]| {
        if le {
            u16::from_le_bytes(b[..2].try_into().unwrap())
        } else {
            u16::from_be_bytes(b[..2].try_into().unwrap())
        }
    };
    let long = |b: &[u8]| {
        if le {
            u32::from_le_bytes(b[..4].try_into().unwrap())
        } else {
            u32::from_be_bytes(b[..4].try_into().unwrap())
        }
    };
    if short(&h[2..]) != 42 {
        return Ok(false);
    }
    let ifd = u64::from(long(&h[4..]));
    let n = usize::from(short(&at(reader, ifd, 2, size)?));
    if n > 4096 {
        return Err(invalid("IFD classifier budget exceeded"));
    }
    let entries = at(reader, ifd + 2, n * 12, size)?;
    Ok(entries.as_chunks::<12>().0.iter().any(|e| {
        short(e) == 262 && short(&e[2..]) == 3 && long(&e[4..]) == 1 && short(&e[8..]) == 34892
    }))
}

fn invalid(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}
fn at<R: Read + Seek>(r: &mut R, offset: u64, n: usize, size: u64) -> io::Result<Vec<u8>> {
    if offset.checked_add(n as u64).is_none_or(|end| end > size) {
        return Err(invalid("TIFF range outside file"));
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut bytes = vec![0; n];
    r.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads from stream start; leaves stream position unspecified.
/// Supports classic II/MM TIFF, exactly one IFD and one uncompressed chunky
/// float32 or uint16 RGB LinearRaw strip, DNG 1.4–1.6, D65 ColorMatrix1 and
/// AsShotNeutral. Integer samples are divided by per-channel WhiteLevel; float
/// samples retain their scene-linear values. Orientation is returned unapplied.
/// Limits: 128 entries, 64 Mi pixels, 1 MiB XMP, 2 MiB total tag payload.
/// Unsupported layouts, malformed data and non-finite pixels return errors.
pub fn read<R: Read + Seek>(reader: &mut R) -> io::Result<LinearDng> {
    let mut r = BufReader::new(reader);
    let size = r.seek(SeekFrom::End(0))?;
    let h = at(&mut r, 0, 8, size)?;
    let le = match &h[..2] {
        b"II" => true,
        b"MM" => false,
        _ => return Err(invalid("not TIFF")),
    };
    let u16v = |b: &[u8]| {
        let a = [b[0], b[1]];
        if le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        }
    };
    let u32v = |b: &[u8]| {
        let a = [b[0], b[1], b[2], b[3]];
        if le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        }
    };
    if u16v(&h[2..]) != 42 {
        return Err(invalid("classic TIFF required"));
    }
    let ifd = u32v(&h[4..]) as u64;
    if ifd < 8 {
        return Err(invalid("invalid IFD offset"));
    }
    let n = u16v(&at(&mut r, ifd, 2, size)?) as usize;
    if n == 0 || n > 128 {
        return Err(invalid("IFD entry budget exceeded"));
    }
    let table = at(&mut r, ifd + 2, n * 12 + 4, size)?;
    if u32v(&table[n * 12..]) != 0 {
        return Err(invalid("multiple IFDs unsupported"));
    }
    let mut tags = BTreeMap::new();
    let mut budget = 0usize;
    for e in table[..n * 12].as_chunks::<12>().0 {
        let tag = u16v(e);
        let typ = u16v(&e[2..]);
        let count = u32v(&e[4..]) as usize;
        let unit = match typ {
            1 | 2 | 7 => 1,
            3 => 2,
            4 | 9 | 11 => 4,
            5 | 10 | 12 => 8,
            _ => return Err(invalid("unsupported TIFF type")),
        };
        let bytes = count
            .checked_mul(unit)
            .ok_or_else(|| invalid("tag size overflow"))?;
        // An embedded original is opaque and unnecessary for rendering. Check
        // its type/range and duplicate tags, but do not allocate/read a raw file
        // into the metadata budget. The sample and XMP budgets stay unchanged.
        if tag == 50828 {
            if typ != 7
                || bytes <= 4
                || u64::from(u32v(&e[8..]))
                    .checked_add(bytes as u64)
                    .is_none_or(|end| end > size)
            {
                return Err(invalid("invalid OriginalRawFileData"));
            }
            if tags.insert(tag, (typ, count, Vec::new())).is_some() {
                return Err(invalid("duplicate TIFF tag"));
            }
            continue;
        }
        budget = budget
            .checked_add(bytes)
            .ok_or_else(|| invalid("tag budget overflow"))?;
        if budget > 2 * 1024 * 1024 || (tag == 700 && bytes > 1024 * 1024) {
            return Err(invalid("tag payload budget exceeded"));
        }
        let data = if bytes <= 4 {
            e[8..8 + bytes].to_vec()
        } else {
            at(&mut r, u32v(&e[8..]) as u64, bytes, size)?
        };
        if tags.insert(tag, (typ, count, data)).is_some() {
            return Err(invalid("duplicate TIFF tag"));
        }
    }
    let get = |tag, typ, count| -> io::Result<&[u8]> {
        let (t, c, d) = tags
            .get(&tag)
            .ok_or_else(|| invalid("missing required DNG tag"))?;
        if *t != typ || *c != count {
            return Err(invalid("wrong tag type/count"));
        }
        Ok(d)
    };
    let short = |tag| -> io::Result<u16> { Ok(u16v(get(tag, 3, 1)?)) };
    let long = |tag| -> io::Result<u32> { Ok(u32v(get(tag, 4, 1)?)) };
    let backward = if tags.contains_key(&50707) {
        get(50707, 1, 4)?
    } else {
        &[1, 0, 0, 0]
    };
    if backward[0] != 1 || backward > [1, 4, 0, 0].as_slice() {
        return Err(invalid("unsupported DNG backward version"));
    }
    if !matches!(get(50706, 1, 4)?, [1, 4..=6, 0, 0])
        || short(259)? != 1
        || short(262)? != 34892
        || short(277)? != 3
        || short(284)? != 1
        || short(50778)? != 21
    {
        return Err(invalid("unsupported DNG layout or calibration illuminant"));
    }
    let bits = u16v(get(258, 3, 3)?);
    if bits != 32 && bits != 16 {
        return Err(invalid("float32 or uint16 samples required"));
    }
    for (tag, expected) in [(258, bits), (339, if bits == 32 { 3 } else { 1 })] {
        if get(tag, 3, 3)?
            .as_chunks::<2>()
            .0
            .iter()
            .any(|b| u16v(b) != expected)
        {
            return Err(invalid("float32 samples required"));
        }
    }
    if tags.contains_key(&330) || (tags.contains_key(&317) && short(317)? != 1) {
        return Err(invalid("SubIFDs and prediction unsupported"));
    }
    let orientation = if tags.contains_key(&274) {
        short(274)?
    } else {
        1
    };
    if !(1..=8).contains(&orientation) {
        return Err(invalid("invalid orientation"));
    }
    let width = long(256)? as usize;
    let height = long(257)? as usize;
    let count = width
        .checked_mul(height)
        .ok_or_else(|| invalid("image dimensions overflow"))?;
    if count == 0 || count > 64 * 1024 * 1024 || long(278)? as usize != height {
        return Err(invalid("invalid dimensions or strip layout"));
    }
    let bytes = count
        .checked_mul(3 * usize::from(bits / 8))
        .ok_or_else(|| invalid("sample size overflow"))?;
    let strip = long(273)? as u64;
    if long(279)? as usize != bytes || strip.checked_add(bytes as u64).is_none_or(|end| end > size)
    {
        return Err(invalid("invalid strip range/byte count"));
    }
    let mut color_matrix = [[0.0; 3]; 3];
    for (v, b) in color_matrix
        .iter_mut()
        .flatten()
        .zip(get(50721, 10, 9)?.as_chunks::<8>().0)
    {
        let den = u32v(&b[4..]) as i32;
        if den == 0 {
            return Err(invalid("zero matrix denominator"));
        }
        *v = (u32v(b) as i32) as f64 / den as f64;
    }
    let mut as_shot_neutral = [0.0; 3];
    for (v, b) in as_shot_neutral
        .iter_mut()
        .zip(get(50728, 5, 3)?.as_chunks::<8>().0)
    {
        let den = u32v(&b[4..]);
        if den == 0 || u32v(b) == 0 {
            return Err(invalid("invalid neutral rational"));
        }
        *v = u32v(b) as f64 / den as f64;
    }
    let xmp = match tags.get(&700) {
        None => String::new(),
        Some((1, _, b)) => String::from_utf8(b.clone()).map_err(|_| invalid("XMP is not UTF-8"))?,
        _ => return Err(invalid("invalid XMP type")),
    };
    // Merge interchange has zero black and an explicit per-channel white.
    // Reject unimplemented linearization rather than silently misinterpreting it.
    if [50712, 50714, 50715, 50716]
        .iter()
        .any(|tag| tags.contains_key(tag))
    {
        return Err(invalid("black levels and linearization unsupported"));
    }
    let mut white = [1.; 3];
    for (value, b) in white.iter_mut().zip(get(50717, 4, 3)?.as_chunks::<4>().0) {
        *value = u32v(b) as f32;
        if *value <= 0. {
            return Err(invalid("invalid white level"));
        }
    }
    let mut pixels = Vec::new();
    pixels
        .try_reserve_exact(count)
        .map_err(|_| invalid("image allocation failed"))?;
    r.seek(SeekFrom::Start(strip))?;
    for _ in 0..count {
        let mut b = [0; 12];
        r.read_exact(&mut b[..3 * usize::from(bits / 8)])?;
        let p = std::array::from_fn(|c| {
            if bits == 32 {
                f32::from_bits(u32v(&b[c * 4..]))
            } else {
                f32::from(u16v(&b[c * 2..])) / white[c]
            }
        });
        if p.iter().any(|v| !v.is_finite()) {
            return Err(invalid("non-finite sample"));
        }
        pixels.push(p);
    }
    Ok(LinearDng {
        width,
        height,
        orientation,
        pixels,
        color_matrix,
        as_shot_neutral,
        xmp,
    })
}
