//! ISO 21496-1 version-0 gain-map JPEG. The primary is sRGB SDR; the
//! full-resolution grayscale auxiliary represents a logarithmic RGB multiplier.
//! MPF offsets are relative to its TIFF header. ISO reserved flag bits are zero.
use crate::{ColorSpace, ExportSettings, Format, encode_error, native::Native};
use engine_api::{EngineResult, jobs::CancellationToken};
use std::io::{Cursor, Seek, Write};
const ISO: &[u8] = b"urn:iso:std:iso:ts:21496:-1\0";
const MPF_SEGMENT_SIZE: usize = 90;
pub(crate) struct Metadata<'a> {
    pub headroom: f32,
    pub clip: bool,
    pub native: &'a Native,
    pub xmp: Option<&'a str>,
}

pub(crate) fn encode(
    writer: &mut (impl Write + Seek),
    rgb: &image::Rgb32FImage,
    settings: &ExportSettings,
    metadata: Metadata<'_>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let headroom = metadata.headroom;
    if !headroom.is_finite() || headroom <= 1. {
        return Err(encode_error(
            "gain-map JPEG requires positive HDR headroom in the recipe",
        ));
    }
    let width = u16::try_from(rgb.width()).map_err(encode_error)?;
    let height = u16::try_from(rgb.height()).map_err(encode_error)?;
    if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 64 * 1024 * 1024 {
        return Err(encode_error("gain-map JPEG is limited to 64 megapixels"));
    }
    let mut base = image::Rgb32FImage::new(rgb.width(), rgb.height());
    let mut gains = Vec::with_capacity(rgb.width() as usize * rgb.height() as usize);
    let log_headroom = headroom.log2();
    for (i, (source, dest)) in rgb.pixels().zip(base.pixels_mut()).enumerate() {
        if i % width as usize == 0 {
            cancel.check()?;
        }
        if source.0.iter().any(|v| !v.is_finite()) {
            return Err(encode_error("non-finite gain-map sample"));
        }
        let [r, g, b] = source.0.map(|v| v.clamp(0., 1.) * headroom);
        // Linear Rec.2020 -> linear sRGB, both D65. Compress out-of-gamut
        // chroma at constant sRGB luminance before deriving the SDR rendition.
        let mut v = [
            1.660491 * r - 0.587641 * g - 0.072850 * b,
            -0.124550 * r + 1.132900 * g - 0.008349 * b,
            -0.018151 * r - 0.100579 * g + 1.118730 * b,
        ];
        if metadata.clip {
            v = v.map(|c| c.clamp(0., headroom));
        } else {
            let y = (0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2]).clamp(0., headroom);
            let mut scale = 1f32;
            for c in v {
                if c < 0. {
                    scale = scale.min(-y / (c - y));
                }
                if c > headroom {
                    scale = scale.min((headroom - y) / (c - y));
                }
            }
            v = v.map(|c| (y + scale * (c - y)).clamp(0., headroom));
        }
        let multiplier = v.into_iter().fold(1f32, f32::max);
        *dest = image::Rgb(v.map(|c| srgb(c / multiplier)));
        gains.push(
            (multiplier.log2() / log_headroom * 255.)
                .round()
                .clamp(0., 255.) as u8,
        );
    }
    let mut gain = Vec::new();
    let mut encoder = jpeg_encoder::Encoder::new(&mut gain, 100);
    encoder
        .add_app_segment(2, &iso_metadata(log_headroom))
        .map_err(encode_error)?;
    encoder
        .encode(&gains, width, height, jpeg_encoder::ColorType::Luma)
        .map_err(encode_error)?;
    cancel.check()?;
    let iso = segment(&[ISO, &[0; 4]].concat())?;
    let overhead = (gain.len() + iso.len() + MPF_SEGMENT_SIZE) as u64;
    let limit = settings
        .max_file_bytes
        .map(|n| {
            n.checked_sub(overhead)
                .filter(|v| *v > 0)
                .ok_or_else(|| encode_error("gain map exceeds JPEG byte budget"))
        })
        .transpose()?;
    let mut encoded = Cursor::new(Vec::new());
    crate::codec::encode_limited(
        &mut encoded,
        &base,
        crate::codec::Encoding {
            format: settings.format,
            space: ColorSpace::Srgb,
            dpi: settings.dpi,
            native: Some(metadata.native),
        },
        metadata.xmp,
        cancel,
        limit,
    )?;
    let encoded = encoded.into_inner();
    let sos = sos_offset(&encoded)?;
    let base_len = encoded.len() + iso.len() + MPF_SEGMENT_SIZE;
    let mpf_at = sos + iso.len();
    let mpf = mpf(base_len, gain.len(), base_len - mpf_at - 8)?;
    cancel.check()?;
    let jfif_end = if encoded.get(3) == Some(&0xe0) {
        4 + u16::from_be_bytes([encoded[4], encoded[5]]) as usize
    } else {
        2
    };
    for bytes in [
        &encoded[..jfif_end],
        &iso,
        &encoded[jfif_end..sos],
        &mpf,
        &encoded[sos..],
        &gain,
    ] {
        writer.write_all(bytes).map_err(encode_error)?;
        cancel.check()?;
    }
    Ok(())
}
fn srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1. / 2.4) - 0.055
    }
}
fn iso_metadata(stops: f32) -> Vec<u8> {
    let n = (stops as f64 * 1_000_000.).round() as u32;
    let mut out = ISO.to_vec();
    out.extend([0, 0, 0, 0, 0x40]);
    // Base/alternate headroom, min/max log gain, gamma, base/alternate offset.
    for (n, d) in [
        (0u32, 1u32),
        (n, 1_000_000),
        (0, 1),
        (n, 1_000_000),
        (1, 1),
        (0, 1),
        (0, 1),
    ] {
        out.extend(n.to_be_bytes());
        out.extend(d.to_be_bytes());
    }
    out
}
fn segment(payload: &[u8]) -> EngineResult<Vec<u8>> {
    let size = u16::try_from(payload.len() + 2).map_err(encode_error)?;
    Ok([&[255, 226], size.to_be_bytes().as_slice(), payload].concat())
}
fn mpf(base: usize, gain: usize, offset: usize) -> EngineResult<Vec<u8>> {
    let mut bytes = b"MPF\0MM\0\x2a\0\0\0\x08\0\x03".to_vec();
    for (id, kind, count, value) in [
        (0xb000u16, 7u16, 4u32, u32::from_be_bytes(*b"0100")),
        (0xb001, 4, 1, 2),
        (0xb002, 7, 32, 50),
    ] {
        bytes.extend(id.to_be_bytes());
        bytes.extend(kind.to_be_bytes());
        bytes.extend(count.to_be_bytes());
        bytes.extend(value.to_be_bytes());
    }
    bytes.extend([0; 4]);
    for (attributes, size, offset) in [(0x20030000u32, base, 0usize), (0, gain, offset)] {
        bytes.extend(attributes.to_be_bytes());
        bytes.extend(u32::try_from(size).map_err(encode_error)?.to_be_bytes());
        bytes.extend(u32::try_from(offset).map_err(encode_error)?.to_be_bytes());
        bytes.extend([0; 4]);
    }
    segment(&bytes)
}
fn sos_offset(bytes: &[u8]) -> EngineResult<usize> {
    let mut p = 2;
    while p + 4 <= bytes.len() {
        if bytes[p] != 255 {
            break;
        }
        if bytes[p + 1] == 0xda {
            return Ok(p);
        }
        let n = u16::from_be_bytes([bytes[p + 2], bytes[p + 3]]) as usize;
        if n < 2 {
            break;
        }
        p += 2 + n;
    }
    Err(encode_error("generated JPEG has no SOS"))
}
pub(crate) fn validate(settings: &ExportSettings) -> EngineResult<()> {
    if settings.gain_map
        && (!matches!(settings.format, Format::Jpeg { .. })
            || !matches!(settings.color_space, ColorSpace::Srgb)
            || settings.hdr.is_some()
            || settings.watermark.is_some())
    {
        return Err(encode_error(
            "gain-map output requires sRGB JPEG without PQ/HLG or watermark",
        ));
    }
    Ok(())
}
