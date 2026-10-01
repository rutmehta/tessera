//! Read-only Lightroom catalog translation. No original photos or catalogs are changed.
//! The import plan is explicit: callers decide when and where to persist it.
pub mod diagnostics;
#[cfg(feature = "fixture")]
pub mod fixture;
mod geometry;
mod lr2;
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

/// Largest text or blob cell retained inline. Larger cells carry an explicit
/// omission descriptor, or a lossless bundle-relative external-storage reference.
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

/// All notes are grouped by message except unedited-image and duplicate-ID
/// last-write-wins notes, which stay individual. One
/// occurrence stays `image <id>: <message>`; more become
/// `<n> images (first: image <id>): <message>` (n counts images, not lines).
#[derive(Default)]
struct ImageReport {
    entries: Vec<(String, usize, i64, i64)>,
    index: HashMap<String, usize>,
}
impl ImageReport {
    fn push(&mut self, image: i64, message: String) {
        if message.contains("imported as unedited")
            || message == "duplicate Adobe_images id; last-write-wins"
            || message == "duplicate develop image id; last-write-wins"
        {
            self.entries.push((message, 1, image, image));
            return;
        }
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
    fn finish(mut self) -> impl Iterator<Item = String> {
        self.entries.sort_by_key(|(_, _, first, _)| *first);
        self.entries
            .into_iter()
            .map(|(message, count, first, _)| match count {
                1 => format!("image {first}: {message}"),
                n => format!("{n} images (first: image {first}): {message}"),
            })
    }
}

/// Project oversized cells to a small placeholder plus length/type metadata.
/// SQLite's octet_length/typeof column opcodes read the record header without
/// materializing overflow pages. This keeps both result rows and ORDER BY's
/// sorter bounded; source_row reads the actual bytes via incremental BLOB I/O.
fn bounded_statement<'c>(
    c: &'c Connection,
    table: &str,
    order: &str,
) -> rusqlite::Result<rusqlite::Statement<'c>> {
    let names: Vec<String> = c
        .prepare(&format!("SELECT * FROM \"{table}\" LIMIT 0"))?
        .column_names()
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut fields = Vec::new();
    let mut metadata = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let quoted = format!("\"{}\"", name.replace('"', "\"\""));
        let oversized = format!(
            "typeof({quoted}) IN ('text','blob') AND octet_length({quoted}) > {MAX_CELL_BYTES}"
        );
        fields.push(format!(
            "CASE WHEN {oversized} THEN '' ELSE {quoted} END AS {quoted}"
        ));
        metadata.push(format!(
            "CASE WHEN {oversized} THEN octet_length({quoted}) ELSE 0 END AS __tessera_length_{i}"
        ));
        metadata.push(format!("typeof({quoted}) AS __tessera_type_{i}"));
    }
    fields.extend(metadata);
    fields.push("rowid AS __tessera_rowid".into());
    // Table and ORDER BY expressions are fixed constants in this module.
    c.prepare(&format!(
        "SELECT {} FROM \"{table}\" ORDER BY {order}",
        fields.join(",")
    ))
}

/// One bounded SQLite row. Oversized cells are explicit descriptors with either
/// a bounded prefix (inspect) or a lossless bundle-relative external reference.
fn source_row(
    c: &Connection,
    row: &rusqlite::Row,
    names: &[String],
    table: &str,
    index: usize,
    oversized: &mut Vec<String>,
    storage: Option<&Path>,
) -> rusqlite::Result<SourceRow> {
    let row_id = row.get::<_, i64>("__tessera_rowid").ok();
    let position = row_id.map_or_else(|| format!("scan ordinal {index}"), |id| format!("row {id}"));
    let image = row
        .get::<_, i64>(if table == "Adobe_images" {
            "id_local"
        } else {
            "image"
        })
        .ok();
    let mut result = BTreeMap::new();
    let column_count = (names.len() - 1) / 3;
    for (i, name) in names[..column_count].iter().enumerate() {
        let cell = row.get_ref(i)?;
        let len = row.get::<_, i64>(column_count + 2 * i)? as usize;
        if len > MAX_CELL_BYTES {
            use std::io::Read;
            let kind: String = row.get(column_count + 2 * i + 1)?;
            let mut blob = c.blob_open(
                rusqlite::MAIN_DB,
                table,
                name.as_str(),
                row_id.ok_or(rusqlite::Error::InvalidQuery)?,
                true,
            )?;
            let mut descriptor = serde_json::json!({
                "status": "omitted", "length": len,
                "table": table, "rowid": row_id, "scan_ordinal": index,
                "image_id": image, "column": name, "kind": kind
            });
            let recovery = if let Some(storage) = storage {
                // Hex-encode the column name: even a hostile schema cannot escape
                // the bundle or collide with another column's filename.
                let column: String = name.as_bytes().iter().map(|b| format!("{b:02x}")).collect();
                let relative = format!(
                    "large/{}-{table}-{}-{column}.bin",
                    image.map_or_else(|| "none".into(), |v| v.to_string()),
                    row_id.unwrap_or(index as i64)
                );
                let mut write = || -> std::io::Result<()> {
                    std::fs::create_dir_all(storage.join("large"))?;
                    let mut file = std::fs::File::create_new(storage.join(&relative))?;
                    // io::copy uses a fixed-size buffer; SQLite reads overflow
                    // pages incrementally for TEXT as well as BLOB columns.
                    let copied = std::io::copy(&mut blob, &mut file)?;
                    if copied != len as u64 {
                        return Err(std::io::Error::other("oversized cell length changed"));
                    }
                    file.sync_all()
                };
                write().map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                descriptor["status"] = "externalized".into();
                descriptor["path"] = relative.clone().into();
                format!("externalized; recovery path {relative}")
            } else {
                let mut prefix_bytes = vec![0; 64 * 1024];
                blob.read_exact(&mut prefix_bytes)
                    .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
                descriptor["prefix"] = if kind == "text" {
                    let end = std::str::from_utf8(&prefix_bytes)
                        .err()
                        .map_or(prefix_bytes.len(), |e| e.valid_up_to());
                    Value::from(String::from_utf8_lossy(&prefix_bytes[..end]).into_owned())
                } else {
                    Value::from(prefix_bytes)
                };
                format!(
                    "omitted; recover from source catalog table {table}, {position}, column {name}"
                )
            };
            let reason = format!(
                "image {}: {table} {position}: column {name} is {len} bytes, over the {MAX_CELL_BYTES}-byte cell limit; {recovery}",
                image.map_or_else(|| "NULL/unassociated".into(), |v| v.to_string())
            );
            oversized.push(reason.clone());
            if table == "Adobe_imageDevelopSettings" && name == "text" {
                result.insert("__oversized_develop".into(), reason.into());
            }
            result.insert(name.clone(), descriptor);
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
}

/// Whether `table` exists; a missing table is an error if `required`, else a
/// report entry.
fn present(
    c: &Connection,
    table: &str,
    required: bool,
    report: &mut Vec<String>,
) -> EngineResult<bool> {
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
    }
    Ok(exists)
}

pub(crate) fn rows(
    c: &Connection,
    table: &str,
    required: bool,
    report: &mut Vec<String>,
) -> EngineResult<Vec<SourceRow>> {
    rows_with_storage(c, table, required, report, None)
}

fn rows_with_storage(
    c: &Connection,
    table: &str,
    required: bool,
    report: &mut Vec<String>,
    storage: Option<&Path>,
) -> EngineResult<Vec<SourceRow>> {
    if !present(c, table, required, report)? {
        return Ok(vec![]);
    }
    all_rows(c, table, report, storage)
}

/// Every row of an existing table, in rowid order.
fn all_rows(
    c: &Connection,
    table: &str,
    report: &mut Vec<String>,
    storage: Option<&Path>,
) -> EngineResult<Vec<SourceRow>> {
    // Table names are fixed constants supplied by this module, never user SQL.
    let mut stmt = bounded_statement(c, table, "rowid").map_err(decode)?;
    let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
    let mut rows = stmt.query([]).map_err(decode)?;
    let (mut out, mut index) = (Vec::new(), 0);
    while let Some(row) = rows.next().map_err(decode)? {
        index += 1;
        out.push(source_row(c, row, &names, table, index, report, storage).map_err(decode)?);
    }
    Ok(out)
}

/// A per-image table read in image order (`ORDER BY image, rowid`): each
/// image's rows are taken in one forward pass, so the table is never held in
/// memory. Rows whose `image` is not an integer never match, as before.
struct ByImage<'s> {
    connection: &'s Connection,
    storage: Option<&'s Path>,
    rows: Option<rusqlite::Rows<'s>>,
    names: Vec<String>,
    table: &'static str,
    index: usize,
    peeked: Option<SourceRow>,
}
impl<'s> ByImage<'s> {
    fn prepare<'c>(
        c: &'c Connection,
        table: &str,
        exists: bool,
    ) -> EngineResult<Option<rusqlite::Statement<'c>>> {
        if !exists {
            return Ok(None);
        }
        bounded_statement(c, table, "image, rowid")
            .map(Some)
            .map_err(decode)
    }
    fn new(
        connection: &'s Connection,
        stmt: Option<&'s mut rusqlite::Statement<'_>>,
        table: &'static str,
        storage: Option<&'s Path>,
    ) -> EngineResult<Self> {
        let (rows, names) = match stmt {
            Some(stmt) => {
                let names = stmt.column_names().iter().map(|n| n.to_string()).collect();
                (Some(stmt.query([]).map_err(decode)?), names)
            }
            None => (None, vec![]),
        };
        Ok(Self {
            connection,
            storage,
            rows,
            names,
            table,
            index: 0,
            peeked: None,
        })
    }
    fn next_row(&mut self, oversized: &mut Vec<String>) -> EngineResult<Option<SourceRow>> {
        if let Some(row) = self.peeked.take() {
            return Ok(Some(row));
        }
        let Some(rows) = &mut self.rows else {
            return Ok(None);
        };
        let Some(row) = rows.next().map_err(decode)? else {
            return Ok(None);
        };
        self.index += 1;
        source_row(
            self.connection,
            row,
            &self.names,
            self.table,
            self.index,
            oversized,
            self.storage,
        )
        .map(Some)
        .map_err(decode)
    }
    /// The rows for `image`, in rowid order. Images must be asked in
    /// ascending order; rows for smaller ids (orphans) are skipped.
    fn take(&mut self, image: i64, oversized: &mut Vec<String>) -> EngineResult<Vec<SourceRow>> {
        let mut out = vec![];
        while let Some(row) = self.next_row(oversized)? {
            match number(&row, "image") {
                Some(k) if k == image => out.push(row),
                Some(k) if k > image => {
                    self.peeked = Some(row);
                    break;
                }
                _ => {}
            }
        }
        Ok(out)
    }
}

/// `f` over `items` on all cores, results in input order.
fn par_map<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let threads = std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(items.len());
    if threads <= 1 {
        return items.iter().map(f).collect();
    }
    let f = &f;
    std::thread::scope(|s| {
        let workers: Vec<_> = items
            .chunks(items.len().div_ceil(threads))
            .map(|chunk| s.spawn(move || chunk.iter().map(f).collect::<Vec<_>>()))
            .collect();
        workers
            .into_iter()
            .flat_map(|w| w.join().unwrap_or_else(|e| std::panic::resume_unwind(e)))
            .collect()
    })
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

/// Images translated in parallel per batch while the catalog is streamed.
const BATCH: usize = 256;

/// Read a catalog into memory, preserving source history, snapshots and face
/// rows in addition to translated current recipes. Missing optional metadata
/// tables are reported; missing core catalog tables are errors.
///
/// This holds every image (with its history rows) in memory; callers that can
/// consume images one at a time should use [`import_each`].
pub fn import(path: impl AsRef<Path>) -> EngineResult<ImportPlan> {
    let mut images = vec![];
    let mut plan = import_each(
        path,
        |_: &ImportPlan| Ok(()),
        |image| {
            images.push(image);
            Ok(())
        },
    )?;
    plan.images = images;
    Ok(plan)
}

/// [`import`] as a stream: `begin` sees the plan without images (everything
/// but the per-image report is final), then `visit` gets each image in
/// catalog-id order. The returned plan has the full report and no images.
///
/// Small tables are loaded; the per-image tables (images, develop settings,
/// history, snapshots, faces, EXIF) are read in image order alongside each
/// other, so memory is bounded by a batch of [`BATCH`] images, and develop
/// settings are translated on all cores.
pub fn import_each(
    path: impl AsRef<Path>,
    begin: impl FnOnce(&ImportPlan) -> EngineResult<()>,
    visit: impl FnMut(ImportedImage) -> EngineResult<()>,
) -> EngineResult<ImportPlan> {
    import_each_with_storage(path, None, begin, visit)
}

/// Stream into a private bundle staging directory. Oversized cells are written
/// losslessly below `storage/large`; references are relative to the bundle root.
/// The caller owns publication and cleanup of the staging directory.
pub fn import_each_with_storage(
    path: impl AsRef<Path>,
    storage: Option<&Path>,
    begin: impl FnOnce(&ImportPlan) -> EngineResult<()>,
    mut visit: impl FnMut(ImportedImage) -> EngineResult<()>,
) -> EngineResult<ImportPlan> {
    if let Some(storage) = storage {
        sidecar::Sidecar::ensure_destination(storage, "import bundle")?;
        sidecar::Sidecar::ensure_destination(storage.join("large"), "import source cells")?;
    }
    let source = path.as_ref();
    let (_temp, copy) = copied_catalog(source)?;
    let c = open_copy(&copy)?;
    let mut report = vec![];
    let vars = rows_with_storage(&c, "Adobe_variablesTable", true, &mut report, storage)?;
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
    let roots = rows_with_storage(&c, "AgLibraryRootFolder", true, &mut report, storage)?;
    let folders = rows_with_storage(&c, "AgLibraryFolder", true, &mut report, storage)?;
    // Only the columns a path needs are kept per file.
    let mut files = rows_with_storage(&c, "AgLibraryFile", true, &mut report, storage)?;
    for f in &mut files {
        f.retain(|k, _| matches!(k.as_str(), "id_local" | "folder" | "baseName" | "extension"));
    }
    // Images and develop settings are streamed below; only the master lookup
    // for virtual copies (id -> rootFile) is loaded.
    present(&c, "Adobe_images", true, &mut report)?;
    present(&c, "Adobe_imageDevelopSettings", true, &mut report)?;
    let master_files: HashMap<i64, Option<i64>> = {
        let mut stmt = bounded_statement(&c, "Adobe_images", "rowid").map_err(decode)?;
        let names: Vec<String> = stmt.column_names().iter().map(|n| n.to_string()).collect();
        let (id_col, root_col) = (
            names.iter().position(|n| n == "id_local"),
            names.iter().position(|n| n == "rootFile"),
        );
        let mut out = HashMap::new();
        let mut rows = stmt.query([]).map_err(decode)?;
        while let Some(row) = rows.next().map_err(decode)? {
            let get = |col: Option<usize>| -> EngineResult<Option<i64>> {
                Ok(
                    match col.map(|i| row.get_ref(i)).transpose().map_err(decode)? {
                        Some(ValueRef::Integer(n)) => Some(n),
                        _ => None,
                    },
                )
            };
            if let Some(id) = get(id_col)? {
                let root = get(root_col)?;
                out.insert(id, root);
            }
        }
        out
    };
    let mut optional = |name| rows_with_storage(&c, name, false, &mut report, storage);
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
    let mut streamed = |name| present(&c, name, false, &mut report);
    let has_history = streamed("Adobe_libraryImageDevelopHistoryStep")?;
    let has_snapshots = streamed("Adobe_libraryImageDevelopSnapshot")?;
    let has_faces = streamed("AgLibraryFace")?;
    let mut optional = |name| rows_with_storage(&c, name, false, &mut report, storage);
    let face_clusters = optional("AgLibraryFaceCluster")?;
    let keyword_faces = optional("AgLibraryKeywordFace")?;
    let has_gps = present(&c, "AgHarvestedExifMetadata", false, &mut report)?;
    let mut optional = |name| rows_with_storage(&c, name, false, &mut report, storage);
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
    let image_id = |id: i64| image_id_for(&source_name, id);
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
    let mut plan = ImportPlan {
        schema_version,
        roots,
        folders,
        images: vec![],
        library,
        stacks,
        face_clusters,
        keyword_faces,
        report,
    };
    begin(&plan)?;
    // Small per-image link tables are indexed once; the large ones stream.
    let files_by_id = first_by(&files, "id_local");
    let folders_by_id = first_by(&plan.folders, "id_local");
    let roots_by_id = first_by(&plan.roots, "id_local");
    let keywords_by_image = group_by(&keyword_images, "image");
    let collections_by_image = group_by(&collection_images, "image");
    let mut image_stmt =
        bounded_statement(&c, "Adobe_images", "id_local, rowid").map_err(decode)?;
    let image_names: Vec<String> = image_stmt
        .column_names()
        .iter()
        .map(|n| n.to_string())
        .collect();
    let mut image_rows = image_stmt.query([]).map_err(decode)?;
    let mut develop_stmt = ByImage::prepare(&c, "Adobe_imageDevelopSettings", true)?;
    let mut history_stmt =
        ByImage::prepare(&c, "Adobe_libraryImageDevelopHistoryStep", has_history)?;
    let mut snapshot_stmt =
        ByImage::prepare(&c, "Adobe_libraryImageDevelopSnapshot", has_snapshots)?;
    let mut face_stmt = ByImage::prepare(&c, "AgLibraryFace", has_faces)?;
    let mut gps_stmt = ByImage::prepare(&c, "AgHarvestedExifMetadata", has_gps)?;
    let mut develops = ByImage::new(
        &c,
        develop_stmt.as_mut(),
        "Adobe_imageDevelopSettings",
        storage,
    )?;
    let mut history = ByImage::new(
        &c,
        history_stmt.as_mut(),
        "Adobe_libraryImageDevelopHistoryStep",
        storage,
    )?;
    let mut snapshots = ByImage::new(
        &c,
        snapshot_stmt.as_mut(),
        "Adobe_libraryImageDevelopSnapshot",
        storage,
    )?;
    let mut faces = ByImage::new(&c, face_stmt.as_mut(), "AgLibraryFace", storage)?;
    let mut gps = ByImage::new(&c, gps_stmt.as_mut(), "AgHarvestedExifMetadata", storage)?;
    let mut image_report = ImageReport::default();
    let mut oversized = vec![];
    let (mut image_index, mut batch) = (0, Vec::<Pending>::with_capacity(BATCH));
    let source_name = &source_name;
    let image_id = |id: i64| image_id_for(source_name, id);
    loop {
        let Some(row) = image_rows.next().map_err(decode)? else {
            flush(&mut batch, &image_id, &mut image_report, &mut visit)?;
            break;
        };
        image_index += 1;
        let image = source_row(
            &c,
            row,
            &image_names,
            "Adobe_images",
            image_index,
            &mut oversized,
            storage,
        )
        .map_err(decode)?;
        let id = required_id(&image, "id_local")?;
        // Duplicate image ids (no primary key) all see the same per-image
        // rows, so a batch never ends between two of them.
        let rows = match batch.last() {
            Some(previous) if previous.id == id => {
                let rows = previous.rows.clone();
                batch.pop();
                image_report.push(id, "duplicate Adobe_images id; last-write-wins".into());
                rows
            }
            _ => {
                if batch.len() >= BATCH {
                    flush(&mut batch, &image_id, &mut image_report, &mut visit)?;
                }
                PerImage {
                    develop: {
                        let mut rows = develops.take(id, &mut oversized)?;
                        if rows.len() > 1 {
                            image_report
                                .push(id, "duplicate develop image id; last-write-wins".into());
                        }
                        rows.pop()
                    },
                    history: history.take(id, &mut oversized)?,
                    snapshots: snapshots.take(id, &mut oversized)?,
                    faces: faces.take(id, &mut oversized)?,
                    gps: gps.take(id, &mut oversized)?.into_iter().next(),
                }
            }
        };
        {
            let master_image = number(&image, "masterImage").filter(|v| *v != 0);
            let file_id = number(&image, "rootFile")
                .or_else(|| master_image.and_then(|m| master_files.get(&m).copied().flatten()))
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
            let copy_name = text(&image, "copyName").filter(|s| !s.is_empty());
            let display_name = if master_image.is_some() {
                format!(
                    "{filename} ({})",
                    copy_name.as_deref().unwrap_or("Virtual copy")
                )
            } else {
                filename
            };
            batch.push(Pending {
                id,
                image,
                path,
                master_image,
                copy_name,
                display_name,
                rows,
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
            });
        }
    }
    plan.report.extend(oversized);
    plan.report.extend(image_report.finish());
    Ok(plan)
}

/// Rows of the streamed per-image tables for one image.
#[derive(Clone)]
struct PerImage {
    develop: Option<SourceRow>,
    history: Vec<SourceRow>,
    snapshots: Vec<SourceRow>,
    faces: Vec<SourceRow>,
    gps: Option<SourceRow>,
}

/// An image whose source rows are gathered, waiting for translation.
struct Pending {
    id: i64,
    image: SourceRow,
    path: PathBuf,
    master_image: Option<i64>,
    copy_name: Option<String>,
    display_name: String,
    rows: PerImage,
    keywords: Vec<i64>,
    collections: Vec<i64>,
}

fn image_id_for(source: &Path, id: i64) -> ImageId {
    let digest = Digest::derive(
        "tessera Lightroom image",
        format!("{}:{id}", source.display()).as_bytes(),
    );
    ImageId(u128::from_le_bytes(
        digest.0[..16].try_into().expect("16 bytes"),
    ))
}

/// Translate a batch on all cores, then report and emit it in order.
fn flush(
    batch: &mut Vec<Pending>,
    image_id: &(impl Fn(i64) -> ImageId + Sync),
    report: &mut ImageReport,
    visit: &mut impl FnMut(ImportedImage) -> EngineResult<()>,
) -> EngineResult<()> {
    let translated = par_map(batch, |p| translate(p, image_id(p.id)));
    for (p, result) in batch.drain(..).zip(translated) {
        let (recipe, notes) = result?;
        for note in notes {
            report.push(p.id, note);
        }
        let selection = recipe.selection.clone();
        let gps = p.rows.gps.as_ref().and_then(|r| {
            Some([
                r.get("gpsLatitude")?.as_f64()?,
                r.get("gpsLongitude")?.as_f64()?,
            ])
        });
        visit(ImportedImage {
            catalog_id: p.id,
            path: p.path,
            master_image: p.master_image,
            copy_name: p.copy_name,
            display_name: p.display_name,
            orientation: text(&p.image, "orientation"),
            capture_time: text(&p.image, "captureTime"),
            recipe,
            selection,
            keywords: p.keywords,
            collections: p.collections,
            gps,
            faces: p.rows.faces,
            history: p.rows.history,
            snapshots: p.rows.snapshots,
            rating: number(&p.image, "rating"),
            pick: number(&p.image, "pick"),
            color_label: text(&p.image, "colorLabels").filter(|s| !s.is_empty()),
        })?;
    }
    Ok(())
}

/// One image's recipe and its report notes (without the `image <id>:` prefix).
fn translate(p: &Pending, image_id: ImageId) -> EngineResult<(Recipe, Vec<String>)> {
    let id = p.id;
    let mut notes = vec![];
    // Lightroom leaves an empty develop row (NULL process version) for
    // images that were never developed.
    let develop_text = p
        .rows
        .develop
        .as_ref()
        .and_then(|r| r.get("text"))
        .filter(|v| !v.is_object())
        .and_then(|_| p.rows.develop.as_ref().and_then(|r| text(r, "text")))
        .filter(|t| !t.trim().is_empty());
    let oversized_cell = p
        .rows
        .develop
        .as_ref()
        .and_then(|r| r.get("text"))
        .filter(|v| v.is_object());
    let mut recipe = if let Some(cell) = oversized_cell {
        let reason = p
            .rows
            .develop
            .as_ref()
            .and_then(|r| text(r, "__oversized_develop"))
            .unwrap_or_default();
        notes.push(format!(
            "edits failed to import: develop settings not imported ({reason}); imported as unedited"
        ));
        let mut recipe = Recipe::default();
        recipe.unknown.insert(
            "lrcat_develop_source".into(),
            serde_json::json!({
                "shape": "cell-descriptor",
                "truncated": cell["status"] == "omitted", "cell": cell,
                "processVersion": p.rows.develop.as_ref().and_then(|r| text(r, "processVersion"))
            }),
        );
        if let Some(prefix) = cell.get("prefix") {
            recipe.unknown.get_mut("lrcat_develop_source").unwrap()["text"] = prefix.clone();
        }
        recipe
    } else if let Some(source) = develop_text {
        let process_version = p
            .rows
            .develop
            .as_ref()
            .and_then(|r| text(r, "processVersion"));
        let decoded = match &process_version {
            Some(pv) => develop(id, &source, pv),
            None => Err(decode("develop settings have no process version")),
        };
        match decoded {
            Ok((recipe, warnings)) => {
                notes.extend(warnings);
                recipe
            }
            // One bad row degrades that image only: unedited, reported,
            // and the source kept verbatim in the recipe.
            Err(e) => {
                let reason = e.to_string().replace(&format!("image {id}: "), "");
                notes.push(format!(
                    "edits failed to import: develop settings not imported ({reason}); imported as unedited, source preserved"
                ));
                let mut recipe = Recipe::default();
                recipe.unknown.insert(
                    "lrcat_develop_source".into(),
                    serde_json::json!({"shape": "raw-text", "text": source, "processVersion": process_version}),
                );
                recipe
            }
        }
    } else {
        notes.push("never developed (no develop settings); imported as unedited".into());
        Recipe::default()
    };
    recipe.image_id = Some(image_id);
    recipe.selection = selection(&p.image);
    // Preserve source edit timelines without inventing replayable patches for
    // undocumented per-release history encodings.
    recipe.unknown.insert(
        "lrcat_history".into(),
        serde_json::to_value(&p.rows.history)?,
    );
    recipe.unknown.insert(
        "lrcat_snapshots".into(),
        serde_json::to_value(&p.rows.snapshots)?,
    );
    recipe.validate()?;
    Ok((recipe, notes))
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

/// Counts from the same validated plan the importer will produce (streamed:
/// images are counted and dropped).
pub fn inspect(path: impl AsRef<Path>) -> EngineResult<Summary> {
    let (mut images, mut virtual_copies, mut faces) = (0, 0, 0);
    let plan = import_each(
        path,
        |_| Ok(()),
        |image| {
            images += 1;
            virtual_copies += usize::from(image.master_image.is_some());
            faces += image.faces.len();
            Ok(())
        },
    )?;
    fn count(ks: &[Keyword]) -> usize {
        ks.iter().map(|k| 1 + count(&k.children)).sum()
    }
    Ok(Summary {
        schema_version: plan.schema_version,
        images,
        virtual_copies,
        folders: plan.folders.len(),
        keywords: count(&plan.library.keywords),
        albums: plan.library.albums.len(),
        album_groups: plan.library.album_groups.len(),
        smart_albums: plan.library.smart_albums.len(),
        stacks: plan.stacks.len(),
        faces,
    })
}

/// Writes an [`ImportPlan`] as the same bytes `serde_json::to_vec_pretty`
/// gives for the whole plan, one image at a time, for use with
/// [`import_each`]: [`PlanJson::begin`] in `begin`, [`PlanJson::image`] in
/// `visit`, then [`PlanJson::finish`] with the returned plan.
pub struct PlanJson<W: std::io::Write> {
    out: W,
    images: usize,
}
impl<W: std::io::Write> PlanJson<W> {
    /// Pretty JSON of `value`, nested at `indent` (every line after the first
    /// is prefixed; JSON strings never contain a raw newline).
    fn nested<T: Serialize + ?Sized>(&mut self, value: &T, indent: &str) -> EngineResult<()> {
        let bytes = serde_json::to_vec_pretty(value)?;
        for (i, line) in bytes.split(|b| *b == b'\n').enumerate() {
            if i > 0 {
                self.out.write_all(b"\n")?;
                self.out.write_all(indent.as_bytes())?;
            }
            self.out.write_all(line)?;
        }
        Ok(())
    }
    fn field<T: Serialize + ?Sized>(
        &mut self,
        name: &str,
        value: &T,
        first: bool,
    ) -> EngineResult<()> {
        let sep = if first { "{\n" } else { ",\n" };
        write!(self.out, "{sep}  \"{name}\": ")?;
        self.nested(value, "  ")
    }
    /// Start the document with the fields that precede `images`.
    pub fn begin(out: W, plan: &ImportPlan) -> EngineResult<Self> {
        let mut json = Self { out, images: 0 };
        json.field("schema_version", &plan.schema_version, true)?;
        json.field("roots", &plan.roots, false)?;
        json.field("folders", &plan.folders, false)?;
        write!(json.out, ",\n  \"images\": ")?;
        Ok(json)
    }
    pub fn image(&mut self, image: &ImportedImage) -> EngineResult<()> {
        self.out.write_all(if self.images == 0 {
            b"[\n    "
        } else {
            b",\n    "
        })?;
        self.images += 1;
        self.nested(image, "    ")
    }
    /// Close `images` and write the fields that follow it (the plan's own
    /// `images` are ignored). Returns the writer.
    pub fn finish(mut self, plan: &ImportPlan) -> EngineResult<W> {
        self.out
            .write_all(if self.images == 0 { b"[]" } else { b"\n  ]" })?;
        self.field("library", &plan.library, false)?;
        self.field("stacks", &plan.stacks, false)?;
        self.field("face_clusters", &plan.face_clusters, false)?;
        self.field("keyword_faces", &plan.keyword_faces, false)?;
        self.field("report", &plan.report, false)?;
        self.out.write_all(b"\n}")?;
        Ok(self.out)
    }
}

#[cfg(test)]
mod report_tests {
    use super::ImageReport;

    #[test]
    fn lrcat_report_groups_all_but_unedited_and_duplicate_ids() {
        let individual = [
            "never developed; imported as unedited",
            "edits failed to import (invalid Lua); imported as unedited",
            "duplicate Adobe_images id; last-write-wins",
            "duplicate develop image id; last-write-wins",
        ];
        let grouped = [
            "Future: unknown Lua develop key; source preserved",
            super::lua_develop::EXTENDED_TONE_CURVE_NOTE,
            "crs:Future: unsupported property; source preserved",
            "legacy Adobe PV1/2: best-effort translation; rendering fidelity is not guaranteed",
            "a future harmless note",
        ];
        let mut report = ImageReport::default();
        for id in [10, 20] {
            for note in individual.iter().chain(grouped.iter()) {
                report.push(id, (*note).into());
            }
            // Repeated diagnostics within an image count that image once.
            report.push(id, grouped[2].into());
        }
        let entries: Vec<_> = report.finish().collect();
        assert_eq!(entries.len(), 13);
        for note in individual {
            for id in [10, 20] {
                assert!(entries.contains(&format!("image {id}: {note}")));
            }
        }
        for note in grouped {
            assert!(entries.contains(&format!("2 images (first: image 10): {note}")));
        }
        let mut singleton = ImageReport::default();
        singleton.push(30, "harmless singleton".into());
        assert_eq!(
            singleton.finish().collect::<Vec<_>>(),
            ["image 30: harmless singleton"]
        );
    }
}
