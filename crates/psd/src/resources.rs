//! Image resources are retained in file order, including duplicate and unknown IDs.
use crate::binary::{self, error, Reader};
use crate::Result;
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
impl crate::PsdDocument {
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
    /// Resource IDs 2000..=2998 contain saved path records.
    pub fn paths(&self) -> impl Iterator<Item = &ImageResource> {
        self.resources
            .iter()
            .filter(|r| (2000..=2998).contains(&r.id))
    }
}
