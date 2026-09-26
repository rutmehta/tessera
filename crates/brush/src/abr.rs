//! Photoshop brush (`.abr`) import, written from the publicly documented
//! layout (no GPL code). Supports v1/v2 (computed and sampled brushes) and
//! v6/v10 (`8BIM` sections; sampled tips in `samp`). Parsing is permissive:
//! a damaged brush record is skipped with a warning, unknown sections are
//! ignored and truncated files yield the brushes read so far. Brush
//! descriptors (`desc`) are kept raw and not interpreted.

use engine_api::{EngineError, EngineResult};

use crate::tip::{SampledTip, Tip};

/// Largest accepted tip side.
pub const MAX_SIDE: u32 = 8192;

/// A brush's tip.
#[derive(Debug, Clone, PartialEq)]
pub enum AbrTip {
    /// Sampled grayscale tip.
    Sampled(SampledTip),
    /// Computed elliptical tip (v1/v2).
    Computed {
        /// Diameter, pixels.
        diameter: f32,
        /// Hardness `0..=1`.
        hardness: f32,
        /// Roundness `0..=1`.
        roundness: f32,
        /// Angle, degrees.
        angle: f32,
    },
}

/// One imported brush.
#[derive(Debug, Clone, PartialEq)]
pub struct AbrBrush {
    /// Name or sample id.
    pub name: String,
    /// Spacing as a fraction of the diameter, when the file says.
    pub spacing: Option<f32>,
    /// Tip.
    pub tip: AbrTip,
}

impl AbrBrush {
    /// The engine tip and the natural diameter.
    pub fn to_tip(&self) -> (Tip, f32) {
        match &self.tip {
            AbrTip::Sampled(s) => (Tip::sampled(s.clone()), s.width.max(s.height) as f32),
            AbrTip::Computed {
                diameter,
                hardness,
                roundness,
                angle,
            } => {
                let mut t = Tip::round(*hardness);
                t.roundness = roundness.clamp(0.01, 1.0);
                t.angle = angle.to_radians();
                (t, *diameter)
            }
        }
    }
}

/// A parsed file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AbrFile {
    /// Major version (1, 2, 6 or 10).
    pub version: u16,
    /// Minor version (v6/v10: 1 or 2).
    pub subversion: u16,
    /// Brushes in file order.
    pub brushes: Vec<AbrBrush>,
    /// Raw `desc` section, if present.
    pub descriptor: Option<Vec<u8>>,
    /// Records or sections that were skipped.
    pub warnings: Vec<String>,
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(b: &'a [u8]) -> Self {
        Self { b, pos: 0 }
    }
    fn remaining(&self) -> usize {
        self.b.len().saturating_sub(self.pos)
    }
    fn take(&mut self, n: usize) -> EngineResult<&'a [u8]> {
        if self.remaining() < n {
            return Err(EngineError::invalid("abr", "truncated"));
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn skip(&mut self, n: usize) -> EngineResult<()> {
        self.take(n).map(|_| ())
    }
    fn u8(&mut self) -> EngineResult<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> EngineResult<u16> {
        let s = self.take(2)?;
        Ok(u16::from_be_bytes([s[0], s[1]]))
    }
    fn i16(&mut self) -> EngineResult<i16> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> EngineResult<u32> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes([s[0], s[1], s[2], s[3]]))
    }
    fn i32(&mut self) -> EngineResult<i32> {
        Ok(self.u32()? as i32)
    }
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// PackBits decode of exactly `n` bytes.
fn unpack_bits(src: &[u8], n: usize) -> EngineResult<Vec<u8>> {
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    while out.len() < n && i < src.len() {
        let h = src[i] as i8;
        i += 1;
        if h >= 0 {
            let k = h as usize + 1;
            let end = (i + k).min(src.len());
            out.extend_from_slice(&src[i..end]);
            i = end;
        } else if h != -128 {
            let k = 1 - h as isize;
            let v = *src
                .get(i)
                .ok_or_else(|| EngineError::invalid("abr", "rle overrun"))?;
            i += 1;
            out.extend(std::iter::repeat_n(v, k as usize));
        }
    }
    if out.len() < n {
        return Err(EngineError::invalid("abr", "rle row too short"));
    }
    out.truncate(n);
    Ok(out)
}

/// PackBits encode.
fn pack_bits(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < src.len() {
        let mut run = 1;
        while i + run < src.len() && run < 128 && src[i + run] == src[i] {
            run += 1;
        }
        if run >= 2 {
            out.push((1 - run as isize) as i8 as u8);
            out.push(src[i]);
            i += run;
            continue;
        }
        let start = i;
        while i < src.len() && i - start < 128 && !(i + 1 < src.len() && src[i + 1] == src[i]) {
            i += 1;
        }
        if i == start {
            i += 1;
        }
        out.push((i - start - 1) as u8);
        out.extend_from_slice(&src[start..i]);
    }
    out
}

/// Reads the sample image that follows the bounds of a sampled brush.
fn read_image(r: &mut Reader<'_>, name: String) -> EngineResult<SampledTip> {
    let top = r.i32()?;
    let left = r.i32()?;
    let bottom = r.i32()?;
    let right = r.i32()?;
    let depth = r.u16()?;
    let compression = r.u8()?;
    let (w, h) = (
        i64::from(right) - i64::from(left),
        i64::from(bottom) - i64::from(top),
    );
    if w <= 0 || h <= 0 || w > i64::from(MAX_SIDE) || h > i64::from(MAX_SIDE) {
        return Err(EngineError::invalid("abr", format!("bad bounds {w}×{h}")));
    }
    if depth != 8 && depth != 16 {
        return Err(EngineError::invalid("abr", format!("depth {depth}")));
    }
    let (w, h) = (w as usize, h as usize);
    let bps = usize::from(depth / 8);
    let row = w * bps;
    let raw = match compression {
        0 => r.take(row * h)?.to_vec(),
        1 => {
            let mut counts = Vec::with_capacity(h);
            for _ in 0..h {
                counts.push(usize::from(r.u16()?));
            }
            let mut v = Vec::with_capacity(row * h);
            for c in counts {
                v.extend(unpack_bits(r.take(c)?, row)?);
            }
            v
        }
        c => return Err(EngineError::invalid("abr", format!("compression {c}"))),
    };
    let data = if bps == 1 {
        raw.iter().map(|&v| f32::from(v) / 255.0).collect()
    } else {
        raw.as_chunks::<2>()
            .0
            .iter()
            .map(|c| f32::from(u16::from_be_bytes(*c)) / 65535.0)
            .collect()
    };
    SampledTip::new(name, w as u32, h as u32, data)
}

fn plausible_bounds(b: &[u8]) -> bool {
    if b.len() < 19 {
        return false;
    }
    let g = |i: usize| i32::from_be_bytes([b[i], b[i + 1], b[i + 2], b[i + 3]]) as i64;
    let (w, h) = (g(12) - g(4), g(8) - g(0));
    let depth = u16::from_be_bytes([b[16], b[17]]);
    w > 0
        && h > 0
        && w <= i64::from(MAX_SIDE)
        && h <= i64::from(MAX_SIDE)
        && (depth == 8 || depth == 16)
        && b[18] <= 1
}

fn parse_v6_sample(rec: &[u8], subversion: u16) -> EngineResult<AbrBrush> {
    let mut r = Reader::new(rec);
    let n = usize::from(r.u8()?);
    let name = latin1(r.take(n)?);
    // Unknown fixed-size fields before the bounds: 10 bytes in 6.1, 264 in
    // 6.2/10. Probe both so files with a mislabelled subversion still load.
    let preferred: [usize; 2] = if subversion == 1 {
        [10, 264]
    } else {
        [264, 10]
    };
    let skip = preferred
        .into_iter()
        .find(|&s| rec.get(r.pos + s..).is_some_and(plausible_bounds))
        .ok_or_else(|| EngineError::invalid("abr", "no sample header"))?;
    r.skip(skip)?;
    let tip = read_image(&mut r, name.clone())?;
    Ok(AbrBrush {
        name,
        spacing: None,
        tip: AbrTip::Sampled(tip),
    })
}

fn parse_v6(r: &mut Reader<'_>, file: &mut AbrFile) -> EngineResult<()> {
    while r.remaining() >= 12 {
        // Tolerate up to 3 bytes of padding between sections.
        let mut found = false;
        for pad in 0..4 {
            if r.b.get(r.pos + pad..r.pos + pad + 4) == Some(b"8BIM") {
                r.pos += pad;
                found = true;
                break;
            }
        }
        if !found {
            file.warnings
                .push(format!("no section signature at {}", r.pos));
            break;
        }
        r.skip(4)?;
        let key = r.take(4)?.to_vec();
        let len = r.u32()? as usize;
        let data = if r.remaining() < len {
            file.warnings.push("truncated section".into());
            let d = &r.b[r.pos..];
            r.pos = r.b.len();
            d
        } else {
            r.take(len)?
        };
        match &key[..] {
            b"samp" => {
                let mut s = Reader::new(data);
                while s.remaining() >= 4 {
                    let size = s.u32()? as usize;
                    let start = s.pos;
                    let end = (start + size).min(data.len());
                    match parse_v6_sample(&data[start..end], file.subversion) {
                        Ok(b) => file.brushes.push(b),
                        Err(e) => file.warnings.push(format!("sample at {start}: {e}")),
                    }
                    s.pos = (start + size.div_ceil(4) * 4).min(data.len());
                }
            }
            b"desc" => file.descriptor = Some(data.to_vec()),
            _ => {}
        }
    }
    Ok(())
}

fn parse_v12_record(
    typ: u16,
    rec: &[u8],
    version: u16,
    index: usize,
) -> EngineResult<Option<AbrBrush>> {
    let mut r = Reader::new(rec);
    match typ {
        1 => {
            let _misc = r.u32()?;
            let spacing = r.u16()?;
            let diameter = r.u16()?;
            let roundness = r.u16()?;
            let angle = r.i16()?;
            let hardness = r.u16()?;
            Ok(Some(AbrBrush {
                name: format!("Computed {}", index + 1),
                spacing: (spacing > 0).then(|| f32::from(spacing) / 100.0),
                tip: AbrTip::Computed {
                    diameter: f32::from(diameter.max(1)),
                    hardness: (f32::from(hardness) / 100.0).clamp(0.0, 1.0),
                    roundness: (f32::from(roundness) / 100.0).clamp(0.01, 1.0),
                    angle: f32::from(angle),
                },
            }))
        }
        2 => {
            let _misc = r.u32()?;
            let spacing = r.u16()?;
            let mut name = format!("Sampled {}", index + 1);
            if version == 2 {
                let n = r.u32()? as usize;
                let units: Vec<u16> = (0..n).map(|_| r.u16()).collect::<EngineResult<_>>()?;
                let s = String::from_utf16_lossy(&units);
                let s = s.trim_end_matches('\0');
                if !s.is_empty() {
                    name = s.to_string();
                }
            }
            let _antialias = r.u8()?;
            r.skip(8)?; // short bounds (duplicated by the long bounds)
            let tip = read_image(&mut r, name.clone())?;
            Ok(Some(AbrBrush {
                name,
                spacing: (spacing > 0).then(|| f32::from(spacing) / 100.0),
                tip: AbrTip::Sampled(tip),
            }))
        }
        _ => Ok(None),
    }
}

/// Parses an `.abr` file.
pub fn parse(bytes: &[u8]) -> EngineResult<AbrFile> {
    let mut r = Reader::new(bytes);
    let mut file = AbrFile {
        version: r.u16()?,
        ..Default::default()
    };
    match file.version {
        1 | 2 => {
            let count = r.u16()?;
            for i in 0..usize::from(count) {
                if r.remaining() < 6 {
                    file.warnings.push("truncated brush list".into());
                    break;
                }
                let typ = r.u16()?;
                let size = r.u32()? as usize;
                let rec = &bytes[r.pos..(r.pos + size).min(bytes.len())];
                r.pos = (r.pos + size).min(bytes.len());
                match parse_v12_record(typ, rec, file.version, i) {
                    Ok(Some(b)) => file.brushes.push(b),
                    Ok(None) => file.warnings.push(format!("brush {i}: type {typ} skipped")),
                    Err(e) => file.warnings.push(format!("brush {i}: {e}")),
                }
            }
        }
        6 | 10 => {
            file.subversion = r.u16()?;
            parse_v6(&mut r, &mut file)?;
        }
        v => {
            return Err(EngineError::invalid(
                "abr",
                format!("unsupported version {v}"),
            ));
        }
    }
    Ok(file)
}

/// Writes sampled tips as an 8-bit v6/v10 file (`samp` section plus an
/// empty `patt` section). Used for tests and brush export.
pub fn write_v6(
    tips: &[SampledTip],
    version: u16,
    subversion: u16,
    rle: bool,
) -> EngineResult<Vec<u8>> {
    if !matches!(version, 6 | 10) || !matches!(subversion, 1 | 2) {
        return Err(EngineError::invalid("abr", "version 6/10, subversion 1/2"));
    }
    let mut samp = Vec::new();
    for t in tips {
        let name = t.name.as_bytes();
        if name.len() > 255 {
            return Err(EngineError::invalid("abr", "name longer than 255 bytes"));
        }
        let mut rec = vec![name.len() as u8];
        rec.extend_from_slice(name);
        rec.extend(std::iter::repeat_n(
            0u8,
            if subversion == 1 { 10 } else { 264 },
        ));
        for v in [0i32, 0, t.height as i32, t.width as i32] {
            rec.extend(v.to_be_bytes());
        }
        rec.extend(8u16.to_be_bytes());
        rec.push(u8::from(rle));
        let px: Vec<u8> = t
            .data
            .iter()
            .map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8)
            .collect();
        if rle {
            let rows: Vec<Vec<u8>> = px.chunks(t.width as usize).map(pack_bits).collect();
            for r in &rows {
                if r.len() > usize::from(u16::MAX) {
                    return Err(EngineError::invalid("abr", "row too long"));
                }
                rec.extend((r.len() as u16).to_be_bytes());
            }
            for r in rows {
                rec.extend(r);
            }
        } else {
            rec.extend(px);
        }
        samp.extend((rec.len() as u32).to_be_bytes());
        let pad = rec.len().div_ceil(4) * 4 - rec.len();
        samp.extend(rec);
        samp.extend(std::iter::repeat_n(0u8, pad));
    }
    let mut out = Vec::new();
    out.extend(version.to_be_bytes());
    out.extend(subversion.to_be_bytes());
    out.extend(b"8BIMsamp");
    out.extend((samp.len() as u32).to_be_bytes());
    out.extend(samp);
    out.extend(b"8BIMpatt");
    out.extend(0u32.to_be_bytes());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packbits_round_trip() {
        let cases: [&[u8]; 5] = [
            &[],
            &[7],
            &[1, 1, 1, 1, 2, 3, 4, 4, 5],
            &[9; 300],
            &[1, 2, 3, 4, 5, 6, 7, 8, 9, 10],
        ];
        for c in cases {
            let p = pack_bits(c);
            assert_eq!(unpack_bits(&p, c.len()).unwrap(), c);
        }
        let long: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 13) as u8).collect();
        assert_eq!(unpack_bits(&pack_bits(&long), long.len()).unwrap(), long);
    }
}
