//! SQLite photo catalog and search index.
mod api;
mod predicate;
mod semantic;
pub use api::{FaceRecord, ImageInfo, Index, Scanner, Score};
pub use predicate::{Comparison, Predicate};
pub use semantic::SemanticSearch;
use std::{path::Path, time::UNIX_EPOCH};

use engine_api::{
    id::ImageId,
    recipe::{Decision, Grade, Mark, Selection},
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use walkdir::WalkDir;

#[derive(Debug, Error)]
enum IndexError {
    #[error(transparent)]
    Engine(#[from] engine_api::error::EngineError),
    #[error("sqlite: {0}")]
    Sql(#[from] rusqlite::Error),
    #[error("filesystem: {0}")]
    Io(#[from] std::io::Error),
    #[error("directory walk: {0}")]
    Walk(#[from] walkdir::Error),
    #[error("metadata: {0}")]
    Metadata(String),
}
type Result<T> = std::result::Result<T, IndexError>;

#[derive(Debug)]
struct Core {
    conn: Connection,
}

impl Core {
    /// Opens an index and applies all known schema migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "mmap_size", 268_435_456_i64)?;
        conn.execute_batch("PRAGMA foreign_keys=ON;
            BEGIN IMMEDIATE;
            CREATE TABLE IF NOT EXISTS migration(version INTEGER PRIMARY KEY, applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP);
            CREATE TABLE IF NOT EXISTS root(id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS folder(id INTEGER PRIMARY KEY, root_id INTEGER NOT NULL REFERENCES root(id), path TEXT NOT NULL UNIQUE);
            CREATE TABLE IF NOT EXISTS file(id INTEGER PRIMARY KEY, folder_id INTEGER NOT NULL REFERENCES folder(id), path TEXT NOT NULL UNIQUE, name TEXT NOT NULL, size INTEGER NOT NULL, mtime INTEGER NOT NULL);
            CREATE TABLE IF NOT EXISTS image(id TEXT PRIMARY KEY, file_id INTEGER NOT NULL UNIQUE REFERENCES file(id), capture_time TEXT, camera TEXT, lens TEXT, caption TEXT, latitude REAL, longitude REAL);
            CREATE TABLE IF NOT EXISTS metadata(image_id TEXT NOT NULL REFERENCES image(id) ON DELETE CASCADE, key TEXT NOT NULL, value TEXT NOT NULL, PRIMARY KEY(image_id,key));
            CREATE TABLE IF NOT EXISTS keyword(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, parent_id INTEGER REFERENCES keyword(id));
            CREATE TABLE IF NOT EXISTS keyword_closure(ancestor_id INTEGER NOT NULL REFERENCES keyword(id), descendant_id INTEGER NOT NULL REFERENCES keyword(id), depth INTEGER NOT NULL, PRIMARY KEY(ancestor_id,descendant_id));
            CREATE TABLE IF NOT EXISTS image_keyword(image_id TEXT NOT NULL REFERENCES image(id), keyword_id INTEGER NOT NULL REFERENCES keyword(id), PRIMARY KEY(image_id,keyword_id));
            CREATE TABLE IF NOT EXISTS selection(image_id TEXT PRIMARY KEY REFERENCES image(id), decision TEXT NOT NULL DEFAULT 'undecided', grade INTEGER, mark TEXT, CHECK(grade IS NULL OR decision='keep'));
            CREATE TABLE IF NOT EXISTS recipe_hash(image_id TEXT PRIMARY KEY REFERENCES image(id), hash TEXT NOT NULL);
            CREATE TABLE IF NOT EXISTS preview(hash TEXT PRIMARY KEY, state TEXT NOT NULL);
            CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(image_id UNINDEXED, filename, keywords, caption, camera, lens);
            CREATE INDEX IF NOT EXISTS file_path_idx ON file(path);
            CREATE INDEX IF NOT EXISTS image_camera_idx ON image(camera);
            CREATE INDEX IF NOT EXISTS image_lens_idx ON image(lens);
            CREATE INDEX IF NOT EXISTS image_latlon_idx ON image(latitude,longitude);
            INSERT OR IGNORE INTO migration(version) VALUES(1);
            COMMIT;")?;
        let version: u32 =
            conn.query_row("SELECT max(version) FROM migration", [], |r| r.get(0))?;
        if version > 5 {
            return Err(engine_api::error::EngineError::SchemaVersion {
                document: "index".into(),
                found: version,
                supported: 5,
            }
            .into());
        }
        if version < 2 {
            conn.execute_batch("BEGIN IMMEDIATE;
                ALTER TABLE file ADD COLUMN sidecar_stamp TEXT NOT NULL DEFAULT '';
                CREATE INDEX image_date_idx ON image(capture_time,id);
                CREATE INDEX keyword_reverse_idx ON image_keyword(keyword_id,image_id);
                CREATE VIRTUAL TABLE gps USING rtree(id,min_lat,max_lat,min_lon,max_lon);
                CREATE TRIGGER gps_insert AFTER INSERT ON image WHEN new.latitude IS NOT NULL AND new.longitude IS NOT NULL BEGIN
                    INSERT INTO gps VALUES(new.rowid,new.latitude,new.latitude,new.longitude,new.longitude); END;
                CREATE TRIGGER gps_update AFTER UPDATE OF latitude,longitude ON image BEGIN
                    DELETE FROM gps WHERE id=old.rowid;
                    INSERT INTO gps SELECT new.rowid,new.latitude,new.latitude,new.longitude,new.longitude WHERE new.latitude IS NOT NULL AND new.longitude IS NOT NULL; END;
                CREATE TRIGGER gps_delete AFTER DELETE ON image BEGIN DELETE FROM gps WHERE id=old.rowid; END;
                INSERT INTO gps SELECT rowid,latitude,latitude,longitude,longitude FROM image WHERE latitude IS NOT NULL AND longitude IS NOT NULL;
                INSERT INTO migration(version) VALUES(2);
                COMMIT;")?;
        }
        if version < 3 {
            conn.execute_batch("BEGIN IMMEDIATE;
                DELETE FROM fts;
                INSERT INTO fts(rowid,image_id,filename,keywords,caption,camera,lens)
                SELECT i.rowid,i.id,f.name,(SELECT group_concat(k.name,' ') FROM keyword k JOIN image_keyword ik ON k.id=ik.keyword_id WHERE ik.image_id=i.id),i.caption,i.camera,i.lens FROM image i JOIN file f ON f.id=i.file_id;
                INSERT INTO migration(version) VALUES(3); COMMIT;")?;
        }
        if version < 4 {
            conn.execute_batch(include_str!("../migrations/004_culling.sql"))?;
        }
        if version < 5 {
            conn.execute_batch(include_str!("../migrations/005_faces.sql"))?;
        }
        Ok(Self { conn })
    }

    /// Scans a filesystem root and indexes supported image files.
    pub fn scan(
        &mut self,
        root: impl AsRef<Path>,
        sidecars: &dyn SidecarReader,
        metadata: &dyn MetadataProvider,
    ) -> Result<usize> {
        let root_path = root.as_ref().canonicalize()?;
        let root = root_path.as_path();
        if !root.is_dir() {
            return Err(
                engine_api::error::EngineError::invalid("root", "expected a directory").into(),
            );
        }
        let root_s = root.to_string_lossy().into_owned();
        self.conn
            .execute("INSERT OR IGNORE INTO root(path) VALUES(?)", [&root_s])?;
        let root_id: i64 =
            self.conn
                .query_row("SELECT id FROM root WHERE path=?", [&root_s], |r| r.get(0))?;
        let mut changed = 0;
        for entry in WalkDir::new(root).follow_links(false).into_iter() {
            let entry = entry?;
            if !entry.file_type().is_file() || !is_image(entry.path()) {
                continue;
            }
            let path = entry.path();
            let path_s = path.to_string_lossy().into_owned();
            let md = entry.metadata()?;
            let size = md.len() as i64;
            let mtime = md
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_nanos() as i64);
            let stamp = sidecar_stamp(path)?;
            let old: Option<(i64, i64, String)> = self
                .conn
                .query_row(
                    "SELECT size,mtime,sidecar_stamp FROM file WHERE path=?",
                    [&path_s],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            if old == Some((size, mtime, stamp.clone())) {
                continue;
            }
            let folder_s = path.parent().unwrap_or(root).to_string_lossy().into_owned();
            self.conn.execute(
                "INSERT OR IGNORE INTO folder(root_id,path) VALUES(?,?)",
                params![root_id, folder_s],
            )?;
            let folder_id: i64 =
                self.conn
                    .query_row("SELECT id FROM folder WHERE path=?", [&folder_s], |r| {
                        r.get(0)
                    })?;
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let id = stable_id(&path_s);
            let mut data = basic_metadata(path)?;
            let supplied = metadata.read(path)?;
            data.values.extend(supplied.values);
            data.capture_time = supplied.capture_time.or(data.capture_time);
            data.camera = supplied.camera.or(data.camera);
            data.lens = supplied.lens.or(data.lens);
            data.latitude = supplied.latitude.or(data.latitude);
            data.longitude = supplied.longitude.or(data.longitude);
            let side = sidecars.read(path)?;
            let tx = self.conn.transaction()?;
            tx.execute("INSERT INTO file(folder_id,path,name,size,mtime) VALUES(?,?,?,?,?) ON CONFLICT(path) DO UPDATE SET folder_id=excluded.folder_id,name=excluded.name,size=excluded.size,mtime=excluded.mtime", params![folder_id,path_s,name,size,mtime])?;
            tx.execute(
                "UPDATE file SET sidecar_stamp=? WHERE path=?",
                params![stamp, path_s],
            )?;
            let file_id: i64 =
                tx.query_row("SELECT id FROM file WHERE path=?", [&path_s], |r| r.get(0))?;
            tx.execute(
                "INSERT OR IGNORE INTO image(id,file_id) VALUES(?,?)",
                params![id, file_id],
            )?;
            tx.execute("INSERT INTO image(id,file_id,capture_time,camera,lens,caption,latitude,longitude) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET file_id=excluded.file_id,capture_time=excluded.capture_time,camera=excluded.camera,lens=excluded.lens,caption=excluded.caption,latitude=excluded.latitude,longitude=excluded.longitude", params![id,file_id,data.capture_time,data.camera,data.lens,side.caption,data.latitude,data.longitude])?;
            tx.execute("DELETE FROM metadata WHERE image_id=?", [&id])?;
            for (key, value) in data.values.into_iter().chain(side.values) {
                tx.execute(
                    "INSERT OR REPLACE INTO metadata(image_id,key,value) VALUES(?,?,?)",
                    params![id, key, value],
                )?;
            }
            tx.execute("INSERT OR IGNORE INTO selection(image_id) VALUES(?)", [&id])?;
            if let Some(selection) = side.selection {
                let s = selection.normalized();
                tx.execute(
                    "UPDATE selection SET decision=?,grade=?,mark=? WHERE image_id=?",
                    params![
                        decision_str(s.decision),
                        s.grade.map(u8::from),
                        s.mark.map(|m| m.0),
                        id
                    ],
                )?;
            }
            tx.execute("DELETE FROM image_keyword WHERE image_id=?", [&id])?;
            for keyword in &side.keywords {
                tx.execute("INSERT OR IGNORE INTO keyword(name) VALUES(?)", [keyword])?;
                tx.execute("INSERT OR IGNORE INTO keyword_closure SELECT id,id,0 FROM keyword WHERE name=?", [keyword])?;
                tx.execute("INSERT INTO image_keyword SELECT ?,id FROM keyword WHERE name=? ON CONFLICT DO NOTHING", params![id,keyword])?;
            }
            if let Some(hash) = side.recipe_hash {
                tx.execute(
                    "INSERT OR REPLACE INTO recipe_hash VALUES(?,?)",
                    params![id, hash.to_string()],
                )?;
            } else {
                tx.execute("DELETE FROM recipe_hash WHERE image_id=?", [&id])?;
            }
            tx.execute(
                "DELETE FROM fts WHERE rowid=(SELECT rowid FROM image WHERE id=?)",
                [&id],
            )?;
            tx.execute("INSERT INTO fts(rowid,image_id,filename,keywords,caption,camera,lens) VALUES((SELECT rowid FROM image WHERE id=?1),?1,?2,?3,?4,?5,?6)", params![id,name,side.keywords.join(" "),side.caption.unwrap_or_default(),data.camera.unwrap_or_default(),data.lens.unwrap_or_default()])?;
            tx.commit()?;
            changed += 1;
        }
        Ok(changed)
    }

    /// Search matching images, ordered by capture time then stable id.
    pub fn search(&self, query: &Query) -> Result<Vec<ImageId>> {
        let sql = format!(
            "SELECT i.id FROM image i JOIN file f ON f.id=i.file_id LEFT JOIN selection s ON s.image_id=i.id WHERE 1=1 {} ORDER BY i.capture_time,i.id LIMIT ? OFFSET ?",
            filter_sql(query)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let values = query_params(query, true);
        stmt.query_map(
            rusqlite::params_from_iter(values.iter().map(|v| v.as_ref())),
            |r| r.get::<_, String>(0),
        )?
        .map(|r| r.map_err(IndexError::from).and_then(|x| parse_id(&x)))
        .collect()
    }

    /// Counts camera, lens, keyword and decision values over the matching set.
    pub fn facets(&self, query: &Query) -> Result<Facets> {
        let base = format!(
            "FROM image i JOIN file f ON f.id=i.file_id LEFT JOIN selection s ON s.image_id=i.id WHERE 1=1 {}",
            filter_sql(query)
        );
        Ok(Facets {
            cameras: self.counts(
                &format!("SELECT COALESCE(i.camera,''),count(*) {base} GROUP BY i.camera"),
                query,
            )?,
            lenses: self.counts(
                &format!("SELECT COALESCE(i.lens,''),count(*) {base} GROUP BY i.lens"),
                query,
            )?,
            decisions: self.counts(
                &format!("SELECT COALESCE(s.decision,'undecided'),count(*) {base} GROUP BY COALESCE(s.decision,'undecided')"),
                query,
            )?,
            keywords: self.keyword_counts(query)?,
        })
    }
    fn counts(&self, sql: &str, query: &Query) -> Result<Vec<(String, u64)>> {
        let mut stmt = self.conn.prepare(sql)?;
        let values = query_params(query, false);
        Ok(stmt
            .query_map(
                rusqlite::params_from_iter(values.iter().map(|v| v.as_ref())),
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }
    fn keyword_counts(&self, query: &Query) -> Result<Vec<(String, u64)>> {
        let sql = format!(
            "SELECT k.name,count(*) FROM keyword k JOIN image_keyword ik ON ik.keyword_id=k.id JOIN image i ON i.id=ik.image_id JOIN file f ON f.id=i.file_id LEFT JOIN selection s ON s.image_id=i.id WHERE 1=1 {} GROUP BY k.name",
            filter_sql(query)
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let values = query_params(query, false);
        Ok(stmt
            .query_map(
                rusqlite::params_from_iter(values.iter().map(|v| v.as_ref())),
                |r| Ok((r.get(0)?, r.get::<_, i64>(1)? as u64)),
            )?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Saves normalized recipe selection state.
    pub fn set_selection(&self, id: ImageId, selection: &Selection) -> Result<()> {
        let s = selection.clone().normalized();
        self.conn.execute("INSERT INTO selection(image_id,decision,grade,mark) VALUES(?,?,?,?) ON CONFLICT(image_id) DO UPDATE SET decision=excluded.decision,grade=excluded.grade,mark=excluded.mark",params![id.to_string(),decision_str(s.decision),s.grade.map(|g|g as u8),s.mark.map(|m|m.0)])?;
        Ok(())
    }
    /// Loads selection state for an image.
    pub fn selection(&self, id: ImageId) -> Result<Option<Selection>> {
        self.conn
            .query_row(
                "SELECT decision,grade,mark FROM selection WHERE image_id=?",
                [id.to_string()],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, Option<u8>>(1)?,
                        r.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()?
            .map(|(d, g, m)| {
                Ok(Selection {
                    decision: parse_decision(&d),
                    grade: g.map(grade),
                    mark: m.map(Mark),
                })
            })
            .transpose()
    }

    /// Inserts a keyword and its ancestor relationships.
    pub fn add_keyword(&self, name: &str, parent: Option<&str>) -> Result<i64> {
        let existing: Option<Option<String>> = self.conn.query_row("SELECT p.name FROM keyword k LEFT JOIN keyword p ON p.id=k.parent_id WHERE k.name=?", [name], |r| r.get(0)).optional()?;
        if let Some(existing) = existing
            && existing.as_deref() != parent
        {
            return Err(engine_api::error::EngineError::invalid(
                "parent",
                "keyword already has a different parent",
            )
            .into());
        }
        if let Some(p) = parent {
            let exists: bool = self.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM keyword WHERE name=?)",
                [p],
                |r| r.get(0),
            )?;
            if !exists || p == name {
                return Err(engine_api::error::EngineError::invalid(
                    "parent",
                    "missing parent or cycle",
                )
                .into());
            }
        }
        self.conn.execute("INSERT OR IGNORE INTO keyword(name,parent_id) VALUES(?,(SELECT id FROM keyword WHERE name=?))",params![name,parent])?;
        let id: i64 = self
            .conn
            .query_row("SELECT id FROM keyword WHERE name=?", [name], |r| r.get(0))?;
        self.conn.execute(
            "INSERT OR IGNORE INTO keyword_closure VALUES(?,?,0)",
            params![id, id],
        )?;
        if let Some(p) = parent {
            let pid: i64 =
                self.conn
                    .query_row("SELECT id FROM keyword WHERE name=?", [p], |r| r.get(0))?;
            self.conn.execute("INSERT OR IGNORE INTO keyword_closure SELECT ancestor_id,?,depth+1 FROM keyword_closure WHERE descendant_id=?",params![id,pid])?;
        }
        Ok(id)
    }
    /// Attaches a keyword to an image.
    pub fn tag(&self, id: ImageId, keyword: &str) -> Result<()> {
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM keyword WHERE name=?)",
            [keyword],
            |r| r.get(0),
        )?;
        if !exists {
            return Err(engine_api::error::EngineError::not_found("keyword", keyword).into());
        }
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO image_keyword SELECT ?,id FROM keyword WHERE name=?",
            params![id.to_string(), keyword],
        )?;
        tx.execute("UPDATE fts SET keywords=(SELECT group_concat(k.name,' ') FROM keyword k JOIN image_keyword ik ON ik.keyword_id=k.id WHERE ik.image_id=?) WHERE rowid=(SELECT rowid FROM image WHERE id=?)", params![id.to_string(),id.to_string()])?;
        tx.commit()?;
        Ok(())
    }
    /// Finds images tagged with a keyword or any descendant.
    pub fn images_with_keyword(&self, name: &str) -> Result<Vec<ImageId>> {
        let mut s=self.conn.prepare("SELECT DISTINCT ik.image_id FROM keyword k JOIN keyword_closure c ON c.ancestor_id=k.id JOIN image_keyword ik ON ik.keyword_id=c.descendant_id WHERE k.name=? ORDER BY ik.image_id")?;
        s.query_map([name], |r| r.get::<_, String>(0))?
            .map(|r| parse_id(&r?))
            .collect()
    }
}

/// Source for sidecar metadata. Implemented by the sidecar crate later.
pub trait SidecarReader: Send + Sync {
    fn read(&self, path: &Path) -> engine_api::error::EngineResult<SidecarData>;
}
/// No-op sidecar reader used until sidecar integration is available.
pub struct NoopSidecarReader;
impl SidecarReader for NoopSidecarReader {
    fn read(&self, _: &Path) -> engine_api::error::EngineResult<SidecarData> {
        Ok(SidecarData::default())
    }
}
/// Sidecar fields relevant to indexing.
#[derive(Default, Debug, Clone)]
pub struct SidecarData {
    pub caption: Option<String>,
    pub keywords: Vec<String>,
    pub values: Vec<(String, String)>,
    pub selection: Option<Selection>,
    pub recipe_hash: Option<engine_api::id::Digest>,
}
/// Source for embedded metadata. Raw decoders can supply metadata later.
pub trait MetadataProvider: Send + Sync {
    fn read(&self, path: &Path) -> engine_api::error::EngineResult<Metadata>;
}
/// No-op metadata provider for raw-only formats.
pub struct NoopMetadataProvider;
impl MetadataProvider for NoopMetadataProvider {
    fn read(&self, _: &Path) -> engine_api::error::EngineResult<Metadata> {
        Ok(Metadata::default())
    }
}
/// Flattened metadata values.
#[derive(Default, Debug, Clone)]
pub struct Metadata {
    pub values: Vec<(String, String)>,
    pub capture_time: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}
fn sidecar_stamp(path: &Path) -> Result<String> {
    let mut appended = path.as_os_str().to_os_string();
    appended.push(".xmp");
    let mut recipe_name = path.file_stem().unwrap_or_default().to_os_string();
    recipe_name.push(".json");
    let recipe = path
        .parent()
        .unwrap_or(Path::new("."))
        .join(".edits")
        .join(recipe_name);
    let mut hash = blake3::Hasher::new();
    for p in [
        std::path::PathBuf::from(appended),
        path.with_extension("xmp"),
        recipe,
    ] {
        match std::fs::read(&p) {
            Ok(bytes) => {
                hash.update(&[1]);
                hash.update(&(bytes.len() as u64).to_le_bytes());
                hash.update(&bytes);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                hash.update(&[0]);
            }
            Err(e) => return Err(e.into()),
        }
    }
    Ok(hash.finalize().to_hex().to_string())
}

fn basic_metadata(path: &Path) -> Result<Metadata> {
    let ext = path
        .extension()
        .and_then(|x| x.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !["jpg", "jpeg", "tif", "tiff", "dng"].contains(&ext.as_str()) {
        return Ok(Metadata::default());
    }
    let file = std::fs::File::open(path)?;
    let reader = exif::Reader::new();
    let mut out = Metadata::default();
    if let Ok(exif) = reader.read_from_container(&mut std::io::BufReader::new(file)) {
        for f in exif.fields() {
            let key = format!("{}:{}", f.ifd_num, f.tag);
            let val = match &f.value {
                exif::Value::Ascii(values) => values
                    .first()
                    .map(|v| String::from_utf8_lossy(v).trim_end_matches('\0').to_owned())
                    .unwrap_or_default(),
                _ => f.display_value().with_unit(&exif).to_string(),
            };
            if f.tag == exif::Tag::Model || (f.tag == exif::Tag::Make && out.camera.is_none()) {
                out.camera = Some(val.clone())
            }
            if f.tag == exif::Tag::LensModel {
                out.lens = Some(val.clone())
            }
            if f.tag == exif::Tag::DateTimeOriginal {
                out.capture_time = Some(val.replacen(':', "-", 2).replacen(' ', "T", 1))
            }
            out.values.push((key, val));
        }
    }
    Ok(out)
}

/// Query filters and pagination.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Query {
    /// Boolean expression ANDed with every legacy filter.
    #[serde(default)]
    pub predicate: Option<Predicate>,
    pub text: Option<String>,
    /// Natural-language vector query. Use `Index::search_with_semantic`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<String>,
    pub folder: Option<String>,
    pub decision: Option<Decision>,
    pub grade: Option<Grade>,
    pub mark: Option<String>,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    pub keyword: Option<String>,
    pub limit: usize,
    pub offset: usize,
}
#[derive(Debug, Default, Clone)]
pub struct Facets {
    pub cameras: Vec<(String, u64)>,
    pub lenses: Vec<(String, u64)>,
    pub keywords: Vec<(String, u64)>,
    pub decisions: Vec<(String, u64)>,
}
fn filter_sql(q: &Query) -> String {
    let mut s = String::new();
    if q.text.is_some() {
        s.push_str(" AND i.rowid IN (SELECT rowid FROM fts WHERE fts MATCH ?)")
    }
    if q.folder.is_some() {
        s.push_str(" AND f.path GLOB ?")
    }
    if q.decision.is_some() {
        s.push_str(" AND s.decision=?")
    }
    if q.grade.is_some() {
        s.push_str(" AND s.grade=?")
    }
    if q.mark.is_some() {
        s.push_str(" AND s.mark=?")
    }
    if q.date_from.is_some() {
        s.push_str(" AND i.capture_time>=?")
    }
    if q.date_to.is_some() {
        s.push_str(" AND i.capture_time<=?")
    }
    if q.camera.is_some() {
        s.push_str(" AND i.camera=?")
    }
    if q.lens.is_some() {
        s.push_str(" AND i.lens=?")
    }
    if q.keyword.is_some() {
        s.push_str(" AND i.id IN (SELECT ik.image_id FROM keyword k JOIN keyword_closure c ON c.ancestor_id=k.id JOIN image_keyword ik ON ik.keyword_id=c.descendant_id WHERE k.name=?)")
    }
    if let Some(predicate) = &q.predicate {
        s.push_str(" AND (");
        s.push_str(&predicate.compile(&mut Vec::new()));
        s.push(')');
    }
    s
}
fn query_params(q: &Query, paged: bool) -> Vec<Box<dyn rusqlite::types::ToSql>> {
    let mut v: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    if let Some(x) = &q.text {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.folder {
        let folder = x
            .trim_end_matches('/')
            .replace('[', "[[]")
            .replace('*', "[*]")
            .replace('?', "[?]");
        v.push(Box::new(format!("{folder}/*")))
    }
    if let Some(x) = q.decision {
        v.push(Box::new(decision_str(x)))
    }
    if let Some(x) = q.grade {
        v.push(Box::new(x as u8))
    }
    if let Some(x) = &q.mark {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.date_from {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.date_to {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.camera {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.lens {
        v.push(Box::new(x.clone()))
    }
    if let Some(x) = &q.keyword {
        v.push(Box::new(x.clone()))
    }
    if let Some(predicate) = &q.predicate {
        predicate.compile(&mut v);
    }
    if paged {
        v.push(Box::new((if q.limit == 0 { 100 } else { q.limit }) as i64));
        v.push(Box::new(q.offset as i64));
    }
    v
}
fn decision_str(d: Decision) -> &'static str {
    match d {
        Decision::Reject => "reject",
        Decision::Undecided => "undecided",
        Decision::Keep => "keep",
    }
}
fn parse_decision(s: &str) -> Decision {
    match s {
        "keep" => Decision::Keep,
        "reject" => Decision::Reject,
        _ => Decision::Undecided,
    }
}
fn grade(v: u8) -> Grade {
    match v {
        1 => Grade::One,
        2 => Grade::Two,
        _ => Grade::Three,
    }
}
fn stable_id(path: &str) -> String {
    let h = blake3::hash(path.as_bytes());
    h.as_bytes()[..16]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn parse_id(s: &str) -> Result<ImageId> {
    u128::from_str_radix(s, 16)
        .map(ImageId)
        .map_err(|e| IndexError::Metadata(e.to_string()))
}
fn is_image(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|x| x.to_str())
            .unwrap_or("")
            .to_ascii_lowercase()
            .as_str(),
        "jpg"
            | "jpeg"
            | "tif"
            | "tiff"
            | "dng"
            | "cr2"
            | "cr3"
            | "arw"
            | "nef"
            | "raf"
            | "rw2"
            | "orf"
    )
}

#[cfg(test)]
mod tests {
    use super::Core as Index;
    use super::*;
    use std::time::Instant;
    #[test]
    fn embedded_tiff_camera_is_searchable_without_quotes() {
        let dir = tempfile::tempdir().unwrap();
        // Little-endian TIFF with a single ASCII Model entry.
        let mut tiff = b"II\x2a\x00\x08\x00\x00\x00\x01\x00\x10\x01\x02\x00\x08\x00\x00\x00\x1a\x00\x00\x00\x00\x00\x00\x00".to_vec();
        tiff.extend_from_slice(b"CameraX\0");
        std::fs::write(dir.path().join("a.tiff"), tiff).unwrap();
        let mut i = Index::open(":memory:").unwrap();
        i.scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        assert_eq!(
            i.search(&Query {
                camera: Some("CameraX".into()),
                ..Default::default()
            })
            .unwrap()
            .len(),
            1
        );
    }

    #[test]
    fn persistent_migrations_pragmas_and_rtree() {
        struct Gps;
        impl MetadataProvider for Gps {
            fn read(&self, _: &Path) -> engine_api::error::EngineResult<Metadata> {
                Ok(Metadata {
                    latitude: Some(40.5),
                    longitude: Some(-73.5),
                    ..Default::default()
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let db = dir.path().join("index.sqlite");
        std::fs::write(dir.path().join("a.cr3"), b"raw").unwrap();
        let mut i = Index::open(&db).unwrap();
        i.scan(dir.path(), &NoopSidecarReader, &Gps).unwrap();
        assert_eq!(
            i.conn
                .query_row(
                    "SELECT count(*) FROM gps WHERE min_lat<=41 AND max_lat>=40",
                    [],
                    |r| r.get::<_, u32>(0)
                )
                .unwrap(),
            1
        );
        assert_eq!(
            i.conn
                .query_row("PRAGMA journal_mode", [], |r| r.get::<_, String>(0))
                .unwrap(),
            "wal"
        );
        assert_eq!(
            i.conn
                .query_row("PRAGMA synchronous", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            1
        );
        drop(i);
        let mut i = Index::open(&db).unwrap();
        let changes = i.conn.total_changes();
        assert_eq!(i.scan(dir.path(), &NoopSidecarReader, &Gps).unwrap(), 0);
        assert_eq!(i.conn.total_changes(), changes);
        assert_eq!(
            i.conn
                .query_row("SELECT count(*) FROM migration", [], |r| r.get::<_, u32>(0))
                .unwrap(),
            5
        );
    }
    #[test]
    fn sidecar_changes_refresh_keywords_and_selection() {
        struct Reader;
        impl SidecarReader for Reader {
            fn read(&self, path: &Path) -> engine_api::error::EngineResult<SidecarData> {
                Ok(SidecarData {
                    keywords: vec![
                        std::fs::read_to_string(path.with_extension("jpg.xmp")).unwrap_or_default(),
                    ],
                    ..Default::default()
                })
            }
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.jpg"), b"jpeg").unwrap();
        std::fs::write(dir.path().join("a.jpg.xmp"), b"family").unwrap();
        let mut i = Index::open(":memory:").unwrap();
        i.scan(dir.path(), &Reader, &NoopMetadataProvider).unwrap();
        assert_eq!(i.images_with_keyword("family").unwrap().len(), 1);
        std::fs::write(dir.path().join("a.jpg.xmp"), b"travel").unwrap();
        assert_eq!(
            i.scan(dir.path(), &Reader, &NoopMetadataProvider).unwrap(),
            1
        );
        assert!(i.images_with_keyword("family").unwrap().is_empty());
        assert_eq!(i.images_with_keyword("travel").unwrap().len(), 1);
        let writes = i.conn.total_changes();
        assert_eq!(
            i.scan(dir.path(), &Reader, &NoopMetadataProvider).unwrap(),
            0
        );
        assert_eq!(writes, i.conn.total_changes());
    }
    #[test]
    fn migrations_create_fts_and_selection_round_trip() {
        let i = Index::open(":memory:").unwrap();
        let tables: i64 = i
            .conn
            .query_row("SELECT count(*) FROM migration", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tables, 5);
        let id = ImageId(7);
        i.conn
            .execute("INSERT INTO root(path) VALUES('root')", [])
            .unwrap();
        i.conn
            .execute("INSERT INTO folder(root_id,path) VALUES(1,'root')", [])
            .unwrap();
        i.conn.execute("INSERT INTO file(folder_id,path,name,size,mtime) VALUES(1,'root/a.jpg','a.jpg',1,1)", []).unwrap();
        i.conn
            .execute(
                "INSERT INTO image(id,file_id) VALUES('00000000000000000000000000000007',1)",
                [],
            )
            .unwrap();
        let selection = Selection {
            decision: Decision::Keep,
            grade: Some(Grade::Two),
            mark: Some(Mark::new("portfolio")),
        };
        i.set_selection(id, &selection).unwrap();
        assert_eq!(i.selection(id).unwrap(), Some(selection));
    }
    #[test]
    fn keyword_closure_includes_descendants() {
        let i = Index::open(":memory:").unwrap();
        i.add_keyword("People", None).unwrap();
        i.add_keyword("Family", Some("People")).unwrap();
        let n:i64=i.conn.query_row("SELECT count(*) FROM keyword_closure c JOIN keyword k ON k.id=c.ancestor_id WHERE k.name='People' AND c.depth=1",[],|r|r.get(0)).unwrap();
        assert_eq!(n, 1);
    }
    #[test]
    fn incremental_scan_skips_unchanged_files() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.jpg");
        std::fs::write(&file, b"not really jpeg").unwrap();
        let mut i = Index::open(":memory:").unwrap();
        let side = NoopSidecarReader;
        let meta = NoopMetadataProvider;
        assert_eq!(i.scan(dir.path(), &side, &meta).unwrap(), 1);
        assert_eq!(i.scan(dir.path(), &side, &meta).unwrap(), 0);
    }

    #[test]
    fn empty_facets_are_valid() {
        let i = Index::open(":memory:").unwrap();
        let facets = i.facets(&Query::default()).unwrap();
        assert!(facets.cameras.is_empty());
        assert!(facets.lenses.is_empty());
        assert!(facets.decisions.is_empty());
    }

    #[test]
    #[ignore = "100k-row performance benchmark; run with cargo test -p index -- --ignored"]
    fn benchmark_search_and_facets_at_100k_images() {
        let i = Index::open(":memory:").unwrap();
        i.conn
            .execute_batch(
                "CREATE TEMP TABLE benchmark_ids(id TEXT PRIMARY KEY);
             INSERT INTO root(path) VALUES('/photos');
             INSERT INTO folder(root_id,path) VALUES(1,'/photos/2026');",
            )
            .unwrap();
        let tx = i.conn.unchecked_transaction().unwrap();
        {
            let mut file = tx.prepare("INSERT INTO file(folder_id,path,name,size,mtime) VALUES(1,?,?,24000000,1780000000)").unwrap();
            let mut image = tx.prepare("INSERT INTO image(id,file_id,capture_time,camera,lens,caption) VALUES(?,?, '2026-06-01',?,?,?)").unwrap();
            let mut selection = tx
                .prepare("INSERT INTO selection(image_id,decision,grade) VALUES(?,'keep',3)")
                .unwrap();
            let mut fts = tx.prepare("INSERT INTO fts(rowid,image_id,filename,keywords,caption,camera,lens) VALUES((SELECT rowid FROM image WHERE id=?1),?1,?2,'travel',?3,?4,?5)").unwrap();
            tx.execute("INSERT INTO keyword(name) VALUES('travel')", [])
                .unwrap();
            tx.execute("INSERT INTO keyword_closure VALUES(1,1,0)", [])
                .unwrap();
            let mut tag = tx.prepare("INSERT INTO image_keyword VALUES(?,1)").unwrap();
            for n in 0..100_000_u32 {
                let path = format!("/photos/2026/IMG_{n:06}.jpg");
                let id = stable_id(&path);
                file.execute(params![path, format!("IMG_{n:06}.jpg")])
                    .unwrap();
                let file_id = tx.last_insert_rowid();
                let camera = format!("Camera {}", n % 10);
                let lens = format!("Lens {}", n % 7);
                let caption = if n % 10 == 0 {
                    "travel landscape mountain"
                } else {
                    "travel portrait city"
                };
                image
                    .execute(params![id, file_id, camera, lens, caption])
                    .unwrap();
                selection.execute([&id]).unwrap();
                tag.execute([&id]).unwrap();
                fts.execute(params![
                    id,
                    format!("IMG_{n:06}.jpg"),
                    caption,
                    camera,
                    lens
                ])
                .unwrap();
            }
        }
        tx.commit().unwrap();

        let total: u32 = i
            .conn
            .query_row("SELECT count(*) FROM image", [], |r| r.get(0))
            .unwrap();
        assert_eq!(total, 100_000);
        let text_query = Query {
            text: Some("landscape".into()),
            limit: 100,
            ..Query::default()
        };
        i.search(&text_query).unwrap();
        let start = Instant::now();
        assert!(!i.search(&text_query).unwrap().is_empty());
        let search_elapsed = start.elapsed();

        let facet_query = Query {
            camera: Some("Camera 0".into()),
            ..Query::default()
        };
        i.facets(&facet_query).unwrap();
        let start = Instant::now();
        let facets = i.facets(&facet_query).unwrap();
        let facet_elapsed = start.elapsed();
        assert_eq!(facets.cameras, vec![("Camera 0".into(), 10_000)]);
        assert_eq!(facets.keywords, vec![("travel".into(), 10_000)]);
        println!("100k image benchmark: text search={search_elapsed:?}, facets={facet_elapsed:?}");
        assert!(
            search_elapsed.as_millis() < 100,
            "text search took {search_elapsed:?}"
        );
        assert!(
            facet_elapsed.as_millis() < 100,
            "facets took {facet_elapsed:?}"
        );
    }
}
