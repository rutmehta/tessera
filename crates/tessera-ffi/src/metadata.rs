//! Keywords (tree in library.json, tags in XMP sidecars) and the metadata
//! panel: flattened EXIF from the catalog plus editable IPTC/Dublin Core
//! fields written to XMP. The recipe stays authoritative for selection; XMP
//! owned-field replacement preserves foreign properties byte for byte.
use crate::{Engine, Result, catalog, collections::LibraryStore, failure, parse_id};
use engine_api::id::ImageId;
use rusqlite::OptionalExtension;
use sidecar::{MarkPreset, Sidecar, XmpPacket};
use std::{collections::BTreeSet, path::PathBuf};

/// One keyword row, in tree preorder. Keywords found only in sidecars (not in
/// the library's tree) follow as roots with `in_tree == false`.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct KeywordInfo {
    pub name: String,
    pub parent: Option<String>,
    pub depth: u32,
    /// Images in the requested folder tagged with exactly this keyword.
    pub count: u32,
    pub in_tree: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct MetadataField {
    pub group: String,
    pub name: String,
    pub value: String,
}

/// IPTC core as stored in XMP (Dublin Core): x-default language alternatives.
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct ImageMetadata {
    pub image_id: String,
    pub title: String,
    pub caption: String,
    pub copyright: String,
    /// dc:creator entries joined with "; ".
    pub creator: String,
    /// Flat keywords (dc:subject).
    pub keywords: Vec<String>,
    /// lr:hierarchicalSubject paths ("Places|France|Paris").
    pub hierarchical_keywords: Vec<String>,
    /// Read-only facts: file, camera and EXIF, in display order.
    pub fields: Vec<MetadataField>,
}

/// None leaves a field unchanged (mixed values in a multi-selection).
#[derive(Clone, Debug, Default, PartialEq, Eq, uniffi::Record)]
pub struct IptcEdit {
    pub title: Option<String>,
    pub caption: Option<String>,
    pub copyright: Option<String>,
    /// Creators separated by ';'.
    pub creator: Option<String>,
    /// Replaces the keyword list (hierarchy paths come from the library tree).
    pub keywords: Option<Vec<String>>,
}

/// EXIF tags shown first, in this order; the rest follow alphabetically.
const EXIF_ORDER: &[&str] = &[
    "Make",
    "Model",
    "LensModel",
    "DateTimeOriginal",
    "ExposureTime",
    "FNumber",
    "PhotographicSensitivity",
    "FocalLength",
    "FocalLengthIn35mmFilm",
    "ExposureBiasValue",
    "ExposureProgram",
    "MeteringMode",
    "Flash",
    "WhiteBalance",
    "PixelXDimension",
    "PixelYDimension",
    "Software",
    "Artist",
    "Copyright",
];

fn clean(s: &str) -> String {
    s.trim().to_owned()
}

impl LibraryStore {
    /// Applies `edit` to each image's XMP metadata and rescans the touched
    /// folders so search and facets see the change.
    fn edit_xmp(
        &self,
        image_ids: &[String],
        mut edit: impl FnMut(&mut sidecar::Metadata) -> Result<()>,
    ) -> Result<()> {
        let ids: Vec<ImageId> = image_ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<_>>()?;
        let pairs = self.read()?.keyword_pairs();
        let mut c = self.engine.lock()?;
        let mut folders = BTreeSet::new();
        for (id, key) in ids.iter().zip(image_ids) {
            let path = PathBuf::from(Engine::path(&c, key)?);
            let doc = catalog::document(&path, *id)?;
            let xmp = catalog::xmp_path(&path);
            let packet = if xmp.exists() {
                Sidecar::read_xmp(&xmp)?
            } else {
                XmpPacket::from_selection(&doc.recipe.selection, &MarkPreset::default())
            };
            let mut meta = packet.metadata()?;
            edit(&mut meta)?;
            let updated =
                packet.with_metadata(&doc.recipe.selection, &meta, &MarkPreset::default())?;
            Sidecar::write_xmp(&xmp, &updated)?;
            if let Some(parent) = path.parent() {
                folders.insert(parent.to_path_buf());
            }
        }
        for folder in folders {
            c.index
                .scan(&folder, &catalog::Sidecars, &catalog::EmbeddedMetadata)?;
        }
        c.index.sync_keyword_tree(&pairs)?;
        Ok(())
    }

    /// Hierarchical path for a keyword ("A|B|leaf"), or the bare name.
    fn path_of(library: &library::Library, name: &str) -> String {
        library
            .keyword_path(name)
            .map(|p| p.join("|"))
            .unwrap_or_else(|| name.to_owned())
    }

    fn sync_keywords(&self, library: &library::Library) -> Result<()> {
        self.engine
            .lock()?
            .index
            .sync_keyword_tree(&library.keyword_pairs())?;
        Ok(())
    }
}

#[uniffi::export]
impl LibraryStore {
    /// Keyword tree with per-keyword counts under `folder` (None: whole catalog).
    pub fn keywords(&self, folder: Option<String>) -> Result<Vec<KeywordInfo>> {
        let library = self.read()?;
        let counts = self
            .engine
            .lock()?
            .index
            .facets(&index::Query {
                folder,
                limit: i64::MAX as usize,
                ..Default::default()
            })?
            .keywords;
        let count = |name: &str| {
            counts
                .iter()
                .find(|(n, _)| n == name)
                .map_or(0, |(_, c)| (*c).min(u32::MAX as u64) as u32)
        };
        let mut out: Vec<KeywordInfo> = library
            .keyword_pairs()
            .into_iter()
            .map(|(name, parent)| KeywordInfo {
                depth: library
                    .keyword_path(&name)
                    .map_or(0, |p| p.len().saturating_sub(1) as u32),
                count: count(&name),
                name,
                parent,
                in_tree: true,
            })
            .collect();
        let known: BTreeSet<String> = out.iter().map(|k| k.name.clone()).collect();
        let mut loose: Vec<_> = counts
            .iter()
            .filter(|(name, n)| *n > 0 && !known.contains(name))
            .map(|(name, n)| KeywordInfo {
                name: name.clone(),
                parent: None,
                depth: 0,
                count: (*n).min(u32::MAX as u64) as u32,
                in_tree: false,
            })
            .collect();
        loose.sort_by_key(|k| k.name.to_lowercase());
        out.extend(loose);
        Ok(out)
    }

    pub fn add_keyword(&self, name: String, parent: Option<String>) -> Result<()> {
        let library = self.edit(|l| {
            l.add_keyword(&name, parent.as_deref())?;
            Ok(l.clone())
        })?;
        self.sync_keywords(&library)
    }
    /// Moves a keyword (and its children) under `parent`, or to the root.
    /// Existing `lr:hierarchicalSubject` paths in sidecars are not rewritten.
    pub fn move_keyword(&self, name: String, parent: Option<String>) -> Result<()> {
        let library = self.edit(|l| {
            if l.keyword_path(&name).is_none() {
                l.add_keyword(&name, None)?;
            }
            l.move_keyword(&name, parent.as_deref())?;
            Ok(l.clone())
        })?;
        self.sync_keywords(&library)
    }
    /// Safe delete: removes the keyword from the list only; photos keep the tag.
    pub fn delete_keyword(&self, name: String) -> Result<()> {
        let library = self.edit(|l| {
            l.delete_keyword(&name)?;
            Ok(l.clone())
        })?;
        self.sync_keywords(&library)
    }

    /// Adds or removes keywords on every image (bulk apply). Unknown keywords
    /// join the tree at the root. Writes dc:subject (flat) and
    /// lr:hierarchicalSubject (path) to each XMP sidecar.
    pub fn apply_keywords(
        &self,
        image_ids: Vec<String>,
        names: Vec<String>,
        add: bool,
    ) -> Result<()> {
        let names: Vec<String> = names
            .iter()
            .map(|n| clean(n))
            .filter(|n| !n.is_empty())
            .collect();
        if names.is_empty() {
            return Err(failure("no keywords given"));
        }
        let library = if add {
            self.edit(|l| {
                for name in &names {
                    if l.keyword_path(name).is_none() {
                        l.add_keyword(name, None)?;
                    }
                }
                Ok(l.clone())
            })?
        } else {
            self.read()?
        };
        let paths: Vec<String> = names.iter().map(|n| Self::path_of(&library, n)).collect();
        self.edit_xmp(&image_ids, |meta| {
            for (name, path) in names.iter().zip(&paths) {
                if add {
                    if !meta.keywords.contains(name) {
                        meta.keywords.push(name.clone());
                    }
                    if !meta.hierarchical_keywords.contains(path) {
                        meta.hierarchical_keywords.push(path.clone());
                    }
                } else {
                    meta.keywords.retain(|k| k != name);
                    let suffix = format!("|{name}");
                    meta.hierarchical_keywords
                        .retain(|h| h != name && !h.ends_with(&suffix));
                }
            }
            Ok(())
        })
    }

    pub fn metadata(&self, image_id: String) -> Result<ImageMetadata> {
        let id = parse_id(&image_id)?;
        let c = self.engine.lock()?;
        let path = PathBuf::from(Engine::path(&c, &image_id)?);
        let mut out = ImageMetadata {
            image_id: image_id.clone(),
            ..Default::default()
        };
        let xmp = catalog::xmp_path(&path);
        if xmp.exists() {
            let meta = Sidecar::read_xmp(&xmp)?.metadata()?;
            out.title = meta.title;
            out.caption = meta.description;
            out.copyright = meta.copyright;
            out.creator = meta.creators.join("; ");
            out.keywords = meta.keywords;
            out.hierarchical_keywords = meta.hierarchical_keywords;
        }
        let mut push = |group: &str, name: &str, value: String| {
            if !value.trim().is_empty() {
                out.fields.push(MetadataField {
                    group: group.into(),
                    name: name.into(),
                    value,
                });
            }
        };
        push(
            "File",
            "Name",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        push(
            "File",
            "Folder",
            path.parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        if let Ok(md) = std::fs::metadata(&path) {
            let bytes = md.len() as f64;
            push(
                "File",
                "Size",
                if bytes >= 1e6 {
                    format!("{:.1} MB", bytes / 1e6)
                } else {
                    format!("{:.0} KB", (bytes / 1e3).max(1.0))
                },
            );
        }
        let row: Option<(Option<String>, Option<String>, Option<String>)> = c
            .reader
            .query_row(
                "SELECT datetime(capture_time,'auto'),camera,lens FROM image WHERE id=?",
                [&image_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        if let Some((captured, camera, lens)) = row {
            push("Camera", "Captured", captured.unwrap_or_default());
            push("Camera", "Camera", camera.unwrap_or_default());
            push("Camera", "Lens", lens.unwrap_or_default());
        }
        let mut stmt = c
            .reader
            .prepare("SELECT key,value FROM metadata WHERE image_id=?")?;
        let mut exif: Vec<(String, String)> = stmt
            .query_map([id.to_string()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        exif.retain(|(k, v)| !k.starts_with("thumbnail:") && v.len() <= 200);
        let name = |k: &str| k.rsplit_once(':').map_or(k, |(_, n)| n).to_owned();
        exif.sort_by_cached_key(|(k, _)| {
            let n = name(k);
            (
                EXIF_ORDER
                    .iter()
                    .position(|o| *o == n)
                    .unwrap_or(EXIF_ORDER.len()),
                n,
            )
        });
        let mut seen = BTreeSet::new();
        for (key, value) in exif {
            let n = name(&key);
            let n = if n == "orientation" {
                "Orientation".to_owned()
            } else {
                n
            };
            if seen.insert(n.clone()) {
                push("EXIF", &n, value);
            }
        }
        Ok(out)
    }

    /// Writes the given IPTC fields to every image's XMP sidecar.
    pub fn set_iptc(&self, image_ids: Vec<String>, edit: IptcEdit) -> Result<()> {
        let library = self.read()?;
        let keyword_paths = edit.keywords.as_ref().map(|names| {
            let names: Vec<String> = names
                .iter()
                .map(|n| clean(n))
                .filter(|n| !n.is_empty())
                .collect();
            let paths: Vec<String> = names.iter().map(|n| Self::path_of(&library, n)).collect();
            (names, paths)
        });
        self.edit_xmp(&image_ids, |meta| {
            if let Some(v) = &edit.title {
                meta.title = clean(v);
            }
            if let Some(v) = &edit.caption {
                meta.description = v.trim().to_owned();
            }
            if let Some(v) = &edit.copyright {
                meta.copyright = clean(v);
            }
            if let Some(v) = &edit.creator {
                meta.creators = v.split(';').map(clean).filter(|s| !s.is_empty()).collect();
            }
            if let Some((names, paths)) = &keyword_paths {
                meta.keywords = names.clone();
                meta.hierarchical_keywords = paths.clone();
            }
            Ok(())
        })
    }
}
