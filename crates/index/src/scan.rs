//! Folder scans in phases (ST-4). A host that keeps the index behind a lock
//! needs the walk, the stats, the EXIF and the sidecar reads to happen with
//! no lock held, and only the catalog writes under it, briefly. So a scan is
//! driven in steps: `begin` registers (and relocates, CS-1) the root; `walk`
//! enumerates and stamps originals; `check` asks the catalog which of a batch
//! changed (the incremental rule, M2-28) and re-keys legacy ids; `read`
//! gathers each pending file's metadata; `apply` writes a batch in one
//! transaction; `finish` trims the change log. `Index::scan` runs the same
//! steps back to back on one connection, so the one-call form and the
//! phased form index identically.
use super::*;
use engine_api::error::{EngineError, EngineResult};
use std::path::PathBuf;

/// Files written per catalog transaction when a host scans in batches; the
/// catalog lock is free between two batches.
pub const SCAN_BATCH: usize = 64;

/// A root being scanned: registered under the lock before the walk, with the
/// volume every file under it shares (nested mounts aside).
#[derive(Debug, Clone)]
pub struct ScanScope {
    root: PathBuf,
    only_file: Option<PathBuf>,
    root_id: i64,
    volume: Option<Volume>,
}
impl ScanScope {
    /// The canonical root.
    pub fn root(&self) -> &Path {
        &self.root
    }
    /// Enumerates the originals under the root (or the one explicit file):
    /// a filesystem pass with no catalog access.
    pub fn walk<'a>(&'a self, sidecars: &'a dyn SidecarReader) -> ScanWalk<'a> {
        ScanWalk {
            scope: self,
            sidecars,
            entries: WalkDir::new(self.only_file.as_deref().unwrap_or(&self.root))
                .follow_links(false)
                .into_iter()
                .filter_entry(admit_entry),
        }
    }
}

/// Drive-resident state (CS-1) is never an original.
fn admit_entry(entry: &walkdir::DirEntry) -> bool {
    entry.depth() == 0 || entry.file_name() != ".tessera"
}

/// An original the walk found, with what the skip rule compares: size,
/// modification time and the stamp of its sidecars.
#[derive(Debug, Clone)]
pub struct ScanEntry {
    path: PathBuf,
    size: i64,
    mtime: i64,
    stamp: String,
}
impl ScanEntry {
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// The lazy walk; `next_batch` is what a batching host drives.
pub struct ScanWalk<'a> {
    scope: &'a ScanScope,
    sidecars: &'a dyn SidecarReader,
    entries: walkdir::FilterEntry<walkdir::IntoIter, fn(&walkdir::DirEntry) -> bool>,
}
impl ScanWalk<'_> {
    /// Up to `n` further originals; empty once the walk is exhausted.
    pub fn next_batch(&mut self, n: usize) -> EngineResult<Vec<ScanEntry>> {
        let mut batch = Vec::with_capacity(n.min(SCAN_BATCH));
        while batch.len() < n {
            match self.next() {
                Some(entry) => batch.push(entry?),
                None => break,
            }
        }
        Ok(batch)
    }
}
impl Iterator for ScanWalk<'_> {
    type Item = EngineResult<ScanEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let entry = match self.entries.next()? {
                Ok(entry) => entry,
                Err(error) => return Some(Err(IndexError::Walk(error).into())),
            };
            // A catalog may explicitly reference a generic .raw original.
            // Preserve ordinary folder discovery's existing extension policy.
            let explicit_raw = self.scope.only_file.is_some()
                && entry
                    .path()
                    .extension()
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("raw"));
            if !entry.file_type().is_file() || !(is_image(entry.path()) || explicit_raw) {
                continue;
            }
            return Some(stat_entry(entry, self.sidecars));
        }
    }
}

fn stat_entry(entry: walkdir::DirEntry, sidecars: &dyn SidecarReader) -> EngineResult<ScanEntry> {
    let path = entry.path();
    let md = entry.metadata().map_err(IndexError::Walk)?;
    let size = md.len() as i64;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos() as i64);
    let stamp = sidecar_stamp(path, sidecars.additional_stamp_paths(path))?;
    Ok(ScanEntry {
        path: path.to_path_buf(),
        size,
        mtime,
        stamp,
    })
}

/// An entry the catalog wants refreshed, with the id its image row carries.
#[derive(Debug, Clone)]
pub struct PendingEntry {
    entry: ScanEntry,
    id: String,
}
impl PendingEntry {
    pub fn path(&self) -> &Path {
        &self.entry.path
    }
    /// Reads the file's embedded metadata and sidecars (no catalog access).
    pub fn read(
        self,
        sidecars: &dyn SidecarReader,
        metadata: &dyn MetadataProvider,
    ) -> EngineResult<ScanRecord> {
        let path = self.entry.path.as_path();
        let mut data = basic_metadata(path)?;
        let supplied = metadata.read(path)?;
        data.values.extend(supplied.values);
        data.capture_time = supplied.capture_time.or(data.capture_time);
        data.camera = supplied.camera.or(data.camera);
        data.lens = supplied.lens.or(data.lens);
        data.latitude = supplied.latitude.or(data.latitude);
        data.longitude = supplied.longitude.or(data.longitude);
        let side = sidecars.read(path)?;
        Ok(ScanRecord {
            entry: self.entry,
            id: self.id,
            data,
            side,
        })
    }
}

/// Everything `apply` writes for one original.
#[derive(Debug, Clone)]
pub struct ScanRecord {
    entry: ScanEntry,
    id: String,
    data: Metadata,
    side: SidecarData,
}

impl Core {
    /// Registers the root (relocating a moved volume's rows first) and
    /// resolves its volume. The first catalog step of a scan.
    pub(crate) fn begin_scan(&self, root: &Path, only_file: Option<&Path>) -> Result<ScanScope> {
        let root = root.canonicalize()?;
        if !root.is_dir() {
            return Err(
                engine_api::error::EngineError::invalid("root", "expected a directory").into(),
            );
        }
        let root_id = self.register_root(&root)?;
        let volume = self.volumes.volume_of(&root)?;
        Ok(ScanScope {
            root,
            only_file: only_file.map(Path::to_path_buf),
            root_id,
            volume,
        })
    }

    /// Which of `entries` the catalog has not got as they are now. A legacy
    /// path id on a volume is re-keyed in place here (one transaction for
    /// the batch), even when the file itself is unchanged.
    pub(crate) fn scan_check(
        &mut self,
        scope: &ScanScope,
        entries: Vec<ScanEntry>,
    ) -> Result<Vec<PendingEntry>> {
        let mut pending = Vec::new();
        let mut rekeys = Vec::new();
        for entry in entries {
            let path_s = entry.path.to_string_lossy().into_owned();
            let old: Option<(i64, i64, String, Option<String>)> = self
                .conn
                .query_row(
                    "SELECT f.size,f.mtime,f.sidecar_stamp,i.id FROM file f LEFT JOIN image i ON i.file_id=f.id WHERE f.path=?",
                    [&path_s],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .optional()?;
            // An indexed original keeps its id (a relocated drive derives the
            // same one anyway).
            let derived = rekey::image_id_on(scope.volume.as_ref(), &entry.path);
            let mut existing = old.as_ref().and_then(|o| o.3.clone());
            if let Some(stored) = existing.as_deref().map(parse_id).transpose()?
                && stored != derived
                && stored == ImageId::for_path(&path_s)
            {
                rekeys.push((stored, derived));
                existing = Some(derived.to_string());
            }
            if old.is_some_and(|(s, m, st, _)| {
                (s, m, st.as_str()) == (entry.size, entry.mtime, entry.stamp.as_str())
            }) {
                continue;
            }
            pending.push(PendingEntry {
                entry,
                id: existing.unwrap_or_else(|| derived.to_string()),
            });
        }
        if !rekeys.is_empty() {
            self.rekey_images(&rekeys)?;
            self.rekeyed.extend(rekeys);
        }
        Ok(pending)
    }

    /// Writes a batch of records in one transaction; returns how many.
    pub(crate) fn scan_apply(
        &mut self,
        scope: &ScanScope,
        records: &[ScanRecord],
    ) -> Result<usize> {
        if records.is_empty() {
            return Ok(0);
        }
        let tx = self.conn.transaction()?;
        for record in records {
            let path = record.entry.path.as_path();
            let path_s = path.to_string_lossy().into_owned();
            let folder_s = path
                .parent()
                .unwrap_or(&scope.root)
                .to_string_lossy()
                .into_owned();
            tx.execute(
                "INSERT OR IGNORE INTO folder(root_id,path) VALUES(?,?)",
                params![scope.root_id, folder_s],
            )?;
            let folder_id: i64 =
                tx.query_row("SELECT id FROM folder WHERE path=?", [&folder_s], |r| {
                    r.get(0)
                })?;
            let name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            let id = &record.id;
            let (data, side) = (&record.data, &record.side);
            tx.execute("INSERT INTO file(folder_id,path,name,size,mtime) VALUES(?,?,?,?,?) ON CONFLICT(path) DO UPDATE SET folder_id=excluded.folder_id,name=excluded.name,size=excluded.size,mtime=excluded.mtime", params![folder_id,path_s,name,record.entry.size,record.entry.mtime])?;
            tx.execute(
                "UPDATE file SET sidecar_stamp=? WHERE path=?",
                params![record.entry.stamp, path_s],
            )?;
            let file_id: i64 =
                tx.query_row("SELECT id FROM file WHERE path=?", [&path_s], |r| r.get(0))?;
            tx.execute(
                "INSERT OR IGNORE INTO image(id,file_id) VALUES(?,?)",
                params![id, file_id],
            )?;
            tx.execute("INSERT INTO image(id,file_id,capture_time,camera,lens,caption,latitude,longitude) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(id) DO UPDATE SET file_id=excluded.file_id,capture_time=excluded.capture_time,camera=excluded.camera,lens=excluded.lens,caption=excluded.caption,latitude=excluded.latitude,longitude=excluded.longitude", params![id,file_id,data.capture_time,data.camera,data.lens,side.caption,data.latitude,data.longitude])?;
            tx.execute("DELETE FROM metadata WHERE image_id=?", [id])?;
            for (key, value) in data.values.iter().chain(&side.values) {
                tx.execute(
                    "INSERT OR REPLACE INTO metadata(image_id,key,value) VALUES(?,?,?)",
                    params![id, key, value],
                )?;
            }
            tx.execute("INSERT OR IGNORE INTO selection(image_id) VALUES(?)", [id])?;
            if let Some(selection) = &side.selection {
                let s = selection.clone().normalized();
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
            tx.execute("DELETE FROM image_keyword WHERE image_id=?", [id])?;
            tx.execute("INSERT INTO image_keyword SELECT image_id,keyword_id FROM accepted_keyword WHERE image_id=?", [id])?;
            for keyword in &side.keywords {
                tx.execute("INSERT OR IGNORE INTO keyword(name) VALUES(?)", [keyword])?;
                tx.execute("INSERT OR IGNORE INTO keyword_closure SELECT id,id,0 FROM keyword WHERE name=?", [keyword])?;
                tx.execute("INSERT INTO image_keyword SELECT ?,id FROM keyword WHERE name=? ON CONFLICT DO NOTHING", params![id,keyword])?;
            }
            if let Some(hash) = &side.recipe_hash {
                tx.execute(
                    "INSERT OR REPLACE INTO recipe_hash VALUES(?,?)",
                    params![id, hash.to_string()],
                )?;
            } else {
                tx.execute("DELETE FROM recipe_hash WHERE image_id=?", [id])?;
            }
            understanding::refresh_fts(&tx, id)?;
        }
        tx.commit()?;
        Ok(records.len())
    }

    /// The one-connection scan: every phase back to back.
    pub(crate) fn scan_scope(
        &mut self,
        root: &Path,
        only_file: Option<&Path>,
        sidecars: &dyn SidecarReader,
        metadata: &dyn MetadataProvider,
    ) -> Result<usize> {
        let scope = self.begin_scan(root, only_file)?;
        let mut walk = scope.walk(sidecars);
        let mut changed = 0;
        loop {
            let entries = walk.next_batch(SCAN_BATCH)?;
            if entries.is_empty() {
                break;
            }
            let pending = self.scan_check(&scope, entries)?;
            let records = pending
                .into_iter()
                .map(|p| p.read(sidecars, metadata))
                .collect::<EngineResult<Vec<_>>>()?;
            changed += self.scan_apply(&scope, &records)?;
        }
        if changed > 0 {
            self.trim_changes()?;
        }
        Ok(changed)
    }
}

impl Index {
    /// Starts a phased scan of a folder (see the module doc). Takes the
    /// catalog briefly: the root row, and a relocation if its volume moved.
    pub fn begin_scan(&mut self, root: &Path) -> EngineResult<ScanScope> {
        self.0.begin_scan(root, None).map_err(Into::into)
    }
    /// Starts a phased scan of one admitted original (see `scan_file`).
    pub fn begin_file_scan(&mut self, path: &Path) -> EngineResult<ScanScope> {
        let path = path.canonicalize()?;
        if !path.is_file() {
            return Err(EngineError::invalid("path", "expected an original file"));
        }
        let parent = path
            .parent()
            .ok_or_else(|| EngineError::invalid("path", "image has no folder"))?;
        self.0.begin_scan(parent, Some(&path)).map_err(Into::into)
    }
    /// Which of a walk's entries need reading (changed, or new). Legacy ids
    /// among them are re-keyed now; `take_rekeyed` reports them.
    pub fn scan_check(
        &mut self,
        scope: &ScanScope,
        entries: Vec<ScanEntry>,
    ) -> EngineResult<Vec<PendingEntry>> {
        self.0.scan_check(scope, entries).map_err(Into::into)
    }
    /// Writes a batch of read records in one transaction. The change feed
    /// carries the batch as soon as it commits.
    pub fn scan_apply(&mut self, scope: &ScanScope, records: &[ScanRecord]) -> EngineResult<usize> {
        self.0.scan_apply(scope, records).map_err(Into::into)
    }
    /// After the last batch: bounds the change log when anything was written.
    pub fn finish_scan(&mut self, changed: usize) -> EngineResult<()> {
        if changed > 0 {
            self.0.trim_changes()?;
        }
        Ok(())
    }
}
