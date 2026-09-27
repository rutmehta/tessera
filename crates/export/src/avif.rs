//! AV1 still pictures with full-range identity (GBR) planes. Direct rav1e use
//! preserves float precision through 8/10/12-bit quantization, unlike an RGB8
//! intermediary. The small HEIF muxer attaches ICC, CICP, XMP and straight alpha.
use crate::{ColorSpace, color_space_icc, encode_error};
use engine_api::{EngineResult, jobs::CancellationToken};
use rav1e::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AvifOptions {
    pub bits: u8,
    /// 1–100; 100 uses AV1's minimum nonzero quantizer, not lossless mode.
    pub quality: u8,
    /// 1 (slow) through 10 (fast).
    pub speed: u8,
}
impl Default for AvifOptions {
    fn default() -> Self {
        Self {
            bits: 8,
            quality: 90,
            speed: 6,
        }
    }
}
impl AvifOptions {
    pub fn validate(self) -> EngineResult<()> {
        if !matches!(self.bits, 8 | 10 | 12)
            || !(1..=100).contains(&self.quality)
            || !(1..=10).contains(&self.speed)
        {
            return Err(encode_error(
                "AVIF requires 8/10/12 bits, quality 1–100, speed 1–10",
            ));
        }
        Ok(())
    }
}

/// Encode document-encoded, straight RGBA floats. RGB is described by `space`;
/// alpha is linear coverage. Values are quantized only at the encoder boundary.
/// The develop renderer supplies opaque RGB; callers with alpha can use this
/// entry point directly. Cancellation is checked between AV1 encoder calls;
/// rav1e cannot interrupt an in-progress frame encode.
pub fn encode_avif(
    rgba: &image::Rgba32FImage,
    options: AvifOptions,
    space: ColorSpace,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<Vec<u8>> {
    encode_inner(rgba, options, space, None, xmp, cancel)
}

pub(crate) fn encode_hdr(
    rgba: &image::Rgba32FImage,
    options: AvifOptions,
    transfer: crate::HdrTransfer,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<Vec<u8>> {
    encode_inner(
        rgba,
        options,
        ColorSpace::Rec2020,
        Some(transfer),
        xmp,
        cancel,
    )
}

fn encode_inner(
    rgba: &image::Rgba32FImage,
    options: AvifOptions,
    space: ColorSpace,
    hdr: Option<crate::HdrTransfer>,
    xmp: Option<&str>,
    cancel: &CancellationToken,
) -> EngineResult<Vec<u8>> {
    cancel.check()?;
    options.validate()?;
    let (w, h) = rgba.dimensions();
    if w == 0 || h == 0 || w > 65535 || h > 65535 || u64::from(w) * u64::from(h) > 100_000_000 {
        return Err(encode_error(
            "AVIF dimensions must be 1–65535 and at most 100 megapixels",
        ));
    }
    if rgba.as_raw().iter().any(|v| !v.is_finite()) {
        return Err(encode_error("AVIF samples must be finite"));
    }
    if xmp.is_some_and(|v| v.len() > 1 << 20) {
        return Err(encode_error("AVIF XMP exceeds 1 MiB"));
    }
    let cicp = match space {
        ColorSpace::Srgb => (ColorPrimaries::BT709, TransferCharacteristics::SRGB),
        ColorSpace::DisplayP3 => (ColorPrimaries::SMPTE432, TransferCharacteristics::SRGB),
        // The built-in Rec.2020 ICC has gamma 2.4, not BT.2020's video OETF.
        // Mark transfer unspecified rather than contradicting the ICC profile.
        ColorSpace::Rec2020 => (ColorPrimaries::BT2020, TransferCharacteristics::Unspecified),
        ColorSpace::ProPhoto => (
            ColorPrimaries::Unspecified,
            TransferCharacteristics::Unspecified,
        ),
    };
    let cicp = match hdr {
        Some(crate::HdrTransfer::Pq) => {
            (ColorPrimaries::BT2020, TransferCharacteristics::SMPTE2084)
        }
        Some(crate::HdrTransfer::Hlg) => (ColorPrimaries::BT2020, TransferCharacteristics::HLG),
        None => cicp,
    };
    let color = av1(rgba, options, Some(cicp), cancel)?;
    let alpha = if rgba.pixels().any(|p| p[3] < 1.0) {
        Some(av1(rgba, options, None, cancel)?)
    } else {
        None
    };
    cancel.check()?;
    // An SDR ICC would override/contradict PQ or HLG in colour-managed readers.
    let icc = if hdr.is_some() {
        Vec::new()
    } else {
        color_space_icc(space)?
    };
    mux(
        w,
        h,
        options.bits,
        cicp,
        &icc,
        &color,
        alpha.as_deref(),
        xmp,
    )
}

fn av1(
    rgba: &image::Rgba32FImage,
    options: AvifOptions,
    color: Option<(ColorPrimaries, TransferCharacteristics)>,
    cancel: &CancellationToken,
) -> EngineResult<Vec<u8>> {
    let (width, height) = (rgba.width() as usize, rgba.height() as usize);
    let mut cfg = EncoderConfig::with_speed_preset(options.speed);
    cfg.width = width;
    cfg.height = height;
    cfg.bit_depth = options.bits as usize;
    cfg.still_picture = true;
    cfg.chroma_sampling = if color.is_some() {
        ChromaSampling::Cs444
    } else {
        ChromaSampling::Cs400
    };
    cfg.pixel_range = PixelRange::Full;
    cfg.color_description =
        color.map(
            |(color_primaries, transfer_characteristics)| ColorDescription {
                color_primaries,
                transfer_characteristics,
                matrix_coefficients: MatrixCoefficients::Identity,
            },
        );
    cfg.quantizer = 1 + (100 - options.quality) as usize * 254 / 99;
    cfg.min_quantizer = cfg.quantizer as u8;
    let mut ctx: Context<u16> = Config::new()
        .with_encoder_config(cfg)
        .with_threads(1)
        .new_context()
        .map_err(encode_error)?;
    let mut frame = ctx.new_frame();
    let max = ((1u32 << options.bits) - 1) as f32;
    for (plane, channel) in frame.planes.iter_mut().zip(if color.is_some() {
        vec![1, 2, 0]
    } else {
        vec![3]
    }) {
        for (y, row) in plane
            .mut_slice(Default::default())
            .rows_iter_mut()
            .take(height)
            .enumerate()
        {
            cancel.check()?;
            for (x, sample) in row[..width].iter_mut().enumerate() {
                *sample = (rgba.get_pixel(x as u32, y as u32)[channel].clamp(0.0, 1.0) * max)
                    .round() as u16;
            }
        }
    }
    ctx.send_frame(frame).map_err(encode_error)?;
    ctx.flush();
    let mut result = Vec::new();
    loop {
        cancel.check()?;
        match ctx.receive_packet() {
            Ok(packet) => result.extend_from_slice(&packet.data),
            Err(EncoderStatus::Encoded) => continue,
            Err(EncoderStatus::LimitReached) => break,
            Err(e) => return Err(encode_error(e)),
        }
    }
    if result.is_empty() {
        return Err(encode_error("AV1 encoder returned no picture"));
    }
    Ok(result)
}

fn box_bytes(kind: &[u8; 4], data: &[u8]) -> EngineResult<Vec<u8>> {
    let size = u32::try_from(
        data.len()
            .checked_add(8)
            .ok_or_else(|| encode_error("AVIF box overflow"))?,
    )
    .map_err(encode_error)?;
    let mut out = size.to_be_bytes().to_vec();
    out.extend(kind);
    out.extend(data);
    Ok(out)
}
fn full_box(kind: &[u8; 4], version: u8, data: &[u8]) -> EngineResult<Vec<u8>> {
    box_bytes(kind, &[&[version, 0, 0, 0], data].concat())
}

// Items 1=color, 2=alpha when present, last=XMP. iloc uses construction_method
// 1 (offsets relative to idat), so metadata size never changes media offsets.
#[allow(clippy::too_many_arguments)]
fn mux(
    w: u32,
    h: u32,
    bits: u8,
    cicp: (ColorPrimaries, TransferCharacteristics),
    icc: &[u8],
    color: &[u8],
    alpha: Option<&[u8]>,
    xmp: Option<&str>,
) -> EngineResult<Vec<u8>> {
    let ftyp = box_bytes(b"ftyp", b"avif\0\0\0\0avifmif1")?;
    let mut items = vec![(b"av01", color)];
    if let Some(a) = alpha {
        items.push((b"av01", a));
    }
    if let Some(x) = xmp {
        items.push((b"mime", x.as_bytes()));
    }
    let mut meta = full_box(b"hdlr", 0, b"\0\0\0\0pict\0\0\0\0\0\0\0\0\0\0\0\0Tessera\0")?;
    meta.extend(full_box(b"pitm", 0, &1u16.to_be_bytes())?);
    let mut info = (items.len() as u16).to_be_bytes().to_vec();
    let mut locations = vec![0x44, 0]; // offset_size=4, length_size=4
    locations.extend((items.len() as u16).to_be_bytes());
    let mut data = Vec::new();
    for (index, (kind, payload)) in items.iter().enumerate() {
        let id = (index + 1) as u16;
        let mut entry = id.to_be_bytes().to_vec();
        entry.extend([0, 0]); // protection index
        entry.extend(*kind);
        entry.push(0); // item name
        if *kind == b"mime" {
            entry.extend(b"application/rdf+xml\0\0");
        }
        info.extend(full_box(b"infe", 2, &entry)?);
        locations.extend(id.to_be_bytes());
        locations.extend(1u16.to_be_bytes()); // construction_method=idat
        locations.extend(0u16.to_be_bytes()); // data reference
        locations.extend(1u16.to_be_bytes()); // extent count
        locations.extend(
            u32::try_from(data.len())
                .map_err(encode_error)?
                .to_be_bytes(),
        );
        locations.extend(
            u32::try_from(payload.len())
                .map_err(encode_error)?
                .to_be_bytes(),
        );
        data.extend_from_slice(payload);
    }
    meta.extend(full_box(b"iinf", 0, &info)?);
    meta.extend(full_box(b"iloc", 1, &locations)?);
    let mut props = full_box(b"ispe", 0, &[w.to_be_bytes(), h.to_be_bytes()].concat())?; // 1
    let profile = if bits == 12 { 2 } else { 1 };
    let depth = if bits > 8 { 0x40 } else { 0 } | if bits == 12 { 0x20 } else { 0 };
    props.extend(box_bytes(b"av1C", &[0x81, (profile << 5) | 31, depth, 0])?); // 2
    props.extend(full_box(b"pixi", 0, &[3, bits, bits, bits])?); // 3
    let mut nclx = b"nclx".to_vec();
    nclx.extend((cicp.0 as u16).to_be_bytes());
    nclx.extend((cicp.1 as u16).to_be_bytes());
    nclx.extend(0u16.to_be_bytes()); // identity matrix
    nclx.push(0x80); // full range
    props.extend(box_bytes(b"colr", &nclx)?); // 4
    let base_props = if icc.is_empty() {
        4u8
    } else {
        props.extend(box_bytes(b"colr", &[b"prof".as_slice(), icc].concat())?); // 5
        5u8
    };
    let mut associations = (if alpha.is_some() { 2u32 } else { 1u32 })
        .to_be_bytes()
        .to_vec();
    associations.extend([0, 1, base_props, 1, 0x82, 3, 4]);
    if !icc.is_empty() {
        associations.push(5);
    }
    let mut refs = Vec::new();
    if alpha.is_some() {
        let profile = if bits == 12 { 2 } else { 0 };
        props.extend(box_bytes(
            b"av1C",
            &[0x81, (profile << 5) | 31, depth | 0x1c, 0],
        )?); // 6
        props.extend(full_box(b"pixi", 0, &[1, bits])?); // 7
        props.extend(full_box(
            b"auxC",
            0,
            b"urn:mpeg:mpegB:cicp:systems:auxiliary:alpha\0",
        )?); // 8
        associations.extend([
            0,
            2,
            4,
            1,
            0x80 | (base_props + 1),
            base_props + 2,
            0x80 | (base_props + 3),
        ]);
        refs.extend(box_bytes(b"auxl", &[0, 2, 0, 1, 0, 1])?);
    }
    if xmp.is_some() {
        let mut r = (items.len() as u16).to_be_bytes().to_vec();
        r.extend([0, 1, 0, 1]);
        refs.extend(box_bytes(b"cdsc", &r)?);
    }
    let mut iprp = box_bytes(b"ipco", &props)?;
    iprp.extend(full_box(b"ipma", 0, &associations)?);
    meta.extend(box_bytes(b"iprp", &iprp)?);
    if !refs.is_empty() {
        meta.extend(full_box(b"iref", 0, &refs)?);
    }
    meta.extend(box_bytes(b"idat", &data)?);
    Ok([ftyp, full_box(b"meta", 0, &meta)?].concat())
}
