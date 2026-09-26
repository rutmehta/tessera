// Read-only PSD metadata decoding. The caller retains the original block bytes.
use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq)]
pub struct Descriptor<'a> {
    pub name: String,
    pub class_id: &'a [u8],
    pub items: Vec<(&'a [u8], Value<'a>)>,
}
#[derive(Clone, Debug, PartialEq)]
pub enum Value<'a> {
    Object(Descriptor<'a>),
    List(Vec<Value<'a>>),
    Double(f64),
    Unit { unit: [u8; 4], value: f64 },
    Text(String),
    Enum { type_id: &'a [u8], value: &'a [u8] },
    Integer(i32),
    LargeInteger(i64),
    Bool(bool),
    Class { name: String, class_id: &'a [u8] },
    Alias(&'a [u8]),
    Raw(&'a [u8]),
}
/// Parse one unversioned Action Descriptor, returning the bytes consumed.
pub fn parse_descriptor(data: &[u8]) -> Result<(Descriptor<'_>, usize)> {
    let mut c = Cursor::new(data);
    let d = c.descriptor(0)?;
    Ok((d, c.pos))
}

// Limits apply even to otherwise well-formed input to bound stack and allocation.
const MAX_DEPTH: usize = 32;
const MAX_ITEMS: usize = 100_000;
const MAX_STRING_UNITS: usize = 1_000_000;
fn error(message: &str) -> Error {
    Error(format!("PSD metadata: {message}"))
}
struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
    budget: usize,
}
impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self {
            data,
            pos: 0,
            budget: MAX_ITEMS,
        }
    }
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or_else(|| error("length overflow"))?;
        let b = self
            .data
            .get(self.pos..end)
            .ok_or_else(|| error("truncated data"))?;
        self.pos = end;
        Ok(b)
    }
    fn array<const N: usize>(&mut self) -> Result<[u8; N]> {
        Ok(self.take(N)?.try_into().unwrap())
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_be_bytes(self.array()?))
    }
    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.array()?))
    }
    fn u64(&mut self) -> Result<u64> {
        Ok(u64::from_be_bytes(self.array()?))
    }
    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_be_bytes(self.array()?))
    }
    fn length(&mut self) -> Result<usize> {
        usize::try_from(self.u32()?).map_err(|_| error("length overflow"))
    }
    fn blob(&mut self) -> Result<&'a [u8]> {
        let n = self.length()?;
        self.take(n)
    }
    fn id(&mut self) -> Result<&'a [u8]> {
        let n = self.length()?;
        self.take(if n == 0 { 4 } else { n })
    }
    fn unicode(&mut self) -> Result<String> {
        let n = self.length()?;
        if n > MAX_STRING_UNITS {
            return Err(error("string limit exceeded"));
        }
        let b = self.take(
            n.checked_mul(2)
                .ok_or_else(|| error("string length overflow"))?,
        )?;
        let units: Vec<u16> = b
            .as_chunks::<2>()
            .0
            .iter()
            .map(|x| u16::from_be_bytes([x[0], x[1]]))
            .collect();
        String::from_utf16(&units).map_err(|_| error("invalid UTF-16"))
    }
    fn boolean(&mut self) -> Result<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(error("invalid boolean")),
        }
    }
    fn expect_u32(&mut self, v: u32) -> Result<()> {
        if self.u32()? == v {
            Ok(())
        } else {
            Err(error("unsupported version"))
        }
    }
    fn finish(&self) -> Result<()> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(error("unexpected trailing data"))
        }
    }
    fn count(&mut self, min_bytes: usize) -> Result<usize> {
        let n = self.length()?;
        if n > self.budget || n > (self.data.len() - self.pos) / min_bytes {
            return Err(error("item count exceeds limits or data"));
        }
        self.budget -= n;
        Ok(n)
    }
    fn descriptor(&mut self, depth: usize) -> Result<Descriptor<'a>> {
        if depth > MAX_DEPTH {
            return Err(error("descriptor nesting limit exceeded"));
        }
        let name = self.unicode()?;
        let class_id = self.id()?;
        let n = self.count(9)?;
        let mut items = Vec::new();
        for _ in 0..n {
            let key = self.id()?;
            let ty = self.array()?;
            items.push((key, self.value(ty, depth + 1)?));
        }
        Ok(Descriptor {
            name,
            class_id,
            items,
        })
    }
    fn versioned_descriptor(&mut self) -> Result<Descriptor<'a>> {
        self.expect_u32(16)?;
        self.descriptor(0)
    }
    fn value(&mut self, ty: [u8; 4], depth: usize) -> Result<Value<'a>> {
        if depth > MAX_DEPTH {
            return Err(error("descriptor nesting limit exceeded"));
        }
        Ok(match &ty {
            b"Objc" | b"GlbO" => Value::Object(self.descriptor(depth)?),
            b"VlLs" => {
                let n = self.count(5)?;
                let mut values = Vec::new();
                for _ in 0..n {
                    let ty = self.array()?;
                    values.push(self.value(ty, depth + 1)?);
                }
                Value::List(values)
            }
            b"doub" => Value::Double(self.f64()?),
            b"UntF" => Value::Unit {
                unit: self.array()?,
                value: self.f64()?,
            },
            b"TEXT" => Value::Text(self.unicode()?),
            b"enum" => Value::Enum {
                type_id: self.id()?,
                value: self.id()?,
            },
            b"long" => Value::Integer(self.u32()? as i32),
            b"comp" => Value::LargeInteger(self.u64()? as i64),
            b"bool" => Value::Bool(self.boolean()?),
            b"type" | b"GlbC" => Value::Class {
                name: self.unicode()?,
                class_id: self.id()?,
            },
            b"alis" => Value::Alias(self.blob()?),
            b"tdta" => Value::Raw(self.blob()?),
            _ => return Err(error(&format!("unsupported descriptor type {:?}", ty))),
        })
    }
}
impl<'a> Descriptor<'a> {
    pub fn get(&self, key: &[u8]) -> Option<&Value<'a>> {
        self.items.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
    }
}
impl<'a> Value<'a> {
    pub fn object(&self) -> Option<&Descriptor<'a>> {
        if let Self::Object(d) = self {
            Some(d)
        } else {
            None
        }
    }
    pub fn text(&self) -> Option<&str> {
        if let Self::Text(s) = self {
            Some(s)
        } else {
            None
        }
    }
    /// Numeric magnitude only; inspect `Unit` before interpreting units.
    pub fn number(&self) -> Option<f64> {
        match self {
            Self::Double(v) | Self::Unit { value: v, .. } => Some(*v),
            Self::Integer(v) => Some(f64::from(*v)),
            _ => None,
        }
    }
}

/// Object-based (`lfx2`) effects. All fields, including unknown effect keys,
/// remain available through `descriptor`; these helpers do not render effects.
#[derive(Clone, Debug, PartialEq)]
pub struct Styles<'a> {
    pub descriptor: Descriptor<'a>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Effect<'d, 'a> {
    pub descriptor: &'d Descriptor<'a>,
}
impl<'a> Styles<'a> {
    /// Common keys: `DrSh` drop shadow, `FrFX` stroke, `OrGl` outer glow.
    pub fn effect(&self, key: &[u8]) -> Option<Effect<'_, 'a>> {
        Some(Effect {
            descriptor: self.descriptor.get(key)?.object()?,
        })
    }
    pub fn drop_shadow(&self) -> Option<Effect<'_, 'a>> {
        self.effect(b"DrSh")
    }
    pub fn stroke(&self) -> Option<Effect<'_, 'a>> {
        self.effect(b"FrFX")
    }
    pub fn outer_glow(&self) -> Option<Effect<'_, 'a>> {
        self.effect(b"OrGl")
    }
}
impl<'d, 'a> Effect<'d, 'a> {
    pub fn enabled(&self) -> Option<bool> {
        match self.descriptor.get(b"enab")? {
            Value::Bool(v) => Some(*v),
            _ => None,
        }
    }
    pub fn opacity(&self) -> Option<&Value<'a>> {
        self.descriptor.get(b"Opct")
    }
    pub fn size(&self) -> Option<&Value<'a>> {
        self.descriptor
            .get(b"Sz  ")
            .or_else(|| self.descriptor.get(b"blur"))
    }
    pub fn distance(&self) -> Option<&Value<'a>> {
        self.descriptor.get(b"Dstn")
    }
    pub fn angle(&self) -> Option<&Value<'a>> {
        self.descriptor.get(b"lagl")
    }
    pub fn color(&self) -> Option<&Descriptor<'a>> {
        self.descriptor.get(b"Clr ")?.object()
    }
    pub fn blend_mode(&self) -> Option<&'a [u8]> {
        match self.descriptor.get(b"Md  ")? {
            Value::Enum { value, .. } => Some(value),
            _ => None,
        }
    }
}
pub fn parse_styles(data: &[u8]) -> Result<Styles<'_>> {
    let mut c = Cursor::new(data);
    c.expect_u32(0)?;
    let descriptor = c.versioned_descriptor()?;
    c.finish()?;
    Ok(Styles { descriptor })
}
#[derive(Clone, Debug, PartialEq)]
pub enum FillKind {
    SolidColor,
    Gradient,
    Pattern,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Fill<'a> {
    pub kind: FillKind,
    pub descriptor: Descriptor<'a>,
}
pub fn parse_fill(key: [u8; 4], data: &[u8]) -> Result<Fill<'_>> {
    let kind = match &key {
        b"SoCo" => FillKind::SolidColor,
        b"GdFl" => FillKind::Gradient,
        b"PtFl" => FillKind::Pattern,
        _ => return Err(error("not a fill block")),
    };
    let mut c = Cursor::new(data);
    let descriptor = c.versioned_descriptor()?;
    c.finish()?;
    Ok(Fill { kind, descriptor })
}
/// Classification only: numeric adjustment layouts are deliberately opaque.
#[derive(Clone, Debug, PartialEq)]
pub enum AdjustmentKind {
    BrightnessContrast,
    Levels,
    Curves,
    Exposure,
    Vibrance,
    HueSaturation,
    ColorBalance,
    BlackWhite,
    PhotoFilter,
    ChannelMixer,
    ColorLookup,
    Invert,
    Posterize,
    Threshold,
    GradientMap,
    SelectiveColor,
}
#[derive(Clone, Debug, PartialEq)]
pub struct Adjustment<'a> {
    pub kind: AdjustmentKind,
    pub data: &'a [u8],
}
pub fn adjustment(key: [u8; 4], data: &[u8]) -> Option<Adjustment<'_>> {
    use AdjustmentKind::*;
    let kind = match &key {
        b"brit" => BrightnessContrast,
        b"levl" => Levels,
        b"curv" => Curves,
        b"expA" => Exposure,
        b"vibA" => Vibrance,
        b"hue2" | b"hue " => HueSaturation,
        b"blnc" => ColorBalance,
        b"blwh" => BlackWhite,
        b"phfl" => PhotoFilter,
        b"mixr" => ChannelMixer,
        b"clrL" => ColorLookup,
        b"nvrt" => Invert,
        b"post" => Posterize,
        b"thrs" => Threshold,
        b"grdm" => GradientMap,
        b"selc" => SelectiveColor,
        _ => return None,
    };
    Some(Adjustment { kind, data })
}

/// `TySh` version 1. EngineData is borrowed verbatim, not tokenized or rewritten.
/// `style_runs` exposes descriptor typography; `engine_styles` separately
/// decodes basic FontSet/StyleRun typography from supported EngineData syntax.
#[derive(Clone, Debug, PartialEq)]
pub struct Text<'a> {
    pub transform: [f64; 6],
    pub descriptor: Descriptor<'a>,
    pub warp: Descriptor<'a>,
    pub bounds: [f64; 4],
}
#[derive(Clone, Debug, PartialEq)]
pub struct TextStyleRun<'d, 'a> {
    pub from: Option<i32>,
    pub to: Option<i32>,
    pub style: &'d Descriptor<'a>,
}
impl<'d, 'a> TextStyleRun<'d, 'a> {
    pub fn font_name(&self) -> Option<&str> {
        self.style
            .get(b"fontPostScriptName")
            .or_else(|| self.style.get(b"fontName"))?
            .text()
    }
    pub fn size(&self) -> Option<&Value<'a>> {
        self.style.get(b"Sz  ").or_else(|| self.style.get(b"size"))
    }
}
impl<'a> Text<'a> {
    pub fn text(&self) -> Option<&str> {
        self.descriptor.get(b"Txt ")?.text()
    }
    pub fn engine_data(&self) -> Option<&'a [u8]> {
        match self.descriptor.get(b"EngineData")? {
            Value::Raw(b) => Some(b),
            _ => None,
        }
    }
    pub fn style_runs(&self) -> Vec<TextStyleRun<'_, 'a>> {
        let Some(Value::List(list)) = self.descriptor.get(b"textStyleRange") else {
            return Vec::new();
        };
        list.iter()
            .filter_map(|v| {
                let range = v.object()?;
                let style = range.get(b"textStyle")?.object()?;
                let integer = |key: &[u8]| match range.get(key) {
                    Some(Value::Integer(n)) => Some(*n),
                    _ => None,
                };
                Some(TextStyleRun {
                    from: integer(b"From"),
                    to: integer(b"T   "),
                    style,
                })
            })
            .collect()
    }
}
pub fn parse_text(data: &[u8]) -> Result<Text<'_>> {
    let mut c = Cursor::new(data);
    if c.u16()? != 1 {
        return Err(error("unsupported TySh version"));
    }
    let mut transform = [0.0; 6];
    for v in &mut transform {
        *v = c.f64()?;
    }
    if c.u16()? != 50 {
        return Err(error("unsupported text version"));
    }
    let descriptor = c.versioned_descriptor()?;
    if c.u16()? != 1 {
        return Err(error("unsupported warp version"));
    }
    let warp = c.versioned_descriptor()?;
    let mut bounds = [0.0; 4];
    for v in &mut bounds {
        *v = c.f64()?;
    }
    c.finish()?;
    Ok(Text {
        transform,
        descriptor,
        warp,
        bounds,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct SmartObject<'a> {
    pub version: u32,
    pub descriptor: Descriptor<'a>,
}
/// `SoLd` / `PlLd` descriptor-based placed object (not lowercase `plLd`).
pub fn parse_smart_object(data: &[u8]) -> Result<SmartObject<'_>> {
    let mut c = Cursor::new(data);
    if c.take(4)? != b"soLD" {
        return Err(error("invalid smart object identifier"));
    }
    let version = c.u32()?;
    if version != 4 {
        return Err(error("unsupported smart object version"));
    }
    let descriptor = c.versioned_descriptor()?;
    c.finish()?;
    Ok(SmartObject {
        version,
        descriptor,
    })
}
#[derive(Clone, Debug, PartialEq)]
pub struct PlacedLayer<'a> {
    pub unique_id: &'a [u8],
    pub page: u32,
    pub total_pages: u32,
    pub anti_alias: u32,
    pub kind: u32,
    pub transform: [f64; 8],
    pub warp: Descriptor<'a>,
}
/// Legacy lowercase `plLd` version 3.
pub fn parse_placed_layer(data: &[u8]) -> Result<PlacedLayer<'_>> {
    let mut c = Cursor::new(data);
    if c.take(4)? != b"plcL" {
        return Err(error("invalid placed layer identifier"));
    }
    c.expect_u32(3)?;
    let len = usize::from(c.u8()?);
    let unique_id = c.take(len)?;
    let page = c.u32()?;
    let total_pages = c.u32()?;
    let anti_alias = c.u32()?;
    let kind = c.u32()?;
    let mut transform = [0.0; 8];
    for v in &mut transform {
        *v = c.f64()?;
    }
    c.expect_u32(0)?;
    let warp = c.versioned_descriptor()?;
    c.finish()?;
    Ok(PlacedLayer {
        unique_id,
        page,
        total_pages,
        anti_alias,
        kind,
        transform,
        warp,
    })
}
#[derive(Clone, Debug, PartialEq)]
pub struct LinkedFile<'a> {
    pub version: u32,
    pub unique_id: &'a [u8],
    pub filename: String,
    pub file_type: [u8; 4],
    pub creator: [u8; 4],
    pub open_parameters: Option<Descriptor<'a>>,
    pub original: &'a [u8],
    pub child_document_id: Option<String>,
    pub asset_mod_time: Option<f64>,
    pub asset_locked: Option<bool>,
}
/// Decode embedded (`liFD`) originals, without copying file data or opening paths.
/// External (`liFE`) and alias (`liFA`) entries are explicitly unsupported.
/// Each length-delimited entry is aligned to four bytes in the enclosing block.
pub fn parse_linked_files(data: &[u8]) -> Result<Vec<LinkedFile<'_>>> {
    let mut c = Cursor::new(data);
    let mut files = Vec::new();
    while c.pos < data.len() {
        if files.len() >= MAX_ITEMS {
            return Err(error("linked file count limit exceeded"));
        }
        let n = usize::try_from(c.u64()?).map_err(|_| error("linked file length overflow"))?;
        let mut e = Cursor::new(c.take(n)?);
        if e.take(4)? != b"liFD" {
            return Err(error("only embedded liFD linked files are supported"));
        }
        let version = e.u32()?;
        if !(1..=7).contains(&version) {
            return Err(error("unsupported linked file version"));
        }
        let len = usize::from(e.u8()?);
        let unique_id = e.take(len)?;
        let filename = e.unicode()?;
        let file_type = e.array()?;
        let creator = e.array()?;
        let len =
            usize::try_from(e.u64()?).map_err(|_| error("embedded original length overflow"))?;
        let open_parameters = if e.boolean()? {
            Some(e.versioned_descriptor()?)
        } else {
            None
        };
        let original = e.take(len)?;
        let child_document_id = if version >= 5 {
            Some(e.unicode()?)
        } else {
            None
        };
        let asset_mod_time = if version >= 6 { Some(e.f64()?) } else { None };
        let asset_locked = if version >= 7 {
            Some(e.boolean()?)
        } else {
            None
        };
        e.finish()?;
        let padding = (4 - n % 4) % 4;
        c.take(padding)?;
        files.push(LinkedFile {
            version,
            unique_id,
            filename,
            file_type,
            creator,
            open_parameters,
            original,
            child_document_id,
            asset_mod_time,
            asset_locked,
        });
    }
    Ok(files)
}

#[derive(Clone, Debug, PartialEq)]
pub struct PathPoint {
    pub y: f64,
    pub x: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub enum PathRecord {
    Subpath {
        closed: bool,
        knots: u16,
        reserved: [u8; 22],
    },
    Knot {
        closed: bool,
        linked: bool,
        points: [PathPoint; 3],
    },
    FillRule {
        reserved: [u8; 24],
    },
    Clipboard {
        bounds: [f64; 4],
        resolution: f64,
        reserved: [u8; 4],
    },
    InitialFill {
        filled: bool,
        reserved: [u8; 22],
    },
}
#[derive(Clone, Debug, PartialEq)]
pub struct VectorMask {
    pub flags: u32,
    pub records: Vec<PathRecord>,
}
impl VectorMask {
    pub fn inverted(&self) -> bool {
        self.flags & 1 != 0
    }
    pub fn not_linked(&self) -> bool {
        self.flags & 2 != 0
    }
    pub fn disabled(&self) -> bool {
        self.flags & 4 != 0
    }
}
/// Decode 26-byte path records (8.24 fixed point, vertical coordinate first).
/// This is a record view, not a validation of subpath topology or a rasterizer.
pub fn parse_vector_mask(data: &[u8]) -> Result<VectorMask> {
    let mut c = Cursor::new(data);
    c.expect_u32(3)?;
    let flags = c.u32()?;
    if !(data.len() - c.pos).is_multiple_of(26) {
        return Err(error("partial vector path record"));
    }
    if (data.len() - c.pos) / 26 > MAX_ITEMS {
        return Err(error("path record limit exceeded"));
    }
    let mut records = Vec::new();
    fn fixed(c: &mut Cursor<'_>) -> Result<f64> {
        Ok(f64::from(c.u32()? as i32) / 16777216.0)
    }
    fn point(c: &mut Cursor<'_>) -> Result<PathPoint> {
        Ok(PathPoint {
            y: fixed(c)?,
            x: fixed(c)?,
        })
    }
    while c.pos < data.len() {
        let selector = c.u16()?;
        records.push(match selector {
            0 | 3 => PathRecord::Subpath {
                closed: selector == 0,
                knots: c.u16()?,
                reserved: c.array()?,
            },
            1 | 2 | 4 | 5 => PathRecord::Knot {
                closed: selector < 3,
                linked: selector == 1 || selector == 4,
                points: [point(&mut c)?, point(&mut c)?, point(&mut c)?],
            },
            6 => PathRecord::FillRule {
                reserved: c.array()?,
            },
            7 => PathRecord::Clipboard {
                bounds: [
                    fixed(&mut c)?,
                    fixed(&mut c)?,
                    fixed(&mut c)?,
                    fixed(&mut c)?,
                ],
                resolution: fixed(&mut c)?,
                reserved: c.array()?,
            },
            8 => {
                let filled = match c.u16()? {
                    0 => false,
                    1 => true,
                    _ => return Err(error("invalid initial fill rule")),
                };
                PathRecord::InitialFill {
                    filled,
                    reserved: c.array()?,
                }
            }
            _ => return Err(error("unknown vector path selector")),
        });
    }
    Ok(VectorMask { flags, records })
}

#[derive(Clone, Debug, PartialEq)]
pub struct LegacyStyleRecord<'a> {
    pub key: [u8; 4],
    pub data: &'a [u8],
    pub effect: LegacyEffect,
}
#[derive(Clone, Debug, PartialEq)]
pub enum LegacyEffect {
    Common { visible: bool },
    Shadow(LegacyShadowGlow),
    Glow(LegacyShadowGlow),
    Unsupported,
}
/// Legacy numeric fields are the stored integers, without unit conversion.
/// Colors retain their color-space identifier and four 16-bit components.
#[derive(Clone, Debug, PartialEq)]
pub struct LegacyShadowGlow {
    pub version: u32,
    pub blur: u32,
    pub intensity: u32,
    pub angle: Option<i32>,
    pub distance: Option<u32>,
    pub color: [u8; 10],
    pub blend_mode: [u8; 4],
    pub enabled: bool,
    pub global_angle: Option<bool>,
    pub opacity: u8,
    pub native_color: Option<[u8; 10]>,
}
/// `lrFX`: common state, drop/inner shadow, and outer glow basics. Other
/// length-delimited effects remain explicit `Unsupported` records with raw data.
pub fn parse_legacy_styles(data: &[u8]) -> Result<Vec<LegacyStyleRecord<'_>>> {
    let mut c = Cursor::new(data);
    if c.u16()? != 0 {
        return Err(error("unsupported lrFX version"));
    }
    let count = usize::from(c.u16()?);
    if count > (data.len() - c.pos) / 12 {
        return Err(error("invalid effect count"));
    }
    let mut records = Vec::new();
    for _ in 0..count {
        if c.take(4)? != b"8BIM" {
            return Err(error("invalid effect signature"));
        }
        let key = c.array()?;
        let data = c.blob()?;
        let mut e = Cursor::new(data);
        let effect = match &key {
            b"cmnS" => {
                e.expect_u32(0)?;
                let visible = e.boolean()?;
                e.take(2)?;
                e.finish()?;
                LegacyEffect::Common { visible }
            }
            b"dsdw" | b"isdw" | b"oglw" => {
                let version = e.u32()?;
                if version != 0 && version != 2 {
                    return Err(error("unsupported legacy effect version"));
                }
                let shadow = key != *b"oglw";
                let blur = e.u32()?;
                let intensity = e.u32()?;
                let angle = if shadow { Some(e.u32()? as i32) } else { None };
                let distance = if shadow { Some(e.u32()?) } else { None };
                let color = e.array()?;
                if e.take(4)? != b"8BIM" {
                    return Err(error("invalid effect blend signature"));
                }
                let blend_mode = e.array()?;
                let enabled = e.boolean()?;
                let global_angle = if shadow { Some(e.boolean()?) } else { None };
                let opacity = e.u8()?;
                let native_color = if version == 2 { Some(e.array()?) } else { None };
                e.finish()?;
                let value = LegacyShadowGlow {
                    version,
                    blur,
                    intensity,
                    angle,
                    distance,
                    color,
                    blend_mode,
                    enabled,
                    global_angle,
                    opacity,
                    native_color,
                };
                if shadow {
                    LegacyEffect::Shadow(value)
                } else {
                    LegacyEffect::Glow(value)
                }
            }
            _ => LegacyEffect::Unsupported,
        };
        records.push(LegacyStyleRecord { key, data, effect });
    }
    c.finish()?;
    Ok(records)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// 0 ordinary layer, 1 open folder, 2 closed folder, 3 bounding divider.
    pub kind: u32,
    pub blend_mode: Option<[u8; 4]>,
    pub subtype: Option<u32>,
}
pub fn parse_group(data: &[u8]) -> Result<Group> {
    let mut c = Cursor::new(data);
    let kind = c.u32()?;
    let blend_mode = if c.pos < data.len() {
        if c.take(4)? != b"8BIM" {
            return Err(error("invalid group signature"));
        }
        Some(c.array()?)
    } else {
        None
    };
    let subtype = if c.pos < data.len() {
        Some(c.u32()?)
    } else {
        None
    };
    c.finish()?;
    Ok(Group {
        kind,
        blend_mode,
        subtype,
    })
}
/// Pattern header plus its unmodified virtual-memory array list. Pattern
/// compression has a distinct format and is deliberately not rasterized here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pattern<'a> {
    pub color_mode: u32,
    pub point: [u16; 2],
    pub name: String,
    pub id: &'a [u8],
    pub palette: Option<&'a [u8]>,
    pub virtual_memory: &'a [u8],
}
pub fn parse_patterns(data: &[u8]) -> Result<Vec<Pattern<'_>>> {
    let mut c = Cursor::new(data);
    let mut patterns = Vec::new();
    while c.pos < data.len() {
        if patterns.len() >= MAX_ITEMS {
            return Err(error("too many patterns"));
        }
        let n = c.length()?;
        let mut p = Cursor::new(c.take(n)?);
        p.expect_u32(1)?;
        let color_mode = p.u32()?;
        let point = [p.u16()?, p.u16()?];
        let name = p.unicode()?;
        let len = p.u8()? as usize;
        let id = p.take(len)?;
        let palette = if color_mode == 2 {
            Some(p.take(768)?)
        } else {
            None
        };
        let start = p.pos;
        p.expect_u32(3)?;
        let len = p.length()?;
        p.take(len)?;
        let virtual_memory = &p.data[start..p.pos];
        p.finish()?;
        c.take((4 - n % 4) % 4)?;
        patterns.push(Pattern {
            color_mode,
            point,
            name,
            id,
            palette,
            virtual_memory,
        });
    }
    Ok(patterns)
}

/// Optional interpretation of an additional-info block. Parsing never mutates
/// the source, and raw preservation/serialization remains the caller's job.
#[derive(Clone, Debug, PartialEq)]
pub enum Metadata<'a> {
    UnicodeName(String),
    LayerId(u32),
    Group(Group),
    Patterns(Vec<Pattern<'a>>),
    Styles(Styles<'a>),
    Fill(Fill<'a>),
    Adjustment(Adjustment<'a>),
    Text(Text<'a>),
    SmartObject(SmartObject<'a>),
    PlacedLayer(PlacedLayer<'a>),
    LinkedFiles(Vec<LinkedFile<'a>>),
    VectorMask(VectorMask),
    LegacyStyles(Vec<LegacyStyleRecord<'a>>),
}
/// Unknown block keys return `Ok(None)`; malformed/unsupported known structures
/// return an error. Unsupported descriptor types are never guessed or skipped.
pub fn parse(block: &crate::AdditionalInfo) -> Result<Option<Metadata<'_>>> {
    if block.signature != *b"8BIM" && block.signature != *b"8B64" {
        return Err(error("invalid additional-info signature"));
    }
    parse_block(block.key, &block.data)
}
/// Raw-byte counterpart of `parse`, without the additional-info envelope.
pub fn parse_block(key: [u8; 4], data: &[u8]) -> Result<Option<Metadata<'_>>> {
    Ok(Some(match &key {
        b"luni" => Metadata::UnicodeName(
            Cursor::new(data)
                .unicode()?
                .trim_end_matches('\0')
                .to_owned(),
        ),
        b"lyid" => Metadata::LayerId(Cursor::new(data).u32()?),
        b"lsct" | b"lsdk" => Metadata::Group(parse_group(data)?),
        b"Patt" | b"Pat2" | b"Pat3" => Metadata::Patterns(parse_patterns(data)?),
        b"lfx2" => Metadata::Styles(parse_styles(data)?),
        b"SoCo" | b"GdFl" | b"PtFl" => Metadata::Fill(parse_fill(key, data)?),
        b"TySh" => Metadata::Text(parse_text(data)?),
        b"SoLd" | b"PlLd" | b"SoLE" => Metadata::SmartObject(parse_smart_object(data)?),
        b"plLd" => Metadata::PlacedLayer(parse_placed_layer(data)?),
        b"lnkD" | b"lnk2" | b"lnk3" => Metadata::LinkedFiles(parse_linked_files(data)?),
        b"vmsk" | b"vsms" => Metadata::VectorMask(parse_vector_mask(data)?),
        b"lrFX" => Metadata::LegacyStyles(parse_legacy_styles(data)?),
        _ => match adjustment(key, data) {
            Some(a) => Metadata::Adjustment(a),
            None => return Ok(None),
        },
    }))
}

/// Basic typography from the PostScript-like EngineData dictionaries. This
/// does not shape text, resolve fonts, apply inheritance, or evaluate operators.
#[derive(Clone, Debug, PartialEq)]
pub struct EngineStyles {
    pub font_names: Vec<String>,
    pub runs: Vec<EngineStyleRun>,
}
#[derive(Clone, Debug, PartialEq)]
pub struct EngineStyleRun {
    pub length: Option<usize>,
    pub font_index: Option<usize>,
    pub size: Option<f64>,
}
impl EngineStyles {
    pub fn font_for_run(&self, run: usize) -> Option<&str> {
        self.font_names
            .get(self.runs.get(run)?.font_index?)
            .map(String::as_str)
    }
}
impl Text<'_> {
    /// Optional separate parsing: an unsupported engine syntax does not prevent
    /// reading TySh's descriptor text or borrowing its original EngineData.
    pub fn engine_styles(&self) -> Result<Option<EngineStyles>> {
        self.engine_data().map(parse_engine_styles).transpose()
    }
}
#[derive(Debug)]
enum EngineValue<'a> {
    Dict(Vec<(&'a [u8], EngineValue<'a>)>),
    Array(Vec<EngineValue<'a>>),
    String(Vec<u8>),
    Number(f64),
    Other,
}
impl<'a> EngineValue<'a> {
    fn get(&self, name: &[u8]) -> Option<&Self> {
        match self {
            Self::Dict(d) => d.iter().find(|(k, _)| *k == name).map(|(_, v)| v),
            _ => None,
        }
    }
    fn array(&self) -> Option<&[Self]> {
        match self {
            Self::Array(a) => Some(a),
            _ => None,
        }
    }
    fn number(&self) -> Option<f64> {
        match self {
            Self::Number(n) => Some(*n),
            _ => None,
        }
    }
    fn index(&self) -> Option<usize> {
        let n = self.number()?;
        if n >= 0.0 && n < usize::MAX as f64 && n.fract() == 0.0 {
            Some(n as usize)
        } else {
            None
        }
    }
}
struct EngineParser<'a> {
    c: Cursor<'a>,
}
impl<'a> EngineParser<'a> {
    fn space(&mut self) {
        loop {
            while self.c.pos < self.c.data.len()
                && (self.c.data[self.c.pos].is_ascii_whitespace() || self.c.data[self.c.pos] == 0)
            {
                self.c.pos += 1;
            }
            if self.c.data.get(self.c.pos) != Some(&b'%') {
                break;
            }
            while self.c.pos < self.c.data.len()
                && !matches!(self.c.data[self.c.pos], b'\r' | b'\n')
            {
                self.c.pos += 1;
            }
        }
    }
    fn starts(&self, token: &[u8]) -> bool {
        self.c.data[self.c.pos..].starts_with(token)
    }
    fn token(&mut self) -> Result<&'a [u8]> {
        let start = self.c.pos;
        while let Some(b) = self.c.data.get(self.c.pos) {
            if b.is_ascii_whitespace() || *b == 0 || b"/[]()<>{}%".contains(b) {
                break;
            }
            self.c.pos += 1;
        }
        if self.c.pos == start {
            return Err(error("missing engine token"));
        }
        Ok(&self.c.data[start..self.c.pos])
    }
    fn string(&mut self) -> Result<Vec<u8>> {
        self.c.take(1)?;
        let mut out = Vec::new();
        let mut depth = 1usize;
        while depth > 0 {
            let b = self.c.u8()?;
            match b {
                b'(' => {
                    depth += 1;
                    if depth > MAX_DEPTH {
                        return Err(error("engine string nesting limit"));
                    }
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth != 0 {
                        out.push(b);
                    }
                }
                b'\\' => {
                    let escaped = self.c.u8()?;
                    match escaped {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\n' => {}
                        b'\r' => {
                            if self.c.data.get(self.c.pos) == Some(&b'\n') {
                                self.c.pos += 1;
                            }
                        }
                        b'0'..=b'7' => {
                            let mut n = u16::from(escaped - b'0');
                            for _ in 0..2 {
                                match self.c.data.get(self.c.pos) {
                                    Some(v @ b'0'..=b'7') => {
                                        n = n * 8 + u16::from(*v - b'0');
                                        self.c.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(n as u8);
                        }
                        _ => out.push(escaped),
                    }
                }
                _ => out.push(b),
            }
            if out.len() > MAX_STRING_UNITS * 2 {
                return Err(error("engine string limit"));
            }
        }
        Ok(out)
    }
    fn value(&mut self, depth: usize) -> Result<EngineValue<'a>> {
        if depth > MAX_DEPTH || self.c.budget == 0 {
            return Err(error("engine nesting/item limit"));
        }
        self.c.budget -= 1;
        self.space();
        if self.starts(b"<<") {
            self.c.take(2)?;
            let mut dict = Vec::new();
            loop {
                self.space();
                if self.starts(b">>") {
                    self.c.take(2)?;
                    break;
                }
                if self.c.u8()? != b'/' {
                    return Err(error("engine dictionary key expected"));
                }
                let key = self.token()?;
                let value = self.value(depth + 1)?;
                dict.push((key, value));
            }
            return Ok(EngineValue::Dict(dict));
        }
        if self.starts(b"[") {
            self.c.take(1)?;
            let mut array = Vec::new();
            loop {
                self.space();
                if self.starts(b"]") {
                    self.c.take(1)?;
                    break;
                }
                array.push(self.value(depth + 1)?);
            }
            return Ok(EngineValue::Array(array));
        }
        if self.starts(b"(") {
            return Ok(EngineValue::String(self.string()?));
        }
        if self.starts(b"/") {
            self.c.take(1)?;
            self.token()?;
            return Ok(EngineValue::Other);
        }
        let token = self.token()?;
        if matches!(token, b"true" | b"false" | b"null") {
            return Ok(EngineValue::Other);
        }
        let number = std::str::from_utf8(token)
            .ok()
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .ok_or_else(|| error("unsupported engine token/operator"))?;
        Ok(EngineValue::Number(number))
    }
}
/// Bounded syntax parser for dictionary/array/literal-string/numeric EngineData.
/// Unsupported syntax returns an error rather than scanning for apparent keys.
pub fn parse_engine_styles(data: &[u8]) -> Result<EngineStyles> {
    if data.len() > 16 * 1024 * 1024 {
        return Err(error("engine data limit exceeded"));
    }
    let mut p = EngineParser {
        c: Cursor::new(data),
    };
    let root = p.value(0)?;
    p.space();
    p.c.finish()?;
    if !matches!(root, EngineValue::Dict(_)) {
        return Err(error("engine root dictionary expected"));
    }
    let mut font_names = Vec::new();
    if let Some(fonts) = root
        .get(b"ResourceDict")
        .or_else(|| root.get(b"DocumentResources"))
        .and_then(|r| r.get(b"FontSet"))
        .and_then(EngineValue::array)
    {
        for font in fonts {
            let Some(EngineValue::String(bytes)) = font.get(b"Name") else {
                return Err(error("missing engine font name"));
            };
            let name = if bytes.starts_with(&[0xfe, 0xff]) {
                if bytes.len() % 2 != 0 {
                    return Err(error("odd UTF-16 engine string"));
                }
                let units: Vec<_> = bytes[2..]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|s| u16::from_be_bytes([s[0], s[1]]))
                    .collect();
                String::from_utf16(&units).map_err(|_| error("invalid engine font UTF-16"))?
            } else {
                std::str::from_utf8(bytes)
                    .map_err(|_| error("unsupported engine font encoding"))?
                    .to_owned()
            };
            font_names.push(name);
        }
    }
    let mut runs = Vec::new();
    if let Some(style_run) = root.get(b"EngineDict").and_then(|e| e.get(b"StyleRun")) {
        let lengths = style_run
            .get(b"RunLengthArray")
            .and_then(EngineValue::array);
        if let Some(array) = style_run.get(b"RunArray").and_then(EngineValue::array) {
            for (index, run) in array.iter().enumerate() {
                let style = run
                    .get(b"StyleSheet")
                    .and_then(|s| s.get(b"StyleSheetData"));
                runs.push(EngineStyleRun {
                    length: lengths
                        .and_then(|a| a.get(index))
                        .and_then(EngineValue::index),
                    font_index: style
                        .and_then(|s| s.get(b"Font"))
                        .and_then(EngineValue::index),
                    size: style
                        .and_then(|s| s.get(b"FontSize"))
                        .and_then(EngineValue::number),
                });
            }
        }
    }
    Ok(EngineStyles { font_names, runs })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn u32b(v: u32) -> Vec<u8> {
        v.to_be_bytes().to_vec()
    }
    fn unicode(s: &str) -> Vec<u8> {
        let units: Vec<_> = s.encode_utf16().collect();
        let mut b = u32b(units.len() as u32);
        for u in units {
            b.extend(u.to_be_bytes());
        }
        b
    }
    fn id(s: &[u8]) -> Vec<u8> {
        let mut b = u32b(if s.len() == 4 { 0 } else { s.len() as u32 });
        b.extend(s);
        b
    }
    fn descriptor(items: Vec<(&[u8], &[u8; 4], Vec<u8>)>) -> Vec<u8> {
        let mut b = unicode("");
        b.extend(id(b"null"));
        b.extend(u32b(items.len() as u32));
        for (key, ty, value) in items {
            b.extend(id(key));
            b.extend(ty);
            b.extend(value);
        }
        b
    }
    #[test]
    fn metadata_engine_fonts_sizes_are_structurally_decoded() {
        let engine = b"<< /EngineDict << /StyleRun << /RunLengthArray [ 5 ] /RunArray [ << /StyleSheet << /StyleSheetData << /Font 0 /FontSize 18.5 /Tracking 0 >> >> >> ] >> >> /ResourceDict << /FontSet [ << /Name (ArialMT) /Script 0 /Synthetic 0 >> ] >> >>";
        let styles = parse_engine_styles(engine).unwrap();
        assert_eq!(styles.font_names, vec!["ArialMT"]);
        assert_eq!(styles.runs.len(), 1);
        assert_eq!(styles.runs[0].font_index, Some(0));
        assert_eq!(styles.runs[0].size, Some(18.5));
        assert_eq!(styles.runs[0].length, Some(5));
        assert_eq!(styles.font_for_run(0), Some("ArialMT"));
        assert!(parse_engine_styles(b"<< /Name (contains /FontSize 999) >>")
            .unwrap()
            .runs
            .is_empty());
        assert!(parse_engine_styles(b"<< /EngineDict <<").is_err());
        assert!(parse_engine_styles(b"<< /Execute dangerousOperator >>").is_err());
        let escaped = b"<< /ResourceDict << /FontSet [ << /Name (A\\(B\\)\\101) >> ] >> >>";
        assert_eq!(
            parse_engine_styles(escaped).unwrap().font_names,
            vec!["A(B)A"]
        );
    }
    #[test]
    fn metadata_additional_info_dispatch_retains_source() {
        let b = crate::AdditionalInfo {
            signature: *b"8BIM",
            key: *b"SoCo",
            data: [u32b(16), descriptor(vec![])].concat(),
        };
        assert!(matches!(parse(&b).unwrap(), Some(Metadata::Fill(_))));
        let unknown = crate::AdditionalInfo {
            signature: *b"8BIM",
            key: *b"????",
            data: vec![1, 2, 3],
        };
        assert_eq!(parse(&unknown).unwrap(), None);
        assert_eq!(unknown.data, vec![1, 2, 3]);
        let unsupported = crate::AdditionalInfo {
            signature: *b"8BIM",
            key: *b"lfx2",
            data: [
                u32b(0),
                u32b(16),
                descriptor(vec![(b"what", b"obj ", vec![0; 4])]),
            ]
            .concat(),
        };
        assert!(parse(&unsupported).is_err());
        assert!(!unsupported.data.is_empty());
    }
    #[test]
    fn metadata_descriptor_nested_primitives_and_limits() {
        let nested = descriptor(vec![
            (b"signed", b"long", (-12i32).to_be_bytes().to_vec()),
            (b"big!", b"comp", (-9000000000i64).to_be_bytes().to_vec()),
            (b"enum", b"enum", [id(b"BlnM"), id(b"Nrml")].concat()),
            (b"kind", b"type", [unicode("Class"), id(b"TxLr")].concat()),
            (b"path", b"alis", [u32b(3), vec![1, 2, 3]].concat()),
        ]);
        let b = descriptor(vec![(
            b"list",
            b"VlLs",
            [
                u32b(2),
                b"Objc".to_vec(),
                nested,
                b"doub".to_vec(),
                3f64.to_be_bytes().to_vec(),
            ]
            .concat(),
        )]);
        let (d, _) = parse_descriptor(&b).unwrap();
        let Value::List(list) = d.get(b"list").unwrap() else {
            panic!("list")
        };
        let obj = list[0].object().unwrap();
        assert_eq!(obj.get(b"signed"), Some(&Value::Integer(-12)));
        assert_eq!(obj.get(b"big!"), Some(&Value::LargeInteger(-9000000000)));
        assert_eq!(list[1].number(), Some(3.0));
        let mut deep = descriptor(vec![]);
        for _ in 0..40 {
            deep = descriptor(vec![(b"nest", b"Objc", deep)]);
        }
        assert!(parse_descriptor(&deep).is_err());
        let bad = descriptor(vec![(
            b"text",
            b"TEXT",
            [u32b(1), vec![0xd8, 0x00]].concat(),
        )]);
        assert!(parse_descriptor(&bad).is_err());
    }
    #[test]
    fn metadata_legacy_effects_keep_unknown_records() {
        let shadow = [
            u32b(0),
            u32b(8),
            u32b(50),
            (-45i32).to_be_bytes().to_vec(),
            u32b(10),
            vec![0; 10],
            b"8BIMmul ".to_vec(),
            vec![1, 0, 128],
        ]
        .concat();
        assert_eq!(shadow.len(), 41);
        let glow = [
            u32b(2),
            u32b(3),
            u32b(40),
            vec![0; 10],
            b"8BIMscrn".to_vec(),
            vec![1, 200],
            vec![0; 10],
        ]
        .concat();
        let mut b = vec![0, 0, 0, 3];
        for (key, payload) in [(b"dsdw", shadow), (b"oglw", glow), (b"new!", vec![1, 2, 3])] {
            b.extend(b"8BIM");
            b.extend(key);
            b.extend(u32b(payload.len() as u32));
            b.extend(payload);
        }
        let effects = parse_legacy_styles(&b).unwrap();
        assert_eq!(effects.len(), 3);
        let LegacyEffect::Shadow(s) = &effects[0].effect else {
            panic!("shadow")
        };
        assert_eq!(s.angle, Some(-45));
        assert_eq!(s.distance, Some(10));
        assert_eq!(s.opacity, 128);
        assert!(s.enabled);
        let LegacyEffect::Glow(g) = &effects[1].effect else {
            panic!("glow")
        };
        assert_eq!(g.native_color, Some([0; 10]));
        assert_eq!(effects[2].effect, LegacyEffect::Unsupported);
        assert_eq!(effects[2].data, &[1, 2, 3]);
        for end in 0..b.len() {
            assert!(parse_legacy_styles(&b[..end]).is_err());
        }
    }
    #[test]
    fn metadata_vector_mask_decodes_records_not_scanned_keys() {
        let mut b = [u32b(3), u32b(5)].concat();
        b.extend(6u16.to_be_bytes());
        b.extend([0; 24]);
        b.extend(0u16.to_be_bytes());
        b.extend(1u16.to_be_bytes());
        b.extend([0; 22]);
        b.extend(2u16.to_be_bytes());
        for v in [0i32, 1 << 24, -(1 << 23), 1 << 23, 1 << 24, 0] {
            b.extend(v.to_be_bytes());
        }
        let mask = parse_vector_mask(&b).unwrap();
        assert!(mask.inverted());
        assert!(mask.disabled());
        assert!(!mask.not_linked());
        assert_eq!(mask.records.len(), 3);
        match &mask.records[2] {
            PathRecord::Knot {
                closed,
                linked,
                points,
            } => {
                assert!(*closed);
                assert!(!linked);
                assert_eq!(points[1], PathPoint { y: -0.5, x: 0.5 });
            }
            _ => panic!("knot expected"),
        }
        assert!(parse_vector_mask(&b[..b.len() - 1]).is_err());
        let mut bad = b.clone();
        bad[8..10].copy_from_slice(&99u16.to_be_bytes());
        assert!(parse_vector_mask(&bad).is_err());
    }
    #[test]
    fn metadata_smart_objects_and_embedded_originals() {
        let b = [
            b"soLD".to_vec(),
            u32b(4),
            u32b(16),
            descriptor(vec![(b"Idnt", b"TEXT", unicode("asset-id"))]),
        ]
        .concat();
        let s = parse_smart_object(&b).unwrap();
        assert_eq!(s.descriptor.get(b"Idnt").unwrap().text(), Some("asset-id"));
        let original = b"original file bytes";
        let mut entry = [
            b"liFD".to_vec(),
            u32b(1),
            vec![2, b'i', b'd'],
            unicode("image.png"),
            b"PNG ".to_vec(),
            b"8BIM".to_vec(),
            (original.len() as u64).to_be_bytes().to_vec(),
            vec![0],
            original.to_vec(),
        ]
        .concat();
        let n = entry.len();
        let mut linked = (n as u64).to_be_bytes().to_vec();
        entry.resize((n + 3) & !3, 0);
        linked.extend(entry);
        let files = parse_linked_files(&linked).unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].unique_id, b"id");
        assert_eq!(files[0].original, original);
        assert_eq!(files[0].filename, "image.png");
        for end in 1..(8 + n) {
            assert!(parse_linked_files(&linked[..end]).is_err());
        }
        let mut placed = [
            b"plcL".to_vec(),
            u32b(3),
            vec![2, b'i', b'd'],
            u32b(1),
            u32b(1),
            u32b(0),
            u32b(2),
        ]
        .concat();
        for x in [0.0f64, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0] {
            placed.extend(x.to_be_bytes());
        }
        placed.extend(u32b(0));
        placed.extend(u32b(16));
        placed.extend(descriptor(vec![]));
        assert_eq!(parse_placed_layer(&placed).unwrap().unique_id, b"id");
    }
    #[test]
    fn metadata_text_preserves_engine_and_style_runs() {
        let style = descriptor(vec![
            (b"fontPostScriptName", b"TEXT", unicode("ArialMT")),
            (
                b"Sz  ",
                b"UntF",
                [b"#Pnt".to_vec(), 24f64.to_be_bytes().to_vec()].concat(),
            ),
        ]);
        let range = descriptor(vec![
            (b"From", b"long", u32b(0)),
            (b"T   ", b"long", u32b(5)),
            (b"textStyle", b"Objc", style),
        ]);
        let engine = b"<< /EngineDict << /Editor << /Text (hello) >> >> >>";
        let text = descriptor(vec![
            (b"Txt ", b"TEXT", unicode("hello")),
            (
                b"textStyleRange",
                b"VlLs",
                [u32b(1), b"Objc".to_vec(), range].concat(),
            ),
            (
                b"EngineData",
                b"tdta",
                [u32b(engine.len() as u32), engine.to_vec()].concat(),
            ),
        ]);
        let mut b = 1u16.to_be_bytes().to_vec();
        for v in [1.0f64, 0.0, 0.0, 1.0, 2.0, 3.0] {
            b.extend(v.to_be_bytes());
        }
        b.extend(50u16.to_be_bytes());
        b.extend(u32b(16));
        b.extend(text);
        b.extend(1u16.to_be_bytes());
        b.extend(u32b(16));
        b.extend(descriptor(vec![]));
        for v in [0.0f64, 0.0, 100.0, 50.0] {
            b.extend(v.to_be_bytes());
        }
        let t = parse_text(&b).unwrap();
        assert_eq!(t.text(), Some("hello"));
        assert_eq!(t.engine_data(), Some(engine.as_slice()));
        let runs = t.style_runs();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].font_name(), Some("ArialMT"));
        assert_eq!(runs[0].size().unwrap().number(), Some(24.0));
        for end in 0..b.len() {
            assert!(parse_text(&b[..end]).is_err());
        }
    }
    #[test]
    fn metadata_styles_fills_and_adjustments() {
        let effect = descriptor(vec![
            (b"enab", b"bool", vec![1]),
            (
                b"blur",
                b"UntF",
                [b"#Pxl".to_vec(), 9f64.to_be_bytes().to_vec()].concat(),
            ),
        ]);
        let bytes = [
            u32b(0),
            u32b(16),
            descriptor(vec![
                (b"DrSh", b"Objc", effect.clone()),
                (b"FrFX", b"Objc", effect.clone()),
                (b"OrGl", b"Objc", effect),
            ]),
        ]
        .concat();
        let styles = parse_styles(&bytes).unwrap();
        for key in [b"DrSh", b"FrFX", b"OrGl"] {
            let e = styles.effect(key).unwrap();
            assert_eq!(e.enabled(), Some(true));
            assert_eq!(e.size().unwrap().number(), Some(9.0));
        }
        assert!(parse_styles(&bytes[1..]).is_err());
        for key in [*b"SoCo", *b"GdFl", *b"PtFl"] {
            let bytes = [
                u32b(16),
                descriptor(vec![(b"Opct", b"doub", 55f64.to_be_bytes().to_vec())]),
            ]
            .concat();
            let fill = parse_fill(key, &bytes).unwrap();
            assert_eq!(fill.descriptor.get(b"Opct").unwrap().number(), Some(55.0));
        }
        let raw = [0, 2, 0, 3, 0, 127, 0];
        let a = adjustment(*b"brit", &raw).unwrap();
        assert_eq!(a.kind, AdjustmentKind::BrightnessContrast);
        assert_eq!(a.data, raw);
        assert!(adjustment(*b"xxxx", &raw).is_none());
    }
    #[test]
    fn metadata_descriptor_decodes_typed_values_and_bounds() {
        let mut unit = b"#Pxl".to_vec();
        unit.extend(12.5f64.to_be_bytes());
        let b = descriptor(vec![
            (b"Txt ", b"TEXT", unicode("Hello 🌍")),
            (b"Sz  ", b"UntF", unit),
            (b"enab", b"bool", vec![1]),
            (b"data", b"tdta", [u32b(3), vec![1, 2, 3]].concat()),
        ]);
        let (d, n) = parse_descriptor(&b).unwrap();
        assert_eq!(n, b.len());
        assert_eq!(d.items[0].1, Value::Text("Hello 🌍".into()));
        assert_eq!(
            d.items[1].1,
            Value::Unit {
                unit: *b"#Pxl",
                value: 12.5
            }
        );
        assert_eq!(d.items[3].1, Value::Raw(&[1, 2, 3]));
        for end in 0..b.len() {
            assert!(parse_descriptor(&b[..end]).is_err(), "truncation {end}");
        }
        assert!(parse_descriptor(&descriptor(vec![(b"bad!", b"xxxx", vec![])])).is_err());
        assert!(parse_descriptor(&[255; 16]).is_err());
    }
}
