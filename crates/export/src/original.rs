//! Non-rendering original + XMP export. Original metadata is intentionally retained.
use crate::{PreparedExport, encode_error, new_output_temp, warning_path};
use engine_api::{EngineResult, jobs::CancellationToken, recipe::Recipe};
use sidecar::{MarkPreset, Sidecar, XmpPacket};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// Copy an original without decoding its samples and merge the recipe into XMP.
/// Non-DNG files are byte-identical, with an appended-name XMP sidecar. Classic
/// TIFF DNGs receive an appended IFD0 and embedded XMP, preserving all original
/// offsets and sample bytes. BigTIFF is rejected, never silently re-encoded.
/// `packet`, when supplied, is the authoritative source XMP (e.g. a sidecar);
/// otherwise a DNG's embedded XMP is used. Pass `recipe=None` when no authoritative
/// edit exists, to retain embedded edits. No privacy filtering is performed:
/// a byte-preserving copy necessarily retains original EXIF/IPTC/MakerNotes.
/// The destination and sidecar must not exist. Cancellation never publishes a
/// partial file. The original is opened read-only and is never modified.
pub fn export_original(
    source: &Path,
    destination: &Path,
    recipe: Option<&Recipe>,
    packet: Option<&XmpPacket>,
    cancel: &CancellationToken,
) -> EngineResult<PathBuf> {
    cancel.check()?;
    Sidecar::ensure_writable_destination(destination)?;
    let extension = |p: &Path| {
        p.extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase)
    };
    let ext =
        extension(source).ok_or_else(|| encode_error("original requires a file extension"))?;
    if extension(destination).as_deref() != Some(&ext) {
        return Err(encode_error(
            "original export must retain the source extension",
        ));
    }
    let side_path = Sidecar::paths(destination).xmp;
    if destination.symlink_metadata().is_ok() || side_path.symlink_metadata().is_ok() {
        return Err(encode_error("original export destination already exists"));
    }
    let mut input = File::open(source).map_err(encode_error)?;
    if !input.metadata().map_err(encode_error)?.is_file() {
        return Err(encode_error("original source must be a regular file"));
    }
    let dng = if ext == "dng" {
        Some(DngHeader::read(&mut input)?)
    } else {
        None
    };
    let embedded = dng.as_ref().and_then(|d| d.xmp.as_ref());
    let base = packet.or(embedded).cloned().unwrap_or_else(|| {
        XmpPacket::from_selection(&Default::default(), &MarkPreset::lightroom())
    });
    let merged = if let Some(recipe) = recipe {
        base.with_recipe(recipe)?
            .with_selection(&recipe.selection, &MarkPreset::lightroom())?
    } else {
        base
    };
    let dir = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(dir).map_err(encode_error)?;
    let mut temp = new_output_temp(dir)?;
    input.rewind().map_err(encode_error)?;
    let mut buffer = vec![0; 1024 * 1024];
    loop {
        cancel.check()?;
        let n = input.read(&mut buffer).map_err(encode_error)?;
        if n == 0 {
            break;
        }
        temp.write_all(&buffer[..n]).map_err(encode_error)?;
    }
    let side_temp = if let Some(dng) = dng {
        dng.append(temp.as_file_mut(), merged.serialize())?;
        None
    } else {
        let mut side = new_output_temp(dir)?;
        side.write_all(merged.serialize().as_bytes())
            .map_err(encode_error)?;
        side.as_file().sync_all().map_err(encode_error)?;
        Some(side)
    };
    temp.as_file().sync_all().map_err(encode_error)?;
    PreparedExport {
        temp,
        side_temp,
        warning_temp: None,
        path: destination.into(),
        side_path,
        warning_path: warning_path(destination),
    }
    .commit(cancel)
}

struct DngHeader {
    little: bool,
    entries: Vec<[u8; 12]>,
    next: [u8; 4],
    xmp: Option<XmpPacket>,
}
impl DngHeader {
    fn u16(&self, b: [u8; 2]) -> u16 {
        if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        }
    }
    fn u32(&self, b: [u8; 4]) -> u32 {
        if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        }
    }
    fn short(&self, v: u16) -> [u8; 2] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
    fn long(&self, v: u32) -> [u8; 4] {
        if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        }
    }
    fn read(file: &mut File) -> EngineResult<Self> {
        let mut header = [0; 8];
        file.read_exact(&mut header).map_err(encode_error)?;
        if &header[..4] != b"II\x2a\0" && &header[..4] != b"MM\0\x2a" {
            return Err(encode_error("original DNG requires classic TIFF"));
        }
        let mut dng = Self {
            little: header[0] == b'I',
            entries: Vec::new(),
            next: [0; 4],
            xmp: None,
        };
        let len = file.metadata().map_err(encode_error)?.len();
        let offset = dng.u32(header[4..8].try_into().unwrap());
        if offset < 8 {
            return Err(encode_error("invalid DNG IFD0 offset"));
        }
        file.seek(SeekFrom::Start(u64::from(offset)))
            .map_err(encode_error)?;
        let mut count = [0; 2];
        file.read_exact(&mut count).map_err(encode_error)?;
        let count = usize::from(dng.u16(count));
        if count == 0 || u64::from(offset) + 6 + count as u64 * 12 > len {
            return Err(encode_error("invalid DNG IFD0 size"));
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..count {
            let mut entry = [0; 12];
            file.read_exact(&mut entry).map_err(encode_error)?;
            if !seen.insert(dng.u16(entry[..2].try_into().unwrap())) {
                return Err(encode_error("duplicate DNG IFD0 tag"));
            }
            dng.entries.push(entry);
        }
        file.read_exact(&mut dng.next).map_err(encode_error)?;
        if !seen.contains(&50706) || dng.u32(dng.next) == offset {
            return Err(encode_error("invalid DNG version or IFD chain"));
        }
        if let Some(entry) = dng
            .entries
            .iter()
            .find(|e| dng.u16(e[..2].try_into().unwrap()) == 700)
        {
            let typ = dng.u16(entry[2..4].try_into().unwrap());
            let count = dng.u32(entry[4..8].try_into().unwrap()) as usize;
            if !matches!(typ, 1 | 7) || count > 1024 * 1024 {
                return Err(encode_error("invalid DNG XMP type or size"));
            }
            let mut bytes = vec![0; count];
            if count <= 4 {
                bytes.copy_from_slice(&entry[8..8 + count]);
            } else {
                let at = u64::from(dng.u32(entry[8..12].try_into().unwrap()));
                if at + count as u64 > len {
                    return Err(encode_error("DNG XMP outside file"));
                }
                file.seek(SeekFrom::Start(at)).map_err(encode_error)?;
                file.read_exact(&mut bytes).map_err(encode_error)?;
            }
            if !bytes.is_empty() {
                dng.xmp = Some(XmpPacket::parse(
                    String::from_utf8(bytes).map_err(encode_error)?,
                )?);
            }
        }
        Ok(dng)
    }
    fn append(mut self, file: &mut File, xmp: &str) -> EngineResult<()> {
        if xmp.len() > 1024 * 1024 {
            return Err(encode_error("DNG XMP exceeds 1 MiB"));
        }
        let mut end = file.seek(SeekFrom::End(0)).map_err(encode_error)?;
        if end % 2 != 0 {
            file.write_all(&[0]).map_err(encode_error)?;
            end += 1;
        }
        let little = self.little;
        let tag = |e: &[u8; 12]| {
            if little {
                u16::from_le_bytes([e[0], e[1]])
            } else {
                u16::from_be_bytes([e[0], e[1]])
            }
        };
        self.entries.retain(|e| tag(e) != 700);
        let count = u16::try_from(self.entries.len() + 1).map_err(encode_error)?;
        let xmp_offset = end + 6 + u64::from(count) * 12;
        u32::try_from(xmp_offset + xmp.len() as u64).map_err(encode_error)?;
        let mut entry = [0; 12];
        entry[..2].copy_from_slice(&self.short(700));
        entry[2..4].copy_from_slice(&self.short(1));
        entry[4..8].copy_from_slice(&self.long(xmp.len() as u32));
        entry[8..12].copy_from_slice(&self.long(xmp_offset as u32));
        self.entries.push(entry);
        self.entries.sort_by_key(tag);
        file.write_all(&self.short(count)).map_err(encode_error)?;
        for entry in &self.entries {
            file.write_all(entry).map_err(encode_error)?;
        }
        file.write_all(&self.next).map_err(encode_error)?;
        file.write_all(xmp.as_bytes()).map_err(encode_error)?;
        file.seek(SeekFrom::Start(4)).map_err(encode_error)?;
        file.write_all(&self.long(end as u32))
            .map_err(encode_error)?;
        Ok(())
    }
}
