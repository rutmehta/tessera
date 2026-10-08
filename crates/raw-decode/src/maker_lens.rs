//! Built-in lens corrections that cameras store in proprietary maker notes
//! (ENG-8), read independently of LibRaw.
//!
//! Lightroom applies these like DNG opcode corrections: always, whatever the
//! lens-profile setting. Only formats with a clear public description and
//! evidence that Lightroom applies them are read; see
//! `tools/orchestrate/wp/ENG-8/HANDOFF.md` for the research and sources.
//!
//! Fujifilm RAF: the FujiIFD (tag 0xF000 of the TIFF that starts the RAF CFA
//! section) carries `GeometricDistortionParams` (0xF00B),
//! `ChromaticAberrationParams` (0xF00F) and `VignettingParams` (0xF010) as
//! signed rationals (ExifTool `FujiFilm::RAF` tags; layouts as read by
//! darktable `src/common/exif.cc` and RawTherapee `rtengine/lensmetadata.cc`).
//! `CropMode` (maker-note tag 0x104D, in the embedded JPEG's EXIF) values 2 and
//! 4 (1.25x crop) scale the knot radii by 1.25.
use std::io::{self, Read, Seek, SeekFrom};

/// A camera's built-in lens correction from its maker notes.
#[derive(Clone, Debug, PartialEq)]
pub enum MakerLens {
    Fujifilm(FujifilmLens),
}

/// Fujifilm built-in correction splines. All vectors have one value per knot.
///
/// Knots are radii in the uncorrected (source) image, as fractions of the
/// half-diagonal of the active area, before `crop_factor`. At knot radius r:
/// - `distortion` (percent): the corrected image shows source radius r at
///   radius r / (1 + distortion / 100);
/// - `ca_red`/`ca_blue`: the red/blue plane is sampled at radius
///   r · (1 + ca) relative to green;
/// - `vignetting` (percent): relative illumination at r (100 = centre).
#[derive(Clone, Debug, PartialEq)]
pub struct FujifilmLens {
    pub knots: Vec<f64>,
    pub distortion: Vec<f64>,
    pub ca_red: Vec<f64>,
    pub ca_blue: Vec<f64>,
    pub vignetting: Vec<f64>,
    /// 1.25 for the 1.25x crop modes (CropMode 2 or 4), else 1.
    pub crop_factor: f64,
}

const MAGIC: &[u8; 16] = b"FUJIFILMCCD-RAW ";
/// Bounded reads: the RAF directory, the JPEG's EXIF segment, the CFA TIFF.
const HEADER: usize = 108;
const WINDOW: usize = 256 * 1024;
const MAX_ENTRIES: usize = 1024;

/// Read the Fujifilm built-in correction of a RAF stream. Non-RAF input and
/// RAFs without (or with malformed or inconsistent) correction data give
/// `Ok(None)`: absent data is never replaced by a guess. I/O errors propagate.
/// Reads at most the RAF directory and two bounded windows.
pub fn extract_raf_lens<R: Read + Seek>(r: &mut R) -> io::Result<Option<MakerLens>> {
    let size = r.seek(SeekFrom::End(0))?;
    let Some(header) = read(r, 0, HEADER, size)? else {
        return Ok(None);
    };
    if &header[..16] != MAGIC {
        return Ok(None);
    }
    let be = |o: usize| u64::from(u32::from_be_bytes(header[o..o + 4].try_into().unwrap()));
    // A section's window: its start, bounded by its length, the file and WINDOW.
    let window =
        |offset: u64, len: u64| len.min(size.saturating_sub(offset)).min(WINDOW as u64) as usize;
    let (jpeg_offset, jpeg_len) = (be(84), be(88));
    let (cfa_offset, cfa_len) = (be(100), be(104));
    let Some(cfa) = read(r, cfa_offset, window(cfa_offset, cfa_len), size)? else {
        return Ok(None);
    };
    let jpeg = read(r, jpeg_offset, window(jpeg_offset, jpeg_len), size)?.unwrap_or_default();
    Ok(parse_fujifilm(&cfa, &jpeg).map(MakerLens::Fujifilm))
}

fn read<R: Read + Seek>(
    r: &mut R,
    offset: u64,
    n: usize,
    size: u64,
) -> io::Result<Option<Vec<u8>>> {
    if n == 0 || offset.checked_add(n as u64).is_none_or(|end| end > size) {
        return Ok(None);
    }
    r.seek(SeekFrom::Start(offset))?;
    let mut b = vec![0; n];
    r.read_exact(&mut b)?;
    Ok(Some(b))
}

/// A classic TIFF structure inside a bounded buffer. Offsets are relative to
/// the buffer start; anything outside it reads as absent.
struct Tiff<'a> {
    b: &'a [u8],
    le: bool,
}

#[derive(Clone, Copy)]
struct Entry {
    at: usize,
    kind: u16,
    count: u32,
}

impl<'a> Tiff<'a> {
    fn new(b: &'a [u8]) -> Option<Self> {
        let le = match b.get(..4)? {
            b"II*\0" => true,
            b"MM\0*" => false,
            _ => return None,
        };
        Some(Self { b, le })
    }
    fn u16(&self, at: usize) -> Option<u16> {
        let a: [u8; 2] = self.b.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.le {
            u16::from_le_bytes(a)
        } else {
            u16::from_be_bytes(a)
        })
    }
    fn u32(&self, at: usize) -> Option<u32> {
        let a: [u8; 4] = self.b.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.le {
            u32::from_le_bytes(a)
        } else {
            u32::from_be_bytes(a)
        })
    }
    fn entry(&self, ifd: usize, tag: u16) -> Option<Entry> {
        let n = usize::from(self.u16(ifd)?);
        if n > MAX_ENTRIES {
            return None;
        }
        (0..n).find_map(|i| {
            let at = ifd + 2 + 12 * i;
            (self.u16(at)? == tag).then_some(Entry {
                at,
                kind: self.u16(at + 2)?,
                count: self.u32(at + 4)?,
            })
        })
    }
    /// Offset of an entry's values (inline when they fit in four bytes).
    fn values(&self, e: Entry, size: usize) -> Option<usize> {
        let len = size.checked_mul(e.count as usize)?;
        let at = if len <= 4 {
            e.at + 8
        } else {
            self.u32(e.at + 8)? as usize
        };
        (at.checked_add(len)? <= self.b.len()).then_some(at)
    }
    /// A single LONG or IFD offset.
    fn offset(&self, e: Entry) -> Option<usize> {
        if !matches!(e.kind, 4 | 13) || e.count != 1 {
            return None;
        }
        Some(self.u32(self.values(e, 4)?)? as usize)
    }
    fn short(&self, e: Entry) -> Option<u16> {
        if e.kind != 3 || e.count != 1 {
            return None;
        }
        self.u16(self.values(e, 2)?)
    }
    /// RATIONAL or SRATIONAL values; a zero denominator is malformed.
    fn rationals(&self, e: Entry) -> Option<Vec<f64>> {
        if !matches!(e.kind, 5 | 10) || e.count > 256 {
            return None;
        }
        let at = self.values(e, 8)?;
        (0..e.count as usize)
            .map(|i| {
                let (n, d) = (self.u32(at + 8 * i)?, self.u32(at + 8 * i + 4)?);
                let (n, d) = if e.kind == 10 {
                    (f64::from(n as i32), f64::from(d as i32))
                } else {
                    (f64::from(n), f64::from(d))
                };
                (d != 0.).then(|| n / d)
            })
            .collect()
    }
}

/// The correction from the start of the RAF CFA section (a TIFF whose IFD0
/// points to the FujiIFD) and the embedded JPEG (for CropMode).
fn parse_fujifilm(cfa: &[u8], jpeg: &[u8]) -> Option<FujifilmLens> {
    let t = Tiff::new(cfa)?;
    let ifd0 = t.u32(4)? as usize;
    let fuji = t.offset(t.entry(ifd0, 0xf000)?)?;
    let d = t.rationals(t.entry(fuji, 0xf00b)?)?;
    let c = t.rationals(t.entry(fuji, 0xf00f)?)?;
    let v = t.rationals(t.entry(fuji, 0xf010)?)?;
    let same = |a: &[f64], b: &[f64]| {
        a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() <= 1e-6)
    };
    // darktable `_check_lens_correction_data`, RawTherapee `FujiMetadataLensCorrection`.
    let (knots, distortion, ca_red, ca_blue, vignetting) = match (d.len(), c.len(), v.len()) {
        // X-Trans I/II/III: 11 knots; the CA tag omits the first (zero) knot.
        (23, 31, 23) => {
            let knots = d[1..12].to_vec();
            if !same(&c[1..11], &knots[1..]) || !same(&v[1..12], &knots) {
                return None;
            }
            let zero_first = |x: &[f64]| [&[0.][..], x].concat();
            (
                knots,
                d[12..23].to_vec(),
                zero_first(&c[11..21]),
                zero_first(&c[21..31]),
                v[12..23].to_vec(),
            )
        }
        // X-Trans IV/V: 9 knots.
        (19, 29, 19) => {
            let knots = d[1..10].to_vec();
            if !same(&c[1..10], &knots) || !same(&v[1..10], &knots) {
                return None;
            }
            (
                knots,
                d[10..19].to_vec(),
                c[10..19].to_vec(),
                c[19..28].to_vec(),
                v[10..19].to_vec(),
            )
        }
        _ => return None,
    };
    let finite = |x: &[f64]| x.iter().all(|v| v.is_finite());
    if !(finite(&knots) && finite(&distortion) && finite(&ca_red) && finite(&ca_blue))
        || knots[0] < 0.
        || knots.windows(2).any(|w| w[1] <= w[0])
        || knots[knots.len() - 1] > 2.
        || distortion.iter().any(|x| x.abs() >= 50.)
        || ca_red.iter().chain(&ca_blue).any(|x| x.abs() >= 0.05)
        || vignetting
            .iter()
            .any(|x| !(x.is_finite() && *x > 0. && *x <= 200.))
    {
        return None;
    }
    Some(FujifilmLens {
        knots,
        distortion,
        ca_red,
        ca_blue,
        vignetting,
        crop_factor: if matches!(crop_mode(jpeg), Some(2 | 4)) {
            1.25
        } else {
            1.
        },
    })
}

/// Fujifilm maker-note CropMode (0x104D) from the embedded JPEG's EXIF:
/// IFD0 → ExifIFD (0x8769) → MakerNote (0x927C, "FUJIFILM", little-endian
/// IFD at the offset stored after the signature, relative to the note).
fn crop_mode(jpeg: &[u8]) -> Option<u16> {
    if jpeg.get(..2)? != [0xff, 0xd8] {
        return None;
    }
    let mut at = 2;
    let exif = loop {
        let (marker, len) = (*jpeg.get(at + 1)?, jpeg.get(at + 2..at + 4)?);
        if *jpeg.get(at)? != 0xff || matches!(marker, 0xd9 | 0xda) {
            return None;
        }
        let len = usize::from(u16::from_be_bytes(len.try_into().ok()?));
        let segment = jpeg.get(at + 4..(at + 2).checked_add(len)?)?;
        if marker == 0xe1 && segment.starts_with(b"Exif\0\0") {
            break &segment[6..];
        }
        at += 2 + len;
    };
    let t = Tiff::new(exif)?;
    let exif_ifd = t.offset(t.entry(t.u32(4)? as usize, 0x8769)?)?;
    let e = t.entry(exif_ifd, 0x927c)?;
    if !matches!(e.kind, 1 | 7) {
        return None;
    }
    let start = t.values(e, 1)?;
    let note = exif.get(start..start + e.count as usize)?;
    if !note.starts_with(b"FUJIFILM") {
        return None;
    }
    let n = Tiff { b: note, le: true };
    n.short(n.entry(n.u32(8)? as usize, 0x104d)?)
}

#[cfg(test)]
#[path = "maker_lens_tests.rs"]
mod tests;
