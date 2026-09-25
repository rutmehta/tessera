use crate::{ColorSpace, Format, encode_error};
use engine_api::{EngineResult, jobs::CancellationToken};
use lcms2::{CIExyY, CIExyYTRIPLE, Profile, ToneCurve};
use std::io::{Seek, Write};

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;

pub(crate) fn profile(space: ColorSpace) -> EngineResult<Profile> {
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let (white, primaries, curve) = match space {
        ColorSpace::Srgb => return Ok(Profile::new_srgb()),
        ColorSpace::DisplayP3 => (
            xy(0.3127, 0.3290),
            [(0.680, 0.320), (0.265, 0.690), (0.150, 0.060)],
            ToneCurve::new_parametric(4, &[2.4, 1.0 / 1.055, 0.055 / 1.055, 1.0 / 12.92, 0.04045]),
        ),
        ColorSpace::Rec2020 => (
            xy(0.3127, 0.3290),
            [(0.708, 0.292), (0.170, 0.797), (0.131, 0.046)],
            ToneCurve::new_parametric(
                4,
                &[
                    1.0 / 0.45,
                    1.0 / 1.09929682680944,
                    0.09929682680944 / 1.09929682680944,
                    1.0 / 4.5,
                    4.5 * 0.018053968510807,
                ],
            ),
        ),
        ColorSpace::ProPhoto => (
            xy(0.3457, 0.3585),
            [(0.7347, 0.2653), (0.1596, 0.8404), (0.0366, 0.0001)],
            ToneCurve::new_parametric(4, &[1.8, 1.0, 0.0, 1.0 / 16.0, 0.03125]),
        ),
    };
    let [r, g, b] = primaries;
    let curve = curve.map_err(encode_error)?;
    Profile::new_rgb(
        &white,
        &CIExyYTRIPLE {
            Red: xy(r.0, r.1),
            Green: xy(g.0, g.1),
            Blue: xy(b.0, b.1),
        },
        &[&curve, &curve, &curve],
    )
    .map_err(encode_error)
}

pub(crate) fn encode(
    writer: &mut (impl Write + Seek),
    rgb: &image::Rgb32FImage,
    format: Format,
    space: ColorSpace,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let mut writer = CancelWriter {
        inner: writer,
        cancel,
    };
    let result = encode_inner(&mut writer, rgb, format, space, xmp, cancel);
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
    format: Format,
    space: ColorSpace,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let source = Profile::new_srgb();
    let profile = profile(space)?;
    let transform = lcms2::Transform::<[f32; 3], [u16; 3]>::new(
        &source,
        lcms2::PixelFormat::RGB_FLT,
        &profile,
        lcms2::PixelFormat::RGB_16,
        lcms2::Intent::RelativeColorimetric,
    )
    .map_err(encode_error)?;
    let mut pixels = vec![[0u16; 3]; (rgb.width() as usize) * (rgb.height() as usize)];
    for (src, dst) in rgb
        .as_raw()
        .chunks(rgb.width() as usize * 3)
        .zip(pixels.chunks_mut(rgb.width() as usize))
    {
        cancel.check()?;
        transform.transform_pixels(src.as_chunks::<3>().0, dst);
    }
    let icc = profile.icc().map_err(encode_error)?;
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
                encoder::{TiffEncoder, colortype},
                tags::Tag,
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
