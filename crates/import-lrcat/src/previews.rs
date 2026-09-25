//! Read-only access to Lightroom's standard previews (`<Catalog> Previews.lrdata`).
//!
//! Layout (as documented by long-standing third-party extractors; verified here
//! only against our own writer, see README):
//!
//! - `<catalog stem> Previews.lrdata/previews.db` is SQLite. `ImageCacheEntry`
//!   maps `imageId` (the catalog's `Adobe_images.id_local`) to `uuid` and
//!   `digest`.
//! - The pyramid for an image lives at `<uuid[0]>/<uuid[0..4]>/<uuid>-<digest>.lrprev`.
//! - An `.lrprev` file is a sequence of sections. Each section starts with a
//!   header: `"AgHg"` magic, header length (u16 big-endian, normally 32),
//!   version (u8), kind (u8), payload length (u64 BE), padding length (u64 BE)
//!   and a NUL-padded ASCII name filling the rest of the header. The payload and
//!   then the padding follow. The `header` section is Lua-like text describing
//!   the levels; `level_1`, `level_2`, ... hold one baseline JPEG each, smallest
//!   first. Other sections are ignored.
//!
//! Nothing here writes into the catalog or the preview cache: `previews.db` is
//! copied to temporary storage before it is opened read-only, exactly like the
//! catalog itself.
use crate::{copied_catalog, decode, number, open_copy, rows, text};
use engine_api::error::{EngineError, EngineResult};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

const MAGIC: &[u8; 4] = b"AgHg";

/// One `.lrprev` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub version: u8,
    pub kind: u8,
    pub data: Vec<u8>,
}

/// Parse every section of an `.lrprev` container. Truncated or foreign data is
/// a decode error rather than a panic.
pub fn parse_lrprev(bytes: &[u8]) -> EngineResult<Vec<Section>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos < bytes.len() {
        let header = bytes
            .get(pos..pos + 24)
            .ok_or_else(|| decode("lrprev: truncated section header"))?;
        if &header[..4] != MAGIC {
            return Err(decode("lrprev: missing AgHg section magic"));
        }
        let header_len = u16::from_be_bytes([header[4], header[5]]) as usize;
        if header_len < 24 {
            return Err(decode("lrprev: invalid section header length"));
        }
        let data_len = u64::from_be_bytes(header[8..16].try_into().expect("8 bytes"));
        let padding = u64::from_be_bytes(header[16..24].try_into().expect("8 bytes"));
        let name = bytes
            .get(pos + 24..pos + header_len)
            .ok_or_else(|| decode("lrprev: truncated section name"))?;
        let name = String::from_utf8_lossy(name)
            .trim_end_matches('\0')
            .to_owned();
        let start = pos + header_len;
        let end = usize::try_from(data_len)
            .ok()
            .and_then(|n| start.checked_add(n))
            .filter(|&end| end <= bytes.len())
            .ok_or_else(|| decode("lrprev: section payload exceeds file"))?;
        out.push(Section {
            name,
            version: header[6],
            kind: header[7],
            data: bytes[start..end].to_vec(),
        });
        pos = usize::try_from(padding)
            .ok()
            .and_then(|p| end.checked_add(p))
            .ok_or_else(|| decode("lrprev: invalid padding"))?
            .min(bytes.len());
    }
    Ok(out)
}

/// Write sections in the same container format (16-byte aligned). Used by the
/// synthetic fixture and tests; the importer never writes preview caches.
pub fn write_lrprev(sections: &[Section]) -> Vec<u8> {
    let mut out = Vec::new();
    for s in sections {
        let mut name = s.name.as_bytes().to_vec();
        let header_len = 24 + name.len().max(8).div_ceil(8) * 8;
        name.resize(header_len - 24, 0);
        let padding = (16 - (s.data.len() % 16)) % 16;
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&(header_len as u16).to_be_bytes());
        out.push(s.version);
        out.push(s.kind);
        out.extend_from_slice(&(s.data.len() as u64).to_be_bytes());
        out.extend_from_slice(&(padding as u64).to_be_bytes());
        out.extend_from_slice(&name);
        out.extend_from_slice(&s.data);
        out.resize(out.len() + padding, 0);
    }
    out
}

/// JPEG levels (`level_N` sections holding a JPEG), in file order.
pub fn jpeg_levels(sections: &[Section]) -> Vec<&Section> {
    sections
        .iter()
        .filter(|s| s.name.starts_with("level_") && s.data.starts_with(&[0xFF, 0xD8]))
        .collect()
}

/// Width and height from a JPEG's first SOF marker, without decoding pixels.
pub fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut pos = 2;
    while pos + 4 <= bytes.len() {
        if bytes[pos] != 0xFF {
            pos += 1;
            continue;
        }
        let marker = bytes[pos + 1];
        if marker == 0xFF {
            pos += 1;
            continue;
        }
        if matches!(marker, 0xD8 | 0x01 | 0xD0..=0xD7) {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        let is_sof = matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_sof {
            let s = bytes.get(pos + 5..pos + 9)?;
            let h = u16::from_be_bytes([s[0], s[1]]) as u32;
            let w = u16::from_be_bytes([s[2], s[3]]) as u32;
            return Some((w, h));
        }
        pos += 2 + len;
    }
    None
}

/// Embedded ICC profile (APP2 `ICC_PROFILE` chunks, reassembled in order).
pub fn jpeg_icc_profile(bytes: &[u8]) -> Option<Vec<u8>> {
    const TAG: &[u8] = b"ICC_PROFILE\0";
    let mut chunks = BTreeMap::new();
    let mut pos = 2;
    while pos + 4 <= bytes.len() && bytes.get(pos) == Some(&0xFF) {
        let marker = bytes[pos + 1];
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let len = u16::from_be_bytes([bytes[pos + 2], bytes[pos + 3]]) as usize;
        let body = bytes.get(pos + 4..pos + 2 + len)?;
        if marker == 0xE2 && body.starts_with(TAG) && body.len() > TAG.len() + 2 {
            chunks.insert(body[TAG.len()], body[TAG.len() + 2..].to_vec());
        }
        pos += 2 + len;
    }
    (!chunks.is_empty()).then(|| chunks.into_values().flatten().collect())
}

/// `<stem> Previews.lrdata` beside the catalog.
pub fn previews_dir(catalog: &Path) -> PathBuf {
    let stem = catalog
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    catalog
        .parent()
        .unwrap_or(Path::new("."))
        .join(format!("{stem} Previews.lrdata"))
}

/// The catalog's preview index: `imageId -> (uuid, digest)`.
#[derive(Debug, Clone)]
pub struct PreviewIndex {
    pub dir: PathBuf,
    entries: BTreeMap<i64, (String, String)>,
}

impl PreviewIndex {
    /// `Ok(None)` when the catalog has no preview cache (it is optional).
    pub fn open(catalog: &Path) -> EngineResult<Option<Self>> {
        let dir = previews_dir(catalog);
        let db = dir.join("previews.db");
        if !db.is_file() {
            return Ok(None);
        }
        let (_temp, copy) = copied_catalog(&db)?;
        let c = open_copy(&copy)?;
        let mut report = vec![];
        let mut entries = BTreeMap::new();
        for r in rows(&c, "ImageCacheEntry", false, &mut report)? {
            if let (Some(image), Some(uuid), Some(digest)) =
                (number(&r, "imageId"), text(&r, "uuid"), text(&r, "digest"))
            {
                entries.insert(image, (uuid, digest));
            }
        }
        Ok(Some(Self { dir, entries }))
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    /// Pyramid path for a catalog image id (the file may be absent).
    pub fn lrprev_path(&self, image: i64) -> Option<PathBuf> {
        let (uuid, digest) = self.entries.get(&image)?;
        let a = uuid.get(..1)?;
        let b = uuid.get(..4)?;
        Some(
            self.dir
                .join(a)
                .join(b)
                .join(format!("{uuid}-{digest}.lrprev")),
        )
    }
    /// The smallest JPEG level whose longer edge is at least `min_edge`, else the
    /// largest level. `Ok(None)` when the image has no cached preview.
    pub fn jpeg(&self, image: i64, min_edge: u32) -> EngineResult<Option<Vec<u8>>> {
        let Some(path) = self.lrprev_path(image) else {
            return Ok(None);
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(EngineError::io_at(&path, &e)),
        };
        let sections = parse_lrprev(&bytes)?;
        let mut levels: Vec<(u32, &Section)> = jpeg_levels(&sections)
            .into_iter()
            .map(|s| (jpeg_dimensions(&s.data).map_or(0, |(w, h)| w.max(h)), s))
            .collect();
        levels.sort_by_key(|(edge, _)| *edge);
        Ok(levels
            .iter()
            .find(|(edge, _)| *edge >= min_edge)
            .or(levels.last())
            .map(|(_, s)| s.data.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn container_round_trip_and_truncation() {
        let sections = vec![
            Section {
                name: "header".into(),
                version: 0,
                kind: 0,
                data: b"levels = { }".to_vec(),
            },
            Section {
                name: "level_1".into(),
                version: 0,
                kind: 1,
                data: vec![0xFF, 0xD8, 1, 2, 3],
            },
        ];
        let bytes = write_lrprev(&sections);
        assert_eq!(bytes.len() % 16, 0);
        assert_eq!(parse_lrprev(&bytes).unwrap(), sections);
        assert_eq!(jpeg_levels(&sections).len(), 1);
        assert!(parse_lrprev(&bytes[..bytes.len() - 20]).is_err());
        assert!(parse_lrprev(b"nope, not a preview").is_err());
    }

    #[test]
    fn jpeg_header_parsing() {
        // SOI, APP2 ICC chunk, SOF0 8-bit 20x10, EOI.
        let mut jpeg = vec![0xFF, 0xD8];
        let icc = b"ICC_PROFILE\0\x01\x01PROFILE";
        jpeg.extend_from_slice(&[0xFF, 0xE2]);
        jpeg.extend_from_slice(&((icc.len() + 2) as u16).to_be_bytes());
        jpeg.extend_from_slice(icc);
        jpeg.extend_from_slice(&[
            0xFF, 0xC0, 0, 11, 8, 0, 10, 0, 20, 1, 1, 0x11, 0, 0xFF, 0xD9,
        ]);
        assert_eq!(jpeg_dimensions(&jpeg), Some((20, 10)));
        assert_eq!(jpeg_icc_profile(&jpeg).unwrap(), b"PROFILE");
        assert_eq!(jpeg_dimensions(b"xx"), None);
    }
}
