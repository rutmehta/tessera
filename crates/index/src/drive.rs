//! Drive-resident state (CS-1, docs/05 §2.3): what the index knows about a
//! folder's photos beyond the sidecars, written into `<root>/.tessera/` so the
//! drive carries it to any Mac. `people.json` holds persons and face
//! assignments; `analysis/<image-id>.json` holds scores, face detections and
//! understanding. Records carry `modified` (Unix ms) and `machine`; the newer
//! record wins, never a lock. Writes are atomic (temp file + rename).
use crate::{
    Core, FaceKey, FaceRecord, Index, IndexError, Result, Understanding, Volume,
    rekey::image_id_on, relative_path,
};
use engine_api::{
    error::{EngineError, EngineResult},
    id::ImageId,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
};

/// The hidden folder at the root of an opened folder.
pub const TESSERA_DIR: &str = ".tessera";
const SCHEMA_VERSION: u32 = 1;

/// Paths inside a root's `.tessera/`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TesseraDir(PathBuf);
impl TesseraDir {
    pub fn of_root(root: impl AsRef<Path>) -> Self {
        Self(root.as_ref().join(TESSERA_DIR))
    }
    pub fn dir(&self) -> &Path {
        &self.0
    }
    pub fn people(&self) -> PathBuf {
        self.0.join("people.json")
    }
    pub fn analysis_dir(&self) -> PathBuf {
        self.0.join("analysis")
    }
    pub fn analysis(&self, id: ImageId) -> PathBuf {
        self.analysis_dir().join(format!("{id}.json"))
    }
    pub fn learning(&self) -> PathBuf {
        self.0.join("learning.json")
    }
    pub fn style_profile(&self) -> PathBuf {
        self.0.join("style-profile.json")
    }
}

/// Same-directory temp file, fsync and rename; the folder is created on demand.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> EngineResult<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|e| EngineError::io_at(parent, &e))?;
    let mut temp =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| EngineError::io_at(path, &e))?;
    temp.write_all(bytes)
        .map_err(|e| EngineError::io_at(path, &e))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| EngineError::io_at(path, &e))?;
    temp.persist(path)
        .map_err(|e| EngineError::io_at(path, &e.error))?;
    Ok(())
}

/// Writes `bytes` only when the file does not already hold them, so unchanged
/// records never touch the drive.
pub fn write_if_changed(path: &Path, bytes: &[u8]) -> EngineResult<bool> {
    match std::fs::read(path) {
        Ok(existing) if existing == bytes => return Ok(false),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(EngineError::io_at(path, &e)),
    }
    atomic_write(path, bytes)?;
    Ok(true)
}

/// Unix milliseconds now.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreRecord {
    pub signal: String,
    pub value: f64,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FaceDetectionRecord {
    pub ordinal: u32,
    pub bbox: [f32; 4],
    pub landmarks5: [[f32; 2]; 5],
    pub confidence: f32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    pub sharpness: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eyes_open: Option<f64>,
}

/// `analysis/<image-id>.json`: everything a model computed for one image.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisRecord {
    pub schema_version: u32,
    pub image_id: ImageId,
    /// The original, relative to the root (forward slashes).
    pub path: String,
    pub modified: i64,
    pub machine: String,
    pub scores: Vec<ScoreRecord>,
    pub faces: Vec<FaceDetectionRecord>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub understanding: Option<Understanding>,
}
impl Default for AnalysisRecord {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            image_id: ImageId::default(),
            path: String::new(),
            modified: 0,
            machine: String::new(),
            scores: Vec::new(),
            faces: Vec::new(),
            understanding: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersonRecord {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub medoid: Option<Vec<f32>>,
    /// The sharpest assigned face (image id, ordinal), for a cover.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover: Option<(ImageId, u32)>,
    pub modified: i64,
    pub machine: String,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaceAssignmentRecord {
    pub ordinal: u32,
    pub person: String,
    pub confirmed: bool,
    pub modified: i64,
    pub machine: String,
}
/// `people.json`: persons and, by image id, which detected face is whom.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PeopleDocument {
    pub schema_version: u32,
    pub persons: BTreeMap<String, PersonRecord>,
    pub faces: BTreeMap<ImageId, Vec<FaceAssignmentRecord>>,
}
impl Default for PeopleDocument {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            persons: BTreeMap::new(),
            faces: BTreeMap::new(),
        }
    }
}

/// What one import pass applied.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportCounts {
    pub analysis: usize,
    pub persons: usize,
    pub assignments: usize,
}

fn sql(error: rusqlite::Error) -> EngineError {
    IndexError::Sql(error).into()
}

/// Scores `replace_faces` regenerates from the detections.
fn is_face_score(signal: &str) -> bool {
    signal.starts_with("face/") || signal == "face_sharpness" || signal == "eyes_open"
}

impl Core {
    fn stamp(&self, id: &str) -> Result<Option<(i64, String, bool)>> {
        Ok(self
            .conn
            .query_row(
                "SELECT modified,machine,dirty FROM drive_stamp WHERE image_id=?",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0)),
            )
            .optional()?)
    }
    fn set_stamp(&self, id: &str, modified: i64, machine: &str, dirty: bool) -> Result<()> {
        self.conn.execute(
            "INSERT INTO drive_stamp(image_id,modified,machine,dirty) VALUES(?,?,?,?)
             ON CONFLICT(image_id) DO UPDATE SET modified=excluded.modified,machine=excluded.machine,dirty=excluded.dirty",
            params![id, modified, machine, dirty],
        )?;
        Ok(())
    }
    /// Suspends the stamping triggers for an import transaction.
    fn importing(&self, on: bool) -> Result<()> {
        if on {
            self.set_meta("importing", "1")
        } else {
            self.conn
                .execute("DELETE FROM catalog_meta WHERE key='importing'", [])?;
            Ok(())
        }
    }
    /// The file row's path for an image id.
    fn image_path(&self, id: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT f.path FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
                [id],
                |r| r.get(0),
            )
            .optional()?)
    }
}

impl Index {
    /// The id this machine stamps its records with (CS-1 last-writer-wins).
    pub fn set_machine(&self, machine: &str) -> EngineResult<()> {
        self.0.set_meta("machine", machine).map_err(Into::into)
    }
    pub fn machine(&self) -> EngineResult<String> {
        Ok(self
            .0
            .meta("machine")
            .map_err(EngineError::from)?
            .unwrap_or_default())
    }

    /// Images whose analysis record changed locally since the drive last saw it.
    pub fn drive_dirty_images(&self) -> EngineResult<Vec<ImageId>> {
        let mut stmt = self
            .0
            .conn
            .prepare("SELECT image_id FROM drive_stamp WHERE dirty=1 ORDER BY image_id")
            .map_err(sql)?;
        stmt.query_map([], |r| r.get::<_, String>(0))
            .map_err(sql)?
            .map(|r| crate::parse_id(&r.map_err(sql)?).map_err(Into::into))
            .collect()
    }
    /// Clears the dirty flag of records written at `modified`; a record
    /// stamped again since stays dirty.
    pub fn mark_drive_clean(&self, written: &[(ImageId, i64)]) -> EngineResult<()> {
        // One commit for the batch, not one per record.
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        for (id, modified) in written {
            tx.execute(
                "UPDATE drive_stamp SET dirty=0 WHERE image_id=? AND modified=?",
                params![id.to_string(), modified],
            )
            .map_err(sql)?;
        }
        tx.commit().map_err(sql)
    }
    /// (current revision of persons and assignments, revision last written).
    pub fn people_rev(&self) -> EngineResult<(i64, i64)> {
        let read = |key: &str| -> EngineResult<i64> {
            Ok(self
                .0
                .meta(key)
                .map_err(EngineError::from)?
                .and_then(|v| v.parse().ok())
                .unwrap_or(0))
        };
        Ok((read("people_rev")?, read("people_flushed")?))
    }
    /// Persons or assignments changed locally since the drive last saw them.
    pub fn people_dirty(&self) -> EngineResult<bool> {
        let (rev, flushed) = self.people_rev()?;
        Ok(rev != flushed)
    }
    /// The writer wrote revision `rev`; later revisions stay pending.
    pub fn mark_people_flushed(&self, rev: i64) -> EngineResult<()> {
        self.0
            .set_meta("people_flushed", &rev.to_string())
            .map_err(Into::into)
    }
    pub fn mark_people_clean(&self) -> EngineResult<()> {
        let (rev, _) = self.people_rev()?;
        self.mark_people_flushed(rev)
    }

    /// Stamps every image under `root` that has analysis rows but no record
    /// yet (a catalog older than CS-1, or a drive without `.tessera/`), so the
    /// writer backfills the drive. Returns how many were marked.
    pub fn mark_root_dirty(&self, root: &Path) -> EngineResult<usize> {
        let root = root.to_string_lossy().into_owned();
        let changed = self
            .0
            .conn
            .execute(
                "INSERT INTO drive_stamp(image_id,modified,machine,dirty)
                 SELECT i.id,?1,(SELECT value FROM catalog_meta WHERE key='machine'),1 FROM image i JOIN file f ON f.id=i.file_id
                 WHERE (f.path=?2 OR substr(f.path,1,length(?2)+1)=?2 || '/')
                 AND NOT EXISTS(SELECT 1 FROM drive_stamp d WHERE d.image_id=i.id)
                 AND (EXISTS(SELECT 1 FROM score s WHERE s.image_id=i.id)
                   OR EXISTS(SELECT 1 FROM face x WHERE x.image_id=i.id)
                   OR EXISTS(SELECT 1 FROM understanding u WHERE u.image_id=i.id))",
                params![now_ms(), root],
            )
            .map_err(sql)?;
        let people: i64 = self
            .0
            .conn
            .query_row("SELECT count(*) FROM person", [], |r| r.get(0))
            .map_err(sql)?;
        if people > 0 {
            self.0
                .conn
                .execute(
                    "UPDATE catalog_meta SET value=CAST(value AS INTEGER)+1 WHERE key='people_rev'",
                    [],
                )
                .map_err(sql)?;
        }
        Ok(changed)
    }

    /// The root (longest path prefix) an image's file lives under.
    pub fn root_of(&self, id: ImageId) -> EngineResult<Option<PathBuf>> {
        let Some(path) = self
            .0
            .image_path(&id.to_string())
            .map_err(EngineError::from)?
        else {
            return Ok(None);
        };
        let path = Path::new(&path);
        Ok(self
            .roots()?
            .into_iter()
            .filter(|r| path.starts_with(&r.path))
            .max_by_key(|r| r.path.as_os_str().len())
            .map(|r| r.path))
    }

    /// The analysis record of an image, relative to `root`.
    pub fn export_analysis(&self, root: &Path, id: ImageId) -> EngineResult<AnalysisRecord> {
        let key = id.to_string();
        let path = self
            .0
            .image_path(&key)
            .map_err(EngineError::from)?
            .ok_or_else(|| EngineError::not_found("image", id))?;
        let (modified, machine) = self
            .0
            .stamp(&key)
            .map_err(EngineError::from)?
            .map(|(m, machine, _)| (m, machine))
            .unwrap_or_else(|| (now_ms(), String::new()));
        Ok(AnalysisRecord {
            schema_version: SCHEMA_VERSION,
            image_id: id,
            path: relative_path(root, Path::new(&path)),
            modified,
            machine,
            scores: self
                .scores(id)?
                .into_iter()
                .filter(|s| !is_face_score(&s.signal))
                .map(|s| ScoreRecord {
                    signal: s.signal,
                    value: s.value,
                    model: s.model,
                })
                .collect(),
            faces: self
                .faces(id)?
                .into_iter()
                .map(|f| FaceDetectionRecord {
                    ordinal: f.id,
                    bbox: f.bbox,
                    landmarks5: f.landmarks5,
                    confidence: f.confidence,
                    embedding: f.embedding,
                    sharpness: f.sharpness,
                    eyes_open: f.eyes_open,
                })
                .collect(),
            understanding: self.understanding(id).map_err(|e| EngineError::Decode {
                format: "understanding".into(),
                message: e.to_string(),
            })?,
        })
    }

    /// Persons (all) and the assignments of images under `root`.
    pub fn export_people(&self, root: &Path) -> EngineResult<PeopleDocument> {
        let mut document = PeopleDocument::default();
        let mut persons = self
            .0
            .conn
            .prepare("SELECT id,name,medoid,modified,machine FROM person ORDER BY id")
            .map_err(sql)?;
        let rows = persons
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, Option<String>>(2)?,
                    r.get::<_, i64>(3)?,
                    r.get::<_, String>(4)?,
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (id, name, medoid, modified, machine) = row.map_err(sql)?;
            let cover = self
                .0
                .conn
                .query_row(
                    "SELECT fp.image_id,fp.ordinal FROM face_person fp JOIN score s ON s.image_id=fp.image_id AND s.signal='face/'||fp.ordinal||'/sharpness' WHERE fp.person_id=? ORDER BY s.value DESC,fp.image_id,fp.ordinal LIMIT 1",
                    [&id],
                    |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)),
                )
                .optional()
                .map_err(sql)?
                .and_then(|(image, ordinal)| crate::parse_id(&image).ok().map(|i| (i, ordinal)));
            document.persons.insert(
                id,
                PersonRecord {
                    name,
                    medoid: medoid.map(|m| serde_json::from_str(&m)).transpose()?,
                    cover,
                    modified,
                    machine,
                },
            );
        }
        let root_s = root.to_string_lossy().into_owned();
        let mut faces = self
            .0
            .conn
            .prepare(
                "SELECT fp.image_id,fp.ordinal,fp.person_id,fp.confirmed,fp.modified,fp.machine
                 FROM face_person fp JOIN image i ON i.id=fp.image_id JOIN file f ON f.id=i.file_id
                 WHERE f.path=?1 OR substr(f.path,1,length(?1)+1)=?1 || '/'
                 ORDER BY fp.image_id,fp.ordinal",
            )
            .map_err(sql)?;
        let rows = faces
            .query_map([&root_s], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, u32>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, bool>(3)?,
                    r.get::<_, i64>(4)?,
                    r.get::<_, String>(5)?,
                ))
            })
            .map_err(sql)?;
        for row in rows {
            let (image, ordinal, person, confirmed, modified, machine) = row.map_err(sql)?;
            document
                .faces
                .entry(crate::parse_id(&image)?)
                .or_default()
                .push(FaceAssignmentRecord {
                    ordinal,
                    person,
                    confirmed,
                    modified,
                    machine,
                });
        }
        Ok(document)
    }

    /// Loads `<root>/.tessera/` into the index: analysis records first (they
    /// create the image rows the scan will refresh, and the faces the
    /// assignments need), then `people.json`. A record is applied when the
    /// index has no stamp for the image or the record is newer; a record whose
    /// id does not match the file it names (another volume, a moved file) is
    /// skipped. `volume` is the root's volume, as the scanner resolves it.
    pub fn import_drive_state(
        &mut self,
        root: &Path,
        volume: Option<&Volume>,
    ) -> EngineResult<ImportCounts> {
        let dir = TesseraDir::of_root(root);
        let mut counts = ImportCounts::default();
        let entries = match std::fs::read_dir(dir.analysis_dir()) {
            Ok(entries) => entries.collect::<Vec<_>>(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(EngineError::io_at(dir.analysis_dir(), &e)),
        };
        let people = match std::fs::read(dir.people()) {
            Ok(bytes) => Some(serde_json::from_slice::<PeopleDocument>(&bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(EngineError::io_at(dir.people(), &e)),
        };
        if entries.is_empty() && people.is_none() {
            return Ok(counts);
        }
        let root_id = self.0.register_root(root).map_err(EngineError::from)?;
        self.0.importing(true).map_err(EngineError::from)?;
        let result = (|| -> EngineResult<()> {
            for entry in entries {
                let entry = entry.map_err(|e| EngineError::io_at(dir.analysis_dir(), &e))?;
                if entry.path().extension().is_none_or(|e| e != "json") {
                    continue;
                }
                let bytes = std::fs::read(entry.path())
                    .map_err(|e| EngineError::io_at(entry.path(), &e))?;
                // One unreadable record must not hide the rest of the drive.
                let Ok(record) = serde_json::from_slice::<AnalysisRecord>(&bytes) else {
                    continue;
                };
                counts.analysis +=
                    usize::from(self.import_analysis(root, root_id, volume, &record)?);
            }
            if let Some(people) = people {
                let (persons, assignments) = self.import_people(&people)?;
                counts.persons += persons;
                counts.assignments += assignments;
            }
            Ok(())
        })();
        self.0.importing(false).map_err(EngineError::from)?;
        result.map(|()| counts)
    }

    fn import_analysis(
        &mut self,
        root: &Path,
        root_id: i64,
        volume: Option<&Volume>,
        record: &AnalysisRecord,
    ) -> EngineResult<bool> {
        if record.schema_version > SCHEMA_VERSION || record.path.is_empty() {
            return Ok(false);
        }
        let path = root.join(&record.path);
        if image_id_on(volume, &path) != record.image_id {
            return Ok(false);
        }
        let key = record.image_id.to_string();
        if let Some((modified, _, _)) = self.0.stamp(&key).map_err(EngineError::from)?
            && modified >= record.modified
        {
            return Ok(false);
        }
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        // The scan refreshes a placeholder row (size and mtime 0) on its way.
        let path_s = path.to_string_lossy().into_owned();
        let folder_s = path.parent().unwrap_or(root).to_string_lossy().into_owned();
        tx.execute(
            "INSERT OR IGNORE INTO folder(root_id,path) VALUES(?,?)",
            params![root_id, folder_s],
        )
        .map_err(sql)?;
        tx.execute(
            "INSERT OR IGNORE INTO file(folder_id,path,name,size,mtime) SELECT id,?,?,0,0 FROM folder WHERE path=?",
            params![path_s, path.file_name().unwrap_or_default().to_string_lossy(), folder_s],
        )
        .map_err(sql)?;
        let file_id: i64 = tx
            .query_row("SELECT id FROM file WHERE path=?", [&path_s], |r| r.get(0))
            .map_err(sql)?;
        let existing: Option<String> = tx
            .query_row("SELECT id FROM image WHERE file_id=?", [file_id], |r| {
                r.get(0)
            })
            .optional()
            .map_err(sql)?;
        match existing {
            // A legacy id still to be re-keyed by the scan: leave it alone.
            Some(id) if id != key => return Ok(false),
            Some(_) => {}
            None => {
                tx.execute(
                    "INSERT INTO image(id,file_id) VALUES(?,?)",
                    params![key, file_id],
                )
                .map_err(sql)?;
                tx.execute(
                    "INSERT OR IGNORE INTO selection(image_id) VALUES(?)",
                    [&key],
                )
                .map_err(sql)?;
                crate::understanding::refresh_fts(&tx, &key).map_err(sql)?;
            }
        }
        tx.commit().map_err(sql)?;
        for score in &record.scores {
            if is_face_score(&score.signal) {
                continue;
            }
            self.set_score(
                record.image_id,
                &crate::Score {
                    signal: score.signal.clone(),
                    value: score.value,
                    model: score.model.clone(),
                },
            )?;
        }
        let faces: Vec<FaceRecord> = record
            .faces
            .iter()
            .map(|f| FaceRecord {
                id: f.ordinal,
                bbox: f.bbox,
                landmarks5: f.landmarks5,
                confidence: f.confidence,
                embedding: f.embedding.clone(),
                sharpness: f.sharpness,
                eyes_open: f.eyes_open,
            })
            .collect();
        if !faces.is_empty() || !self.faces(record.image_id)?.is_empty() {
            self.replace_faces(record.image_id, &faces)?;
        }
        if let Some(understanding) = &record.understanding {
            self.set_understanding(record.image_id, understanding)
                .map_err(|e| EngineError::Decode {
                    format: "understanding".into(),
                    message: e.to_string(),
                })?;
        }
        self.0
            .set_stamp(&key, record.modified, &record.machine, false)
            .map_err(EngineError::from)?;
        Ok(true)
    }

    fn import_people(&self, document: &PeopleDocument) -> EngineResult<(usize, usize)> {
        if document.schema_version > SCHEMA_VERSION {
            return Ok((0, 0));
        }
        let tx = self.0.conn.unchecked_transaction().map_err(sql)?;
        let mut persons = 0;
        for (id, record) in &document.persons {
            if id.trim().is_empty() {
                continue;
            }
            let current: Option<i64> = tx
                .query_row("SELECT modified FROM person WHERE id=?", [id], |r| r.get(0))
                .optional()
                .map_err(sql)?;
            if current.is_some_and(|m| m >= record.modified) {
                continue;
            }
            let medoid = record
                .medoid
                .as_ref()
                .filter(|m| m.len() == 128 && m.iter().all(|v| v.is_finite()))
                .map(serde_json::to_string)
                .transpose()?;
            tx.execute(
                "INSERT INTO person(id,name,medoid,modified,machine) VALUES(?1,?2,?3,?4,?5)
                 ON CONFLICT(id) DO UPDATE SET name=excluded.name,medoid=excluded.medoid,modified=excluded.modified,machine=excluded.machine",
                params![id, record.name, medoid, record.modified, record.machine],
            )
            .map_err(sql)?;
            persons += 1;
        }
        let mut assignments = 0;
        for (image, faces) in &document.faces {
            let image_s = image.to_string();
            for face in faces {
                // The detection must exist (analysis records import first).
                let detected: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM face WHERE image_id=? AND id=?) AND EXISTS(SELECT 1 FROM person WHERE id=?)",
                        params![image_s, face.ordinal, face.person],
                        |r| r.get(0),
                    )
                    .map_err(sql)?;
                if !detected {
                    continue;
                }
                let current: Option<i64> = tx
                    .query_row(
                        "SELECT modified FROM face_person WHERE image_id=? AND ordinal=?",
                        params![image_s, face.ordinal],
                        |r| r.get(0),
                    )
                    .optional()
                    .map_err(sql)?;
                if current.is_some_and(|m| m >= face.modified) {
                    continue;
                }
                tx.execute(
                    "INSERT INTO face_person(image_id,ordinal,person_id,confirmed,modified,machine) VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(image_id,ordinal) DO UPDATE SET person_id=excluded.person_id,confirmed=excluded.confirmed,modified=excluded.modified,machine=excluded.machine",
                    params![image_s, face.ordinal, face.person, face.confirmed, face.modified, face.machine],
                )
                .map_err(sql)?;
                assignments += 1;
            }
        }
        tx.commit().map_err(sql)?;
        Ok((persons, assignments))
    }
}

/// Convenience for tests and hosts: a face key from an assignment.
impl FaceAssignmentRecord {
    pub fn key(&self, image: ImageId) -> FaceKey {
        FaceKey {
            image_id: image,
            ordinal: self.ordinal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FixedVolumes, NoopMetadataProvider, NoopSidecarReader, Query, Score};

    fn face(ordinal: u32, sharpness: f64) -> FaceRecord {
        let mut embedding = vec![0.; 128];
        embedding[ordinal as usize] = 1.;
        FaceRecord {
            id: ordinal,
            bbox: [1., 1., 10., 10.],
            landmarks5: [[2.; 2]; 5],
            confidence: 0.9,
            embedding: Some(embedding),
            sharpness,
            eyes_open: Some(0.8),
        }
    }

    #[test]
    fn drive_records_round_trip_into_a_fresh_index_before_its_scan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap().join("shoot");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("a.jpg"), b"a").unwrap();
        std::fs::write(root.join("b.jpg"), b"b").unwrap();
        let mut first = Index::open(dir.path().join("one.sqlite")).unwrap();
        first.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        first.set_machine("mac-one").unwrap();
        first
            .scan(&root, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        let a = first.image_at(&root.join("a.jpg")).unwrap().unwrap();
        let b = first.image_at(&root.join("b.jpg")).unwrap().unwrap();
        first
            .set_score(
                a,
                &Score {
                    signal: "quality".into(),
                    value: 0.8,
                    model: "q1".into(),
                },
            )
            .unwrap();
        first
            .replace_faces(a, &[face(0, 0.4), face(1, 0.9)])
            .unwrap();
        first.create_person("ada", Some("Ada"), None).unwrap();
        first
            .assign_face(
                FaceKey {
                    image_id: a,
                    ordinal: 1,
                },
                "ada",
            )
            .unwrap();
        first
            .confirm_face(
                FaceKey {
                    image_id: a,
                    ordinal: 1,
                },
                true,
            )
            .unwrap();
        first
            .set_understanding(
                b,
                &Understanding {
                    model_version: "m1".into(),
                    keywords: vec![],
                    caption: "a beach".into(),
                    alt_text: "beach".into(),
                    ocr: vec![],
                },
            )
            .unwrap();
        // Local writes were stamped by this machine and flagged for the writer.
        let mut dirty = first.drive_dirty_images().unwrap();
        dirty.sort();
        let mut expected = vec![a, b];
        expected.sort();
        assert_eq!(dirty, expected);
        assert!(first.people_dirty().unwrap());
        let record = first.export_analysis(&root, a).unwrap();
        assert_eq!(record.path, "a.jpg");
        assert_eq!(record.machine, "mac-one");
        assert!(record.modified > 0);
        assert_eq!(record.scores.len(), 1, "face scores travel as detections");
        assert_eq!(record.faces.len(), 2);
        let people = first.export_people(&root).unwrap();
        assert_eq!(people.persons["ada"].name.as_deref(), Some("Ada"));
        assert_eq!(people.persons["ada"].cover, Some((a, 1)));
        assert_eq!(people.persons["ada"].machine, "mac-one");
        assert!(people.faces[&a][0].confirmed);
        let tessera = TesseraDir::of_root(&root);
        for id in [a, b] {
            let record = first.export_analysis(&root, id).unwrap();
            atomic_write(
                &tessera.analysis(id),
                &serde_json::to_vec_pretty(&record).unwrap(),
            )
            .unwrap();
        }
        atomic_write(
            &tessera.people(),
            &serde_json::to_vec_pretty(&people).unwrap(),
        )
        .unwrap();
        let stamps: Vec<_> = [a, b]
            .into_iter()
            .map(|id| (id, first.export_analysis(&root, id).unwrap().modified))
            .collect();
        first.mark_drive_clean(&stamps).unwrap();
        first.mark_people_clean().unwrap();
        assert!(first.drive_dirty_images().unwrap().is_empty());
        assert!(!first.people_dirty().unwrap());
        assert!(
            !write_if_changed(
                &tessera.people(),
                &serde_json::to_vec_pretty(&people).unwrap()
            )
            .unwrap()
        );

        // A fresh index on another machine: everything is there before the scan.
        let mut second = Index::open(dir.path().join("two.sqlite")).unwrap();
        second.set_volume_resolver(Box::new(FixedVolumes(vec![])));
        second.set_machine("mac-two").unwrap();
        let counts = second.import_drive_state(&root, None).unwrap();
        assert_eq!(
            counts,
            ImportCounts {
                analysis: 2,
                persons: 1,
                assignments: 1
            }
        );
        assert_eq!(
            second.scores(a).unwrap().len(),
            1 + 2 * 2 + 2,
            "quality, per-face, aggregates"
        );
        assert_eq!(second.faces(a).unwrap().len(), 2);
        let assignment = &second.face_assignments(a).unwrap()[0];
        assert_eq!(assignment.person_id, "ada");
        assert_eq!(assignment.person_name.as_deref(), Some("Ada"));
        assert!(assignment.confirmed);
        assert_eq!(second.understanding(b).unwrap().unwrap().caption, "a beach");
        assert_eq!(
            second
                .search(&Query {
                    text: Some("beach".into()),
                    ..Default::default()
                })
                .unwrap(),
            vec![b]
        );
        // Imported records are not dirty (they came from the drive) and keep their stamps.
        assert!(second.drive_dirty_images().unwrap().is_empty());
        assert!(!second.people_dirty().unwrap());
        assert_eq!(second.export_analysis(&root, a).unwrap().machine, "mac-one");
        // The scan refreshes the placeholder rows without changing ids.
        assert_eq!(
            second
                .scan(&root, &NoopSidecarReader, &NoopMetadataProvider)
                .unwrap(),
            2
        );
        assert_eq!(second.image_info(a).unwrap().size, 1);
        assert_eq!(second.search(&Query::default()).unwrap().len(), 2);
        // Re-importing the same drive is a no-op; a newer record wins.
        assert_eq!(
            second.import_drive_state(&root, None).unwrap(),
            ImportCounts::default()
        );
        let mut newer = people.clone();
        newer.persons.get_mut("ada").unwrap().name = Some("Ada Lovelace".into());
        newer.persons.get_mut("ada").unwrap().modified += 1;
        atomic_write(&tessera.people(), &serde_json::to_vec(&newer).unwrap()).unwrap();
        assert_eq!(second.import_drive_state(&root, None).unwrap().persons, 1);
        assert_eq!(
            second.people().unwrap()[0].name.as_deref(),
            Some("Ada Lovelace")
        );
        // A record for a file on another volume (its id does not match) is skipped.
        let mut foreign = second.export_analysis(&root, a).unwrap();
        foreign.image_id = ImageId::for_volume_path("OTHER", "a.jpg");
        foreign.modified += 1;
        atomic_write(
            &tessera.analysis(foreign.image_id),
            &serde_json::to_vec(&foreign).unwrap(),
        )
        .unwrap();
        assert_eq!(second.import_drive_state(&root, None).unwrap().analysis, 0);
        assert_eq!(second.search(&Query::default()).unwrap().len(), 2);
        // Backfill marks only images that have analysis rows.
        assert_eq!(first.mark_root_dirty(&root).unwrap(), 0, "already stamped");
        assert_eq!(second.root_of(a).unwrap(), Some(root.clone()));
    }
}
