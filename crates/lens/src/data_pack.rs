//! Explicitly acquired, versioned Lensfun data; never bundled into the engine.
use crate::{ProfileDatabase, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LensDataPackSpec {
    pub version: String,
    pub download_url: String,
    pub sha256: String,
    pub archive_root: String,
    pub attribution: String,
    pub license: String,
}

pub struct LensDataPack {
    spec: LensDataPackSpec,
    cache: PathBuf,
}
#[derive(Debug)]
pub struct LoadedLensDataPack {
    pub spec: LensDataPackSpec,
    pub path: PathBuf,
    pub database: ProfileDatabase,
    /// Source file and reason for each unsupported/uncalibrated lens.
    pub skipped: Vec<String>,
}
impl LensDataPackSpec {
    /// Lensfun v0.3.4 source archive, pinned to the release commit (not a mutable tag).
    /// SHA-256 verified against the upstream bytes; only data/db/*.xml is interpreted.
    pub fn lensfun() -> Self {
        Self {
            version: "0.3.4+101c745e847a5de4a1e569a94368ce2027198598".into(),
            download_url: "https://codeload.github.com/lensfun/lensfun/tar.gz/101c745e847a5de4a1e569a94368ce2027198598".into(),
            sha256: "a11cbe6aeec657839540448b253217c25d20b7a45b6aebfef406f7239933c7a6".into(),
            archive_root: "lensfun-101c745e847a5de4a1e569a94368ce2027198598".into(),
            attribution: "Lensfun contributors, https://github.com/lensfun/lensfun; original PTLens data by Tom Niemann; https://creativecommons.org/licenses/by-sa/3.0/".into(),
            license: "CC-BY-SA-3.0".into(),
        }
    }
}
impl LensDataPack {
    /// Construct without IO. The caller supplies its application-data directory.
    pub fn new(app_dir: impl AsRef<Path>) -> Result<Self> {
        Self::with_spec(app_dir, LensDataPackSpec::lensfun())
    }
    /// Explicit opt-in download on a cache miss. No networking occurs at startup.
    pub fn resolve(&self) -> Result<LoadedLensDataPack> {
        self.resolve_with(|url| {
            let agent = ureq::Agent::config_builder()
                .https_only(true)
                .timeout_global(Some(std::time::Duration::from_secs(120)))
                .build()
                .new_agent();
            let response = agent
                .get(url)
                .call()
                .map_err(|e| invalid(format!("lens pack download: {e}")))?;
            Ok(response.into_body().into_reader())
        })
    }

    pub fn with_spec(app_dir: impl AsRef<Path>, spec: LensDataPackSpec) -> Result<Self> {
        if spec.sha256.len() != 64
            || !spec
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || [&spec.version, &spec.attribution, &spec.license]
                .iter()
                .any(|v| v.trim().is_empty())
            || spec.archive_root.is_empty()
            || !spec
                .archive_root
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            || !spec.download_url.starts_with("https://")
        {
            return Err(crate::Error::Invalid("invalid lens pack manifest".into()));
        }
        Ok(Self {
            spec,
            cache: app_dir.as_ref().join("lens"),
        })
    }
    /// Use a caller-supplied transport only on a cache miss.
    pub fn resolve_with<R: Read>(
        &self,
        fetch: impl FnOnce(&str) -> Result<R>,
    ) -> Result<LoadedLensDataPack> {
        let path = self.cache.join(format!("{}.tar.gz", self.spec.sha256));
        if path.try_exists()? {
            let bytes = bounded_read(fs::File::open(&path)?, MAX_COMPRESSED, "compressed")?;
            let (database, skipped) = self.decode(&bytes)?;
            return Ok(LoadedLensDataPack {
                spec: self.spec.clone(),
                path,
                database,
                skipped,
            });
        }
        fs::create_dir_all(&self.cache)?;
        let bytes = bounded_read(
            fetch(&self.spec.download_url)?,
            MAX_COMPRESSED,
            "compressed",
        )?;
        let (database, skipped) = self.decode(&bytes)?;
        let mut tmp = tempfile::NamedTempFile::new_in(&self.cache)?;
        tmp.write_all(&bytes)?;
        tmp.flush()?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path).map_err(|e| e.error)?;
        Ok(LoadedLensDataPack {
            spec: self.spec.clone(),
            path,
            database,
            skipped,
        })
    }
    fn decode(&self, bytes: &[u8]) -> Result<(ProfileDatabase, Vec<String>)> {
        use sha2::{Digest, Sha256};
        if format!("{:x}", Sha256::digest(bytes)) != self.spec.sha256 {
            return Err(invalid("lens pack SHA-256 mismatch"));
        }
        let expanded = bounded_read(
            flate2::read::MultiGzDecoder::new(bytes),
            MAX_EXPANDED,
            "expanded",
        )?;
        let mut archive = tar::Archive::new(expanded.as_slice());
        let mut database = ProfileDatabase::default();
        let mut skipped = Vec::new();
        let mut paths = std::collections::HashSet::new();
        for (index, entry) in archive.entries()?.enumerate() {
            if index >= 4096 {
                return Err(invalid("too many archive entries"));
            }
            let mut entry = entry?;
            // GitHub git-archive emits a leading global PAX commit comment.
            // It is metadata, never a filesystem object; nothing is unpacked.
            if index == 0 && entry.header().entry_type().is_pax_global_extensions() {
                bounded_read(&mut entry, 64 * 1024, "PAX metadata")?;
                continue;
            }
            let path = entry.path()?;
            let name = path
                .to_str()
                .ok_or_else(|| invalid("non-UTF8 archive path"))?
                .to_owned();
            if name.contains('\\')
                || name.contains(':')
                || name.starts_with('/')
                || name.split('/').any(|c| c == ".." || c == ".")
                || !(name == self.spec.archive_root
                    || name.starts_with(&format!("{}/", self.spec.archive_root)))
                || !paths.insert(name.clone())
            {
                return Err(invalid("unsafe or duplicate archive path"));
            }
            let kind = entry.header().entry_type();
            if kind.is_dir() {
                continue;
            }
            if !kind.is_file() {
                return Err(invalid("archive links/special files are forbidden"));
            }
            let prefix = format!("{}/data/db/", self.spec.archive_root);
            if let Some(relative) = name.strip_prefix(&prefix) {
                if relative.ends_with(".xml") && !relative.contains('/') {
                    let xml = bounded_read(&mut entry, MAX_XML, "XML member")?;
                    let xml = std::str::from_utf8(&xml).map_err(|_| invalid("non-UTF8 XML"))?;
                    load_xml(xml, &name, &mut database, &mut skipped)?;
                }
            }
        }
        if database.profiles.is_empty() {
            return Err(invalid("no supported lens profiles in pack"));
        }
        Ok((database, skipped))
    }
}
const MAX_COMPRESSED: u64 = 16 * 1024 * 1024;
const MAX_EXPANDED: u64 = 64 * 1024 * 1024;
const MAX_XML: u64 = 2 * 1024 * 1024;
fn invalid(message: impl Into<String>) -> crate::Error {
    crate::Error::Invalid(message.into())
}
fn bounded_read(reader: impl Read, max: u64, label: &str) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(invalid(format!("{label} size limit exceeded")));
    }
    Ok(bytes)
}

// Split bounded, well-formed documents into lenses so an unsupported model
// never discards the supported neighbors in the same upstream XML file.
fn load_xml(
    xml: &str,
    source: &str,
    database: &mut ProfileDatabase,
    skipped: &mut Vec<String>,
) -> Result<()> {
    use quick_xml::{events::Event, Reader, Writer};
    use std::collections::HashSet;
    let mut reader = Reader::from_str(xml);
    let mut depth = 0usize;
    let mut roots = 0;
    let mut events = 0;
    let mut lens_events = 0;
    let mut writer: Option<Writer<Vec<u8>>> = None;
    let mut focals = HashSet::new();
    let mut captures = HashSet::new();
    let mut in_type = false;
    let mut unsupported_type = false;
    let mut total_samples: usize = database.profiles.iter().map(|p| p.samples.len()).sum();
    loop {
        let event = reader.read_event().map_err(|e| invalid(e.to_string()))?;
        events += 1;
        if events > 200_000 {
            return Err(invalid("XML event limit exceeded"));
        }
        match &event {
            Event::Start(e) | Event::Empty(e) => {
                if depth == 0 {
                    roots += 1;
                    if roots != 1 || e.name().as_ref() != b"lensdatabase" {
                        return Err(invalid("expected lensdatabase root"));
                    }
                }
                if matches!(&event, Event::Start(_)) {
                    depth += 1;
                }
                if depth > 32 {
                    return Err(invalid("XML depth limit exceeded"));
                }
                if e.name().as_ref() == b"lens" {
                    if depth != 2 || writer.is_some() || matches!(&event, Event::Empty(_)) {
                        return Err(invalid("invalid lens element"));
                    }
                    writer = Some(Writer::new(Vec::new()));
                    focals.clear();
                    captures.clear();
                    lens_events = 0;
                    unsupported_type = false;
                }
                if writer.is_some() {
                    in_type = e.name().as_ref() == b"type";
                    if matches!(e.name().as_ref(), b"distortion" | b"tca" | b"vignetting") {
                        let mut focal = "50".to_string();
                        let mut aperture = "4".to_string();
                        let mut distance = "10".to_string();
                        for attr in e.attributes() {
                            let attr = attr.map_err(|e| invalid(e.to_string()))?;
                            let value = attr
                                .decode_and_unescape_value(reader.decoder())
                                .map_err(|e| invalid(e.to_string()))?
                                .into_owned();
                            match attr.key.as_ref() {
                                b"focal" => focal = value,
                                b"aperture" => aperture = value,
                                b"distance" => distance = value,
                                _ => {}
                            }
                        }
                        focals.insert(focal);
                        if e.name().as_ref() == b"vignetting" {
                            captures.insert((aperture, distance));
                        }
                        if focals.len() * captures.len().max(1) > 8192 {
                            return Err(invalid("calibration grid limit exceeded"));
                        }
                    }
                }
            }
            Event::End(e) => {
                depth = depth
                    .checked_sub(1)
                    .ok_or_else(|| invalid("unbalanced XML"))?;
                if e.name().as_ref() == b"type" {
                    in_type = false;
                }
            }
            Event::Text(t) if in_type => {
                let text = t.decode().map_err(|e| invalid(e.to_string()))?;
                if !text.trim().is_empty() && text.trim() != "rectilinear" {
                    unsupported_type = true;
                }
            }
            Event::DocType(_) => return Err(invalid("DTD not supported")),
            Event::Eof => {
                if roots != 1 || depth != 0 || writer.is_some() {
                    return Err(invalid("truncated XML"));
                }
                break;
            }
            _ => {}
        }
        if let Some(w) = &mut writer {
            lens_events += 1;
            if lens_events > 4096 {
                return Err(invalid("lens event limit exceeded"));
            }
            w.write_event(event.clone())?;
        }
        if matches!(&event, Event::End(e) if e.name().as_ref() == b"lens") {
            let bytes = writer
                .take()
                .ok_or_else(|| invalid("unbalanced lens"))?
                .into_inner();
            let lens = std::str::from_utf8(&bytes).map_err(|_| invalid("non-UTF8 lens"))?;
            let parsed = if unsupported_type {
                Err(invalid("non-rectilinear lens unsupported"))
            } else {
                ProfileDatabase::from_lensfun(lens)
            };
            match parsed {
                Ok(p) => {
                    total_samples += p.profiles.iter().map(|p| p.samples.len()).sum::<usize>();
                    database.profiles.extend(p.profiles);
                }
                Err(crate::Error::Invalid(reason)) => skipped.push(format!(
                    "{source}: lens {}: {reason}",
                    database.profiles.len() + skipped.len() + 1
                )),
                Err(e) => return Err(e),
            }
            if database.profiles.len() + skipped.len() > 10_000 || total_samples > 200_000 {
                return Err(invalid("pack profile/sample limit exceeded"));
            }
        }
    }
    Ok(())
}
