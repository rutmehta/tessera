//! Bounded native metadata extraction and relocation. Never copy image offsets,
//! previews or opaque MakerNotes into a developed image.
use crate::{ExportSettings, Metadata, encode_error};
use engine_api::{EngineResult, jobs::CancellationToken};
use sidecar::XmpPacket;
use std::{
    collections::BTreeSet,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

const LIMIT: usize = 2 * 1024 * 1024;
#[derive(Clone, Debug)]
pub(crate) struct Tag {
    pub id: u16,
    pub kind: u16,
    pub count: u32,
    pub data: Vec<u8>,
}
#[derive(Clone, Default, Debug)]
pub(crate) struct Native {
    pub root: Vec<Tag>,
    pub exif: Vec<Tag>,
    pub gps: Vec<Tag>,
    pub interop: Vec<Tag>,
    pub iim: Vec<(u8, u8, Vec<u8>)>,
    pub xmp: Option<XmpPacket>,
    used: usize,
}
struct Reader<'a, R> {
    io: &'a mut R,
    len: u64,
    budget: usize,
}
impl<R: Read + Seek> Reader<'_, R> {
    fn at(&mut self, offset: u64, count: usize) -> EngineResult<Vec<u8>> {
        if count > self.budget
            || offset
                .checked_add(count as u64)
                .is_none_or(|end| end > self.len)
        {
            return Err(encode_error(
                "native metadata exceeds bounds or 2 MiB budget",
            ));
        }
        self.budget -= count;
        self.io
            .seek(SeekFrom::Start(offset))
            .map_err(encode_error)?;
        let mut bytes = vec![0; count];
        self.io.read_exact(&mut bytes).map_err(encode_error)?;
        Ok(bytes)
    }
}
fn u16n(b: &[u8], le: bool) -> u16 {
    let a = b[..2].try_into().unwrap();
    if le {
        u16::from_le_bytes(a)
    } else {
        u16::from_be_bytes(a)
    }
}
fn u32n(b: &[u8], le: bool) -> u32 {
    let a = b[..4].try_into().unwrap();
    if le {
        u32::from_le_bytes(a)
    } else {
        u32::from_be_bytes(a)
    }
}
fn width(kind: u16) -> EngineResult<usize> {
    match kind {
        1 | 2 | 6 | 7 => Ok(1),
        3 | 8 => Ok(2),
        4 | 9 | 11 | 13 => Ok(4),
        5 | 10 | 12 => Ok(8),
        _ => Err(encode_error("unsupported native TIFF field type")),
    }
}
impl Native {
    fn charge(&mut self, size: usize) -> EngineResult<()> {
        self.used = self
            .used
            .checked_add(size)
            .filter(|n| *n <= LIMIT)
            .ok_or_else(|| {
                encode_error("expanded native metadata exceeds 2 MiB aggregate budget")
            })?;
        Ok(())
    }
    pub fn read(path: &Path, cancel: &CancellationToken) -> EngineResult<Self> {
        cancel.check()?;
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NONBLOCK);
        }
        let mut file = options.open(path).map_err(encode_error)?;
        let meta = file.metadata().map_err(encode_error)?;
        if !meta.is_file() {
            return Err(encode_error("metadata source must be a regular file"));
        }
        let mut r = Reader {
            io: &mut file,
            len: meta.len(),
            budget: LIMIT,
        };
        let head = r.at(0, 12.min(meta.len() as usize))?;
        let mut result = Self::default();
        if head.starts_with(b"II") || head.starts_with(b"MM") {
            result.tiff_reader(&mut r, 0, meta.len())?;
        } else if head.starts_with(&[255, 216]) {
            let mut at = 2;
            let mut complete = false;
            for _ in 0..4096 {
                cancel.check()?;
                let h = r.at(at, 4)?;
                if h[0] != 255 {
                    return Err(encode_error("invalid JPEG metadata marker"));
                }
                if matches!(h[1], 0xda | 0xd9) {
                    complete = true;
                    break;
                }
                let size = u16n(&h[2..], false) as usize;
                if size < 2 {
                    return Err(encode_error("invalid JPEG segment size"));
                }
                if matches!(h[1], 0xe1 | 0xed) {
                    let bytes = r.at(at + 4, size - 2)?;
                    if let Some(t) = bytes.strip_prefix(b"Exif\0\0") {
                        result.tiff(t)?;
                    } else if let Some(x) = bytes.strip_prefix(b"http://ns.adobe.com/xap/1.0/\0") {
                        result.set_xmp(x)?;
                    } else if bytes.starts_with(b"http://ns.adobe.com/xmp/extension/\0") {
                        // Do not silently omit identities from privacy filtering. The
                        // segment has already passed the bounded metadata reader;
                        // reject rather than allocate or assemble extension chunks.
                        return Err(encode_error("Extended XMP is not supported"));
                    } else if let Some(irb) = bytes.strip_prefix(b"Photoshop 3.0\0") {
                        result.photoshop(irb)?;
                    }
                }
                at = at
                    .checked_add(2 + size as u64)
                    .ok_or_else(|| encode_error("JPEG offset overflow"))?;
                if at >= r.len {
                    return Err(encode_error("truncated JPEG metadata"));
                }
            }
            if !complete {
                return Err(encode_error("JPEG metadata segment limit exceeded"));
            }
        } else if head.starts_with(b"\x89PNG\r\n\x1a\n") {
            let mut at = 8;
            while at + 12 <= r.len {
                cancel.check()?;
                let h = r.at(at, 8)?;
                let size = u32n(&h, false) as usize;
                let end = at + 12 + size as u64;
                if end > r.len {
                    return Err(encode_error("truncated PNG metadata"));
                }
                if &h[4..] == b"eXIf" {
                    result.tiff(&r.at(at + 8, size)?)?;
                }
                if &h[4..] == b"tEXt" || &h[4..] == b"zTXt" {
                    let bytes = r.at(at + 8, size)?;
                    if let Some(zero) = bytes.iter().position(|&b| b == 0) {
                        let key = &bytes[..zero];
                        if matches!(
                            key,
                            b"Raw profile type iptc"
                                | b"Raw profile type exif"
                                | b"Raw profile type APP1"
                                | b"Raw profile type xmp"
                        ) {
                            let text = if &h[4..] == b"zTXt" {
                                if bytes.get(zero + 1) != Some(&0) {
                                    return Err(encode_error("invalid native profile compression"));
                                }
                                inflate(&bytes[zero + 2..])?
                            } else {
                                bytes[zero + 1..].to_vec()
                            };
                            let data = raw_profile(&text)?;
                            match key {
                                b"Raw profile type iptc" => {
                                    if data.starts_with(b"8BIM") {
                                        result.photoshop(&data)?;
                                    } else {
                                        result.iptc(&data)?;
                                    }
                                }
                                b"Raw profile type xmp" => result.set_xmp(&data)?,
                                _ => {
                                    result.tiff(data.strip_prefix(b"Exif\0\0").unwrap_or(&data))?
                                }
                            }
                        }
                    } else {
                        return Err(encode_error("invalid PNG text keyword"));
                    }
                }
                if &h[4..] == b"iTXt" {
                    let bytes = r.at(at + 8, size)?;
                    if let Some(mut text) = bytes.strip_prefix(b"XML:com.adobe.xmp\0") {
                        if text.len() < 2 || text[1] != 0 {
                            return Err(encode_error("invalid PNG XMP"));
                        }
                        let compressed = text[0];
                        text = &text[2..];
                        for _ in 0..2 {
                            let n = text
                                .iter()
                                .position(|b| *b == 0)
                                .ok_or_else(|| encode_error("invalid PNG XMP"))?;
                            text = &text[n + 1..];
                        }
                        if compressed == 0 {
                            result.set_xmp(text)?;
                        } else if compressed == 1 {
                            let mut decoded = Vec::new();
                            flate2::read::ZlibDecoder::new(text)
                                .take((LIMIT + 1) as u64)
                                .read_to_end(&mut decoded)
                                .map_err(encode_error)?;
                            result.set_xmp(&decoded)?;
                        } else {
                            return Err(encode_error("invalid PNG XMP compression"));
                        }
                    }
                }
                at = end;
                if &h[4..] == b"IEND" {
                    break;
                }
            }
        } else if head.get(4..8) == Some(b"JXL ") || head.get(4..8) == Some(b"ftyp") {
            result.bmff(&mut r, cancel)?;
        }
        cancel.check()?;
        Ok(result)
    }
    fn set_xmp(&mut self, bytes: &[u8]) -> EngineResult<()> {
        if bytes.len() > 1024 * 1024 || self.xmp.is_some() {
            return Err(encode_error("duplicate or oversized source XMP"));
        }
        self.charge(bytes.len())?;
        self.xmp = Some(XmpPacket::parse(
            String::from_utf8(bytes.to_vec()).map_err(encode_error)?,
        )?);
        Ok(())
    }
    fn tiff(&mut self, bytes: &[u8]) -> EngineResult<()> {
        let mut io = std::io::Cursor::new(bytes);
        let mut r = Reader {
            io: &mut io,
            len: bytes.len() as u64,
            budget: LIMIT,
        };
        self.tiff_reader(&mut r, 0, bytes.len() as u64)
    }
    fn tiff_reader<R: Read + Seek>(
        &mut self,
        r: &mut Reader<'_, R>,
        base: u64,
        len: u64,
    ) -> EngineResult<()> {
        let h = r.at(base, 8)?;
        let le = &h[..2] == b"II";
        if !le && &h[..2] != b"MM" {
            return Err(encode_error("invalid EXIF byte order"));
        }
        if !matches!(u16n(&h[2..], le), 42 | 0x4f52 | 0x55) {
            return Err(encode_error(
                "unsupported native TIFF header (BigTIFF is not supported)",
            ));
        }
        let mut todo = vec![(u32n(&h[4..], le) as u64, 0)];
        let mut visited = BTreeSet::new();
        while let Some((offset, group)) = todo.pop() {
            if !visited.insert(offset) || visited.len() > 4 || offset + 2 > len {
                return Err(encode_error("cyclic or invalid EXIF directory"));
            }
            let count = u16n(&r.at(base + offset, 2)?, le) as usize;
            if count > 1024 || offset + 2 + count as u64 * 12 + 4 > len {
                return Err(encode_error("EXIF directory exceeds bounds"));
            }
            let entries = r.at(base + offset + 2, count * 12)?;
            let mut ids = BTreeSet::new();
            for e in entries.as_chunks::<12>().0 {
                let id = u16n(e, le);
                let kind = u16n(&e[2..], le);
                let count = u32n(&e[4..], le);
                if !ids.insert(id) {
                    return Err(encode_error("duplicate EXIF tag"));
                }
                if matches!(id, 34665 | 34853 | 40965)
                    && !matches!((group, id), (0, 34665 | 34853) | (1, 40965))
                {
                    return Err(encode_error("EXIF directory pointer in unexpected IFD"));
                }
                if matches!((group, id), (0, 34665 | 34853) | (1, 40965)) {
                    if kind != 4 || count != 1 {
                        return Err(encode_error("invalid EXIF directory pointer"));
                    }
                    todo.push((
                        u32n(&e[8..], le) as u64,
                        match id {
                            34665 => 1,
                            34853 => 2,
                            _ => 3,
                        },
                    ));
                    continue;
                }
                // Only descriptive IFD0 fields. All sample/layout/DNG/preview
                // offsets remain owned by the destination codec.
                let keep = match group {
                    0 => matches!(
                        id,
                        270 | 271 | 272 | 305 | 306 | 315 | 33432 | 33723 | 34377 | 700 | 40091
                            ..=40095
                    ),
                    1 => !matches!(id,37500|40961..=40964|41728..=41730),
                    2 => true,
                    3 => matches!(id, 1 | 2),
                    _ => false,
                };
                if !keep {
                    continue;
                }
                let size = width(kind)?
                    .checked_mul(count as usize)
                    .ok_or_else(|| encode_error("EXIF count overflow"))?;
                if size > LIMIT {
                    return Err(encode_error("EXIF field exceeds budget"));
                }
                let mut data = if size <= 4 {
                    e[8..8 + size].to_vec()
                } else {
                    let p = u32n(&e[8..], le) as u64;
                    if p.checked_add(size as u64).is_none_or(|v| v > len) {
                        return Err(encode_error("EXIF payload out of range"));
                    }
                    r.at(base + p, size)?
                };
                if group == 0 && id == 700 {
                    self.set_xmp(&data)?;
                    continue;
                }
                if group == 0 && id == 33723 {
                    self.iptc(&data)?;
                    continue;
                }
                if group == 0 && id == 34377 {
                    self.photoshop(&data)?;
                    continue;
                }
                if !le && width(kind)? > 1 {
                    let unit = if matches!(kind, 5 | 10) {
                        4
                    } else {
                        width(kind)?
                    };
                    for n in data.chunks_exact_mut(unit) {
                        n.reverse();
                    }
                }
                self.charge(data.len() + 16)?;
                let tag = Tag {
                    id,
                    kind,
                    count,
                    data,
                };
                match group {
                    0 => &mut self.root,
                    1 => &mut self.exif,
                    2 => &mut self.gps,
                    _ => &mut self.interop,
                }
                .push(tag);
            }
        }
        Ok(())
    }
    fn iptc(&mut self, mut b: &[u8]) -> EngineResult<()> {
        while !b.is_empty() && b.iter().any(|v| *v != 0) {
            if b.len() < 5 || b[0] != 0x1c {
                return Err(encode_error("invalid IPTC dataset"));
            }
            let (record, dataset) = (b[1], b[2]);
            let n = u16n(&b[3..], false);
            b = &b[5..];
            let len = if n & 0x8000 == 0 {
                n as usize
            } else {
                let size = (n & 0x7fff) as usize;
                if !(1..=4).contains(&size) || b.len() < size {
                    return Err(encode_error("invalid IPTC extended length"));
                }
                let len = b[..size].iter().fold(0usize, |v, b| v * 256 + *b as usize);
                b = &b[size..];
                len
            };
            if len > b.len() || self.iim.len() >= 4096 {
                return Err(encode_error("IPTC dataset exceeds bounds"));
            }
            self.charge(len + 8)?;
            self.iim.push((record, dataset, b[..len].to_vec()));
            b = &b[len..];
        }
        Ok(())
    }
    fn photoshop(&mut self, mut b: &[u8]) -> EngineResult<()> {
        while !b.is_empty() {
            if b.len() < 7 || &b[..4] != b"8BIM" {
                return Err(encode_error("invalid Photoshop resource"));
            }
            let id = u16n(&b[4..], false);
            let name = (1 + b[6] as usize + 1) & !1;
            if b.len() < 6 + name + 4 {
                return Err(encode_error("truncated Photoshop resource"));
            }
            let n = u32n(&b[6 + name..], false) as usize;
            b = &b[10 + name..];
            if n > b.len() {
                return Err(encode_error("Photoshop resource out of bounds"));
            }
            if id == 0x404 {
                self.iptc(&b[..n])?;
            }
            let padded = n + (n & 1);
            if padded > b.len() {
                return Err(encode_error("missing Photoshop padding"));
            }
            b = &b[padded..];
        }
        Ok(())
    }
    fn utf8_iptc(&self) -> EngineResult<bool> {
        let charset = self
            .iim
            .iter()
            .find(|(r, d, _)| *r == 1 && *d == 90)
            .map(|(_, _, v)| v.as_slice());
        match charset {
            None | Some(b"\x1b-A") => Ok(false),
            Some(b"\x1b%G") => Ok(true),
            _ => Err(encode_error("unsupported IPTC coded character set")),
        }
    }
    pub fn keywords(&self) -> EngineResult<Vec<String>> {
        let utf8 = self.utf8_iptc()?;
        self.iim
            .iter()
            .filter(|(r, d, _)| *r == 2 && *d == 25)
            .map(|(_, _, v)| iptc_text(v, utf8))
            .collect()
    }
    pub fn filter(&mut self, s: &ExportSettings, packet: Option<&XmpPacket>) -> EngineResult<()> {
        if matches!(s.metadata, Metadata::None) {
            *self = Self::default();
            return Ok(());
        }
        let general = matches!(s.metadata, Metadata::All | Metadata::AllExceptCamera);
        self.root.retain(|t| match s.metadata {
            Metadata::All => true,
            Metadata::AllExceptCamera => matches!(t.id, 270 | 315 | 33432 | 40091..=40095),
            Metadata::CopyrightOnly => t.id == 33432,
            Metadata::CopyrightAndContact => matches!(t.id, 315 | 33432),
            Metadata::None => false,
        });
        if !matches!(s.metadata, Metadata::All) {
            self.exif.clear();
            self.gps.clear();
            self.interop.clear();
        }
        if s.remove_location {
            self.gps.clear();
        }
        if s.remove_person_info {
            // XPKeywords/XPSubject and UserComment are unstructured, without a
            // reliable person taxonomy. Remove rather than leak opaque names.
            self.root.retain(|t| !matches!(t.id, 40091..=40095));
            self.exif.retain(|t| t.id != 37510);
        }
        let allowed = packet
            .map(XmpPacket::metadata)
            .transpose()?
            .map(|m| m.keywords)
            .unwrap_or_default();
        let utf8 = self.utf8_iptc()?;
        let keyword_texts = self
            .iim
            .iter()
            .filter(|(r, d, _)| *r == 2 && *d == 25)
            .map(|(_, _, v)| Ok((v.clone(), iptc_text(v, utf8)?)))
            .collect::<EngineResult<std::collections::BTreeMap<_, _>>>()?;
        self.iim.retain(|(r, d, v)| {
            if *r == 1 {
                return *d == 90;
            }
            if *r != 2 {
                return general && !s.remove_person_info && !s.remove_location;
            }
            let policy = general
                || matches!(*d, 110 | 115 | 116)
                || (matches!(s.metadata, Metadata::CopyrightAndContact)
                    && matches!(*d, 80 | 85 | 118));
            let location = matches!(*d, 26 | 27 | 90 | 92 | 95 | 100 | 101);
            policy
                && !(s.remove_location && location)
                // XMP already contains source-native keywords followed by
                // sidecar edits and privacy filtering. Do not preserve a stale
                // native keyword that an explicit sidecar property removed.
                && !(*d == 25
                    && !allowed.iter().any(|k| {
                        keyword_texts
                            .get(v)
                            .is_some_and(|text| k.to_lowercase() == text.to_lowercase())
                    }))
        });
        self.xmp = None;
        Ok(())
    }
    pub fn iim_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        for (r, d, v) in &self.iim {
            b.extend([0x1c, *r, *d]);
            if v.len() < 0x8000 {
                b.extend((v.len() as u16).to_be_bytes());
            } else {
                b.extend(0x8004u16.to_be_bytes());
                b.extend((v.len() as u32).to_be_bytes());
            }
            b.extend(v);
        }
        b
    }
    pub fn tiff_bytes(&self, include_iim: bool) -> EngineResult<Vec<u8>> {
        let mut b = b"II\x2a\0\0\0\0\0".to_vec();
        let tags = self.tags(&mut b, include_iim)?;
        if tags.is_empty() {
            return Ok(Vec::new());
        }
        let offset = directory(&mut b, tags)?;
        b[4..8].copy_from_slice(&offset.to_le_bytes());
        Ok(b)
    }
    pub fn tags(&self, b: &mut Vec<u8>, include_iim: bool) -> EngineResult<Vec<Tag>> {
        let mut root = self.root.clone();
        let mut exif = self.exif.clone();
        if !self.interop.is_empty() {
            let p = directory(b, self.interop.clone())?;
            exif.push(pointer(40965, p));
        }
        if !exif.is_empty() {
            let p = directory(b, exif)?;
            root.push(pointer(34665, p));
        }
        if !self.gps.is_empty() {
            let p = directory(b, self.gps.clone())?;
            root.push(pointer(34853, p));
        }
        if include_iim && !self.iim.is_empty() {
            let data = self.iim_bytes();
            root.push(Tag {
                id: 33723,
                kind: 7,
                count: data.len() as u32,
                data,
            });
        }
        Ok(root)
    }
}
fn pointer(id: u16, p: u32) -> Tag {
    Tag {
        id,
        kind: 4,
        count: 1,
        data: p.to_le_bytes().to_vec(),
    }
}
pub(crate) fn directory(b: &mut Vec<u8>, mut tags: Vec<Tag>) -> EngineResult<u32> {
    tags.sort_by_key(|t| t.id);
    if tags.len() > 1024 || tags.windows(2).any(|w| w[0].id == w[1].id) {
        return Err(encode_error("duplicate or oversized output EXIF directory"));
    }
    if !b.len().is_multiple_of(2) {
        b.push(0);
    }
    let offset = u32::try_from(b.len()).map_err(encode_error)?;
    b.extend((tags.len() as u16).to_le_bytes());
    let start = b.len();
    b.resize(start + tags.len() * 12 + 4, 0);
    for (i, t) in tags.iter().enumerate() {
        let p = start + i * 12;
        b[p..p + 2].copy_from_slice(&t.id.to_le_bytes());
        b[p + 2..p + 4].copy_from_slice(&t.kind.to_le_bytes());
        b[p + 4..p + 8].copy_from_slice(&t.count.to_le_bytes());
        if t.data.len() <= 4 {
            b[p + 8..p + 8 + t.data.len()].copy_from_slice(&t.data);
        } else {
            if !b.len().is_multiple_of(2) {
                b.push(0);
            }
            let off = u32::try_from(b.len()).map_err(encode_error)?;
            b[p + 8..p + 12].copy_from_slice(&off.to_le_bytes());
            b.extend(&t.data);
        }
    }
    Ok(offset)
}

impl Native {
    fn bmff<R: Read + Seek>(
        &mut self,
        r: &mut Reader<'_, R>,
        cancel: &CancellationToken,
    ) -> EngineResult<()> {
        let mut at = 0;
        while at < r.len {
            cancel.check()?;
            let (kind, start, end) = box_header(r, at, r.len)?;
            match &kind {
                b"Exif" => {
                    let b = r.at(start, (end - start) as usize)?;
                    self.exif_box(&b)?;
                }
                b"xml " => {
                    self.set_xmp(&r.at(start, (end - start) as usize)?)?;
                }
                b"brob" => return Err(encode_error("compressed JXL metadata is not supported")),
                b"meta" => self.heif(r, start + 4, end)?,
                _ => {}
            }
            at = end;
        }
        Ok(())
    }
    fn exif_box(&mut self, b: &[u8]) -> EngineResult<()> {
        if b.len() < 4 {
            return Err(encode_error("truncated Exif box"));
        }
        let offset = 4 + u32n(b, false) as usize;
        let bytes = b
            .get(offset..)
            .ok_or_else(|| encode_error("Exif box offset out of bounds"))?;
        self.tiff(bytes)
    }
    fn heif<R: Read + Seek>(
        &mut self,
        r: &mut Reader<'_, R>,
        mut at: u64,
        end: u64,
    ) -> EngineResult<()> {
        let mut types = std::collections::BTreeMap::new();
        let mut locations = None;
        let mut idat = None;
        let mut primary = None;
        let mut descriptions = Vec::new();
        while at < end {
            let (kind, start, stop) = box_header(r, at, end)?;
            match &kind {
                b"pitm" => {
                    let bytes = r.at(start, (stop - start) as usize)?;
                    let mut c = Cursor { b: &bytes, p: 0 };
                    let version = c.n(1)?;
                    c.n(3)?;
                    if version > 1 || primary.is_some() {
                        return Err(encode_error("invalid HEIF primary item"));
                    }
                    primary = Some(c.n(if version == 0 { 2 } else { 4 })? as u32);
                }
                b"iref" => {
                    let h = r.at(start, 4)?;
                    if h[0] > 1 {
                        return Err(encode_error("unsupported HEIF reference version"));
                    }
                    let width = if h[0] == 0 { 2 } else { 4 };
                    let mut p = start + 4;
                    while p < stop {
                        let (kind, s, e) = box_header(r, p, stop)?;
                        if &kind == b"cdsc" {
                            let bytes = r.at(s, (e - s) as usize)?;
                            let mut c = Cursor { b: &bytes, p: 0 };
                            let from = c.n(width)? as u32;
                            let count = c.n(2)?;
                            for _ in 0..count {
                                descriptions.push((from, c.n(width)? as u32));
                            }
                        }
                        p = e;
                    }
                }
                b"iinf" => {
                    let header = r.at(start, 6)?;
                    let skip = if header[0] == 0 { 6 } else { 8 };
                    let mut p = start + skip;
                    while p < stop {
                        let (kind, s, e) = box_header(r, p, stop)?;
                        if &kind == b"infe" {
                            let b = r.at(s, (e - s) as usize)?;
                            if b.len() < 12 {
                                return Err(encode_error("truncated HEIF item info"));
                            }
                            let (id, k) = match b[0] {
                                2 => (u16n(&b[4..], false) as u32, 8),
                                3 => (u32n(&b[4..], false), 10),
                                _ => {
                                    p = e;
                                    continue;
                                }
                            };
                            let item = b
                                .get(k..k + 4)
                                .ok_or_else(|| encode_error("truncated HEIF item type"))?;
                            if item == b"Exif" {
                                types.insert(id, true);
                            } else if item == b"mime"
                                && b[k + 4..]
                                    .windows(20)
                                    .any(|w| w == b"application/rdf+xml\0")
                            {
                                types.insert(id, false);
                            }
                        }
                        p = e;
                    }
                }
                b"iloc" => {
                    if locations.is_some() {
                        return Err(encode_error("duplicate HEIF locations"));
                    }
                    locations = Some(r.at(start, (stop - start) as usize)?);
                }
                b"idat" => {
                    idat = Some((start, stop));
                }
                _ => {}
            }
            at = stop;
        }
        if types.is_empty() {
            return Ok(());
        }
        let primary = primary.ok_or_else(|| encode_error("HEIF metadata without primary item"))?;
        types.retain(|id, _| {
            descriptions
                .iter()
                .any(|&(from, to)| from == *id && to == primary)
        });
        if types.is_empty() {
            return Ok(());
        }
        let bytes = locations.ok_or_else(|| encode_error("HEIF metadata lacks locations"))?;
        let mut c = Cursor { b: &bytes, p: 0 };
        let version = c.n(1)?;
        c.n(3)?;
        if version > 2 {
            return Err(encode_error("unsupported HEIF location version"));
        }
        let sizes = c.n(1)?;
        let sizes2 = c.n(1)?;
        let (os, ls, bs, is) = (
            (sizes >> 4) as usize,
            (sizes & 15) as usize,
            (sizes2 >> 4) as usize,
            if version == 0 {
                0
            } else {
                (sizes2 & 15) as usize
            },
        );
        if [os, ls, bs, is].iter().any(|n| *n > 8) {
            return Err(encode_error("invalid HEIF extent widths"));
        }
        let count = c.n(if version < 2 { 2 } else { 4 })?;
        if count > 4096 {
            return Err(encode_error("too many HEIF items"));
        }
        for _ in 0..count {
            let id = c.n(if version < 2 { 2 } else { 4 })? as u32;
            let method = if version > 0 { c.n(2)? & 15 } else { 0 };
            let reference = c.n(2)?;
            let base = c.n(bs)?;
            let extents = c.n(2)?;
            if extents > 4096 {
                return Err(encode_error("too many HEIF extents"));
            }
            let mut payload = Vec::new();
            for _ in 0..extents {
                if version > 0 && is > 0 {
                    c.n(is)?;
                }
                let offset = c.n(os)?;
                let len = c.n(ls)?;
                if types.contains_key(&id) {
                    if reference != 0 || method > 1 || len == 0 {
                        return Err(encode_error("unsupported HEIF metadata extent"));
                    }
                    let (origin, limit) = if method == 1 {
                        idat.ok_or_else(|| encode_error("missing HEIF idat"))?
                    } else {
                        (0, r.len)
                    };
                    let pos = origin
                        .checked_add(base)
                        .and_then(|v| v.checked_add(offset))
                        .ok_or_else(|| encode_error("HEIF offset overflow"))?;
                    if len > LIMIT as u64 || pos.checked_add(len).is_none_or(|v| v > limit) {
                        return Err(encode_error("HEIF metadata exceeds bounds"));
                    }
                    payload.extend(r.at(pos, len as usize)?);
                }
            }
            if let Some(exif) = types.remove(&id) {
                if exif {
                    self.exif_box(&payload)?;
                } else {
                    self.set_xmp(&payload)?;
                }
            }
        }
        if !types.is_empty() {
            return Err(encode_error("missing HEIF metadata extent"));
        }
        Ok(())
    }
}
struct Cursor<'a> {
    b: &'a [u8],
    p: usize,
}
impl Cursor<'_> {
    fn n(&mut self, n: usize) -> EngineResult<u64> {
        let b = self
            .b
            .get(self.p..self.p + n)
            .ok_or_else(|| encode_error("truncated metadata field"))?;
        self.p += n;
        Ok(b.iter().fold(0, |v, b| (v << 8) | *b as u64))
    }
}
fn box_header<R: Read + Seek>(
    r: &mut Reader<'_, R>,
    at: u64,
    end: u64,
) -> EngineResult<([u8; 4], u64, u64)> {
    let b = r.at(at, 8)?;
    let size = u32n(&b, false) as u64;
    let (size, header) = match size {
        0 => (end - at, 8),
        1 => (u64::from_be_bytes(r.at(at + 8, 8)?.try_into().unwrap()), 16),
        _ => (size, 8),
    };
    let stop = at
        .checked_add(size)
        .ok_or_else(|| encode_error("box size overflow"))?;
    if size < header || stop > end {
        return Err(encode_error("box exceeds container"));
    }
    Ok((b[4..8].try_into().unwrap(), at + header, stop))
}

pub(crate) fn append_tiff(file: &mut File, native: &Native) -> EngineResult<()> {
    use std::io::Write;
    let mut blob = native.tiff_bytes(true)?;
    if blob.is_empty() {
        return Ok(());
    }
    let len = file.metadata().map_err(encode_error)?.len();
    let base = u32::try_from(len + (len & 1)).map_err(encode_error)?;
    let root = u32n(&blob[4..], true) as usize;
    let mut todo = vec![root];
    while let Some(p) = todo.pop() {
        let n = u16n(&blob[p..], true) as usize;
        for i in 0..n {
            let at = p + 2 + 12 * i;
            let id = u16n(&blob[at..], true);
            let kind = u16n(&blob[at + 2..], true);
            let count = u32n(&blob[at + 4..], true) as usize;
            if matches!(id, 34665 | 34853 | 40965) {
                todo.push(u32n(&blob[at + 8..], true) as usize);
            }
            if count * width(kind)? > 4 || matches!(id, 34665 | 34853 | 40965) {
                let v = u32n(&blob[at + 8..], true)
                    .checked_add(base)
                    .ok_or_else(|| encode_error("TIFF offset overflow"))?;
                blob[at + 8..at + 12].copy_from_slice(&v.to_le_bytes());
            }
        }
    }
    let count = u16n(&blob[root..], true) as usize;
    let mut added = blob[root + 2..root + 2 + count * 12]
        .as_chunks::<12>()
        .0
        .to_vec();
    file.rewind().map_err(encode_error)?;
    let mut h = [0; 8];
    file.read_exact(&mut h).map_err(encode_error)?;
    if &h[..4] != b"II\x2a\0" {
        return Err(encode_error("expected generated little-endian TIFF"));
    }
    file.seek(SeekFrom::Start(u32n(&h[4..], true) as u64))
        .map_err(encode_error)?;
    let mut n = [0; 2];
    file.read_exact(&mut n).map_err(encode_error)?;
    let n = u16n(&n, true) as usize;
    if n > 1024 {
        return Err(encode_error("generated TIFF directory too large"));
    }
    let mut entries = vec![[0; 12]; n];
    for e in &mut entries {
        file.read_exact(e).map_err(encode_error)?;
    }
    let mut next = [0; 4];
    file.read_exact(&mut next).map_err(encode_error)?;
    entries.retain(|e| !added.iter().any(|a| a[..2] == e[..2]));
    entries.append(&mut added);
    entries.sort_by_key(|e| u16n(e, true));
    file.seek(SeekFrom::End(0)).map_err(encode_error)?;
    if len & 1 != 0 {
        file.write_all(&[0]).map_err(encode_error)?;
    }
    file.write_all(&blob).map_err(encode_error)?;
    let pos = file.stream_position().map_err(encode_error)?;
    if pos & 1 != 0 {
        file.write_all(&[0]).map_err(encode_error)?;
    }
    let offset =
        u32::try_from(file.stream_position().map_err(encode_error)?).map_err(encode_error)?;
    file.write_all(&(entries.len() as u16).to_le_bytes())
        .map_err(encode_error)?;
    for e in entries {
        file.write_all(&e).map_err(encode_error)?;
    }
    file.write_all(&next).map_err(encode_error)?;
    file.seek(SeekFrom::Start(4)).map_err(encode_error)?;
    file.write_all(&offset.to_le_bytes())
        .map_err(encode_error)?;
    Ok(())
}

pub(crate) fn jpeg_segments(native: &Native) -> EngineResult<Vec<u8>> {
    let mut bytes = Vec::new();
    let exif = native.tiff_bytes(false)?;
    let mut segment = |marker: u8, payload: &[u8]| -> EngineResult<()> {
        let len = u16::try_from(payload.len() + 2)
            .map_err(|_| encode_error("native JPEG metadata exceeds APP segment capacity"))?;
        bytes.extend([255, marker]);
        bytes.extend(len.to_be_bytes());
        bytes.extend(payload);
        Ok(())
    };
    if !exif.is_empty() {
        segment(0xe1, &[b"Exif\0\0".as_slice(), &exif].concat())?;
    }
    let iim = native.iim_bytes();
    if !iim.is_empty() {
        let mut p = b"Photoshop 3.0\08BIM\x04\x04\0\0".to_vec();
        p.extend((iim.len() as u32).to_be_bytes());
        p.extend(&iim);
        if !iim.len().is_multiple_of(2) {
            p.push(0);
        }
        segment(0xed, &p)?;
    }
    Ok(bytes)
}

fn iptc_text(bytes: &[u8], utf8: bool) -> EngineResult<String> {
    if utf8 {
        String::from_utf8(bytes.to_vec()).map_err(encode_error)
    } else {
        Ok(bytes.iter().map(|&b| char::from(b)).collect())
    }
}

fn inflate(bytes: &[u8]) -> EngineResult<Vec<u8>> {
    let mut result = Vec::new();
    flate2::read::ZlibDecoder::new(bytes)
        .take((LIMIT + 1) as u64)
        .read_to_end(&mut result)
        .map_err(encode_error)?;
    if result.len() > LIMIT {
        return Err(encode_error("native profile exceeds decompression budget"));
    }
    Ok(result)
}
fn raw_profile(text: &[u8]) -> EngineResult<Vec<u8>> {
    let text = std::str::from_utf8(text).map_err(encode_error)?;
    let mut lines = text.lines();
    if lines.next() != Some("") || lines.next().is_none() {
        return Err(encode_error("invalid PNG raw profile header"));
    }
    let len: usize = lines
        .next()
        .ok_or_else(|| encode_error("missing PNG profile length"))?
        .trim()
        .parse()
        .map_err(encode_error)?;
    if len > LIMIT {
        return Err(encode_error("PNG native profile too large"));
    }
    let hex: Vec<_> = lines
        .flat_map(str::bytes)
        .filter(|b| !b.is_ascii_whitespace())
        .collect();
    if hex.len() != len * 2 {
        return Err(encode_error("PNG profile length mismatch"));
    }
    hex.as_chunks::<2>()
        .0
        .iter()
        .map(|pair| {
            let digit = |b: u8| {
                (b as char)
                    .to_digit(16)
                    .ok_or_else(|| encode_error("invalid PNG profile hex"))
            };
            Ok((digit(pair[0])? * 16 + digit(pair[1])?) as u8)
        })
        .collect()
}
