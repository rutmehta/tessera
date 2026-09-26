//! File-order layer records. Group boundaries remain explicit `lsct` records.
use crate::binary::{self, error, Reader};
use crate::{compression, AdditionalInfo, Compression, Result, Version};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub top: i32,
    pub left: i32,
    pub bottom: i32,
    pub right: i32,
}
impl Rect {
    pub fn dimensions(self) -> Result<(usize, usize)> {
        let w = i64::from(self.right) - i64::from(self.left);
        let h = i64::from(self.bottom) - i64::from(self.top);
        if !(0..=300_000).contains(&w) || !(0..=300_000).contains(&h) {
            return Err(error("invalid layer rectangle"));
        }
        Ok((w as usize, h as usize))
    }
    fn read(r: &mut Reader<'_>) -> Result<Self> {
        let rect = Self {
            top: r.i32()?,
            left: r.i32()?,
            bottom: r.i32()?,
            right: r.i32()?,
        };
        rect.dimensions()?;
        Ok(rect)
    }
    fn write(self, out: &mut Vec<u8>) {
        for v in [self.top, self.left, self.bottom, self.right] {
            out.extend_from_slice(&v.to_be_bytes());
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Channel {
    /// Color plane >=0; transparency -1; user mask -2; real user mask -3.
    pub id: i16,
    pub compression: Compression,
    /// Decoded samples in big-endian, planar file representation.
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub bounds: Rect,
    pub channels: Vec<Channel>,
    /// Four-byte Photoshop key; unknown modes are preserved, not substituted.
    pub blend_mode: [u8; 4],
    pub opacity: u8,
    pub clipping: u8,
    pub flags: u8,
    pub filler: u8,
    pub name: Vec<u8>,
    /// Exact mask payload, including density/feather and real-mask extension.
    pub mask_data: Vec<u8>,
    /// Each pair is source and destination split black/white sliders.
    pub blending_ranges: Vec<[u8; 8]>,
    pub additional: Vec<AdditionalInfo>,
}
impl Default for Layer {
    fn default() -> Self {
        Self {
            bounds: Rect::default(),
            channels: Vec::new(),
            blend_mode: *b"norm",
            opacity: 255,
            clipping: 0,
            flags: 0,
            filler: 0,
            name: Vec::new(),
            mask_data: Vec::new(),
            blending_ranges: Vec::new(),
            additional: Vec::new(),
        }
    }
}
impl Layer {
    pub fn visible(&self) -> bool {
        self.flags & 2 == 0
    }
    fn channel_bounds(&self, id: i16) -> Result<Rect> {
        if id == -2 || id == -3 {
            let mask = self
                .mask()?
                .ok_or_else(|| error("mask channel has no mask rectangle"))?;
            if id == -3 {
                Ok(mask.real.map(|x| x.bounds).unwrap_or(mask.bounds))
            } else {
                Ok(mask.bounds)
            }
        } else {
            Ok(self.bounds)
        }
    }
    pub fn info(&self, key: &[u8; 4]) -> Option<&AdditionalInfo> {
        self.additional.iter().find(|b| &b.key == key)
    }
    pub fn mask(&self) -> Result<Option<Mask>> {
        if self.mask_data.is_empty() {
            return Ok(None);
        }
        let mut r = Reader::new(&self.mask_data);
        let bounds = Rect::read(&mut r)?;
        let default_color = r.u8()?;
        let flags = r.u8()?;
        let mut mask = Mask {
            bounds,
            default_color,
            flags,
            parameters: None,
            real: None,
        };
        if flags & 16 != 0 {
            let bits = r.u8()?;
            let user_density = if bits & 1 != 0 { Some(r.u8()?) } else { None };
            let user_feather = if bits & 2 != 0 {
                Some(f64::from_be_bytes(r.array()?))
            } else {
                None
            };
            let vector_density = if bits & 4 != 0 { Some(r.u8()?) } else { None };
            let vector_feather = if bits & 8 != 0 {
                Some(f64::from_be_bytes(r.array()?))
            } else {
                None
            };
            mask.parameters = Some(MaskParameters {
                flags: bits,
                user_density,
                user_feather,
                vector_density,
                vector_feather,
            });
        }
        if self.mask_data.len() != 20 && r.remaining() >= 18 {
            mask.real = Some(RealMask {
                flags: r.u8()?,
                default_color: r.u8()?,
                bounds: Rect::read(&mut r)?,
            });
        }
        Ok(Some(mask))
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mask {
    pub bounds: Rect,
    pub default_color: u8,
    pub flags: u8,
    pub parameters: Option<MaskParameters>,
    pub real: Option<RealMask>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaskParameters {
    pub flags: u8,
    pub user_density: Option<u8>,
    pub user_feather: Option<f64>,
    pub vector_density: Option<u8>,
    pub vector_feather: Option<f64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RealMask {
    pub flags: u8,
    pub default_color: u8,
    pub bounds: Rect,
}

/// Index into global additional blocks when Photoshop stores layers in Lr16/Lr32/Layr.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LayerLocation {
    #[default]
    Main,
    Additional(usize),
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayerSection {
    pub layers: Vec<Layer>,
    pub merged_alpha: bool,
    pub global_mask: Vec<u8>,
    pub additional: Vec<AdditionalInfo>,
    pub location: LayerLocation,
    /// Original primary layer-info bytes when a high-depth tagged block overrides it.
    pub fallback_layer_info: Vec<u8>,
}
const MAX_DECODED_LAYERS: usize = 512 * 1024 * 1024;
impl LayerSection {
    pub(crate) fn read(mut r: Reader<'_>, version: Version, depth: u16) -> Result<Self> {
        if r.remaining() == 0 {
            return Ok(Self::default());
        }
        let mut section = Self::default();
        let info = r.section(version.wide())?;
        if r.remaining() != 0 {
            section.global_mask = r.blob()?;
        }
        section.additional = read_tags(&mut r, version, 4)?;
        let desired = match depth {
            16 => b"Lr16",
            32 => b"Lr32",
            _ => b"Layr",
        };
        if let Some((i, block)) = section
            .additional
            .iter()
            .enumerate()
            .find(|(_, b)| &b.key == desired)
        {
            let (layers, alpha) = read_layers(Reader::new(&block.data), version, depth)?;
            section.layers = layers;
            section.merged_alpha = alpha;
            section.location = LayerLocation::Additional(i);
            section.fallback_layer_info = info.data.to_vec();
        } else {
            let (layers, alpha) = read_layers(info, version, depth)?;
            section.layers = layers;
            section.merged_alpha = alpha;
        }
        Ok(section)
    }
    pub(crate) fn write(&self, version: Version, depth: u16) -> Result<Vec<u8>> {
        if self == &Self::default() {
            return Ok(Vec::new());
        }
        let info = write_layers(&self.layers, self.merged_alpha, version, depth)?;
        let mut out = Vec::new();
        match self.location {
            LayerLocation::Main => binary::section(&mut out, &info, version.wide())?,
            LayerLocation::Additional(_) => {
                binary::section(&mut out, &self.fallback_layer_info, version.wide())?
            }
        }
        binary::section(&mut out, &self.global_mask, false)?;
        let mut additional = self.additional.clone();
        if let LayerLocation::Additional(i) = self.location {
            let b = additional
                .get_mut(i)
                .ok_or_else(|| error("missing alternate layer block"))?;
            if !matches!(&b.key, b"Layr" | b"Lr16" | b"Lr32") {
                return Err(error("invalid alternate layer block"));
            }
            b.data = info;
        }
        write_tags(&mut out, &additional, version, 4)?;
        Ok(out)
    }
}
fn read_layers(mut r: Reader<'_>, version: Version, depth: u16) -> Result<(Vec<Layer>, bool)> {
    if r.remaining() == 0 {
        return Ok((Vec::new(), false));
    }
    let count = r.i16()?;
    let n = count.unsigned_abs() as usize;
    if n > r.remaining() / 34 {
        return Err(error("layer count exceeds section"));
    }
    let mut layers = Vec::with_capacity(n);
    let mut lengths = Vec::with_capacity(n);
    for _ in 0..n {
        let bounds = Rect::read(&mut r)?;
        let channels = r.u16()? as usize;
        if channels > 56 {
            return Err(error("too many layer channels"));
        }
        let mut channel_lengths = Vec::with_capacity(channels);
        for _ in 0..channels {
            channel_lengths.push((r.i16()?, r.length(version.wide())?));
        }
        r.signature(b"8BIM")?;
        let blend_mode = r.array()?;
        let opacity = r.u8()?;
        let clipping = r.u8()?;
        let flags = r.u8()?;
        let filler = r.u8()?;
        let mut extra = r.section(false)?;
        let mask_data = extra.blob()?;
        let ranges = extra.blob()?;
        if ranges.len() % 8 != 0 {
            return Err(error("invalid Blend If length"));
        }
        let blending_ranges = ranges.as_chunks::<8>().0.to_vec();
        let name = extra.pascal(4)?;
        let additional = read_tags(&mut extra, version, 2)?;
        let layer = Layer {
            bounds,
            channels: Vec::new(),
            blend_mode,
            opacity,
            clipping,
            flags,
            filler,
            name,
            mask_data,
            blending_ranges,
            additional,
        };
        layer.mask()?;
        layers.push(layer);
        lengths.push(channel_lengths);
    }
    let mut budget = MAX_DECODED_LAYERS;
    for (layer, lengths) in layers.iter_mut().zip(lengths) {
        for (id, len) in lengths {
            let mut c = Reader::new(r.take(len)?);
            let compression = Compression::try_from(c.u16()?)?;
            let (w, h) = layer.channel_bounds(id)?.dimensions()?;
            let expected = w
                .checked_mul(depth as usize)
                .and_then(|v| v.checked_add(7))
                .map(|v| v / 8)
                .and_then(|v| v.checked_mul(h))
                .ok_or_else(|| error("channel size overflow"))?;
            budget = budget
                .checked_sub(expected)
                .ok_or_else(|| error("decoded layers exceed 512 MiB"))?;
            let data = compression::decode(
                c.take(c.remaining())?,
                compression,
                w,
                h,
                depth,
                version.wide(),
            )?;
            layer.channels.push(Channel {
                id,
                compression,
                data,
            });
        }
    }
    if r.remaining() > 1 || r.take(r.remaining())?.iter().any(|&b| b != 0) {
        return Err(error("unexpected trailing layer data"));
    }
    Ok((layers, count < 0))
}
fn write_layers(layers: &[Layer], alpha: bool, version: Version, depth: u16) -> Result<Vec<u8>> {
    let count = i16::try_from(layers.len()).map_err(|_| error("too many layers"))?;
    let mut out = (if alpha { -count } else { count }).to_be_bytes().to_vec();
    let mut pixels = Vec::new();
    for layer in layers {
        layer.bounds.dimensions()?;
        layer.mask()?;
        layer.bounds.write(&mut out);
        if layer.channels.len() > 56 {
            return Err(error("too many layer channels"));
        }
        out.extend_from_slice(&(layer.channels.len() as u16).to_be_bytes());
        for channel in &layer.channels {
            let (w, h) = layer.channel_bounds(channel.id)?.dimensions()?;
            let encoded = compression::encode(
                &channel.data,
                channel.compression,
                w,
                h,
                depth,
                version.wide(),
            )?;
            out.extend_from_slice(&channel.id.to_be_bytes());
            binary::length(&mut out, encoded.len() + 2, version.wide())?;
            pixels.extend_from_slice(&(channel.compression as u16).to_be_bytes());
            pixels.extend(encoded);
        }
        out.extend_from_slice(b"8BIM");
        out.extend_from_slice(&layer.blend_mode);
        out.extend_from_slice(&[layer.opacity, layer.clipping, layer.flags, layer.filler]);
        let mut extra = Vec::new();
        binary::section(&mut extra, &layer.mask_data, false)?;
        let ranges: Vec<u8> = layer.blending_ranges.iter().flatten().copied().collect();
        binary::section(&mut extra, &ranges, false)?;
        binary::pascal(&mut extra, &layer.name, 4)?;
        write_tags(&mut extra, &layer.additional, version, 2)?;
        binary::section(&mut out, &extra, false)?;
    }
    out.extend(pixels);
    binary::pad(&mut out, 2);
    Ok(out)
}
fn wide_key(version: Version, signature: &[u8; 4], key: &[u8; 4]) -> bool {
    signature == b"8B64"
        || version.wide()
            && matches!(
                key,
                b"LMsk"
                    | b"Lr16"
                    | b"Lr32"
                    | b"Layr"
                    | b"Mt16"
                    | b"Mt32"
                    | b"Mtrn"
                    | b"Alph"
                    | b"FMsk"
                    | b"lnk2"
                    | b"FEid"
                    | b"FXid"
                    | b"PxSD"
            )
}
fn read_tags(r: &mut Reader<'_>, version: Version, align: usize) -> Result<Vec<AdditionalInfo>> {
    let mut blocks = Vec::new();
    while r.remaining() != 0 {
        if r.remaining() < align && r.data[r.pos..].iter().all(|&b| b == 0) {
            r.take(r.remaining())?;
            break;
        }
        let signature = r.array()?;
        if !matches!(&signature, b"8BIM" | b"8B64") {
            return Err(error("invalid additional info signature"));
        }
        let key = r.array()?;
        let len = r.length(wide_key(version, &signature, &key))?;
        let data = r.take(len)?.to_vec();
        r.take((align - len % align) % align)?;
        blocks.push(AdditionalInfo {
            signature,
            key,
            data,
        });
    }
    Ok(blocks)
}
fn write_tags(
    out: &mut Vec<u8>,
    blocks: &[AdditionalInfo],
    version: Version,
    align: usize,
) -> Result<()> {
    for b in blocks {
        if !matches!(&b.signature, b"8BIM" | b"8B64") {
            return Err(error("invalid additional info signature"));
        }
        out.extend_from_slice(&b.signature);
        out.extend_from_slice(&b.key);
        binary::section(out, &b.data, wide_key(version, &b.signature, &b.key))?;
        out.resize(out.len() + (align - b.data.len() % align) % align, 0);
    }
    Ok(())
}
