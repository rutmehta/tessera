//! PSD/PSB file interchange, independent of the compositor.
mod binary;
pub mod compression;
pub mod layers;
pub use layers::{Channel, Layer, LayerLocation, LayerSection, Rect};
pub mod metadata;
pub mod resources;
use binary::{error, Reader};
pub use compression::Compression;
pub use resources::{ImageResource, Resolution};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdditionalInfo {
    pub signature: [u8; 4],
    pub key: [u8; 4],
    pub data: Vec<u8>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PsdDocument {
    pub version: Version,
    pub width: u32,
    pub height: u32,
    pub depth: u16,
    pub channels: u16,
    pub color_mode: ColorMode,
    pub color_data: Vec<u8>,
    pub resources: Vec<ImageResource>,
    pub layer_section: LayerSection,
    pub composite: Vec<u8>,
    pub compression: Compression,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Version {
    Psd,
    Psb,
}
impl Version {
    fn wide(self) -> bool {
        self == Self::Psb
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum ColorMode {
    Bitmap = 0,
    Grayscale = 1,
    Indexed = 2,
    Rgb = 3,
    Cmyk = 4,
    Multichannel = 7,
    Duotone = 8,
    Lab = 9,
}
impl TryFrom<u16> for ColorMode {
    type Error = Error;
    fn try_from(n: u16) -> Result<Self> {
        Ok(match n {
            0 => Self::Bitmap,
            1 => Self::Grayscale,
            2 => Self::Indexed,
            3 => Self::Rgb,
            4 => Self::Cmyk,
            7 => Self::Multichannel,
            8 => Self::Duotone,
            9 => Self::Lab,
            _ => return Err(error("invalid color mode")),
        })
    }
}
impl PsdDocument {
    pub fn read(bytes: &[u8]) -> Result<Self> {
        let mut r = Reader::new(bytes);
        r.signature(b"8BPS")?;
        let version = match r.u16()? {
            1 => Version::Psd,
            2 => Version::Psb,
            _ => return Err(error("invalid version")),
        };
        if r.take(6)? != [0; 6] {
            return Err(error("nonzero reserved header"));
        }
        let channels = r.u16()?;
        let height = r.u32()?;
        let width = r.u32()?;
        let depth = r.u16()?;
        let color_mode = ColorMode::try_from(r.u16()?)?;
        let mut d = Self {
            version,
            width,
            height,
            depth,
            channels,
            color_mode,
            color_data: Vec::new(),
            resources: Vec::new(),
            layer_section: LayerSection::default(),
            composite: Vec::new(),
            compression: Compression::Raw,
        };
        d.validate()?;
        d.color_data = r.blob()?;
        d.resources = resources::read(r.section(false)?)?;
        d.layer_section = LayerSection::read(r.section(version.wide())?, version, depth)?;
        d.compression = Compression::try_from(r.u16()?)?;
        d.composite = compression::decode(
            r.take(r.remaining())?,
            d.compression,
            width as usize,
            height as usize * channels as usize,
            depth,
            version.wide(),
        )?;
        Ok(d)
    }
    fn validate(&self) -> Result<()> {
        let max = if self.version.wide() { 300_000 } else { 30_000 };
        if self.width == 0
            || self.height == 0
            || self.width > max
            || self.height > max
            || !(1..=56).contains(&self.channels)
            || !matches!(self.depth, 1 | 8 | 16 | 32)
        {
            return Err(error("invalid header dimensions/channels/depth"));
        }
        Ok(())
    }
    pub fn write(&self) -> Result<Vec<u8>> {
        self.validate()?;
        let mut out = b"8BPS".to_vec();
        out.extend_from_slice(&(if self.version.wide() { 2u16 } else { 1 }).to_be_bytes());
        out.extend_from_slice(&[0; 6]);
        out.extend_from_slice(&self.channels.to_be_bytes());
        out.extend_from_slice(&self.height.to_be_bytes());
        out.extend_from_slice(&self.width.to_be_bytes());
        out.extend_from_slice(&self.depth.to_be_bytes());
        out.extend_from_slice(&(self.color_mode as u16).to_be_bytes());
        binary::section(&mut out, &self.color_data, false)?;
        binary::section(&mut out, &resources::write(&self.resources)?, false)?;
        binary::section(
            &mut out,
            &self.layer_section.write(self.version, self.depth)?,
            self.version.wide(),
        )?;
        out.extend_from_slice(&(self.compression as u16).to_be_bytes());
        out.extend_from_slice(&compression::encode(
            &self.composite,
            self.compression,
            self.width as usize,
            self.height as usize * self.channels as usize,
            self.depth,
            self.version.wide(),
        )?);
        Ok(out)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn resource_framing() {
        let mut resource = b"8BIM".to_vec();
        resource.extend_from_slice(&1039u16.to_be_bytes());
        resource.extend_from_slice(&[1, b'x']);
        resource.extend_from_slice(&3u32.to_be_bytes());
        resource.extend_from_slice(&[1, 2, 3, 0]);
        let b = [
            b"8BPS".as_slice(),
            &1u16.to_be_bytes(),
            &[0; 6],
            &1u16.to_be_bytes(),
            &1u32.to_be_bytes(),
            &2u32.to_be_bytes(),
            &8u16.to_be_bytes(),
            &1u16.to_be_bytes(),
            &[0; 4],
            &(resource.len() as u32).to_be_bytes(),
            &resource,
            &[0; 4],
            &[0, 0, 7, 42],
        ]
        .concat();
        let d = PsdDocument::read(&b).unwrap();
        assert_eq!(PsdDocument::read(&d.write().unwrap()).unwrap(), d);
    }
    #[test]
    fn hand_built_minimal_raw() {
        let b = [
            b"8BPS".as_slice(),
            &1u16.to_be_bytes(),
            &[0; 6],
            &1u16.to_be_bytes(),
            &1u32.to_be_bytes(),
            &2u32.to_be_bytes(),
            &8u16.to_be_bytes(),
            &1u16.to_be_bytes(),
            &[0; 12],
            &[0, 0, 7, 42],
        ]
        .concat();
        let d = PsdDocument::read(&b).unwrap();
        assert_eq!((d.width, d.height, d.depth, d.channels), (2, 1, 8, 1));
        assert_eq!(d.composite, [7, 42]);
        assert_eq!(PsdDocument::read(&d.write().unwrap()).unwrap(), d);
        for n in 0..b.len() {
            assert!(PsdDocument::read(&b[..n]).is_err(), "{n}");
        }
    }
}
