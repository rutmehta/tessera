//! RGB PSD/PSB interchange with lossless opaque source retention.
//!
//! The editable state uses canvas-clipped rasters. Source records retain
//! off-canvas pixels and metadata that the compositor cannot interpret.
use crate::{Depth, DocState, Layer, LayerId, LayerKind, Raster, Rect};
use ::psd::{Channel, ColorMode, Compression, PsdDocument};
use engine_api::{EngineError, EngineResult, tile::Extent};
use std::{
    collections::BTreeMap,
    ops::{Deref, DerefMut},
    sync::Arc,
};

/// Editable document plus original PSD records. Keep this wrapper when editing
/// to retain opaque metadata; the native document format does not serialize it.
#[derive(Clone, Debug)]
pub struct ImportedPsd {
    /// Editable compositor state, including its normal history-compatible IDs.
    pub state: DocState,
    /// Features preserved in the source but not fully rendered by the compositor.
    pub warnings: Vec<String>,
    source: PsdDocument,
    originals: BTreeMap<LayerId, ::psd::Layer>,
    endings: BTreeMap<LayerId, ::psd::Layer>,
}
impl ImportedPsd {
    /// Wrap a native RGB state for PSD export. Unsupported procedural content
    /// returns an error from `to_psd`; it is never silently discarded.
    pub fn from_state(mut state: DocState) -> EngineResult<Self> {
        if state.canvas.width == 0
            || state.canvas.height == 0
            || state.canvas.width > 300_000
            || state.canvas.height > 300_000
        {
            return Err(error("invalid PSD canvas"));
        }
        let mut root = std::mem::take(&mut state.root);
        for layer in &mut root {
            state.assign_ids(Arc::make_mut(layer), false);
        }
        state.root = root;
        let source = PsdDocument {
            version: if state.canvas.width > 30_000 || state.canvas.height > 30_000 {
                ::psd::Version::Psb
            } else {
                ::psd::Version::Psd
            },
            width: state.canvas.width,
            height: state.canvas.height,
            depth: match state.depth {
                Depth::U8 => 8,
                Depth::U16 => 16,
                Depth::F32 => 32,
            },
            channels: 4,
            color_mode: ColorMode::Rgb,
            color_data: Vec::new(),
            resources: Vec::new(),
            layer_section: ::psd::LayerSection {
                merged_alpha: true,
                ..Default::default()
            },
            composite: Vec::new(),
            compression: Compression::Raw,
        };
        Ok(Self {
            state,
            source,
            originals: BTreeMap::new(),
            endings: BTreeMap::new(),
            warnings: Vec::new(),
        })
    }
    /// Exact imported record, including smart-object originals, text EngineData,
    /// and unknown tagged blocks. Layer IDs survive ordinary edits/history.
    pub fn original_layer(&self, id: LayerId) -> Option<&::psd::Layer> {
        self.originals.get(&id)
    }
    /// Original file IR, including document-level linked-file and resource blocks.
    pub fn original_document(&self) -> &PsdDocument {
        &self.source
    }
}
impl Deref for ImportedPsd {
    type Target = DocState;
    fn deref(&self) -> &DocState {
        &self.state
    }
}
impl DerefMut for ImportedPsd {
    fn deref_mut(&mut self) -> &mut DocState {
        &mut self.state
    }
}
fn error(message: impl ToString) -> EngineError {
    EngineError::invalid("PSD adapter", message.to_string())
}
fn depth(bits: u16) -> EngineResult<Depth> {
    match bits {
        8 => Ok(Depth::U8),
        16 => Ok(Depth::U16),
        32 => Ok(Depth::F32),
        _ => Err(error("only 8/16/32-bit RGB is supported")),
    }
}
fn sample(bytes: &[u8], index: usize, depth: Depth) -> f32 {
    let i = index * depth.bytes();
    match depth {
        Depth::U8 => bytes[i] as f32 / 255.0,
        Depth::U16 => u16::from_be_bytes(bytes[i..i + 2].try_into().unwrap()) as f32 / 65535.0,
        Depth::F32 => f32::from_be_bytes(bytes[i..i + 4].try_into().unwrap()),
    }
}
fn raster(layer: &::psd::Layer, extent: Extent, depth: Depth) -> EngineResult<Raster> {
    let (w, h) = layer.bounds.dimensions().map_err(error)?;
    let mut out = Raster::new(extent, 4, depth, 0.0);
    for channel in &layer.channels {
        if (-1..=2).contains(&channel.id) && channel.data.len() != w * h * depth.bytes() {
            return Err(error("incorrect channel sample count"));
        }
    }
    let b = layer.bounds;
    out.edit_region(
        Rect::new(b.left as i64, b.top as i64, b.right as i64, b.bottom as i64),
        0,
        |x, y, p| {
            let index = (i64::from(y) - i64::from(b.top)) as usize * w
                + (i64::from(x) - i64::from(b.left)) as usize;
            *p = [0.0, 0.0, 0.0, 1.0];
            for c in &layer.channels {
                let component = match c.id {
                    0..=2 => c.id as usize,
                    -1 => 3,
                    _ => continue,
                };
                p[component] = sample(&c.data, index, depth);
            }
        },
    )?;
    Ok(out)
}
/// Import RGB layers; PSD file order is top-first, compositor order bottom-first.
pub fn from_psd(source: &PsdDocument) -> EngineResult<ImportedPsd> {
    if source.color_mode != ColorMode::Rgb {
        return Err(error("only RGB PSD documents are supported"));
    }
    if source.width == 0 || source.height == 0 {
        return Err(error("empty canvas"));
    }
    let mut state = DocState::new(
        Extent::new(source.width, source.height),
        depth(source.depth)?,
    );
    if let Some(resolution) = source.resolution().map_err(error)? {
        state.ppi = resolution.horizontal as f32 / 65536.0;
    }
    for (id, target) in [
        (1037, &mut state.global_light.angle),
        (1049, &mut state.global_light.elevation),
    ] {
        if let Some(bytes) = source
            .resources
            .iter()
            .find(|r| r.id == id)
            .and_then(|r| r.data.get(..4))
        {
            *target = i32::from_be_bytes(bytes.try_into().unwrap()) as f32;
        }
    }
    state.profile = source
        .icc_profile()
        .map(|bytes| crate::ColorProfile::from_icc("Embedded PSD profile", bytes.to_vec()));
    let mut imported = ImportedPsd {
        state,
        warnings: Vec::new(),
        source: source.clone(),
        originals: BTreeMap::new(),
        endings: BTreeMap::new(),
    };
    imported.state.root =
        import_nodes(&source.layer_section.layers, &mut 0, &mut imported, None, 0)?;
    if source.layer_section.layers.is_empty() {
        let bytes = source.width as usize * source.height as usize * imported.depth.bytes();
        if source.channels < 3 || source.composite.len() != bytes * source.channels as usize {
            return Err(error("incorrect merged sample count"));
        }
        let original = ::psd::Layer {
            name: b"Background".to_vec(),
            bounds: ::psd::Rect {
                top: 0,
                left: 0,
                bottom: source.height as i32,
                right: source.width as i32,
            },
            channels: (0..source.channels.min(4))
                .map(|c| Channel {
                    id: if c == 3 { -1 } else { c as i16 },
                    compression: source.compression,
                    data: source.composite[c as usize * bytes..(c as usize + 1) * bytes].to_vec(),
                })
                .collect(),
            ..Default::default()
        };
        let mut layer = Layer::new(
            "Background",
            LayerKind::Pixel(raster(&original, imported.canvas, imported.depth)?),
        );
        imported.state.assign_ids(&mut layer, false);
        imported.originals.insert(layer.id, original);
        imported.state.root.push(Arc::new(layer));
    }
    Ok(imported)
}
fn read_u16(data: &[u8], offset: usize) -> EngineResult<u16> {
    Ok(u16::from_be_bytes(
        data.get(offset..offset + 2)
            .ok_or_else(|| error("truncated adjustment"))?
            .try_into()
            .unwrap(),
    ))
}
fn read_curve(data: &[u8], offset: &mut usize) -> EngineResult<crate::Curve> {
    let count = read_u16(data, *offset)? as usize;
    *offset += 2;
    if !(2..=19).contains(&count) {
        return Err(error("invalid curve point count"));
    }
    let mut points = Vec::with_capacity(count);
    for _ in 0..count {
        let output = read_u16(data, *offset)?;
        let input = read_u16(data, *offset + 2)?;
        *offset += 4;
        if output > 255 || input > 255 {
            return Err(error("curve coordinates must be in 0..255"));
        }
        points.push([input as f32 / 255.0, output as f32 / 255.0]);
    }
    Ok(crate::Curve(points))
}

fn import_adjustment(layer: &::psd::Layer) -> EngineResult<Option<crate::Adjustment>> {
    use crate::{Adjustment as A, LevelsChannel};
    for b in &layer.additional {
        let d = &b.data;
        let a = match &b.key {
            b"nvrt" => A::Invert,
            b"post" => A::Posterize {
                levels: u32::from(read_u16(d, 0)?),
            },
            b"thrs" => A::Threshold {
                level: read_u16(d, 0)? as f32 / 255.0,
            },
            b"expA" => {
                if read_u16(d, 0)? != 1 {
                    return Err(error("unsupported exposure version"));
                }
                let f = |offset| -> EngineResult<f32> {
                    Ok(f32::from_be_bytes(
                        d.get(offset..offset + 4)
                            .ok_or_else(|| error("truncated exposure"))?
                            .try_into()
                            .unwrap(),
                    ))
                };
                A::Exposure {
                    exposure: f(2)?,
                    offset: f(6)?,
                    gamma: f(10)?,
                }
            }
            b"curv" => {
                let version = read_u16(d, 1)?;
                let count = (u32::from(read_u16(d, 3)?) << 16) | u32::from(read_u16(d, 5)?);
                if d[0] != 0
                    || !matches!(version, 1 | 4)
                    || (version == 1 && count & !15 != 0)
                    || (version == 4 && count > 4)
                {
                    continue; // Sampled maps and non-RGB channels stay opaque.
                }
                let mut curves: [crate::Curve; 4] = Default::default();
                let mut offset = 7;
                for (i, curve) in curves.iter_mut().enumerate() {
                    if (version == 1 && count & (1 << i) != 0)
                        || (version == 4 && (i as u32) < count)
                    {
                        *curve = read_curve(d, &mut offset)?;
                    }
                }
                if d.get(offset..offset + 4) == Some(b"Crv ") {
                    if read_u16(d, offset + 4)? != 4 {
                        continue;
                    }
                    let extra_count = (u32::from(read_u16(d, offset + 6)?) << 16)
                        | u32::from(read_u16(d, offset + 8)?);
                    if extra_count > 4 {
                        continue;
                    }
                    offset += 10;
                    let mut unsupported = false;
                    for _ in 0..extra_count {
                        let channel = read_u16(d, offset)? as usize;
                        offset += 2;
                        let curve = read_curve(d, &mut offset)?;
                        if let Some(target) = curves.get_mut(channel) {
                            *target = curve;
                        } else {
                            unsupported = true;
                        }
                    }
                    if unsupported {
                        continue;
                    }
                }
                let [master, r, g, b] = curves;
                A::Curves {
                    master,
                    rgb: [r, g, b],
                }
            }
            b"mixr" => {
                if read_u16(d, 0)? != 1 {
                    return Err(error("unsupported channel mixer version"));
                }
                let monochrome = read_u16(d, 2)? != 0;
                let count = if monochrome { 1 } else { 3 };
                if d.len() < 4 + count * 10 {
                    return Err(error("truncated channel mixer"));
                }
                let mut matrix = [[0.0; 3]; 3];
                let mut constant = [0.0; 3];
                for i in 0..count {
                    for (j, coefficient) in matrix[i].iter_mut().enumerate() {
                        *coefficient = read_u16(d, 4 + i * 10 + j * 2)? as i16 as f32 / 100.0;
                    }
                    constant[i] = read_u16(d, 12 + i * 10)? as i16 as f32 / 100.0;
                }
                A::ChannelMixer {
                    matrix,
                    constant,
                    monochrome,
                }
            }
            b"hue2" => {
                if read_u16(d, 0)? != 2 || d.len() < 100 {
                    return Err(error("invalid hue/saturation version or length"));
                }
                // The compositor implements master/colorize only, not the six
                // selective bands. Keep those layers opaque rather than render
                // a misleading partial adjustment.
                if (0..6).any(|i| d[24 + i * 14..30 + i * 14] != [0; 6]) {
                    continue;
                }
                let colorize = d[2] != 0;
                let offset = if colorize { 4 } else { 10 };
                A::HueSaturation {
                    hue: read_u16(d, offset)? as i16 as f32,
                    saturation: read_u16(d, offset + 2)? as i16 as f32,
                    lightness: read_u16(d, offset + 4)? as i16 as f32,
                    colorize,
                }
            }
            b"levl" => {
                if read_u16(d, 0)? != 2 {
                    return Err(error("unsupported levels version"));
                }
                let mut channels = [LevelsChannel::default(); 4];
                for (i, c) in channels.iter_mut().enumerate() {
                    let o = 2 + i * 10;
                    *c = LevelsChannel {
                        in_black: read_u16(d, o)? as f32 / 255.0,
                        in_white: read_u16(d, o + 2)? as f32 / 255.0,
                        out_black: read_u16(d, o + 4)? as f32 / 255.0,
                        out_white: read_u16(d, o + 6)? as f32 / 255.0,
                        gamma: read_u16(d, o + 8)? as f32 / 100.0,
                    };
                }
                A::Levels {
                    master: channels[0],
                    rgb: [channels[1], channels[2], channels[3]],
                }
            }
            _ => continue,
        };
        return Ok(Some(a));
    }
    Ok(None)
}
fn export_adjustment(a: &crate::Adjustment, layer: &mut ::psd::Layer) -> EngineResult<()> {
    if import_adjustment(layer)?.as_ref() == Some(a) {
        return Ok(());
    }
    use crate::Adjustment as A;
    let (key, data) = match a {
        A::Invert => (*b"nvrt", Vec::new()),
        A::Posterize { levels } => (*b"post", (*levels as u16).to_be_bytes().to_vec()),
        A::Threshold { level } => (
            *b"thrs",
            (crate::raster::quantize_u8(*level) as u16)
                .to_be_bytes()
                .to_vec(),
        ),
        A::Exposure {
            exposure,
            offset,
            gamma,
        } => (
            *b"expA",
            [
                1u16.to_be_bytes().as_slice(),
                &exposure.to_be_bytes(),
                &offset.to_be_bytes(),
                &gamma.to_be_bytes(),
            ]
            .concat(),
        ),
        A::ChannelMixer {
            matrix,
            constant,
            monochrome,
        } => {
            // RGB and monochrome both store five signed percentages per row:
            // R, G, B, reserved, constant. Monochrome uses only the first row.
            // Retain inactive rows and trailing extension data on same-mode edits.
            let mut data = layer
                .info(b"mixr")
                .filter(|b| b.data.len() >= 14 && (b.data[2..4] != [0, 0]) == *monochrome)
                .map(|b| b.data.clone())
                .unwrap_or_else(|| vec![0; 44]);
            data[0..2].copy_from_slice(&1u16.to_be_bytes());
            data[2..4].copy_from_slice(&u16::from(*monochrome).to_be_bytes());
            let count = if *monochrome { 1 } else { 3 };
            for i in 0..count {
                for (slot, value) in [
                    (0, matrix[i][0]),
                    (1, matrix[i][1]),
                    (2, matrix[i][2]),
                    (4, constant[i]),
                ] {
                    let percent = (value * 100.0).round();
                    if !percent.is_finite() || !(-32768.0..=32767.0).contains(&percent) {
                        return Err(error("invalid channel mixer percentage"));
                    }
                    let offset = 4 + i * 10 + slot * 2;
                    data[offset..offset + 2].copy_from_slice(&(percent as i16).to_be_bytes());
                }
            }
            (*b"mixr", data)
        }
        A::Curves { master, rgb } => {
            let mut data = vec![0, 0, 4, 0, 0, 0, 4];
            for curve in std::iter::once(master).chain(rgb) {
                let identity = [[0.0, 0.0], [1.0, 1.0]];
                let points = if curve.0.is_empty() {
                    &identity[..]
                } else {
                    &curve.0
                };
                if !(2..=19).contains(&points.len()) {
                    return Err(error("PSD curves require 2..19 control points"));
                }
                data.extend_from_slice(&(points.len() as u16).to_be_bytes());
                for [input, output] in points {
                    for value in [output, input] {
                        if !value.is_finite() || !(0.0..=1.0).contains(value) {
                            return Err(error("invalid curve coordinate"));
                        }
                        data.extend_from_slice(
                            &(crate::raster::quantize_u8(*value) as u16).to_be_bytes(),
                        );
                    }
                }
            }
            (*b"curv", data)
        }
        A::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
        } => {
            // Preserve inactive parameters, band bounds, and opaque extensions
            // when editing an imported master adjustment.
            let mut data = layer
                .info(b"hue2")
                .filter(|b| b.data.len() >= 100)
                .map(|b| b.data.clone())
                .unwrap_or_else(|| {
                    let mut d = vec![0; 100];
                    d[1] = 2;
                    for (i, band) in [
                        [315i16, 345, 15, 45],
                        [15, 45, 75, 105],
                        [75, 105, 135, 165],
                        [135, 165, 195, 225],
                        [195, 225, 255, 285],
                        [255, 285, 315, 345],
                    ]
                    .iter()
                    .enumerate()
                    {
                        for (j, v) in band.iter().enumerate() {
                            d[16 + i * 14 + j * 2..18 + i * 14 + j * 2]
                                .copy_from_slice(&v.to_be_bytes());
                        }
                    }
                    d
                });
            data[2] = u8::from(*colorize);
            // Replacing an unsupported selective adjustment is explicit:
            // retain band boundaries/extensions but remove its old effect.
            for i in 0..6 {
                data[24 + i * 14..30 + i * 14].fill(0);
            }
            let offset = if *colorize { 4 } else { 10 };
            for (i, value) in [*hue, *saturation, *lightness].iter().enumerate() {
                if !value.is_finite() || !(-32768.0..=32767.0).contains(value) {
                    return Err(error("invalid hue/saturation parameter"));
                }
                data[offset + i * 2..offset + i * 2 + 2]
                    .copy_from_slice(&(value.round() as i16).to_be_bytes());
            }
            (*b"hue2", data)
        }
        A::Levels { master, rgb } => {
            let mut data = 2u16.to_be_bytes().to_vec();
            for c in std::iter::once(*master)
                .chain(*rgb)
                .chain(std::iter::repeat_n(crate::LevelsChannel::default(), 25))
            {
                for v in [
                    c.in_black * 255.0,
                    c.in_white * 255.0,
                    c.out_black * 255.0,
                    c.out_white * 255.0,
                    c.gamma * 100.0,
                ] {
                    data.extend_from_slice(&(v.round() as u16).to_be_bytes());
                }
            }
            (*b"levl", data)
        }
    };
    layer
        .additional
        .retain(|b| ::psd::metadata::adjustment(b.key, &b.data).is_none());
    set_tag(layer, key, data);
    Ok(())
}
fn import_kind(layer: &::psd::Layer, canvas: Extent, depth: Depth) -> EngineResult<LayerKind> {
    if let Some(a) = import_adjustment(layer)? {
        return Ok(LayerKind::Adjustment(a));
    }
    let proxy = raster(layer, canvas, depth)?;
    if let Some(block) = layer.info(b"TySh") {
        let parsed = ::psd::metadata::parse_text(&block.data).ok();
        return Ok(LayerKind::Text(crate::TextLayer {
            text: parsed
                .as_ref()
                .and_then(|p| p.text())
                .unwrap_or("")
                .to_owned(),
            font: parsed
                .as_ref()
                .and_then(|p| {
                    p.style_runs()
                        .first()
                        .and_then(|r| r.font_name().map(str::to_owned))
                })
                .unwrap_or_default(),
            size: parsed
                .as_ref()
                .and_then(|p| {
                    p.style_runs()
                        .first()
                        .and_then(|r| r.size())
                        .and_then(|v| v.number())
                })
                .unwrap_or(0.0) as f32,
            color: [0.0; 3],
            proxy,
        }));
    }
    if [b"SoLd", b"SoLE", b"PlLd", b"plLd"]
        .iter()
        .any(|key| layer.info(key).is_some())
    {
        let mut state = DocState::new(canvas, depth);
        let mut child = Layer::new("PSD rendered smart-object proxy", LayerKind::Pixel(proxy));
        state.assign_ids(&mut child, false);
        state.root.push(Arc::new(child));
        return Ok(LayerKind::SmartObject(crate::SmartObject::new(
            state,
            crate::Affine::default(),
        )));
    }
    Ok(LayerKind::Pixel(proxy))
}
fn mask_coordinates(
    layer: &::psd::Layer,
    mask: ::psd::layers::Mask,
) -> EngineResult<(i16, ::psd::Rect, u8, u8)> {
    let (id, mut bounds, flags, default) = if let Some(real) = mask
        .real
        .filter(|_| layer.channels.iter().any(|c| c.id == -3))
    {
        (-3, real.bounds, real.flags, real.default_color)
    } else {
        (-2, mask.bounds, mask.flags, mask.default_color)
    };
    if flags & 1 != 0 {
        bounds.left = bounds
            .left
            .checked_add(layer.bounds.left)
            .ok_or_else(|| error("mask coordinate overflow"))?;
        bounds.right = bounds
            .right
            .checked_add(layer.bounds.left)
            .ok_or_else(|| error("mask coordinate overflow"))?;
        bounds.top = bounds
            .top
            .checked_add(layer.bounds.top)
            .ok_or_else(|| error("mask coordinate overflow"))?;
        bounds.bottom = bounds
            .bottom
            .checked_add(layer.bounds.top)
            .ok_or_else(|| error("mask coordinate overflow"))?;
    }
    Ok((id, bounds, flags, default))
}
fn import_mask(
    layer: &::psd::Layer,
    canvas: Extent,
    depth: Depth,
) -> EngineResult<Option<crate::Mask>> {
    let Some(mask) = layer.mask().map_err(error)? else {
        return Ok(None);
    };
    let (id, b, flags, default) = mask_coordinates(layer, mask)?;
    let (w, h) = b.dimensions().map_err(error)?;
    let invert = |v: f32| if flags & 4 != 0 { 1.0 - v } else { v };
    let mut raster = Raster::new(canvas, 1, depth, invert(default as f32 / 255.0));
    if let Some(c) = layer.channels.iter().find(|c| c.id == id) {
        if c.data.len() != w * h * depth.bytes() {
            return Err(error("incorrect mask sample count"));
        }
        raster.edit_region(
            Rect::new(b.left as i64, b.top as i64, b.right as i64, b.bottom as i64),
            0,
            |x, y, p| {
                let i = (i64::from(y) - i64::from(b.top)) as usize * w
                    + (i64::from(x) - i64::from(b.left)) as usize;
                p[0] = invert(sample(&c.data, i, depth));
            },
        )?;
    }
    Ok(Some(crate::Mask {
        raster,
        enabled: flags & 2 == 0,
        density: mask.parameters.and_then(|p| p.user_density).unwrap_or(255) as f32 / 255.0,
        feather: mask.parameters.and_then(|p| p.user_feather).unwrap_or(0.0) as f32,
    }))
}
fn export_mask(
    mask: Option<&crate::Mask>,
    layer: &mut ::psd::Layer,
    canvas: Extent,
    depth: Depth,
) -> EngineResult<()> {
    let Some(mask) = mask else {
        layer.mask_data.clear();
        layer.channels.retain(|c| c.id != -2 && c.id != -3);
        return Ok(());
    };
    if layer.mask_data.is_empty() {
        layer.mask_data = [
            0i32.to_be_bytes(),
            0i32.to_be_bytes(),
            (canvas.height as i32).to_be_bytes(),
            (canvas.width as i32).to_be_bytes(),
        ]
        .concat();
        layer.mask_data.extend_from_slice(&[
            crate::raster::quantize_u8(mask.raster.default_value()),
            0,
            0,
            0,
        ]);
    }
    let before = layer.mask().map_err(error)?.unwrap();
    let (id, b, flags, _) = mask_coordinates(layer, before)?;
    let base = if id == -3 {
        // The real-mask record follows the variable mask-parameter record.
        18 + before
            .parameters
            .map(|p| {
                1 + usize::from(p.user_density.is_some())
                    + 8 * usize::from(p.user_feather.is_some())
                    + usize::from(p.vector_density.is_some())
                    + 8 * usize::from(p.vector_feather.is_some())
            })
            .unwrap_or(0)
    } else {
        17
    };
    layer.mask_data[base] = (flags & !2) | if mask.enabled { 0 } else { 2 };
    if mask.density
        != before
            .parameters
            .and_then(|p| p.user_density)
            .unwrap_or(255) as f32
            / 255.0
        || mask.feather
            != before
                .parameters
                .and_then(|p| p.user_feather)
                .unwrap_or(0.0) as f32
    {
        let old_len = before
            .parameters
            .map(|p| {
                1 + usize::from(p.user_density.is_some())
                    + 8 * usize::from(p.user_feather.is_some())
                    + usize::from(p.vector_density.is_some())
                    + 8 * usize::from(p.vector_feather.is_some())
            })
            .unwrap_or(0);
        let old = before.parameters;
        let mut params = vec![
            3 | old.map(|p| p.flags & !3).unwrap_or(0),
            crate::raster::quantize_u8(mask.density),
        ];
        params.extend_from_slice(&(mask.feather as f64).to_be_bytes());
        if let Some(v) = old.and_then(|p| p.vector_density) {
            params.push(v)
        }
        if let Some(v) = old.and_then(|p| p.vector_feather) {
            params.extend_from_slice(&v.to_be_bytes())
        }
        layer.mask_data.splice(18..18 + old_len, params);
        layer.mask_data[17] |= 16;
    }
    if !layer.channels.iter().any(|c| c.id == id) {
        layer.channels.push(Channel {
            id,
            compression: Compression::Raw,
            data: Vec::new(),
        });
    }
    let channel = layer.channels.iter_mut().find(|c| c.id == id).unwrap();
    let old = std::mem::take(&mut channel.data);
    let (w, _) = b.dimensions().map_err(error)?;
    for y in b.top..b.bottom {
        for x in b.left..b.right {
            if x >= 0 && y >= 0 && x < canvas.width as i32 && y < canvas.height as i32 {
                let v = mask.raster.pixel(x as u32, y as u32)[0];
                encode(
                    if flags & 4 != 0 { 1.0 - v } else { v },
                    depth,
                    &mut channel.data,
                );
            } else {
                let i = ((i64::from(y) - i64::from(b.top)) as usize * w
                    + (i64::from(x) - i64::from(b.left)) as usize)
                    * depth.bytes();
                if let Some(bytes) = old.get(i..i + depth.bytes()) {
                    channel.data.extend_from_slice(bytes)
                } else {
                    encode(mask.raster.default_value(), depth, &mut channel.data)
                }
            }
        }
    }
    Ok(())
}
fn section(layer: &::psd::Layer) -> EngineResult<(u32, Option<[u8; 4]>)> {
    let Some(b) = layer.info(b"lsct").or_else(|| layer.info(b"lsdk")) else {
        return Ok((0, None));
    };
    if b.data.len() < 4 {
        return Err(error("truncated section divider"));
    }
    let kind = u32::from_be_bytes(b.data[..4].try_into().unwrap());
    if kind > 3 {
        return Err(error("unknown section divider"));
    }
    let mode = if b.data.len() >= 12 {
        Some(b.data[8..12].try_into().unwrap())
    } else {
        None
    };
    Ok((kind, mode))
}
fn import_nodes(
    records: &[::psd::Layer],
    cursor: &mut usize,
    imported: &mut ImportedPsd,
    parent: Option<LayerId>,
    nesting: usize,
) -> EngineResult<Vec<Arc<Layer>>> {
    if nesting > 128 {
        return Err(error("group nesting exceeds 128"));
    }
    let mut nodes = Vec::new();
    while let Some(original) = records.get(*cursor) {
        *cursor += 1;
        let (kind, mode) = section(original)?;
        if kind == 3 {
            let id = parent.ok_or_else(|| error("unmatched group end"))?;
            imported.endings.insert(id, original.clone());
            nodes.reverse();
            return Ok(nodes);
        }
        let mut layer = Layer::new("", import_kind(original, imported.canvas, imported.depth)?);
        layer.props = props(original)?;
        layer.mask = import_mask(original, imported.canvas, imported.depth)?;
        if crate::BlendMode::from_psd_key(&original.blend_mode).is_none()
            && original.blend_mode != *b"pass"
        {
            imported.warnings.push(format!(
                "{}: unknown blend key {:?}; rendering as Normal",
                layer.props.name, original.blend_mode
            ));
        }
        for block in &original.additional {
            let warning = match &block.key {
                b"vmsk" | b"vsms" => Some("vector mask is retained but not rasterized"),
                b"lfx2" => Some(
                    "basic solid layer styles are rendered approximately; unsupported effect fields remain retained",
                ),
                b"lrFX" => Some("legacy layer styles are retained but not rendered"),
                b"TySh" => {
                    Some("text is rendered from the stored raster proxy; descriptors are retained")
                }
                b"SoLd" | b"SoLE" | b"PlLd" | b"plLd" => Some(
                    "smart object uses the stored raster proxy; embedded/linked originals are retained, not reopened",
                ),
                b"SoCo" | b"GdFl" | b"PtFl" => {
                    Some("fill descriptor is retained; rendering uses the stored raster proxy")
                }
                _ if ::psd::metadata::adjustment(block.key, &block.data).is_some()
                    && !matches!(layer.kind, LayerKind::Adjustment(_)) =>
                {
                    Some(
                        "unsupported adjustment retained; rendering uses its stored raster proxy (or no effect when absent)",
                    )
                }
                _ => None,
            };
            if let Some(w) = warning {
                imported.warnings.push(format!("{}: {w}", layer.props.name));
            }
        }
        if layer.mask.as_ref().is_some_and(|m| m.feather != 0.0) {
            imported.warnings.push(format!(
                "{}: mask feather is retained but not rendered",
                layer.props.name
            ));
        }
        imported.state.assign_ids(&mut layer, false);
        if kind == 1 || kind == 2 {
            let key = mode.unwrap_or(original.blend_mode);
            layer.props.blend_mode = crate::BlendMode::from_psd_key(&key).unwrap_or_default();
            layer.kind = LayerKind::Group {
                mode: if key == *b"pass" {
                    crate::GroupMode::PassThrough
                } else {
                    crate::GroupMode::Isolated
                },
                children: import_nodes(records, cursor, imported, Some(layer.id), nesting + 1)?,
            };
        }
        imported.originals.insert(layer.id, original.clone());
        nodes.push(Arc::new(layer));
    }
    if parent.is_some() {
        return Err(error("unclosed group"));
    }
    nodes.reverse();
    Ok(nodes)
}
fn name(layer: &::psd::Layer) -> EngineResult<String> {
    if let Some(block) = layer.info(b"luni") {
        if block.data.len() < 4 {
            return Err(error("truncated Unicode name"));
        }
        let n = u32::from_be_bytes(block.data[..4].try_into().unwrap()) as usize;
        let data = block
            .data
            .get(4..4 + n.checked_mul(2).ok_or_else(|| error("name overflow"))?)
            .ok_or_else(|| error("truncated Unicode name"))?;
        return String::from_utf16(
            &data
                .as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_be_bytes([b[0], b[1]]))
                .collect::<Vec<_>>(),
        )
        .map_err(error);
    }
    Ok(String::from_utf8_lossy(&layer.name).into_owned())
}
// Native Action Descriptor interop. Keep original bytes on no-op exports and
// retain unrecognized descriptor fields when editing supported effects.
mod style_interop {
    use super::*;
    use crate::render::styles::*;
    use ::psd::metadata::{Descriptor as D, Value as V};

    fn number(d: &D<'_>, key: &[u8], default: f32) -> f32 {
        d.get(key)
            .and_then(V::number)
            .map(|n| n as f32)
            .unwrap_or(default)
    }
    fn boolean(d: &D<'_>, key: &[u8], default: bool) -> bool {
        match d.get(key) {
            Some(V::Bool(v)) => *v,
            _ => default,
        }
    }
    fn enumeration<'a>(d: &D<'a>, key: &[u8]) -> Option<&'a [u8]> {
        match d.get(key) {
            Some(V::Enum { value, .. }) => Some(value),
            _ => None,
        }
    }
    // Action Descriptor blend IDs differ from layer-record blend keys.
    const MODES: &[(crate::BlendMode, &[u8])] = &[
        (crate::BlendMode::Normal, b"Nrml"),
        (crate::BlendMode::Dissolve, b"Dslv"),
        (crate::BlendMode::Darken, b"Drkn"),
        (crate::BlendMode::Multiply, b"Mltp"),
        (crate::BlendMode::ColorBurn, b"CBrn"),
        (crate::BlendMode::LinearBurn, b"linearBurn"),
        (crate::BlendMode::DarkerColor, b"darkerColor"),
        (crate::BlendMode::Lighten, b"Lghn"),
        (crate::BlendMode::Screen, b"Scrn"),
        (crate::BlendMode::ColorDodge, b"CDdg"),
        (crate::BlendMode::LinearDodge, b"linearDodge"),
        (crate::BlendMode::LighterColor, b"lighterColor"),
        (crate::BlendMode::Overlay, b"Ovrl"),
        (crate::BlendMode::SoftLight, b"SftL"),
        (crate::BlendMode::HardLight, b"HrdL"),
        (crate::BlendMode::VividLight, b"vividLight"),
        (crate::BlendMode::LinearLight, b"linearLight"),
        (crate::BlendMode::PinLight, b"pinLight"),
        (crate::BlendMode::HardMix, b"hardMix"),
        (crate::BlendMode::Difference, b"Dfrn"),
        (crate::BlendMode::Exclusion, b"Xclu"),
        (crate::BlendMode::Subtract, b"blendSubtraction"),
        (crate::BlendMode::Divide, b"blendDivide"),
        (crate::BlendMode::Hue, b"H   "),
        (crate::BlendMode::Saturation, b"Strt"),
        (crate::BlendMode::Color, b"Clr "),
        (crate::BlendMode::Luminosity, b"Lmns"),
    ];
    fn rgb(d: &D<'_>) -> Option<[f32; 4]> {
        let c = d.get(b"Clr ")?.object()?;
        if c.class_id != b"RGBC" {
            return None;
        }
        Some([
            number(c, b"Rd  ", 0.0) / 255.0,
            number(c, b"Grn ", 0.0) / 255.0,
            number(c, b"Bl  ", 0.0) / 255.0,
            1.0,
        ])
    }
    fn units_supported(d: &D<'_>) -> bool {
        [
            (b"Scl ", b"#Prc"),
            (b"Opct", b"#Prc"),
            (b"Ckmt", b"#Prc"),
            (b"blur", b"#Pxl"),
            (b"Sz  ", b"#Pxl"),
            (b"Dstn", b"#Pxl"),
            (b"lagl", b"#Ang"),
        ]
        .iter()
        .all(|(key, expected)| match d.get(*key) {
            Some(V::Unit { unit, .. }) => unit == *expected,
            Some(V::Double(_) | V::Integer(_)) | None => true,
            _ => false,
        })
    }
    fn effect(key: &[u8], d: &D<'_>, master: bool) -> Option<StyleEffect> {
        if !units_supported(d) {
            return None;
        }
        let color = rgb(d)?;
        let mode = MODES
            .iter()
            .find(|(_, id)| Some(*id) == enumeration(d, b"Md  "))?
            .0;
        let enabled = master && boolean(d, b"enab", true);
        let opacity = number(d, b"Opct", 100.0) / 100.0;
        let size = number(d, b"blur", 5.0);
        let spread = number(d, b"Ckmt", 0.0) * size / 100.0;
        Some(match key {
            b"DrSh" | b"IrSh" => {
                let s = Shadow {
                    enabled,
                    color,
                    mode,
                    opacity,
                    size,
                    spread,
                    angle: number(d, b"lagl", 120.0),
                    distance: number(d, b"Dstn", 5.0),
                    use_global_light: boolean(d, b"uglg", true),
                    ..Default::default()
                };
                if key == b"DrSh" {
                    StyleEffect::DropShadow(s)
                } else {
                    StyleEffect::InnerShadow(s)
                }
            }
            b"OrGl" | b"IrGl" => {
                // Gradient glows cannot be represented by this solid-color subset.
                if d.get(b"Grad").is_some() {
                    return None;
                }
                let g = Glow {
                    enabled,
                    color,
                    mode,
                    opacity,
                    size,
                    spread,
                    center: enumeration(d, b"glwS") == Some(b"SrcC"),
                    ..Default::default()
                };
                if key == b"OrGl" {
                    StyleEffect::OuterGlow(g)
                } else {
                    StyleEffect::InnerGlow(g)
                }
            }
            b"SoFi" => StyleEffect::ColorOverlay(Overlay {
                enabled,
                mode,
                opacity,
                fill: crate::Fill::Solid {
                    color: [color[0], color[1], color[2]],
                },
            }),
            b"FrFX" => {
                if enumeration(d, b"PntT").is_some_and(|v| v != b"SClr") {
                    return None;
                }
                StyleEffect::Stroke(Stroke {
                    enabled,
                    mode,
                    opacity,
                    fill: crate::Fill::Solid {
                        color: [color[0], color[1], color[2]],
                    },
                    size: number(d, b"Sz  ", 3.0),
                    position: match enumeration(d, b"Styl") {
                        Some(b"InsF") => StrokePosition::Inside,
                        Some(b"CtrF") => StrokePosition::Center,
                        _ => StrokePosition::Outside,
                    },
                })
            }
            _ => return None,
        })
    }
    pub(super) fn import(layer: &::psd::Layer) -> LayerStyles {
        let Some(s) = layer
            .info(b"lfx2")
            .and_then(|b| ::psd::metadata::parse_styles(&b.data).ok())
        else {
            return LayerStyles::default();
        };
        let master = boolean(&s.descriptor, b"masterFXSwitch", true);
        let styles = LayerStyles {
            scale: number(&s.descriptor, b"Scl ", 100.0) / 100.0,
            effects: s
                .descriptor
                .items
                .iter()
                .filter_map(|(k, v)| effect(k, v.object()?, master))
                .collect(),
        };
        // Malformed/unsupported descriptors remain opaque rather than poison rendering.
        if styles.validate().is_ok() {
            styles
        } else {
            LayerStyles::default()
        }
    }
    fn object(class_id: &'static [u8]) -> D<'static> {
        D {
            name: String::new(),
            class_id,
            items: Vec::new(),
        }
    }
    fn put<'a>(d: &mut D<'a>, key: &'a [u8], value: V<'a>) {
        if let Some((_, v)) = d.items.iter_mut().find(|(k, _)| *k == key) {
            *v = value;
        } else {
            d.items.push((key, value));
        }
    }
    fn unit(unit: [u8; 4], value: f32) -> V<'static> {
        V::Unit {
            unit,
            value: value as f64,
        }
    }
    fn enum_value(type_id: &'static [u8], value: &'static [u8]) -> V<'static> {
        V::Enum { type_id, value }
    }
    fn solid(fill: &crate::Fill) -> EngineResult<[f32; 4]> {
        match fill {
            crate::Fill::Solid { color } => Ok([color[0], color[1], color[2], 1.0]),
            _ => Err(error("PSD styles currently require solid-color fills")),
        }
    }
    fn encode_effect(e: &StyleEffect) -> EngineResult<(&'static [u8], D<'static>)> {
        let (key, enabled, color, mode, opacity) = match e {
            StyleEffect::DropShadow(s) => (b"DrSh" as &[u8], s.enabled, s.color, s.mode, s.opacity),
            StyleEffect::InnerShadow(s) => {
                (b"IrSh" as &[u8], s.enabled, s.color, s.mode, s.opacity)
            }
            StyleEffect::OuterGlow(g) => (b"OrGl" as &[u8], g.enabled, g.color, g.mode, g.opacity),
            StyleEffect::InnerGlow(g) => (b"IrGl" as &[u8], g.enabled, g.color, g.mode, g.opacity),
            StyleEffect::ColorOverlay(o) | StyleEffect::Overlay(o) => (
                b"SoFi" as &[u8],
                o.enabled,
                solid(&o.fill)?,
                o.mode,
                o.opacity,
            ),
            StyleEffect::Stroke(s) => (
                b"FrFX" as &[u8],
                s.enabled,
                solid(&s.fill)?,
                s.mode,
                s.opacity,
            ),
            _ => {
                return Err(error(
                    "PSD export supports shadow/glow/solid overlay/stroke styles only",
                ));
            }
        };
        if color[3] != 1.0 {
            return Err(error(
                "PSD effect color alpha must be one; use effect opacity",
            ));
        }
        let mut d = object(key);
        put(&mut d, b"enab", V::Bool(enabled));
        put(&mut d, b"present", V::Bool(true));
        put(&mut d, b"showInDialog", V::Bool(true));
        put(
            &mut d,
            b"Md  ",
            enum_value(b"BlnM", MODES.iter().find(|(m, _)| *m == mode).unwrap().1),
        );
        put(&mut d, b"Opct", unit(*b"#Prc", opacity * 100.0));
        let mut c = object(b"RGBC");
        for (key, value) in [b"Rd  ", b"Grn ", b"Bl  "].into_iter().zip(color) {
            put(&mut c, key, V::Double(value as f64 * 255.0));
        }
        put(&mut d, b"Clr ", V::Object(c));
        let geometry = match e {
            StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => {
                put(&mut d, b"uglg", V::Bool(s.use_global_light));
                put(&mut d, b"lagl", unit(*b"#Ang", s.angle));
                put(&mut d, b"Dstn", unit(*b"#Pxl", s.distance));
                Some((s.size, s.spread, &s.shape))
            }
            StyleEffect::OuterGlow(g) | StyleEffect::InnerGlow(g) => {
                put(&mut d, b"GlwT", enum_value(b"BETE", b"SfBL"));
                if matches!(e, StyleEffect::InnerGlow(_)) {
                    put(
                        &mut d,
                        b"glwS",
                        enum_value(b"IGSr", if g.center { b"SrcC" } else { b"SrcE" }),
                    );
                }
                Some((g.size, g.spread, &g.shape))
            }
            StyleEffect::Stroke(s) => {
                put(&mut d, b"Sz  ", unit(*b"#Pxl", s.size));
                put(&mut d, b"PntT", enum_value(b"FrFl", b"SClr"));
                put(
                    &mut d,
                    b"Styl",
                    enum_value(
                        b"FStl",
                        match s.position {
                            StrokePosition::Inside => b"InsF",
                            StrokePosition::Center => b"CtrF",
                            StrokePosition::Outside => b"OutF",
                        },
                    ),
                );
                None
            }
            _ => None,
        };
        if let Some((size, spread, shape)) = geometry {
            if !shape.contour.is_empty() || shape.jitter != 0.0 {
                return Err(error("PSD contour/jitter export not supported"));
            }
            if spread > size {
                return Err(error("PSD choke cannot exceed effect size"));
            }
            put(&mut d, b"blur", unit(*b"#Pxl", size));
            put(
                &mut d,
                b"Ckmt",
                unit(
                    *b"#Prc",
                    if size == 0.0 {
                        0.0
                    } else {
                        spread / size * 100.0
                    },
                ),
            );
        }
        Ok((key, d))
    }
    fn id(bytes: &[u8], out: &mut Vec<u8>) {
        out.extend_from_slice(
            &(if bytes.len() == 4 {
                0
            } else {
                bytes.len() as u32
            })
            .to_be_bytes(),
        );
        out.extend_from_slice(bytes);
    }
    fn text(s: &str, out: &mut Vec<u8>) {
        let units: Vec<_> = s.encode_utf16().collect();
        out.extend_from_slice(&(units.len() as u32).to_be_bytes());
        for u in units {
            out.extend_from_slice(&u.to_be_bytes());
        }
    }
    fn descriptor(d: &D<'_>, out: &mut Vec<u8>) {
        text(&d.name, out);
        id(d.class_id, out);
        out.extend_from_slice(&(d.items.len() as u32).to_be_bytes());
        for (k, v) in &d.items {
            id(k, out);
            value(v, out);
        }
    }
    fn value(v: &V<'_>, out: &mut Vec<u8>) {
        let ty = match v {
            V::Object(_) => b"Objc",
            V::List(_) => b"VlLs",
            V::Double(_) => b"doub",
            V::Unit { .. } => b"UntF",
            V::Text(_) => b"TEXT",
            V::Enum { .. } => b"enum",
            V::Integer(_) => b"long",
            V::LargeInteger(_) => b"comp",
            V::Bool(_) => b"bool",
            V::Class { .. } => b"type",
            V::Alias(_) => b"alis",
            V::Raw(_) => b"tdta",
        };
        out.extend_from_slice(ty);
        match v {
            V::Object(d) => descriptor(d, out),
            V::List(vs) => {
                out.extend_from_slice(&(vs.len() as u32).to_be_bytes());
                for v in vs {
                    value(v, out);
                }
            }
            V::Double(v) => out.extend_from_slice(&v.to_be_bytes()),
            V::Unit { unit, value } => {
                out.extend_from_slice(unit);
                out.extend_from_slice(&value.to_be_bytes());
            }
            V::Text(s) => text(s, out),
            V::Enum { type_id, value } => {
                id(type_id, out);
                id(value, out);
            }
            V::Integer(v) => out.extend_from_slice(&v.to_be_bytes()),
            V::LargeInteger(v) => out.extend_from_slice(&v.to_be_bytes()),
            V::Bool(v) => out.push(u8::from(*v)),
            V::Class { name, class_id } => {
                text(name, out);
                id(class_id, out);
            }
            V::Alias(b) | V::Raw(b) => {
                out.extend_from_slice(&(b.len() as u32).to_be_bytes());
                out.extend_from_slice(b);
            }
        }
    }
    pub(super) fn export(styles: &LayerStyles, layer: &mut ::psd::Layer) -> EngineResult<()> {
        if *styles == import(layer) {
            return Ok(());
        }
        styles.validate()?;
        let old = layer.info(b"lfx2");
        let mut d = match old {
            Some(b) => {
                ::psd::metadata::parse_styles(&b.data)
                    .map_err(|_| error("cannot edit opaque lfx2 descriptor"))?
                    .descriptor
            }
            None => object(b"Lefx"),
        };
        let mut encoded = Vec::new();
        for e in &styles.effects {
            let (k, v) = encode_effect(e)?;
            if encoded.iter().any(|(key, _)| *key == k) {
                return Err(error(
                    "repeated effects are not supported by basic lfx2 export",
                ));
            }
            encoded.push((k, v));
        }
        let master = boolean(&d, b"masterFXSwitch", true)
            || encoded
                .iter()
                .any(|(_, effect)| boolean(effect, b"enab", false));
        // Remove only supported effects, retaining unsupported kinds and fields.
        d.items.retain(|(k, v)| {
            v.object().and_then(|v| effect(k, v, true)).is_none()
                || encoded.iter().any(|(key, _)| key == k)
        });
        for (k, v) in encoded {
            let mut merged = d
                .get(k)
                .and_then(V::object)
                .cloned()
                .unwrap_or_else(|| object(k));
            for (key, value) in v.items {
                put(&mut merged, key, value);
            }
            put(&mut d, k, V::Object(merged));
        }
        put(&mut d, b"Scl ", unit(*b"#Prc", styles.scale * 100.0));
        put(&mut d, b"masterFXSwitch", V::Bool(master));
        let mut data = [0u32.to_be_bytes(), 16u32.to_be_bytes()].concat();
        descriptor(&d, &mut data);
        set_tag(layer, *b"lfx2", data);
        Ok(())
    }
}
fn props(layer: &::psd::Layer) -> EngineResult<crate::LayerProps> {
    let mut p = crate::LayerProps {
        name: name(layer)?,
        styles: style_interop::import(layer),
        visible: layer.visible(),
        opacity: layer.opacity as f32 / 255.0,
        fill_opacity: layer
            .info(b"iOpa")
            .and_then(|b| b.data.first())
            .copied()
            .unwrap_or(255) as f32
            / 255.0,
        blend_mode: crate::BlendMode::from_psd_key(&layer.blend_mode).unwrap_or_default(),
        clipped: layer.clipping != 0,
        ..Default::default()
    };
    for (dst, src) in std::iter::once(&mut p.blend_if.gray)
        .chain(p.blend_if.rgb.iter_mut())
        .zip(&layer.blending_ranges)
    {
        dst.this_layer = std::array::from_fn(|i| src[i] as f32 / 255.0);
        dst.underlying = std::array::from_fn(|i| src[i + 4] as f32 / 255.0);
    }
    Ok(p)
}
fn set_tag(layer: &mut ::psd::Layer, key: [u8; 4], data: Vec<u8>) {
    if let Some(b) = layer.additional.iter_mut().find(|b| b.key == key) {
        b.data = data;
    } else {
        layer.additional.push(::psd::AdditionalInfo {
            signature: *b"8BIM",
            key,
            data,
        });
    }
}
fn export_props(p: &crate::LayerProps, layer: &mut ::psd::Layer) -> EngineResult<()> {
    let before = props(layer)?;
    style_interop::export(&p.styles, layer)?;
    if p.name != before.name {
        layer.name = p.name.bytes().take(255).collect();
        let units: Vec<u16> = p.name.encode_utf16().collect();
        let mut bytes = (units.len() as u32).to_be_bytes().to_vec();
        for unit in units {
            bytes.extend_from_slice(&unit.to_be_bytes());
        }
        set_tag(layer, *b"luni", bytes);
    }
    layer.opacity = crate::raster::quantize_u8(p.opacity);
    layer.flags = (layer.flags & !2) | if p.visible { 0 } else { 2 };
    if p.clipped != before.clipped {
        layer.clipping = u8::from(p.clipped);
    }
    if p.blend_mode != before.blend_mode {
        layer.blend_mode = p.blend_mode.psd_key();
    }
    if p.fill_opacity != before.fill_opacity {
        set_tag(
            layer,
            *b"iOpa",
            vec![crate::raster::quantize_u8(p.fill_opacity)],
        );
    }
    if p.blend_if != before.blend_if {
        let ranges: Vec<[u8; 8]> = std::iter::once(p.blend_if.gray)
            .chain(p.blend_if.rgb)
            .map(|r| {
                std::array::from_fn(|i| {
                    crate::raster::quantize_u8(if i < 4 {
                        r.this_layer[i]
                    } else {
                        r.underlying[i - 4]
                    })
                })
            })
            .collect();
        if layer.blending_ranges.len() < 4 {
            layer
                .blending_ranges
                .resize(4, [0, 0, 255, 255, 0, 0, 255, 255]);
        }
        layer.blending_ranges[..4].copy_from_slice(&ranges);
    }
    Ok(())
}
fn encode(value: f32, depth: Depth, out: &mut Vec<u8>) {
    match depth {
        Depth::U8 => out.push(crate::raster::quantize_u8(value)),
        Depth::U16 => out.extend_from_slice(&crate::raster::quantize_u16(value).to_be_bytes()),
        Depth::F32 => out.extend_from_slice(&value.to_be_bytes()),
    }
}
/// Export the current edited layer state, retaining original opaque PSD data.
pub fn to_psd(imported: &impl PsdExport) -> EngineResult<PsdDocument> {
    imported.export_psd()
}

/// Sources supporting PSD export with retained opaque metadata.
pub trait PsdExport {
    /// Export editable state and retained original records.
    fn export_psd(&self) -> EngineResult<PsdDocument>;
}
impl PsdExport for ImportedPsd {
    fn export_psd(&self) -> EngineResult<PsdDocument> {
        export_imported(self)
    }
}
impl PsdExport for crate::Document {
    fn export_psd(&self) -> EngineResult<PsdDocument> {
        let mut imported = match &self.psd_source {
            Some(source) => source.as_ref().clone(),
            None => ImportedPsd::from_state((**self.state()).clone())?,
        };
        imported.state = (**self.state()).clone();
        export_imported(&imported)
    }
}
impl crate::Document {
    /// Import RGB PSD with original records retained across edits and history.
    /// Native tessera-doc serialization currently does not retain these records.
    pub fn from_psd(source: PsdDocument) -> EngineResult<Self> {
        let imported = from_psd(&source)?;
        let mut document = Self::new(imported.state.clone());
        document.psd_source = Some(Arc::new(imported));
        Ok(document)
    }
}
fn export_imported(imported: &ImportedPsd) -> EngineResult<PsdDocument> {
    let mut source = imported.source.clone();
    if imported.canvas.width != source.width
        || imported.canvas.height != source.height
        || imported.depth != depth(source.depth)?
    {
        return Err(error(
            "canvas/depth conversion requires explicit resampling before PSD export",
        ));
    }
    imported.global_light.validate()?;
    for (id, value, default) in [
        (1037, imported.global_light.angle, 120.0),
        (1049, imported.global_light.elevation, 30.0),
    ] {
        let old = source.resources.iter_mut().find(|r| r.id == id);
        if let Some(r) = old {
            let before = r
                .data
                .get(..4)
                .map(|b| i32::from_be_bytes(b.try_into().unwrap()) as f32);
            if before != Some(value) {
                r.data = (value.round() as i32).to_be_bytes().to_vec();
            }
        } else if value != default {
            source.resources.push(::psd::ImageResource::new(
                id,
                (value.round() as i32).to_be_bytes().to_vec(),
            ));
        }
    }
    source.layer_section.layers = export_nodes(&imported.root, imported)?;
    let document = crate::Document::new(imported.state.clone());
    let (_, rgba) = crate::Compositor::new(64 << 20).render_level_rgba(&document, 0)?;
    let plane = source.width as usize * source.height as usize * imported.depth.bytes();
    source.composite.resize(plane * source.channels as usize, 0);
    for c in 0..usize::from(source.channels.min(4)) {
        let mut bytes = Vec::with_capacity(plane);
        for p in rgba.as_chunks::<4>().0 {
            encode(p[c], imported.depth, &mut bytes);
        }
        source.composite[c * plane..(c + 1) * plane].copy_from_slice(&bytes);
    }
    if let Some(profile) = &imported.profile {
        if let Some(bytes) = &profile.icc {
            if let Some(r) = source.resources.iter_mut().find(|r| r.id == 1039) {
                r.data = bytes.as_ref().clone();
            } else {
                source
                    .resources
                    .push(::psd::ImageResource::new(1039, bytes.as_ref().clone()));
            }
        }
    } else {
        source.resources.retain(|r| r.id != 1039);
    }
    let old_ppi = source
        .resolution()
        .map_err(error)?
        .map(|r| r.horizontal as f32 / 65536.0)
        .unwrap_or(72.0);
    if imported.ppi != old_ppi {
        let fixed = (imported.ppi * 65536.0).round() as u32;
        let r = ::psd::Resolution {
            horizontal: fixed,
            vertical: fixed,
            horizontal_unit: 1,
            vertical_unit: 1,
            width_unit: 1,
            height_unit: 1,
        }
        .to_resource();
        if let Some(old) = source.resources.iter_mut().find(|r| r.id == 1005) {
            *old = r;
        } else {
            source.resources.push(r);
        }
    }
    Ok(source)
}
fn export_raster(
    r: &Raster,
    layer: &mut ::psd::Layer,
    canvas: Extent,
    depth: Depth,
) -> EngineResult<()> {
    let old_bounds = layer.bounds;
    let (old_w, _) = old_bounds.dimensions().map_err(error)?;
    let mut bounds = old_bounds;
    for y in 0..canvas.height {
        for x in 0..canvas.width {
            let p = r.pixel(x, y);
            if p.iter().any(|v| *v != 0.0) {
                bounds.left = bounds.left.min(x as i32);
                bounds.right = bounds.right.max(x as i32 + 1);
                bounds.top = bounds.top.min(y as i32);
                bounds.bottom = bounds.bottom.max(y as i32 + 1);
            }
        }
    }
    for (id, component) in [(0, 0), (1, 1), (2, 2), (-1, 3)] {
        if layer.channels.iter().any(|c| c.id == id) {
            continue;
        }
        let default = if id == -1 { 1.0 } else { 0.0 };
        let needs = (bounds.top.max(0)..bounds.bottom.min(canvas.height as i32)).any(|y| {
            (bounds.left.max(0)..bounds.right.min(canvas.width as i32))
                .any(|x| r.pixel(x as u32, y as u32)[component] != default)
        });
        if needs {
            layer.channels.push(Channel {
                id,
                compression: Compression::Raw,
                data: Vec::new(),
            });
        }
    }
    for channel in &mut layer.channels {
        if channel.id < -1 {
            continue;
        }
        let component = match channel.id {
            0..=2 => Some(channel.id as usize),
            -1 => Some(3),
            _ => None,
        };
        let old = std::mem::take(&mut channel.data);
        for y in bounds.top..bounds.bottom {
            for x in bounds.left..bounds.right {
                if let Some(c) = component.filter(|_| {
                    x >= 0 && y >= 0 && x < canvas.width as i32 && y < canvas.height as i32
                }) {
                    encode(r.pixel(x as u32, y as u32)[c], depth, &mut channel.data);
                } else if x >= old_bounds.left
                    && x < old_bounds.right
                    && y >= old_bounds.top
                    && y < old_bounds.bottom
                {
                    let i = ((i64::from(y) - i64::from(old_bounds.top)) as usize * old_w
                        + (i64::from(x) - i64::from(old_bounds.left)) as usize)
                        * depth.bytes();
                    if let Some(bytes) = old.get(i..i + depth.bytes()) {
                        channel.data.extend_from_slice(bytes)
                    } else {
                        encode(
                            if channel.id == -1 { 1.0 } else { 0.0 },
                            depth,
                            &mut channel.data,
                        )
                    }
                } else {
                    encode(0.0, depth, &mut channel.data);
                }
            }
        }
    }
    layer.bounds = bounds;
    Ok(())
}
fn export_nodes(nodes: &[Arc<Layer>], imported: &ImportedPsd) -> EngineResult<Vec<::psd::Layer>> {
    let mut records = Vec::new();
    for layer in nodes.iter().rev() {
        let mut original = imported
            .originals
            .get(&layer.id)
            .cloned()
            .unwrap_or_default();
        export_mask(
            layer.mask.as_ref(),
            &mut original,
            imported.canvas,
            imported.depth,
        )?;
        if let LayerKind::Group { mode, children } = &layer.kind {
            let key = if *mode == crate::GroupMode::PassThrough {
                *b"pass"
            } else {
                layer.props.blend_mode.psd_key()
            };
            let (kind, old_key) = section(&original)?;
            let mut p = layer.props.clone();
            p.blend_mode = props(&original)?.blend_mode;
            export_props(&p, &mut original)?;
            if kind == 0 || old_key.unwrap_or(original.blend_mode) != key {
                let tag_key = if original.info(b"lsdk").is_some() {
                    *b"lsdk"
                } else {
                    *b"lsct"
                };
                let data = [
                    (if kind == 0 { 1u32 } else { kind })
                        .to_be_bytes()
                        .as_slice(),
                    b"8BIM",
                    &key,
                ]
                .concat();
                set_tag(&mut original, tag_key, data);
            }
            records.push(original);
            records.extend(export_nodes(children, imported)?);
            let end = imported.endings.get(&layer.id).cloned().unwrap_or_else(|| {
                let mut e = ::psd::Layer::default();
                set_tag(&mut e, *b"lsct", 3u32.to_be_bytes().to_vec());
                e
            });
            records.push(end);
            continue;
        }
        let r = match &layer.kind {
            LayerKind::Pixel(r) => r,
            LayerKind::Text(t) => {
                let LayerKind::Text(before) =
                    import_kind(&original, imported.canvas, imported.depth)?
                else {
                    return Err(error("new text requires a PSD text descriptor"));
                };
                if t.text != before.text
                    || t.font != before.font
                    || t.size != before.size
                    || t.color != before.color
                {
                    return Err(error(
                        "text descriptor edits are not supported; rasterize explicitly",
                    ));
                }
                &t.proxy
            }
            LayerKind::SmartObject(so) => {
                if so.transform != crate::Affine::default() || so.state.root.len() != 1 {
                    return Err(error("edited smart object needs a rasterized proxy"));
                }
                so.state.root[0]
                    .raster()
                    .ok_or_else(|| error("smart object proxy is not a raster"))?
            }
            LayerKind::Adjustment(adjustment) => {
                export_adjustment(adjustment, &mut original)?;
                export_props(&layer.props, &mut original)?;
                records.push(original);
                continue;
            }
            _ => return Err(error("unsupported export layer kind")),
        };
        if !imported.originals.contains_key(&layer.id) {
            original.channels.extend(
                [0, 1, 2, -1]
                    .into_iter()
                    .map(|id| Channel {
                        id,
                        compression: Compression::Raw,
                        data: Vec::new(),
                    })
                    .collect::<Vec<_>>(),
            );
        }
        export_raster(r, &mut original, imported.canvas, imported.depth)?;
        export_props(&layer.props, &mut original)?;
        records.push(original);
    }
    Ok(records)
}
