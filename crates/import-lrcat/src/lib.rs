//! Read-only Lightroom catalog translation. No original photos or catalogs are changed.
//! The import plan is explicit: callers decide when and where to persist it.
#[cfg(feature = "fixture")]
pub mod fixture;
pub mod lua;
pub mod previews;
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
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

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
    let mapped = stmt
        .query_map([], |row| {
            let mut result = BTreeMap::new();
            for (i, name) in names.iter().enumerate() {
                let value = match row.get_ref(i)? {
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
    mapped.collect::<Result<Vec<_>, _>>().map_err(decode)
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
fn keyword_tree(rows: &[SourceRow], synonyms: &[SourceRow]) -> EngineResult<Vec<Keyword>> {
    fn build(
        id: i64,
        rows: &[SourceRow],
        synonyms: &[SourceRow],
        seen: &mut BTreeSet<i64>,
    ) -> EngineResult<Keyword> {
        if !seen.insert(id) {
            return Err(decode("keyword hierarchy cycle or duplicate id"));
        }
        let r = rows
            .iter()
            .find(|r| number(r, "id_local") == Some(id))
            .ok_or_else(|| decode("missing keyword"))?;
        let mut children = vec![];
        for child in rows.iter().filter(|r| number(r, "parent") == Some(id)) {
            children.push(build(
                required_id(child, "id_local")?,
                rows,
                synonyms,
                seen,
            )?);
        }
        Ok(Keyword {
            id,
            name: required_text(r, "name")?,
            synonyms: synonyms
                .iter()
                .filter(|r| number(r, "keyword") == Some(id))
                .filter_map(|r| text(r, "name"))
                .collect(),
            children,
        })
    }
    let mut seen = BTreeSet::new();
    let mut roots = vec![];
    for row in rows
        .iter()
        .filter(|r| number(r, "parent").is_none_or(|p| p == 0))
    {
        roots.push(build(
            required_id(row, "id_local")?,
            rows,
            synonyms,
            &mut seen,
        )?);
    }
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
        for r in stack_rows {
            let id = required_id(&r, "id_local")?;
            stacks.push(Stack {
                id,
                scope: table.into(),
                images: members
                    .iter()
                    .filter(|m| number(m, "stack") == Some(id))
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
        keywords: keyword_tree(&keywords, &synonyms)?,
        ..Library::default()
    };
    let mut people = BTreeSet::new();
    for r in &face_clusters {
        if let Some(name) = text(r, "name") {
            people.insert(name);
        }
    }
    for r in &keyword_faces {
        if let Some(k) = keywords
            .iter()
            .find(|k| number(k, "id_local") == number(r, "keyword"))
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
    for row in &collections {
        let id = required_id(row, "id_local")?;
        let name = required_text(row, "name")?;
        let parent = number(row, "parent").filter(|p| *p != 0);
        let kind = required_text(row, "creationId")?;
        if kind.contains("smart") {
            let raw = contents
                .iter()
                .filter(|r| number(r, "collection") == Some(id))
                .find_map(|r| text(r, "content").filter(|s| s.contains('{')))
                .ok_or_else(|| decode(format!("smart collection {id} missing rules")))?;
            library.smart_albums.push(SmartAlbum {
                id,
                name,
                parent,
                search: lua::parse(&raw)?,
                // Keeps the M2-11 behaviour: a smart album inside a group is scoped.
                scoped: true,
            });
        } else if kind.contains("set") {
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
                    images: collection_images
                        .iter()
                        .filter(|r| number(r, "collection") == Some(id))
                        .filter_map(|r| number(r, "image"))
                        .map(image_id)
                        .collect(),
                    ..Default::default()
                },
            );
        }
    }
    let mut result = vec![];
    for image in &images {
        let id = required_id(image, "id_local")?;
        let master_image = number(image, "masterImage").filter(|v| *v != 0);
        let file_id = number(image, "rootFile")
            .or_else(|| {
                master_image
                    .and_then(|m| images.iter().find(|r| number(r, "id_local") == Some(m)))
                    .and_then(|m| number(m, "rootFile"))
            })
            .ok_or_else(|| decode(format!("image {id} missing rootFile")))?;
        let file = files
            .iter()
            .find(|r| number(r, "id_local") == Some(file_id))
            .ok_or_else(|| decode(format!("image {id} references missing file {file_id}")))?;
        let folder_id = required_id(file, "folder")?;
        let folder = folders
            .iter()
            .find(|r| number(r, "id_local") == Some(folder_id))
            .ok_or_else(|| decode(format!("missing folder {folder_id}")))?;
        let root_id = required_id(folder, "rootFolder")?;
        let root = roots
            .iter()
            .find(|r| number(r, "id_local") == Some(root_id))
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
        let mut recipe = if let Some(row) = develops.iter().find(|r| number(r, "image") == Some(id))
        {
            let (recipe, warnings) = xmp::parse(
                &required_text(row, "text")?,
                &required_text(row, "processVersion")?,
            )?;
            report.extend(warnings.into_iter().map(|w| format!("image {id}: {w}")));
            recipe
        } else {
            Recipe::default()
        };
        recipe.image_id = Some(image_id(id));
        let selection = selection(image);
        recipe.selection = selection.clone();
        let source_rows = |all: &[SourceRow]| {
            all.iter()
                .filter(|r| number(r, "image") == Some(id))
                .cloned()
                .collect::<Vec<_>>()
        };
        let history = source_rows(&history);
        let snapshots = source_rows(&snapshots);
        // Preserve source edit timelines without inventing replayable patches for
        // undocumented per-release history encodings.
        recipe
            .unknown
            .insert("lrcat_history".into(), serde_json::to_value(&history)?);
        recipe
            .unknown
            .insert("lrcat_snapshots".into(), serde_json::to_value(&snapshots)?);
        recipe.validate()?;
        let gps = gps
            .iter()
            .find(|r| number(r, "image") == Some(id))
            .and_then(|r| {
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
            keywords: keyword_images
                .iter()
                .filter(|r| number(r, "image") == Some(id))
                .filter_map(|r| number(r, "tag"))
                .collect(),
            collections: collection_images
                .iter()
                .filter(|r| number(r, "image") == Some(id))
                .filter_map(|r| number(r, "collection"))
                .collect(),
            gps,
            faces: source_rows(&faces),
            history,
            snapshots,
            rating: number(image, "rating"),
            pick: number(image, "pick"),
            color_label: text(image, "colorLabels").filter(|s| !s.is_empty()),
        });
    }
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
