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
pub fn extract_raf_lens<R: Read + Seek>(r: &mut R) -> io::Result<Option<MakerLens>> {
    let size = r.seek(SeekFrom::End(0))?;
    if size < HEADER as u64 {
        return Ok(None);
    }
    let header = read(r, 0, HEADER, size)?.unwrap_or_default();
    if header.len() < HEADER || &header[..16] != MAGIC {
        return Ok(None);
    }
    let be = |o: usize| u32::from_be_bytes(header[o..o + 4].try_into().unwrap()) as u64;
    let window = |len: u64| len.min(WINDOW as u64) as usize;
    let (jpeg_offset, jpeg_len) = (be(84), be(88));
    let (cfa_offset, cfa_len) = (be(100), be(104));
    let Some(cfa) = read(r, cfa_offset, window(cfa_len), size)? else {
        return Ok(None);
    };
    let jpeg = read(r, jpeg_offset, window(jpeg_len), size)?.unwrap_or_default();
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

/// The correction from the start of the RAF CFA section (a TIFF whose IFD0
/// points to the FujiIFD) and the embedded JPEG (for CropMode).
fn parse_fujifilm(cfa: &[u8], jpeg: &[u8]) -> Option<FujifilmLens> {
    let _ = (cfa, jpeg);
    None
}

#[cfg(test)]
#[path = "maker_lens_tests.rs"]
mod tests;
