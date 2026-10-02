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
            // Some Lightroom preview databases declare imageId REAL even though
            // catalog IDs are integers. Accept only exactly representable IDs;
            // do not broaden the catalog's general integer/schema predicates.
            let image = number(&r, "imageId").or_else(|| {
                r.get("imageId")?
                    .as_f64()
                    .filter(|v| {
                        v.is_finite() && v.fract() == 0. && v.abs() <= 9_007_199_254_740_991.
                    })
                    .map(|v| v as i64)
            });
            if let (Some(image), Some(uuid), Some(digest)) =
                (image, text(&r, "uuid"), text(&r, "digest"))
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
    /// Whether a legacy container or a split JPEG level exists for this exact
    /// catalog digest. Old cache generations are never substituted.
    pub fn has_preview(&self, image: i64) -> EngineResult<bool> {
        Ok(!self.preview_files(image)?.is_empty())
    }

    fn preview_files(&self, image: i64) -> EngineResult<Vec<PathBuf>> {
        let Some(legacy) = self.lrprev_path(image) else {
            return Ok(Vec::new());
        };
        let mut paths = Vec::new();
        for extension in ["lrprev", "lrfprev", "lrmprev"] {
            let path = legacy.with_extension(extension);
            if path.is_file() {
                paths.push(path);
            }
        }
        let Some(parent) = legacy.parent() else {
            return Ok(paths);
        };
        let Some(stem) = legacy.file_stem().and_then(|s| s.to_str()) else {
            return Ok(paths);
        };
        let prefix = format!("{stem}_");
        let entries = match std::fs::read_dir(parent) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(paths),
            Err(e) => return Err(EngineError::io_at(parent, &e)),
        };
        for entry in entries {
            let entry = entry.map_err(|e| EngineError::io_at(parent, &e))?;
            let name = entry.file_name();
            if name
                .to_str()
                .and_then(|n| n.strip_prefix(&prefix))
                .is_some_and(|n| {
                    !n.is_empty() && n.len() <= 10 && n.bytes().all(|b| b.is_ascii_digit())
                })
                && entry.path().is_file()
            {
                paths.push(entry.path());
            }
        }
        paths.sort();
        Ok(paths)
    }

    /// The smallest JPEG level whose longer edge is at least `min_edge`, else the
    /// largest level. Supports AgHg containers and newer `<uuid>-<digest>_<edge>`
    /// JPEG files. Encoded dimensions, not the filename hint, select the level.
    pub fn jpeg(&self, image: i64, min_edge: u32) -> EngineResult<Option<Vec<u8>>> {
        let mut levels = Vec::new();
        for path in self.preview_files(image)? {
            let bytes = std::fs::read(&path).map_err(|e| EngineError::io_at(&path, &e))?;
            if bytes.starts_with(&[0xff, 0xd8]) {
                levels.push(bytes);
            } else {
                levels.extend(parse_lrprev(&bytes)?.into_iter().filter_map(|section| {
                    (section.name.starts_with("level_") && section.data.starts_with(&[0xff, 0xd8]))
                        .then_some(section.data)
                }));
            }
        }
        levels.sort_by_key(|bytes| jpeg_dimensions(bytes).map_or(0, |(w, h)| w.max(h)));
        let selected = levels
            .iter()
            .position(|bytes| jpeg_dimensions(bytes).is_some_and(|(w, h)| w.max(h) >= min_edge))
            .or_else(|| levels.len().checked_sub(1));
        Ok(selected.map(|i| levels.swap_remove(i)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_index_accepts_exact_real_image_ids_only() {
        let temp = tempfile::tempdir().unwrap();
        let catalog = temp.path().join("synthetic.lrcat");
        let dir = previews_dir(&catalog);
        std::fs::create_dir_all(&dir).unwrap();
        let db = rusqlite::Connection::open(dir.join("previews.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE ImageCacheEntry(imageId REAL, uuid TEXT, digest TEXT);
             INSERT INTO ImageCacheEntry VALUES (1.0, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');
             INSERT INTO ImageCacheEntry VALUES (2.5, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');
             INSERT INTO ImageCacheEntry VALUES (1e30, 'aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa', 'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');",
        ).unwrap();
        drop(db);
        let index = PreviewIndex::open(&catalog).unwrap().unwrap();
        assert_eq!(index.len(), 1);
        assert!(index.lrprev_path(1).is_some());
    }

    #[test]
    fn split_jpeg_levels_select_current_digest_and_largest_dimensions() {
        let temp = tempfile::tempdir().unwrap();
        let uuid = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let index = PreviewIndex {
            dir: temp.path().to_owned(),
            entries: [(1, (uuid.into(), digest.into()))].into(),
        };
        let legacy = index.lrprev_path(1).unwrap();
        std::fs::create_dir_all(legacy.parent().unwrap()).unwrap();
        let jpeg = |width: u16| {
            let mut bytes = vec![0xff, 0xd8, 0xff, 0xc0, 0, 11, 8, 0, 10];
            bytes.extend(width.to_be_bytes());
            bytes.extend([1, 1, 0x11, 0, 0xff, 0xd9]);
            bytes
        };
        let small = jpeg(20);
        let large = jpeg(40);
        for (suffix, bytes) in [("_256", &small), ("_1024", &large)] {
            std::fs::write(
                legacy.with_file_name(format!("{uuid}-{digest}{suffix}")),
                bytes,
            )
            .unwrap();
        }
        std::fs::write(
            legacy.with_file_name(format!("{uuid}-cccccccccccccccccccccccccccccccc_2048")),
            jpeg(80),
        )
        .unwrap();
        assert!(index.has_preview(1).unwrap());
        assert!(!index.has_preview(2).unwrap());
        assert_eq!(index.jpeg(1, 15).unwrap().unwrap(), small);
        assert_eq!(index.jpeg(1, u32::MAX).unwrap().unwrap(), large);
    }

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
