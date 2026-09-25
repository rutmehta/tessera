use crate::{ColorSpace, Format, encode_error};
use color_mgmt::{Builtin, Profile, Registry};
use engine_api::{EngineResult, jobs::CancellationToken};
use std::io::{Seek, Write};
use std::sync::Arc;

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;

pub(crate) fn profile(registry: &mut Registry, space: ColorSpace) -> EngineResult<Arc<Profile>> {
    registry
        .builtin(match space {
            ColorSpace::Srgb => Builtin::Srgb,
            ColorSpace::DisplayP3 => Builtin::DisplayP3,
            ColorSpace::Rec2020 => Builtin::Rec2020,
            ColorSpace::ProPhoto => Builtin::ProPhoto,
        })
        .map_err(encode_error)
}

/// Container choices for one encode.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Encoding {
    pub format: Format,
    pub space: ColorSpace,
    /// Pixel density to record (metadata only).
    pub dpi: Option<u32>,
}

pub(crate) fn encode(
    writer: &mut (impl Write + Seek),
    rgb: &image::Rgb32FImage,
    encoding: Encoding,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let mut writer = CancelWriter {
        inner: writer,
        cancel,
    };
    let result = encode_inner(&mut writer, rgb, encoding, xmp, cancel);
    cancel.check()?;
    result
}

struct CancelWriter<'a, W> {
    inner: W,
    cancel: &'a CancellationToken,
}
impl<W> CancelWriter<'_, W> {
    fn check(&self) -> std::io::Result<()> {
        // Not Interrupted: write_all retries Interrupted forever.
        if self.cancel.is_cancelled() {
            Err(std::io::Error::other("export cancelled"))
        } else {
            Ok(())
        }
    }
}
impl<W: Write> Write for CancelWriter<'_, W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.check()?;
        self.inner.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.check()?;
        self.inner.flush()
    }
}
impl<W: Seek> Seek for CancelWriter<'_, W> {
    fn seek(&mut self, pos: std::io::SeekFrom) -> std::io::Result<u64> {
        self.check()?;
        self.inner.seek(pos)
    }
}

fn encode_inner(
    writer: &mut (impl Write + Seek),
    rgb: &image::Rgb32FImage,
    encoding: Encoding,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let Encoding { format, space, dpi } = encoding;
    let dpi = dpi.filter(|d| (1..=u32::from(u16::MAX)).contains(d));
    // render_full supplies destination-encoded float RGB. Quantize only here;
    // a second CMM conversion would double-encode the document colour space.
    let mut registry = Registry::new();
    let profile = profile(&mut registry, space)?;
    if let Format::Jpeg { quality } = format
        && let Some(stitched) = jpeg_stripes(rgb, quality, dpi, profile.icc_bytes(), xmp, cancel)?
    {
        return writer.write_all(&stitched).map_err(encode_error);
    }
    let mut pixels = vec![[0u16; 3]; (rgb.width() as usize) * (rgb.height() as usize)];
    for (src, dst) in rgb
        .as_raw()
        .chunks(rgb.width() as usize * 3)
        .zip(pixels.chunks_mut(rgb.width() as usize))
    {
        cancel.check()?;
        for (src, dst) in src.as_chunks::<3>().0.iter().zip(dst) {
            *dst = src.map(|v| (v.clamp(0.0, 1.0) * 65535.0).round() as u16);
        }
    }
    let icc = profile.icc_bytes().to_vec();
    let bytes = || {
        pixels
            .as_flattened()
            .iter()
            .map(|v| ((u32::from(*v) + 128) / 257) as u8)
            .collect::<Vec<_>>()
    };
    match format {
        Format::Jpeg { quality } => {
            let mut encoder = jpeg_encoder::Encoder::new(writer, quality);
            if let Some(dpi) = dpi {
                let dpi = dpi as u16;
                encoder.set_density(jpeg_encoder::Density::Inch { x: dpi, y: dpi });
            }
            encoder.add_icc_profile(&icc).map_err(encode_error)?;
            if let Some(text) = xmp {
                let mut packet = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
                packet.extend_from_slice(text.as_bytes());
                encoder.add_app_segment(1, &packet).map_err(encode_error)?;
            }
            encoder
                .encode(
                    &bytes(),
                    u16::try_from(rgb.width()).map_err(encode_error)?,
                    u16::try_from(rgb.height()).map_err(encode_error)?,
                    jpeg_encoder::ColorType::Rgb,
                )
                .map_err(encode_error)?;
        }
        Format::Png => {
            let mut info = png::Info::with_size(rgb.width(), rgb.height());
            info.color_type = png::ColorType::Rgb;
            info.bit_depth = png::BitDepth::Eight;
            info.icc_profile = Some(icc.into());
            if let Some(dpi) = dpi {
                // pHYs is per metre: 1 inch = 0.0254 m.
                let ppm = (f64::from(dpi) / 0.0254).round() as u32;
                info.pixel_dims = Some(png::PixelDimensions {
                    xppu: ppm,
                    yppu: ppm,
                    unit: png::Unit::Meter,
                });
            }
            let mut encoder = png::Encoder::with_info(writer, info).map_err(encode_error)?;
            if let Some(text) = xmp {
                encoder
                    .add_itxt_chunk("XML:com.adobe.xmp".into(), text.into())
                    .map_err(encode_error)?;
            }
            let mut writer = encoder.write_header().map_err(encode_error)?;
            writer.write_image_data(&bytes()).map_err(encode_error)?;
            writer.finish().map_err(encode_error)?;
        }
        Format::Tiff { bits } => {
            use tiff::{
                encoder::{Rational, TiffEncoder, colortype},
                tags::{ResolutionUnit, Tag},
            };
            let mut encoder = TiffEncoder::new(writer).map_err(encode_error)?;
            macro_rules! write_tiff {
                ($color:ty, $data:expr) => {{
                    let mut image = encoder
                        .new_image::<$color>(rgb.width(), rgb.height())
                        .map_err(encode_error)?;
                    image
                        .encoder()
                        .write_tag(Tag::Unknown(34675), icc.as_slice())
                        .map_err(encode_error)?;
                    if let Some(text) = xmp {
                        image
                            .encoder()
                            .write_tag(Tag::Unknown(700), text.as_bytes())
                            .map_err(encode_error)?;
                    }
                    if let Some(dpi) = dpi {
                        image.resolution(ResolutionUnit::Inch, Rational { n: dpi, d: 1 });
                    }
                    image.write_data($data).map_err(encode_error)?;
                }};
            }
            if bits == 16 {
                write_tiff!(colortype::RGB16, pixels.as_flattened());
            } else {
                write_tiff!(colortype::RGB8, &bytes());
            }
        }
    }
    Ok(())
}

/// The 8-bit sample the 16-bit path produces for `v` (identical rounding).
fn quantize8(v: f32) -> u8 {
    let v = (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
    ((u32::from(v) + 128) / 257) as u8
}

fn jpeg_encoder<W: jpeg_encoder::JfifWrite>(
    writer: W,
    quality: u8,
    dpi: Option<u32>,
) -> jpeg_encoder::Encoder<W> {
    let mut encoder = jpeg_encoder::Encoder::new(writer, quality);
    if let Some(dpi) = dpi {
        let dpi = dpi as u16;
        encoder.set_density(jpeg_encoder::Density::Inch { x: dpi, y: dpi });
    }
    encoder
}

/// Stripe-parallel baseline JPEG. The image is cut into stripes of whole
/// MCU rows, each encoded independently on the rayon pool with the same
/// tables, and stitched with restart markers: the result is byte-identical
/// to one sequential encode with a restart interval of one stripe (tested),
/// and decodes to the same pixels as the unstriped encode. Restart markers
/// reset the DC predictors exactly where each stripe's encoder started.
/// None when the image is too small to split (the caller encodes serially).
pub(crate) fn jpeg_stripes(
    rgb: &image::Rgb32FImage,
    quality: u8,
    dpi: Option<u32>,
    icc: &[u8],
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<Option<Vec<u8>>> {
    let (width, height) = rgb.dimensions();
    // The encoder's default sampling for this quality (4:2:0 below 90).
    let mcu = match jpeg_encoder::Encoder::new(Vec::new(), quality).sampling_factor() {
        jpeg_encoder::SamplingFactor::F_2_2 => 16,
        jpeg_encoder::SamplingFactor::F_1_1 => 8,
        _ => return Ok(None),
    };
    let threads = rayon::current_num_threads().max(1) as u32;
    if threads < 2 || u64::from(width) * u64::from(height) < 1 << 18 {
        return Ok(None);
    }
    let stripe_mcu_rows = height.div_ceil(mcu).div_ceil(4 * threads);
    jpeg_stripes_of(rgb, quality, dpi, icc, xmp, cancel, (mcu, stripe_mcu_rows))
}

/// [`jpeg_stripes`] with `stripe_mcu_rows` MCU rows (of `mcu` pixels) per stripe.
pub(crate) fn jpeg_stripes_of(
    rgb: &image::Rgb32FImage,
    quality: u8,
    dpi: Option<u32>,
    icc: &[u8],
    xmp: Option<&str>,
    cancel: &CancellationToken,
    (mcu, stripe_mcu_rows): (u32, u32),
) -> EngineResult<Option<Vec<u8>>> {
    use rayon::prelude::*;
    let (width, height) = rgb.dimensions();
    let (Ok(w16), Ok(_)) = (u16::try_from(width), u16::try_from(height)) else {
        return Ok(None);
    };
    let mcus_per_row = width.div_ceil(mcu);
    let mcu_rows = height.div_ceil(mcu);
    let stripe_mcu_rows = stripe_mcu_rows
        .max(1)
        .min(u32::from(u16::MAX) / mcus_per_row.max(1));
    if stripe_mcu_rows == 0 {
        return Ok(None);
    }
    let stripes = mcu_rows.div_ceil(stripe_mcu_rows);
    if stripes < 2 {
        return Ok(None);
    }
    let stripe_rows = stripe_mcu_rows * mcu;
    let row_len = width as usize * 3;
    let encoded = (0..stripes)
        .into_par_iter()
        .map(|i| -> EngineResult<Vec<u8>> {
            cancel.check()?;
            let top = i * stripe_rows;
            let rows = stripe_rows.min(height - top);
            let samples = &rgb.as_raw()[top as usize * row_len..(top + rows) as usize * row_len];
            let bytes: Vec<u8> = samples.iter().map(|&v| quantize8(v)).collect();
            let mut out = Vec::with_capacity(bytes.len() / 4);
            let mut encoder = jpeg_encoder(&mut out, quality, dpi);
            if i == 0 {
                encoder.add_icc_profile(icc).map_err(encode_error)?;
                if let Some(text) = xmp {
                    let mut packet = b"http://ns.adobe.com/xap/1.0/\0".to_vec();
                    packet.extend_from_slice(text.as_bytes());
                    encoder.add_app_segment(1, &packet).map_err(encode_error)?;
                }
            }
            encoder
                .encode(&bytes, w16, rows as u16, jpeg_encoder::ColorType::Rgb)
                .map_err(encode_error)?;
            Ok(out)
        })
        .collect::<EngineResult<Vec<_>>>()?;
    cancel.check()?;
    let mut out = Vec::with_capacity(encoded.iter().map(Vec::len).sum::<usize>() + 64);
    for (i, stripe) in encoded.iter().enumerate() {
        let (header, sos, data) = split_jpeg(stripe)?;
        if i == 0 {
            let mut header = header.to_vec();
            patch_height(&mut header, height as u16)?;
            out.extend_from_slice(&header);
            let interval = (stripe_mcu_rows * mcus_per_row) as u16;
            out.extend_from_slice(&[0xFF, 0xDD, 0x00, 0x04]);
            out.extend_from_slice(&interval.to_be_bytes());
            out.extend_from_slice(sos);
        } else {
            out.extend_from_slice(&[0xFF, 0xD0 + ((i - 1) % 8) as u8]);
        }
        out.extend_from_slice(data);
    }
    out.extend_from_slice(&[0xFF, 0xD9]);
    Ok(Some(out))
}

/// (SOI .. last table segment, SOS segment, entropy-coded data) of a
/// baseline JPEG produced by jpeg-encoder (no restart markers, EOI last).
fn split_jpeg(bytes: &[u8]) -> EngineResult<(&[u8], &[u8], &[u8])> {
    let bad = || encode_error("unexpected JPEG stripe layout");
    if bytes.len() < 4 || bytes[..2] != [0xFF, 0xD8] || bytes[bytes.len() - 2..] != [0xFF, 0xD9] {
        return Err(bad());
    }
    let mut at = 2;
    loop {
        if at + 4 > bytes.len() || bytes[at] != 0xFF {
            return Err(bad());
        }
        let len = usize::from(u16::from_be_bytes([bytes[at + 2], bytes[at + 3]]));
        let end = at + 2 + len;
        if end > bytes.len() - 2 {
            return Err(bad());
        }
        if bytes[at + 1] == 0xDA {
            return Ok((&bytes[..at], &bytes[at..end], &bytes[end..bytes.len() - 2]));
        }
        at = end;
    }
}

/// Sets the SOF0 frame height in a JPEG header.
fn patch_height(header: &mut [u8], height: u16) -> EngineResult<()> {
    let mut at = 2;
    while at + 4 <= header.len() {
        let len = usize::from(u16::from_be_bytes([header[at + 2], header[at + 3]]));
        if header[at + 1] == 0xC0 && at + 7 <= header.len() {
            header[at + 5..at + 7].copy_from_slice(&height.to_be_bytes());
            return Ok(());
        }
        at += 2 + len;
    }
    Err(encode_error("JPEG stripe without a baseline frame header"))
}
