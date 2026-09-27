//! Lossless modular JPEG XL. zune-jpegxl's codestream declares sRGB only.
//! Other document spaces must be rejected, not mislabeled or converted silently.
use crate::encode_error;
use engine_api::{EngineResult, jobs::CancellationToken};
use std::io::Write;
use zune_core::{bit_depth::BitDepth, colorspace::ColorSpace, options::EncoderOptions};

pub(crate) fn encode(
    writer: &mut impl Write,
    rgb: &image::Rgb32FImage,
    bits: u8,
    xmp: Option<&str>,
    native: Option<&crate::native::Native>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    cancel.check()?;
    if !matches!(bits, 8 | 16)
        || rgb.width() < 2
        || rgb.height() < 2
        || u64::from(rgb.width()) * u64::from(rgb.height()) > 100_000_000
        || rgb.as_raw().iter().any(|v| !v.is_finite())
        || xmp.is_some_and(|v| v.len() > 1024 * 1024)
    {
        return Err(encode_error(
            "JPEG XL requires finite RGB, 8/16 bits, dimensions >= 2, <= 100 megapixels and <= 1 MiB XMP",
        ));
    }
    let depth = if bits == 8 {
        BitDepth::Eight
    } else {
        BitDepth::Sixteen
    };
    let mut pixels = Vec::with_capacity(rgb.as_raw().len() * usize::from(bits / 8));
    for row in rgb.as_raw().chunks(rgb.width() as usize * 3) {
        cancel.check()?;
        for &sample in row {
            if bits == 8 {
                pixels.push((sample.clamp(0.0, 1.0) * 255.0).round() as u8);
            } else {
                pixels.extend_from_slice(
                    &((sample.clamp(0.0, 1.0) * 65535.0).round() as u16).to_ne_bytes(),
                );
            }
        }
    }
    let options = EncoderOptions::new(
        rgb.width() as usize,
        rgb.height() as usize,
        ColorSpace::RGB,
        depth,
    );
    let mut codestream = Vec::new();
    zune_jpegxl::JxlSimpleEncoder::new(&pixels, options)
        .encode(&mut codestream)
        .map_err(|e| encode_error(format!("JPEG XL: {e:?}")))?;
    cancel.check()?;
    // ISO/IEC 18181-2 container. Colour and orientation (identity) are in the
    // codestream; XML boxes carry the already-filtered XMP verbatim.
    write_box(writer, b"JXL ", &[13, 10, 135, 10])?;
    write_box(writer, b"ftyp", b"jxl \0\0\0\0jxl ")?;
    if let Some(native) = native {
        let exif = native.tiff_bytes(true)?;
        if !exif.is_empty() {
            write_box(writer, b"Exif", &[&[0; 4], exif.as_slice()].concat())?;
        }
    }
    if let Some(xmp) = xmp {
        write_box(writer, b"xml ", xmp.as_bytes())?;
    }
    write_box(writer, b"jxlc", &codestream)
}

fn write_box(writer: &mut impl Write, kind: &[u8; 4], payload: &[u8]) -> EngineResult<()> {
    let size = u32::try_from(payload.len())
        .ok()
        .and_then(|n| n.checked_add(8))
        .ok_or_else(|| encode_error("JPEG XL box exceeds 4 GiB"))?;
    writer
        .write_all(&size.to_be_bytes())
        .map_err(encode_error)?;
    writer.write_all(kind).map_err(encode_error)?;
    writer.write_all(payload).map_err(encode_error)
}
