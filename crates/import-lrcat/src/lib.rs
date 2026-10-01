//! Read-only Lightroom catalog translation. No original photos or catalogs are changed.
//! The import plan is explicit: callers decide when and where to persist it.
#[cfg(feature = "fixture")]
pub mod fixture;
pub mod lua;
pub mod lua_develop;
pub mod previews;
mod search_map;
pub mod xmp;
pub use lua::SavedSearch;

use engine_api::{
    error::{EngineError, EngineResult},
    id::{Digest, ImageId},
    recipe::{Decision, Grade, Mark, Recipe, Selection},
};
use rusqlite::{Connection, OpenFlags, types::ValueRef};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
};

/// Largest text or blob cell loaded from the catalog. Larger cells are not
/// copied into memory: the column reads as NULL and the plan report says so.
pub const MAX_CELL_BYTES: usize = 8 << 20;

/// Original source rows retained for data whose schema varies across releases.
pub type SourceRow = BTreeMap<String, Value>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedImage {
    pub catalog_id: i64,
    pub path: PathBuf,
    /// Virtual copies share the original path, but have independent identities/names.
    pub master_image: Option<i64>,
    pub copy_name: Option<String>,
    pub display_name: String,
    pub orientation: Option<String>,
    pub capture_time: Option<String>,
    pub recipe: Recipe,
    pub selection: Selection,
    pub keywords: Vec<i64>,
    pub collections: Vec<i64>,
    pub gps: Option<[f64; 2]>,
    pub faces: Vec<SourceRow>,
    pub history: Vec<SourceRow>,
    pub snapshots: Vec<SourceRow>,
    /// Source selection columns (`Adobe_images.rating`, `pick`, `colorLabels`),
    /// kept so a host can preview how they map before committing.
    #[serde(default)]
    pub rating: Option<i64>,
    #[serde(default)]
    pub pick: Option<i64>,
    #[serde(default)]
    pub color_label: Option<String>,
}

pub use library::{Album, AlbumGroup, Keyword, Library, SmartAlbum};
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Stack {
    pub id: i64,
    pub scope: String,
    pub source: SourceRow,
    pub images: Vec<i64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportPlan {
    pub schema_version: String,
    /// `AgLibraryRootFolder` rows (`id_local`, `absolutePath`, ...), for relocation.
    #[serde(default)]
    pub roots: Vec<SourceRow>,
    pub folders: Vec<SourceRow>,
    pub images: Vec<ImportedImage>,
    pub library: Library,
    pub stacks: Vec<Stack>,
    pub face_clusters: Vec<SourceRow>,
    pub keyword_faces: Vec<SourceRow>,
    /// Unknown keys, unsupported translations, and absent optional tables.
    pub report: Vec<String>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub schema_version: String,
    pub images: usize,
    pub virtual_copies: usize,
    pub folders: usize,
    pub keywords: usize,
    pub albums: usize,
    pub album_groups: usize,
    pub smart_albums: usize,
    pub stacks: usize,
    pub faces: usize,
}
pub(crate) fn decode(message: impl ToString) -> EngineError {
    EngineError::Decode {
        format: "lrcat".into(),
        message: message.to_string(),
    }
}
pub(crate) fn number(row: &SourceRow, key: &str) -> Option<i64> {
    row.get(key).and_then(Value::as_i64)
}
pub(crate) fn text(row: &SourceRow, key: &str) -> Option<String> {
    row.get(key).filter(|v| !v.is_null()).map(|v| {
        v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string())
    })
}
fn required_id(row: &SourceRow, key: &str) -> EngineResult<i64> {
    number(row, key).ok_or_else(|| decode(format!("missing or invalid integer column {key}")))
}
fn required_text(row: &SourceRow, key: &str) -> EngineResult<String> {
    text(row, key).ok_or_else(|| decode(format!("missing column {key}")))
}
/// Rows grouped by an integer column, each group in source (rowid) order.
/// Built once so per-image lookups are O(1) instead of a scan per image.
fn group_by<'a>(rows: &'a [SourceRow], key: &str) -> HashMap<i64, Vec<&'a SourceRow>> {
    let mut out: HashMap<i64, Vec<&SourceRow>> = HashMap::new();
    for r in rows {
        if let Some(k) = number(r, key) {
            out.entry(k).or_default().push(r);
        }
    }
    out
}
/// The first row for each value of an integer column (what `find` returned).
fn first_by<'a>(rows: &'a [SourceRow], key: &str) -> HashMap<i64, &'a SourceRow> {
    let mut out = HashMap::new();
    for r in rows {
        if let Some(k) = number(r, key) {
            out.entry(k).or_insert(r);
        }
    }
    out
}

/// Per-image report entries grouped by message, in first-seen order: one
/// occurrence stays `image <id>: <message>`; more become
/// `<n> images (first: image <id>): <message>` (n counts images, not lines).
#[derive(Default)]
struct ImageReport {
    entries: Vec<(String, usize, i64, i64)>,
    index: HashMap<String, usize>,
}
impl ImageReport {
    fn push(&mut self, image: i64, message: String) {
        match self.index.get(&message) {
            Some(&i) => {
                let (_, count, _, last) = &mut self.entries[i];
                if *last != image {
                    *count += 1;
                    *last = image;
                }
            }
            None => {
                self.index.insert(message.clone(), self.entries.len());
                self.entries.push((message, 1, image, image));
            }
        }
    }
    fn finish(self) -> impl Iterator<Item = String> {
        self.entries
            .into_iter()
            .map(|(message, count, first, _)| match count {
                1 => format!("image {first}: {message}"),
                n => format!("{n} images (first: image {first}): {message}"),
            })
    }
}

pub(crate) fn rows(
    c: &Connection,
    table: &str,
    required: bool,
    report: &mut Vec<String>,
) -> EngineResult<Vec<SourceRow>> {
    let exists: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
            [table],
            |r| r.get(0),
        )
        .map_err(decode)?;
    if !exists {
        let message = format!("missing table {table}");
        if required {
            return Err(decode(message));
        }
        report.push(message);
        return Ok(vec![]);
    }
    // Table names are fixed constants supplied by this module, never user SQL.
    let mut stmt = c
        .prepare(&format!("SELECT * FROM \"{table}\" ORDER BY rowid"))
        .map_err(decode)?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let mut oversized = Vec::new();
    let mut index = 0usize;
    let mapped = stmt
        .query_map([], |row| {
            index += 1;
            let mut result = BTreeMap::new();
            for (i, name) in names.iter().enumerate() {
                let cell = row.get_ref(i)?;
                // Measured on SQLite's borrowed cell, before anything is copied.
                let len = match cell {
                    ValueRef::Text(b) | ValueRef::Blob(b) => b.len(),
                    _ => 0,
                };
                if len > MAX_CELL_BYTES {
                    oversized.push(format!(
                        "{table} row {index}: column {name} is {len} bytes, over the {MAX_CELL_BYTES}-byte cell limit; not loaded"
                    ));
                    result.insert(name.clone(), Value::Null);
                    continue;
                }
                let value = match cell {
                    ValueRef::Null => Value::Null,
                    ValueRef::Integer(n) => n.into(),
                    ValueRef::Real(n) => Value::from(n),
                    ValueRef::Text(s) => Value::String(String::from_utf8_lossy(s).into_owned()),
                    ValueRef::Blob(b) => Value::Array(b.iter().map(|v| Value::from(*v)).collect()),
                };
                result.insert(name.clone(), value);
            }
            Ok(result)
        })
        .map_err(decode)?;
    let loaded = mapped.collect::<Result<Vec<_>, _>>().map_err(decode)?;
    report.extend(oversized);
    Ok(loaded)
}

fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}
/// Copy both SQLite and its WAL before opening. Do not let SQLite create a SHM
/// file alongside the original. Concurrent changes abort rather than lose edits.
pub(crate) fn copied_catalog(path: &Path) -> EngineResult<(tempfile::TempDir, PathBuf)> {
    fn stamp(p: &Path) -> EngineResult<Option<(u64, std::time::SystemTime)>> {
        match std::fs::metadata(p) {
            Ok(m) => Ok(Some((m.len(), m.modified()?))),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(EngineError::io_at(p, &e)),
        }
    }
    let temp = tempfile::tempdir()?;
    let dest = temp.path().join("catalog.lrcat");
    let wal = sidecar(path, "-wal");
    let before = (stamp(path)?, stamp(&wal)?);
    std::fs::copy(path, &dest).map_err(|e| EngineError::io_at(path, &e))?;
    if before.1.is_some() {
        std::fs::copy(&wal, sidecar(&dest, "-wal")).map_err(|e| EngineError::io_at(&wal, &e))?;
    }
    if before != (stamp(path)?, stamp(&wal)?) {
        return Err(EngineError::Conflict {
            message: "catalog changed while copying; close Lightroom and retry".into(),
        });
    }
    Ok((temp, dest))
}
pub(crate) fn open_copy(path: &Path) -> EngineResult<Connection> {
    // Percent encoding protects ?, #, %, colon and non-ASCII path components.
    let bytes = path.as_os_str().as_encoded_bytes();
    let mut uri = String::from("file:");
    for &b in bytes {
        if b.is_ascii_alphanumeric() || b"/-._~".contains(&b) {
            uri.push(b as char);
        } else {
            uri.push_str(&format!("%{b:02X}"));
        }
    }
    uri.push_str("?mode=ro");
    Connection::open_with_flags(
        uri,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI,
    )
    .map_err(decode)
}
fn selection(row: &SourceRow) -> Selection {
    let rating = number(row, "rating").unwrap_or(0);
    let pick = number(row, "pick").unwrap_or(0);
    let decision = if pick == -1 || rating == -1 {
        Decision::Reject
    } else if pick == 1 || rating > 0 {
        Decision::Keep
    } else {
        Decision::Undecided
    };
    Selection {
        decision,
        grade: match rating {
            2 => Some(Grade::One),
            3 | 4 => Some(Grade::Two),
            5 => Some(Grade::Three),
            _ => None,
        },
        mark: text(row, "colorLabels")
            .filter(|s| !s.is_empty())
            .map(Mark::new),
    }
    .normalized()
}
/// Build the keyword hierarchy. Lightroom stores one unnamed root row (NULL
/// name, NULL parent); its children are the top-level keywords and the root
/// itself is not a keyword. Any other unnamed keyword is skipped with a report
/// entry and its children take its place under its parent.
fn keyword_tree(
    rows: &[SourceRow],
    synonyms: &[SourceRow],
    report: &mut Vec<String>,
) -> EngineResult<Vec<Keyword>> {
    struct Tree<'a> {
        by_id: HashMap<i64, &'a SourceRow>,
        children: HashMap<Option<i64>, Vec<&'a SourceRow>>,
        synonyms: HashMap<i64, Vec<&'a SourceRow>>,
    }
    fn build(
        id: i64,
        tree: &Tree,
        seen: &mut BTreeSet<i64>,
        report: &mut Vec<String>,
    ) -> EngineResult<Vec<Keyword>> {
        if !seen.insert(id) {
            return Err(decode("keyword hierarchy cycle or duplicate id"));
        }
        let r = tree
            .by_id
            .get(&id)
            .ok_or_else(|| decode("missing keyword"))?;
        let children = children(Some(id), tree, seen, report)?;
        let Some(name) = text(r, "name") else {
            report.push(format!(
                "keyword {id} has no name; skipped (its children moved up a level)"
            ));
            return Ok(children);
        };
        Ok(vec![Keyword {
            id,
            name,
            synonyms: tree
                .synonyms
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|r| text(r, "name"))
                .collect(),
            children,
        }])
    }
    fn children(
        parent: Option<i64>,
        tree: &Tree,
        seen: &mut BTreeSet<i64>,
        report: &mut Vec<String>,
    ) -> EngineResult<Vec<Keyword>> {
        let mut out = vec![];
        for child in tree.children.get(&parent).into_iter().flatten() {
            let id = required_id(child, "id_local")?;
            if parent.is_none() && text(child, "name").is_none() {
                continue; // unnamed roots are handled by the caller
            }
            out.extend(build(id, tree, seen, report)?);
        }
        Ok(out)
    }
    let mut tree = Tree {
        by_id: first_by(rows, "id_local"),
        children: HashMap::new(),
        synonyms: group_by(synonyms, "keyword"),
    };
    for r in rows {
        tree.children
            .entry(number(r, "parent").filter(|p| *p != 0))
            .or_default()
            .push(r);
    }
    let mut seen = BTreeSet::new();
    let mut roots = vec![];
    let mut unnamed_roots = 0;
    for row in rows
        .iter()
        .filter(|r| number(r, "parent").is_none_or(|p| p == 0) && text(r, "name").is_none())
    {
        let id = required_id(row, "id_local")?;
        if !seen.insert(id) {
            return Err(decode("keyword hierarchy cycle or duplicate id"));
        }
        unnamed_roots += 1;
        if unnamed_roots > 1 {
            report.push(format!(
                "keyword {id} is a second unnamed root keyword; its children were imported at the top level"
            ));
        }
        roots.extend(children(Some(id), &tree, &mut seen, report)?);
    }
    roots.extend(children(None, &tree, &mut seen, report)?);
    if seen.len() != rows.len() {
        return Err(decode("keyword hierarchy has missing parents or cycles"));
    }
    Ok(roots)
}

/// Read a catalog into memory, preserving source history, snapshots and face
/// rows in addition to translated current recipes. Missing optional metadata
/// tables are reported; missing core catalog tables are errors.
pub fn import(path: impl AsRef<Path>) -> EngineResult<ImportPlan> {
    let source = path.as_ref();
    let (_temp, copy) = copied_catalog(source)?;
    let c = open_copy(&copy)?;
    let mut report = vec![];
    let vars = rows(&c, "Adobe_variablesTable", true, &mut report)?;
    let schema_version = vars
        .iter()
        .find(|r| {
            text(r, "name").is_some_and(|s| {
                matches!(
                    s.as_str(),
                    "Adobe_DBVersion" | "Adobe_DBVersionNumber" | "schemaVersion"
                )
            })
        })
        .and_then(|r| text(r, "value"))
        .ok_or_else(|| decode("Adobe_variablesTable missing Adobe_DBVersion"))?;
    let roots = rows(&c, "AgLibraryRootFolder", true, &mut report)?;
    let folders = rows(&c, "AgLibraryFolder", true, &mut report)?;
    let files = rows(&c, "AgLibraryFile", true, &mut report)?;
    let images = rows(&c, "Adobe_images", true, &mut report)?;
    let develops = rows(&c, "Adobe_imageDevelopSettings", true, &mut report)?;
    let mut optional = |name| rows(&c, name, false, &mut report);
    let keywords = optional("AgLibraryKeyword")?;
    let synonyms = optional("AgLibraryKeywordSynonym")?;
    let keyword_images = optional("AgLibraryKeywordImage")?;
    let collections = optional("AgLibraryCollection")?;
    let mut collection_images = optional("AgLibraryCollectionImage")?;
    collection_images.sort_by(|a, b| {
        a.get("position")
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
            .total_cmp(&b.get("position").and_then(Value::as_f64).unwrap_or(0.0))
    });
    let contents = optional("AgLibraryCollectionContent")?;
    let history = optional("Adobe_libraryImageDevelopHistoryStep")?;
    let snapshots = optional("Adobe_libraryImageDevelopSnapshot")?;
    let faces = optional("AgLibraryFace")?;
    let face_clusters = optional("AgLibraryFaceCluster")?;
    let keyword_faces = optional("AgLibraryKeywordFace")?;
    let gps = optional("AgHarvestedExifMetadata")?;
    let mut stacks = vec![];
    for (table, links) in [
        ("AgLibraryFolderStack", "AgLibraryFolderStackImage"),
        ("AgLibraryCollectionStack", "AgLibraryCollectionStackImage"),
    ] {
        let stack_rows = optional(table)?;
        let mut members = optional(links)?;
        members.sort_by_key(|r| number(r, "position").unwrap_or(0));
        let members = group_by(&members, "stack");
        for r in stack_rows {
            let id = required_id(&r, "id_local")?;
            stacks.push(Stack {
                id,
                scope: table.into(),
                images: members
                    .get(&id)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| number(m, "image"))
                    .collect(),
                source: r,
            });
        }
    }
    let mut library = Library {
        roots: roots
            .iter()
            .map(|r| required_text(r, "absolutePath").map(PathBuf::from))
            .collect::<EngineResult<_>>()?,
        keywords: keyword_tree(&keywords, &synonyms, &mut report)?,
        ..Library::default()
    };
    let mut people = BTreeSet::new();
    for r in &face_clusters {
        if let Some(name) = text(r, "name") {
            people.insert(name);
        }
    }
    let keywords_by_id = first_by(&keywords, "id_local");
    for r in &keyword_faces {
        if let Some(k) = number(r, "keyword").and_then(|id| keywords_by_id.get(&id))
            && let Some(name) = text(k, "name")
        {
            people.insert(name);
        }
    }
    library.people = people.into_iter().collect();
    let source_name = source
        .canonicalize()
        .map_err(|e| EngineError::io_at(source, &e))?;
    let image_id = |id: i64| {
        let digest = Digest::derive(
            "tessera Lightroom image",
            format!("{}:{id}", source_name.display()).as_bytes(),
        );
        ImageId(u128::from_le_bytes(
            digest.0[..16].try_into().expect("16 bytes"),
        ))
    };
    let contents_by_collection = group_by(&contents, "collection");
    let images_by_collection = group_by(&collection_images, "collection");
    for row in &collections {
        let id = required_id(row, "id_local")?;
        let parent = number(row, "parent").filter(|p| *p != 0);
        // Only library collections are imported; slideshow/print/book/web
        // creations (and rows with no kind) are reported, not guessed.
        let Some(kind) = text(row, "creationId")
            .and_then(|k| k.strip_prefix("com.adobe.ag.library.").map(str::to_owned))
        else {
            let kind = text(row, "creationId").unwrap_or_else(|| "no kind".into());
            report.push(format!(
                "collection {id} ({kind}) is not a library collection; skipped"
            ));
            continue;
        };
        let name = text(row, "name").unwrap_or_else(|| {
            report.push(format!(
                "collection {id} has no name; imported as \"Untitled {id}\""
            ));
            format!("Untitled {id}")
        });
        if kind.contains("smart") {
            let Some(raw) = contents_by_collection
                .get(&id)
                .into_iter()
                .flatten()
                .find_map(|r| text(r, "content").filter(|s| s.contains('{')))
            else {
                report.push(format!("smart collection {id} has no rules; skipped"));
                continue;
            };
            // Rules the reader cannot represent skip that smart collection only.
            let search = match lua::parse(&raw) {
                Ok(search) => search_map::translate(search),
                Err(e) => {
                    report.push(format!(
                        "smart collection {id} rules not translated ({e}); skipped"
                    ));
                    continue;
                }
            };
            library.smart_albums.push(SmartAlbum {
                id,
                name,
                parent,
                search,
                // Keeps the M2-11 behaviour: a smart album inside a group is scoped.
                scoped: true,
            });
        } else if kind.contains("set") || kind == "group" {
            library.album_groups.push(AlbumGroup { id, name, parent });
        } else {
            // Catalogs can contain identically named collections. Use an ID
            // handle in that case rather than overwriting either membership.
            let mut key = name.clone();
            while library.albums.contains_key(&key) {
                key = format!("{key} [{id}]");
            }
            library.albums.insert(
                key,
                Album {
                    id,
                    name,
                    parent,
                    images: images_by_collection
                        .get(&id)
                        .into_iter()
                        .flatten()
                        .filter_map(|r| number(r, "image"))
                        .map(image_id)
                        .collect(),
                    ..Default::default()
                },
            );
        }
    }
    // Index every per-image table once (B5-29c): the loop below used to scan
    // each table per image, which made import quadratic in the image count.
    let images_by_id = first_by(&images, "id_local");
    let files_by_id = first_by(&files, "id_local");
    let folders_by_id = first_by(&folders, "id_local");
    let roots_by_id = first_by(&roots, "id_local");
    let develops_by_image = first_by(&develops, "image");
    let history_by_image = group_by(&history, "image");
    let snapshots_by_image = group_by(&snapshots, "image");
    let faces_by_image = group_by(&faces, "image");
    let gps_by_image = first_by(&gps, "image");
    let keywords_by_image = group_by(&keyword_images, "image");
    let collections_by_image = group_by(&collection_images, "image");
    let mut image_report = ImageReport::default();
    let mut result = vec![];
    for image in &images {
        let id = required_id(image, "id_local")?;
        let master_image = number(image, "masterImage").filter(|v| *v != 0);
        let file_id = number(image, "rootFile")
            .or_else(|| {
                master_image
                    .and_then(|m| images_by_id.get(&m))
                    .and_then(|m| number(m, "rootFile"))
            })
            .ok_or_else(|| decode(format!("image {id} missing rootFile")))?;
        let file = files_by_id
            .get(&file_id)
            .ok_or_else(|| decode(format!("image {id} references missing file {file_id}")))?;
        let folder_id = required_id(file, "folder")?;
        let folder = folders_by_id
            .get(&folder_id)
            .ok_or_else(|| decode(format!("missing folder {folder_id}")))?;
        let root_id = required_id(folder, "rootFolder")?;
        let root = roots_by_id
            .get(&root_id)
            .ok_or_else(|| decode(format!("missing root {root_id}")))?;
        let base = required_text(file, "baseName")?;
        let ext = text(file, "extension").unwrap_or_default();
        let filename = if ext.is_empty() {
            base
        } else {
            format!("{base}.{ext}")
        };
        let path = PathBuf::from(required_text(root, "absolutePath")?)
            .join(required_text(folder, "pathFromRoot")?)
            .join(&filename);
        let copy_name = text(image, "copyName").filter(|s| !s.is_empty());
        let display_name = if master_image.is_some() {
            format!(
                "{filename} ({})",
                copy_name.as_deref().unwrap_or("Virtual copy")
            )
        } else {
            filename
        };
        // Lightroom leaves an empty develop row (NULL process version) for
        // images that were never developed.
        let develop_text = develops_by_image
            .get(&id)
            .and_then(|r| text(r, "text"))
            .filter(|t| !t.trim().is_empty());
        let mut recipe = if let Some(source) = develop_text {
            let process_version = develops_by_image
                .get(&id)
                .and_then(|r| text(r, "processVersion"));
            let decoded = match &process_version {
                Some(pv) => develop(id, &source, pv),
                None => Err(decode("develop settings have no process version")),
            };
            match decoded {
                Ok((recipe, warnings)) => {
                    for w in warnings {
                        image_report.push(id, w);
                    }
                    recipe
                }
                // One bad row degrades that image only: unedited, reported,
                // and the source kept verbatim in the recipe.
                Err(e) => {
                    let reason = e.to_string().replace(&format!("image {id}: "), "");
                    image_report.push(
                        id,
                        format!(
                            "develop settings not imported ({reason}); imported as unedited, source preserved"
                        ),
                    );
                    let mut recipe = Recipe::default();
                    recipe.unknown.insert(
                        "lrcat_develop_source".into(),
                        serde_json::json!({"text": source, "processVersion": process_version}),
                    );
                    recipe
                }
            }
        } else {
            Recipe::default()
        };
        recipe.image_id = Some(image_id(id));
        let selection = selection(image);
        recipe.selection = selection.clone();
        let source_rows = |all: &HashMap<i64, Vec<&SourceRow>>| {
            all.get(&id)
                .into_iter()
                .flatten()
                .map(|r| (*r).clone())
                .collect::<Vec<_>>()
        };
        let history = source_rows(&history_by_image);
        let snapshots = source_rows(&snapshots_by_image);
        // Preserve source edit timelines without inventing replayable patches for
        // undocumented per-release history encodings.
        recipe
            .unknown
            .insert("lrcat_history".into(), serde_json::to_value(&history)?);
        recipe
            .unknown
            .insert("lrcat_snapshots".into(), serde_json::to_value(&snapshots)?);
        recipe.validate()?;
        let gps = gps_by_image.get(&id).and_then(|r| {
            Some([
                r.get("gpsLatitude")?.as_f64()?,
                r.get("gpsLongitude")?.as_f64()?,
            ])
        });
        result.push(ImportedImage {
            catalog_id: id,
            path,
            master_image,
            copy_name,
            display_name,
            orientation: text(image, "orientation"),
            capture_time: text(image, "captureTime"),
            recipe,
            selection,
            keywords: keywords_by_image
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|r| number(r, "tag"))
                .collect(),
            collections: collections_by_image
                .get(&id)
                .into_iter()
                .flatten()
                .filter_map(|r| number(r, "collection"))
                .collect(),
            gps,
            faces: source_rows(&faces_by_image),
            history,
            snapshots,
            rating: number(image, "rating"),
            pick: number(image, "pick"),
            color_label: text(image, "colorLabels").filter(|s| !s.is_empty()),
        });
    }
    report.extend(image_report.finish());
    result.sort_by_key(|r| r.catalog_id);
    Ok(ImportPlan {
        schema_version,
        roots,
        folders,
        images: result,
        library,
        stacks,
        face_clusters,
        keyword_faces,
        report,
    })
}
/// Decode one `Adobe_imageDevelopSettings.text` value. The format is chosen by
/// the first non-space token: `<` is XMP (older catalogs), `s` followed by `=`
/// is the Lua table literal LrC 15.5 writes. Decode errors name the image.
pub fn develop(
    image: i64,
    text: &str,
    process_version: &str,
) -> EngineResult<(Recipe, Vec<String>)> {
    let head = text.trim_start();
    let result = if head.starts_with('<') {
        xmp::parse(text, process_version)
    } else if head
        .strip_prefix('s')
        .is_some_and(|rest| rest.trim_start().starts_with('='))
    {
        lua_develop::parse(text, process_version)
    } else {
        Err(decode(
            "develop settings are neither XMP nor an `s =` Lua literal",
        ))
    };
    result.map_err(|e| match e {
        EngineError::Decode { format, message } => EngineError::Decode {
            format,
            message: format!("image {image}: {message}"),
        },
        other => other,
    })
}

/// Counts from the same validated plan the importer will produce.
pub fn inspect(path: impl AsRef<Path>) -> EngineResult<Summary> {
    let plan = import(path)?;
    fn count(ks: &[Keyword]) -> usize {
        ks.iter().map(|k| 1 + count(&k.children)).sum()
    }
    Ok(Summary {
        schema_version: plan.schema_version,
        images: plan.images.len(),
        virtual_copies: plan
            .images
            .iter()
            .filter(|r| r.master_image.is_some())
            .count(),
        folders: plan.folders.len(),
        keywords: count(&plan.library.keywords),
        albums: plan.library.albums.len(),
        album_groups: plan.library.album_groups.len(),
        smart_albums: plan.library.smart_albums.len(),
        stacks: plan.stacks.len(),
        faces: plan.images.iter().map(|r| r.faces.len()).sum(),
    })
}
