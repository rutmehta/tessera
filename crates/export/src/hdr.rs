//! HDR file output. SDR diffuse white is 203 cd/m² (BT.2408). PQ is
//! absolute ST 2084; HLG uses the BT.2100 1000-nit reference display and
//! system gamma 1.2, including the luminance-dependent inverse OOTF.
use crate::{ExportImage, ExportSettings, Format, HdrTransfer, encode_error};
use engine_api::{EngineResult, jobs::CancellationToken, recipe::Recipe};
use std::io::{Seek, Write};

pub(crate) fn validate(settings: &ExportSettings) -> EngineResult<()> {
    if settings.hdr.is_none() {
        return Ok(());
    }
    if !matches!(
        settings.format,
        Format::Png | Format::Avif(crate::AvifOptions { bits: 10 | 12, .. })
    ) || !matches!(settings.color_space, crate::ColorSpace::Rec2020)
    {
        return Err(encode_error("HDR requires Rec.2020 PNG16 or AVIF10/12"));
    }
    if settings.watermark.is_some() {
        return Err(encode_error(
            "SDR watermark compositing is not supported for HDR",
        ));
    }
    Ok(())
}

fn headroom(recipe: &Recipe, transfer: HdrTransfer) -> EngineResult<f32> {
    let stops = recipe.settings.output.hdr_headroom_stops;
    if !stops.is_finite() || !(0.0..=16.0).contains(&stops) {
        return Err(encode_error("HDR headroom must be finite and 0–16 stops"));
    }
    let peak = match transfer {
        HdrTransfer::Pq => 10000.0,
        HdrTransfer::Hlg => 1000.0,
    };
    let headroom = if recipe.settings.output.hdr {
        stops.exp2()
    } else {
        1.0
    }
    .min(peak / 203.0);
    Ok(headroom)
}

/// Gamut-mapped display-linear Rec.2020 normalized to the recipe peak.
/// Output filters clamp to 0–1, so their ceiling must mean headroom, not
/// the transfer's absolute maximum (10,000 nits for PQ).
pub(crate) fn render(
    image: &ExportImage<'_>,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
) -> EngineResult<image::Rgb32FImage> {
    let headroom = headroom(recipe, settings.hdr.expect("HDR render selected"))?;
    let mut develop = recipe.settings.clone();
    // These output controls are consumed here, not by the scene renderer.
    develop.output.proof_profile = None;
    develop.output.hdr = false;
    develop.output.hdr_headroom_stops = 0.0;
    let scene = pipeline_cpu::render_linear_scaled(&develop, &image.source, settings.render_scale)?;
    let mut rgb = image::Rgb32FImage::new(scene.width(), scene.height());
    let a = pipeline_cpu::hdr_sigmoid_ln_a(headroom).exp();
    let p = pipeline_cpu::SigmoidSettings::default().contrast;
    for (i, pixel) in rgb.pixels_mut().enumerate() {
        if i % scene.width() as usize == 0 {
            cancel.check()?;
        }
        let v: [f32; 3] = std::array::from_fn(|c| scene.planes()[c][i]);
        if v.iter().any(|v| !v.is_finite()) {
            return Err(encode_error("non-finite HDR sample"));
        }
        let y = 0.2627 * v[0] + 0.6780 * v[1] + 0.0593 * v[2];
        let toned = if y > 0.0 {
            let out = headroom / (1.0 + (a / y).powf(p));
            let toned = v.map(|c| c * out / y);
            if develop.output.gamut_mapping == engine_api::recipe::settings::GamutMapping::Clip {
                toned.map(|c| c.clamp(0.0, headroom))
            } else {
                compress_gamut(toned, out, headroom)
            }
        } else {
            [0.0; 3]
        };
        *pixel = image::Rgb(toned.map(|c| c / headroom));
    }
    Ok(rgb)
}

/// Compress along a constant Rec.2020-luminance ray, preserving hue.
fn compress_gamut(rgb: [f32; 3], y: f32, max: f32) -> [f32; 3] {
    let mut chroma = 1.0f32;
    for c in rgb {
        if c < 0.0 {
            chroma = chroma.min(-y / (c - y));
        }
        if c > max {
            chroma = chroma.min((max - y) / (c - y));
        }
    }
    rgb.map(|c| (y + chroma * (c - y)).clamp(0.0, max))
}

/// Apply transfer-dependent gamut bounds and encode only after output filters.
pub(crate) fn finalize(
    rgb: &mut image::Rgb32FImage,
    recipe: &Recipe,
    settings: &ExportSettings,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let transfer = settings.hdr.expect("HDR finalize selected");
    let headroom = headroom(recipe, transfer)?;
    let width = rgb.width() as usize;
    for (i, pixel) in rgb.pixels_mut().enumerate() {
        if i % width == 0 {
            cancel.check()?;
        }
        let mut display = pixel.0.map(|c| c.clamp(0.0, 1.0) * headroom);
        if transfer == HdrTransfer::Hlg
            && recipe.settings.output.gamut_mapping
                != engine_api::recipe::settings::GamutMapping::Clip
        {
            // Filtering changes luminance, so recompute the inverse-OOTF
            // channel bound here rather than baking it into filter input.
            let y = 0.2627 * display[0] + 0.6780 * display[1] + 0.0593 * display[2];
            let max = headroom.min(1000.0 / 203.0 * (y * 203.0 / 1000.0).powf(1.0 / 6.0));
            display = compress_gamut(display, y, max);
        }
        *pixel = image::Rgb(encode_transfer(display, transfer));
    }
    Ok(())
}

fn encode_transfer(rgb: [f32; 3], transfer: HdrTransfer) -> [f32; 3] {
    match transfer {
        HdrTransfer::Pq => rgb.map(|v| {
            let l = (f64::from(v) * 203.0 / 10000.0)
                .clamp(0.0, 1.0)
                .powf(2610.0 / 16384.0);
            ((3424.0 / 4096.0 + 2413.0 / 128.0 * l) / (1.0 + 2392.0 / 128.0 * l))
                .powf(2523.0 / 32.0) as f32
        }),
        HdrTransfer::Hlg => {
            let display = rgb.map(|v| f64::from(v) * 203.0 / 1000.0);
            let yd = display[0] * 0.2627 + display[1] * 0.6780 + display[2] * 0.0593;
            let scale = if yd > 0.0 {
                yd.powf((1.0 - 1.2) / 1.2)
            } else {
                0.0
            };
            display.map(|v| {
                let scene = (v * scale).clamp(0.0, 1.0);
                (if scene <= 1.0 / 12.0 {
                    (3.0 * scene).sqrt()
                } else {
                    0.17883277 * (12.0 * scene - 0.28466892).ln() + 0.55991073
                }) as f32
            })
        }
    }
}

pub(crate) fn encode(
    writer: &mut (impl Write + Seek),
    rgb: &image::Rgb32FImage,
    settings: &ExportSettings,
    native: &crate::native::Native,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<()> {
    let transfer = settings.hdr.expect("HDR encode selected");
    cancel.check()?;
    match settings.format {
        Format::Png => {
            let mut info = png::Info::with_size(rgb.width(), rgb.height());
            info.color_type = png::ColorType::Rgb;
            info.bit_depth = png::BitDepth::Sixteen;
            if let Some(dpi) = settings.dpi.filter(|d| (1..=65535).contains(d)) {
                let ppm = (f64::from(dpi) / 0.0254).round() as u32;
                info.pixel_dims = Some(png::PixelDimensions {
                    xppu: ppm,
                    yppu: ppm,
                    unit: png::Unit::Meter,
                });
            }
            let mut encoder = png::Encoder::with_info(writer, info).map_err(encode_error)?;
            if let Some(xmp) = xmp {
                encoder
                    .add_itxt_chunk("XML:com.adobe.xmp".into(), xmp.into())
                    .map_err(encode_error)?;
            }
            let mut writer = encoder.write_header().map_err(encode_error)?;
            writer
                .write_chunk(
                    png::chunk::ChunkType(*b"cICP"),
                    &[
                        9,
                        match transfer {
                            HdrTransfer::Pq => 16,
                            HdrTransfer::Hlg => 18,
                        },
                        0,
                        1,
                    ],
                )
                .map_err(encode_error)?;
            let exif = native.tiff_bytes(true)?;
            if !exif.is_empty() {
                writer
                    .write_chunk(png::chunk::ChunkType(*b"eXIf"), &exif)
                    .map_err(encode_error)?;
            }
            let mut stream = writer.stream_writer().map_err(encode_error)?;
            let mut row = Vec::with_capacity(rgb.width() as usize * 6);
            for pixels in rgb.as_raw().chunks(rgb.width() as usize * 3) {
                cancel.check()?;
                row.clear();
                for v in pixels {
                    row.extend(((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_be_bytes());
                }
                stream.write_all(&row).map_err(encode_error)?;
            }
            stream.finish().map_err(encode_error)?;
            writer.finish().map_err(encode_error)?;
        }
        Format::Avif(options) => {
            let rgba = image::Rgba32FImage::from_fn(rgb.width(), rgb.height(), |x, y| {
                let [r, g, b] = rgb.get_pixel(x, y).0;
                image::Rgba([r, g, b, 1.0])
            });
            writer
                .write_all(&crate::avif::encode_hdr(
                    &rgba,
                    options,
                    transfer,
                    xmp,
                    Some(native),
                    cancel,
                )?)
                .map_err(encode_error)?;
        }
        _ => return Err(encode_error("unsupported HDR format")),
    }
    cancel.check()
}
