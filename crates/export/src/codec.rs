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
