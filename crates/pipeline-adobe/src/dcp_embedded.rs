//! Bounded TIFF metadata extraction. Pixel strips/tiles are never read.
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Seek, SeekFrom},
};

pub(super) fn profile_tag(tag: u16) -> bool {
    matches!(
        tag,
        50721
            | 50722
            | 50730
            | 50778
            | 50779
            | 50937
            | 50938
            | 50939
            | 50940
            | 50964
            | 50965
            | 50981
            | 50982
            | 51107
            | 51108
            | 51109
            | 51110
            | 52529
            | 52530
            | 52531
            | 52532
            | 52535
            | 52537
            | 52538
    )
}

#[derive(Clone)]
struct Field {
    kind: u16,
    count: u32,
    data: Vec<u8>,
}
struct Reader<'a, R> {
    input: &'a mut R,
    be: bool,
    length: u64,
    allocated: usize,
}
impl<R: Read + Seek> Reader<'_, R> {
    fn short(&self, bytes: [u8; 2]) -> u16 {
        if self.be {
            u16::from_be_bytes(bytes)
        } else {
            u16::from_le_bytes(bytes)
        }
    }
    fn long(&self, bytes: [u8; 4]) -> u32 {
        if self.be {
            u32::from_be_bytes(bytes)
        } else {
            u32::from_le_bytes(bytes)
        }
    }
    fn read(&mut self, offset: u64, count: usize) -> Result<Vec<u8>, String> {
        if offset
            .checked_add(count as u64)
            .is_none_or(|end| end > self.length)
        {
            return Err("Truncated embedded profile".into());
        }
        self.allocated = self
            .allocated
            .checked_add(count)
            .ok_or("Profile size overflow")?;
        if self.allocated > 48 << 20 {
            return Err("Embedded profile exceeds resource limit".into());
        }
        self.input
            .seek(SeekFrom::Start(offset))
            .map_err(|e| e.to_string())?;
        let mut bytes = vec![0; count];
        self.input
            .read_exact(&mut bytes)
            .map_err(|e| e.to_string())?;
        Ok(bytes)
    }
}

/// Extract the camera profile tags from IFD0 and the full camera IFD, retaining
/// IFD0 defaults when a raw SubIFD omits them. The returned small TIFF contains
/// profile metadata only and can be parsed by DcpProfile or hashed for cache keys.
/// Supports classic TIFF in either byte order; caps IFD traversal, tag counts and
/// allocation. Extra camera-profile IFDs and pixel payloads are not selected.
pub fn read_embedded_profile(input: &mut (impl Read + Seek)) -> Result<Option<Vec<u8>>, String> {
    let length = input.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
    let mut reader = Reader {
        input,
        be: false,
        length,
        allocated: 0,
    };
    let header = reader.read(0, 8)?;
    reader.be = match &header[..2] {
        b"II" => false,
        b"MM" => true,
        _ => return Err("Invalid TIFF byte order".into()),
    };
    if !matches!(reader.short(header[2..4].try_into().unwrap()), 42 | 0x4352) {
        return Err("Not a classic TIFF/DCP header".into());
    }
    let root = reader.long(header[4..8].try_into().unwrap());
    let mut pending = vec![root];
    let mut seen = BTreeSet::new();
    let mut defaults = BTreeMap::new();
    let mut camera = None;
    while let Some(offset) = pending.pop() {
        if offset < 8 || !seen.insert(offset) || seen.len() > 32 {
            return Err("Invalid or cyclic profile IFD chain".into());
        }
        let bytes = reader.read(u64::from(offset), 2)?;
        let count = usize::from(reader.short(bytes.try_into().unwrap()));
        if count > 4096 {
            return Err("Too many embedded profile fields".into());
        }
        let directory = reader.read(u64::from(offset) + 2, count * 12 + 4)?;
        let mut fields = BTreeMap::new();
        let mut raw = false;
        let mut reduced = false;
        for entry in directory[..count * 12].as_chunks::<12>().0 {
            let tag = reader.short(entry[..2].try_into().unwrap());
            if !profile_tag(tag) && !matches!(tag, 254 | 262 | 330) {
                continue;
            }
            let kind = reader.short(entry[2..4].try_into().unwrap());
            let count = reader.long(entry[4..8].try_into().unwrap());
            let size = match kind {
                1 | 2 | 6 | 7 => 1,
                3 | 8 => 2,
                4 | 9 | 11 | 13 => 4,
                5 | 10 | 12 => 8,
                _ => return Err("Unknown profile field type".into()),
            };
            let size = (count as usize)
                .checked_mul(size)
                .ok_or("Profile size overflow")?;
            if size > 24 << 20 {
                return Err("Profile field exceeds resource limit".into());
            }
            let data = if size <= 4 {
                entry[8..8 + size].to_vec()
            } else {
                let location = reader.long(entry[8..12].try_into().unwrap());
                reader.read(u64::from(location), size)?
            };
            match tag {
                254 if kind == 4 && count == 1 => {
                    reduced = reader.long(data[..4].try_into().unwrap()) & 1 != 0
                }
                262 if kind == 3 && count == 1 => {
                    raw = matches!(reader.short(data[..2].try_into().unwrap()), 32803 | 34892)
                }
                330 if matches!(kind, 4 | 13) && count <= 32 => {
                    for bytes in data.as_chunks::<4>().0 {
                        pending.push(reader.long(*bytes));
                    }
                }
                254 | 262 | 330 => return Err("Invalid image IFD descriptor".into()),
                _ => {
                    if fields.insert(tag, Field { kind, count, data }).is_some() {
                        return Err("Duplicate profile tag".into());
                    }
                }
            }
        }
        if offset == root {
            defaults = fields.clone();
        }
        if raw && !reduced && camera.is_none() {
            camera = Some(fields);
        }
        let next = reader.long(directory[count * 12..].try_into().unwrap());
        if next != 0 {
            pending.push(next);
        }
    }
    if let Some(camera) = camera {
        defaults.extend(camera);
    }
    if !defaults.contains_key(&50721) && !defaults.contains_key(&50722) {
        return Ok(None);
    }
    let be = reader.be;
    let short = |v: u16| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let long = |v: u32| if be { v.to_be_bytes() } else { v.to_le_bytes() };
    let mut output = vec![0; 14 + defaults.len() * 12];
    output[..2].copy_from_slice(if be { b"MM" } else { b"II" });
    output[2..4].copy_from_slice(&short(0x4352));
    output[4..8].copy_from_slice(&long(8));
    output[8..10].copy_from_slice(&short(defaults.len() as u16));
    for (index, (tag, field)) in defaults.into_iter().enumerate() {
        let p = 10 + index * 12;
        output[p..p + 2].copy_from_slice(&short(tag));
        output[p + 2..p + 4].copy_from_slice(&short(field.kind));
        output[p + 4..p + 8].copy_from_slice(&long(field.count));
        if field.data.len() <= 4 {
            output[p + 8..p + 8 + field.data.len()].copy_from_slice(&field.data);
        } else {
            let offset = output.len() as u32;
            output[p + 8..p + 12].copy_from_slice(&long(offset));
            output.extend(field.data);
        }
    }
    Ok(Some(output))
}
