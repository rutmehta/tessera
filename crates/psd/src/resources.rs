//! Image resources are retained in file order, including duplicate and unknown IDs.
use crate::binary::{self, error, Reader};
use crate::Result;
const MAC_ROMAN: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»…\u{a0}ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImageResource {
    pub signature: [u8; 4],
    pub id: u16,
    /// Original Pascal name bytes (not necessarily UTF-8).
    pub name: Vec<u8>,
    pub data: Vec<u8>,
}
impl ImageResource {
    pub fn new(id: u16, data: Vec<u8>) -> Self {
        Self {
            signature: *b"8BIM",
            id,
            name: Vec::new(),
            data,
        }
    }
}
pub(crate) fn read(mut r: Reader<'_>) -> Result<Vec<ImageResource>> {
    let mut resources = Vec::new();
    while r.remaining() != 0 {
        let signature = r.array()?;
        let id = r.u16()?;
        let name = r.pascal(2)?;
        let data = r.blob()?;
        r.take(data.len() % 2)?;
        resources.push(ImageResource {
            signature,
            id,
            name,
            data,
        });
    }
    Ok(resources)
}
pub(crate) fn write(resources: &[ImageResource]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    for r in resources {
        out.extend_from_slice(&r.signature);
        out.extend_from_slice(&r.id.to_be_bytes());
        binary::pascal(&mut out, &r.name, 2)?;
        binary::section(&mut out, &r.data, false)?;
        binary::pad(&mut out, 2);
    }
    Ok(out)
}
/// ResolutionInfo uses unsigned 16.16 fixed-point values and unit codes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub horizontal: u32,
    pub horizontal_unit: u16,
    pub width_unit: u16,
    pub vertical: u32,
    pub vertical_unit: u16,
    pub height_unit: u16,
}
impl Resolution {
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() != 16 {
            return Err(error("resolution must be 16 bytes"));
        }
        let mut r = Reader::new(data);
        Ok(Self {
            horizontal: r.u32()?,
            horizontal_unit: r.u16()?,
            width_unit: r.u16()?,
            vertical: r.u32()?,
            vertical_unit: r.u16()?,
            height_unit: r.u16()?,
        })
    }
    pub fn to_resource(self) -> ImageResource {
        let mut data = self.horizontal.to_be_bytes().to_vec();
        data.extend_from_slice(&self.horizontal_unit.to_be_bytes());
        data.extend_from_slice(&self.width_unit.to_be_bytes());
        data.extend_from_slice(&self.vertical.to_be_bytes());
        data.extend_from_slice(&self.vertical_unit.to_be_bytes());
        data.extend_from_slice(&self.height_unit.to_be_bytes());
        ImageResource::new(1005, data)
    }
}
/// Encode standard Unicode alpha names and a MacRoman compatibility resource.
/// Unicode is authoritative when a name cannot be represented in MacRoman.
pub fn alpha_name_resources(names: &[String]) -> Result<Vec<ImageResource>> {
    let mut unicode = Vec::new();
    let mut legacy = Vec::new();
    for name in names {
        let units: Vec<_> = name.encode_utf16().collect();
        binary::length(&mut unicode, units.len(), false)?;
        for unit in units {
            unicode.extend_from_slice(&unit.to_be_bytes());
        }
        let bytes: Vec<_> = name
            .chars()
            .take(255)
            .map(|c| {
                if c.is_ascii() {
                    c as u8
                } else {
                    MAC_ROMAN
                        .chars()
                        .position(|v| v == c)
                        .map(|i| i as u8 + 128)
                        .unwrap_or(b'?')
                }
            })
            .collect();
        binary::pascal(&mut legacy, &bytes, 1)?;
    }
    Ok(vec![
        ImageResource::new(1006, legacy),
        ImageResource::new(1045, unicode),
    ])
}

/// Display information for one extra composite plane. Modes 0/1 describe
/// alpha display polarity; mode 2 is a spot ink. Opacity is a percentage
/// (solidity for spot inks), not an 8-bit opacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelDisplayInfo {
    pub color_space: u16,
    pub color: [u16; 4],
    pub opacity: u16,
    pub mode: u8,
}
impl crate::PsdDocument {
    /// Read legacy 14-byte records (1007) or versioned 13-byte records (1077).
    /// The modern representation takes precedence, but both are validated.
    pub fn channel_display_info(&self) -> Result<Option<Vec<ChannelDisplayInfo>>> {
        let mut legacy = None;
        let mut modern = None;
        for resource in &self.resources {
            if !matches!(resource.id, 1007 | 1077) {
                continue;
            }
            let target = if resource.id == 1077 {
                &mut modern
            } else {
                &mut legacy
            };
            if target.is_some() {
                return Err(error("duplicate channel display resource"));
            }
            let mut r = Reader::new(&resource.data);
            if resource.id == 1077 && r.u32()? != 1 {
                return Err(error("unsupported channel display version"));
            }
            let mut records = Vec::new();
            while r.remaining() != 0 {
                let info = ChannelDisplayInfo {
                    color_space: r.u16()?,
                    color: [r.u16()?, r.u16()?, r.u16()?, r.u16()?],
                    opacity: r.u16()?,
                    mode: r.u8()?,
                };
                if info.opacity > 100 || info.mode > 2 {
                    return Err(error("invalid channel display opacity or mode"));
                }
                if resource.id == 1007 && r.u8()? != 0 {
                    return Err(error("nonzero legacy channel display padding"));
                }
                records.push(info);
            }
            *target = Some(records);
        }
        Ok(modern.or(legacy))
    }
    pub fn resource(&self, id: u16) -> Option<&[u8]> {
        self.resources
            .iter()
            .find(|r| r.id == id)
            .map(|r| r.data.as_slice())
    }
    pub fn icc_profile(&self) -> Option<&[u8]> {
        self.resource(1039)
    }
    pub fn resolution(&self) -> Result<Option<Resolution>> {
        self.resource(1005).map(Resolution::from_bytes).transpose()
    }
    pub fn selected_layer_ids(&self) -> Result<Vec<u32>> {
        let Some(data) = self.resource(1069) else {
            return Ok(Vec::new());
        };
        let mut r = Reader::new(data);
        let n = r.u16()? as usize;
        if r.remaining() != n * 4 {
            return Err(error("invalid layer selection IDs"));
        }
        (0..n).map(|_| r.u32()).collect()
    }
    /// Saved-channel names. Unicode resource 1045 takes precedence over the
    /// unpadded MacRoman Pascal strings in resource 1006 (not layer `unam`).
    pub fn alpha_names(&self) -> Result<Option<Vec<String>>> {
        let mut legacy = None;
        let mut unicode = None;
        for resource in &self.resources {
            if !matches!(resource.id, 1006 | 1045) {
                continue;
            }
            let target = if resource.id == 1045 {
                &mut unicode
            } else {
                &mut legacy
            };
            if target.is_some() {
                return Err(error("duplicate alpha names resource"));
            }
            let mut r = Reader::new(&resource.data);
            let mut names = Vec::new();
            while r.remaining() != 0 {
                let name = if resource.id == 1045 {
                    let n = r.u32()? as usize;
                    let bytes = r.take(
                        n.checked_mul(2)
                            .ok_or_else(|| error("alpha name overflow"))?,
                    )?;
                    let units: Vec<_> = bytes
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| u16::from_be_bytes([b[0], b[1]]))
                        .collect();
                    String::from_utf16(&units).map_err(|_| error("invalid Unicode alpha name"))?
                } else {
                    r.pascal(1)?
                        .into_iter()
                        .map(|b| {
                            if b < 128 {
                                b as char
                            } else {
                                MAC_ROMAN.chars().nth(usize::from(b - 128)).unwrap()
                            }
                        })
                        .collect()
                };
                names.push(name);
            }
            *target = Some(names);
        }
        Ok(unicode.or(legacy))
    }
    /// Resource IDs 2000..=2998 contain saved path records.
    pub fn paths(&self) -> impl Iterator<Item = &ImageResource> {
        self.resources
            .iter()
            .filter(|r| (2000..=2998).contains(&r.id))
    }
}
