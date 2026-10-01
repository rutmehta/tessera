//! Durable local journal for edits made against a Smart Preview.
//!
//! This store is intentionally separate from preview generation and Develop
//! admission. Callers key it by the original catalog `ImageId`; the id is never
//! derived from a file path.

use engine_api::id::ImageId;
use serde::{Deserialize, Serialize};
use sidecar::RecipeDocument;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    sync::{Arc, Mutex, MutexGuard, OnceLock},
    time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const MAX_RECORD_BYTES: u64 = 64 * 1024 * 1024;
static TEMP_ID: AtomicU64 = AtomicU64::new(0);
static LOCKS: OnceLock<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>> = OnceLock::new();

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Smart Preview journal I/O: {0}")]
    Io(#[from] io::Error),
    #[error("Smart Preview journal is corrupt: {0}")]
    Corrupt(String),
    #[error("Smart Preview journal belongs to another image")]
    WrongImage,
    #[error("Smart Preview journal exceeds the size limit")]
    TooLarge,
    #[error("Smart Preview journal changed since it was opened")]
    RevisionConflict,
    #[error("cannot discard a dirty Smart Preview journal")]
    DirtyDiscard,
    #[error("Smart Preview journal lock poisoned")]
    LockPoisoned,
}

pub type StoreResult<T> = Result<T, StoreError>;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Record {
    version: u32,
    image_id: ImageId,
    generation: u64,
    incarnation: [u8; 32],
    source_digest: [u8; 32],
    source_len: u64,
    recipe: Vec<u8>,
    recipe_digest: [u8; 32],
    baseline_recipe: Option<Vec<u8>>,
    baseline_recipe_digest: Option<[u8; 32]>,
    baseline_xmp: Option<Vec<u8>>,
    baseline_xmp_digest: Option<[u8; 32]>,
    dirty: bool,
}

/// Open journal state. All mutations are revision-checked under a process-wide
/// per-record lock, so two independently opened handles cannot overwrite each
/// other's newer journal. Reopen after `RevisionConflict` to recover the latest.
pub struct SmartPreviewJournal {
    root: PathBuf,
    id: ImageId,
    revision: u64,
    incarnation: [u8; 32],
    _guard: Arc<Mutex<()>>,
}

#[derive(Debug, Clone)]
pub struct JournalSnapshot {
    pub image_id: ImageId,
    pub generation: u64,
    pub source_digest: [u8; 32],
    pub source_len: u64,
    /// Exact serialized recipe document bytes, retaining unknown recipe fields.
    pub recipe: Vec<u8>,
    pub recipe_digest: [u8; 32],
    /// Exact source-sidecar bytes captured when the preview was built.
    pub baseline_recipe: Option<Vec<u8>>,
    pub baseline_recipe_digest: Option<[u8; 32]>,
    pub baseline_xmp: Option<Vec<u8>>,
    pub baseline_xmp_digest: Option<[u8; 32]>,
    pub dirty: bool,
}

/// Existing generated-store files only. Reject links before any content read.
/// Callers hold image read admission against participating in-process writers.
pub(crate) fn local_regular_file(root: &Path, id: ImageId, name: &str) -> StoreResult<PathBuf> {
    let previews = fs::canonicalize(root)?.join("smart-previews");
    let directory = previews.join(id.to_string());
    for path in [&previews, &directory] {
        if !fs::symlink_metadata(path)?.file_type().is_dir() {
            return Err(StoreError::Corrupt(
                "Smart Preview directory must not be a symlink".into(),
            ));
        }
    }
    let path = directory.join(name);
    if !fs::symlink_metadata(&path)?.file_type().is_file() {
        return Err(StoreError::Corrupt(
            "Smart Preview file must be a regular non-symlink file".into(),
        ));
    }
    Ok(path)
}

impl SmartPreviewJournal {
    /// Read-only admission: never creates store directories and never touches originals.
    pub(crate) fn read_local_snapshot(
        root: &Path,
        id: ImageId,
    ) -> StoreResult<([u8; 32], JournalSnapshot)> {
        let path = local_regular_file(root, id, "journal.json")?;
        // Same canonical per-record process lock as writers, without directory creation.
        let guard = {
            let mut table = LOCKS
                .get_or_init(|| Mutex::new(HashMap::new()))
                .lock()
                .map_err(|_| StoreError::LockPoisoned)?;
            table
                .entry(path.clone())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _lock = lock_guard(&guard)?;
        let record = read_record(&path, id)?;
        Ok((record.incarnation, record.snapshot()))
    }

    /// Create a clean journal from the recipe owner and exact original sidecar baseline.
    /// `root` must be an existing application-support directory; generated store
    /// entries are synchronized beneath it before this call succeeds.
    pub fn create(
        root: impl AsRef<Path>,
        id: ImageId,
        source_digest: [u8; 32],
        source_len: u64,
        recipe: Vec<u8>,
        baseline_recipe: Option<Vec<u8>>,
        baseline_xmp: Option<Vec<u8>>,
    ) -> StoreResult<Self> {
        validate_recipe(id, &recipe)?;
        if let Some(baseline) = baseline_recipe.as_deref() {
            validate_recipe(id, baseline)?;
        }
        check_payload_sizes(&recipe, baseline_recipe.as_deref(), baseline_xmp.as_deref())?;
        if source_len == 0 {
            return Err(StoreError::Corrupt("source length must be positive".into()));
        }
        let journal = Self::new(root.as_ref(), id)?;
        let guard = journal._guard.clone();
        let _lock = lock_guard(&guard)?;
        let path = journal.path();
        if path.exists() {
            return Err(io::Error::new(io::ErrorKind::AlreadyExists, "journal exists").into());
        }
        let record = Record {
            version: VERSION,
            image_id: id,
            generation: 1,
            incarnation: new_incarnation(&path),
            source_digest,
            source_len,
            recipe,
            recipe_digest: [0; 32],
            baseline_recipe_digest: baseline_recipe
                .as_deref()
                .map(|bytes| *blake3::hash(bytes).as_bytes()),
            baseline_recipe,
            baseline_xmp_digest: baseline_xmp
                .as_deref()
                .map(|bytes| *blake3::hash(bytes).as_bytes()),
            baseline_xmp,
            dirty: false,
        };
        let mut record = record;
        record.recipe_digest = *blake3::hash(&record.recipe).as_bytes();
        write_record(&path, &record)?;
        Ok(Self {
            revision: 1,
            incarnation: record.incarnation,
            ..journal
        })
    }

    /// Reopen and validate a journal for the expected original owner.
    pub fn open(root: impl AsRef<Path>, id: ImageId) -> StoreResult<(Self, JournalSnapshot)> {
        let journal = Self::new(root.as_ref(), id)?;
        let guard = journal._guard.clone();
        let _lock = lock_guard(&guard)?;
        let record = read_record(&journal.path(), id)?;
        let snapshot = record.snapshot();
        let journal = Self {
            revision: record.generation,
            incarnation: record.incarnation,
            ..journal
        };
        Ok((journal, snapshot))
    }

    /// Replace the local recipe document and durably advance the generation.
    pub fn save_recipe(&mut self, recipe: Vec<u8>) -> StoreResult<u64> {
        validate_recipe(self.id, &recipe)?;
        check_payload_sizes(&recipe, None, None)?;
        let guard = self._guard.clone();
        let _lock = lock_guard(&guard)?;
        let path = self.path();
        let mut record = read_record(&path, self.id)?;
        self.check_record(&record)?;
        record.generation = record
            .generation
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt("journal generation exhausted".into()))?;
        record.recipe = recipe;
        record.recipe_digest = *blake3::hash(&record.recipe).as_bytes();
        record.dirty = true;
        write_record(&path, &record)?;
        self.revision = record.generation;
        Ok(self.revision)
    }

    /// Acknowledge published original sidecars only against this exact incarnation/revision.
    pub fn mark_synced(
        &mut self,
        recipe: Vec<u8>,
        baseline_recipe: Option<Vec<u8>>,
        baseline_xmp: Option<Vec<u8>>,
    ) -> StoreResult<()> {
        validate_recipe(self.id, &recipe)?;
        check_payload_sizes(&recipe, baseline_recipe.as_deref(), baseline_xmp.as_deref())?;
        let guard = self._guard.clone();
        let _lock = lock_guard(&guard)?;
        let mut record = read_record(&self.path(), self.id)?;
        self.check_record(&record)?;
        record.generation = record
            .generation
            .checked_add(1)
            .ok_or_else(|| StoreError::Corrupt("journal generation exhausted".into()))?;
        record.recipe_digest = *blake3::hash(&recipe).as_bytes();
        record.recipe = recipe;
        record.baseline_recipe_digest = baseline_recipe
            .as_deref()
            .map(|v| *blake3::hash(v).as_bytes());
        record.baseline_xmp_digest = baseline_xmp.as_deref().map(|v| *blake3::hash(v).as_bytes());
        record.baseline_recipe = baseline_recipe;
        record.baseline_xmp = baseline_xmp;
        record.dirty = false;
        write_record(&self.path(), &record)?;
        self.revision = record.generation;
        Ok(())
    }

    /// Read the latest validated snapshot for this handle's current generation.
    pub fn snapshot(&self) -> StoreResult<JournalSnapshot> {
        let guard = self._guard.clone();
        let _lock = lock_guard(&guard)?;
        let record = read_record(&self.path(), self.id)?;
        self.check_record(&record)?;
        Ok(record.snapshot())
    }

    /// Remove a clean journal. Dirty edits require a separate explicit recovery action.
    pub fn discard_clean(&self) -> StoreResult<()> {
        let guard = self._guard.clone();
        let _lock = lock_guard(&guard)?;
        let path = self.path();
        let record = read_record(&path, self.id)?;
        self.check_record(&record)?;
        if record.dirty {
            return Err(StoreError::DirtyDiscard);
        }
        remove_file_and_sync_directory(&path, sync_directory)?;
        Ok(())
    }

    fn new(root: &Path, id: ImageId) -> StoreResult<Self> {
        // The caller supplies an existing app-support root. Canonicalize it
        // first so symlink aliases converge on the same generated store path.
        let app_root = fs::canonicalize(root)?;
        if !fs::metadata(&app_root)?.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                app_root.display().to_string(),
            )
            .into());
        }
        let preview_root = ensure_child_dir_durable(&app_root, "smart-previews")?;
        let root = ensure_child_dir_durable(&preview_root, &id.to_string())?;
        let lock_path = root.join("journal.json");
        let table = LOCKS.get_or_init(|| Mutex::new(HashMap::new()));
        let mut table = table.lock().map_err(|_| StoreError::LockPoisoned)?;
        let guard = table
            .entry(lock_path)
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone();
        Ok(Self {
            root,
            id,
            revision: 0,
            incarnation: [0; 32],
            _guard: guard,
        })
    }

    pub(crate) fn directory(&self) -> &Path {
        &self.root
    }

    fn path(&self) -> PathBuf {
        self.root.join("journal.json")
    }

    fn check_record(&self, record: &Record) -> StoreResult<()> {
        if self.revision == record.generation && self.incarnation == record.incarnation {
            Ok(())
        } else {
            Err(StoreError::RevisionConflict)
        }
    }
}

impl Record {
    fn snapshot(&self) -> JournalSnapshot {
        JournalSnapshot {
            image_id: self.image_id,
            generation: self.generation,
            source_digest: self.source_digest,
            source_len: self.source_len,
            recipe: self.recipe.clone(),
            recipe_digest: self.recipe_digest,
            baseline_recipe: self.baseline_recipe.clone(),
            baseline_recipe_digest: self.baseline_recipe_digest,
            baseline_xmp: self.baseline_xmp.clone(),
            baseline_xmp_digest: self.baseline_xmp_digest,
            dirty: self.dirty,
        }
    }
}

fn validate_recipe(id: ImageId, bytes: &[u8]) -> StoreResult<()> {
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StoreError::TooLarge);
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    let schema_version = value
        .get("recipe")
        .and_then(|recipe| recipe.get("schema_version"))
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| StoreError::Corrupt("recipe schema version is missing or invalid".into()))?;
    if schema_version > u64::from(engine_api::recipe::max_writable_schema_version()) {
        return Err(StoreError::Corrupt(format!(
            "unsupported future recipe schema {schema_version}"
        )));
    }
    let doc: RecipeDocument =
        serde_json::from_value(value).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    doc.recipe
        .validate()
        .map_err(|e| StoreError::Corrupt(e.to_string()))?;
    // `from_json` deliberately accepts newer schemas for read-only access.
    // Exercise the writer check, but retain and persist the original exact bytes.
    doc.recipe
        .to_json()
        .map_err(|e| StoreError::Corrupt(e.to_string()))?;
    if doc.recipe.image_id != Some(id) {
        return Err(StoreError::WrongImage);
    }
    Ok(())
}

fn check_payload_sizes(
    recipe: &[u8],
    baseline_recipe: Option<&[u8]>,
    baseline_xmp: Option<&[u8]>,
) -> StoreResult<()> {
    let total = recipe.len() as u64
        + baseline_recipe.map_or(0, |v| v.len() as u64)
        + baseline_xmp.map_or(0, |v| v.len() as u64);
    if total > MAX_RECORD_BYTES {
        Err(StoreError::TooLarge)
    } else {
        Ok(())
    }
}

fn read_record(path: &Path, expected: ImageId) -> StoreResult<Record> {
    let file = fs::File::open(path)?;
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StoreError::TooLarge);
    }
    let record: Record =
        serde_json::from_slice(&bytes).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    if record.version != VERSION {
        return Err(StoreError::Corrupt("unsupported version".into()));
    }
    if record.image_id != expected {
        return Err(StoreError::WrongImage);
    }
    if record.generation == 0 {
        return Err(StoreError::Corrupt("invalid generation".into()));
    }
    if record.source_len == 0 {
        return Err(StoreError::Corrupt("source length must be positive".into()));
    }
    check_payload_sizes(
        &record.recipe,
        record.baseline_recipe.as_deref(),
        record.baseline_xmp.as_deref(),
    )?;
    if record
        .baseline_recipe
        .as_deref()
        .map(|b| *blake3::hash(b).as_bytes())
        != record.baseline_recipe_digest
        || record
            .baseline_xmp
            .as_deref()
            .map(|b| *blake3::hash(b).as_bytes())
            != record.baseline_xmp_digest
    {
        return Err(StoreError::Corrupt(
            "sidecar baseline digest mismatch".into(),
        ));
    }
    validate_recipe(expected, &record.recipe)?;
    if *blake3::hash(&record.recipe).as_bytes() != record.recipe_digest {
        return Err(StoreError::Corrupt("recipe digest mismatch".into()));
    }
    if let Some(baseline) = record.baseline_recipe.as_deref() {
        validate_recipe(expected, baseline)?;
    }
    Ok(record)
}

fn lock_guard(guard: &Arc<Mutex<()>>) -> StoreResult<MutexGuard<'_, ()>> {
    guard.lock().map_err(|_| StoreError::LockPoisoned)
}

fn sync_directory(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

fn ensure_child_dir_durable(parent: &Path, name: &str) -> StoreResult<PathBuf> {
    ensure_child_dir_durable_with(parent, name, sync_directory).map_err(StoreError::from)
}

fn ensure_child_dir_durable_with(
    parent: &Path,
    name: &str,
    sync: impl FnOnce(&Path) -> io::Result<()>,
) -> io::Result<PathBuf> {
    let child = parent.join(name);
    match fs::symlink_metadata(&child) {
        Ok(meta) if meta.file_type().is_dir() => {}
        Ok(_) => {
            return Err(io::Error::new(
                io::ErrorKind::NotADirectory,
                child.display().to_string(),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => match fs::create_dir(&child) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !fs::symlink_metadata(&child)?.file_type().is_dir() {
                    return Err(io::Error::new(
                        io::ErrorKind::NotADirectory,
                        child.display().to_string(),
                    ));
                }
            }
            Err(error) => return Err(error),
        },
        Err(error) => return Err(error),
    }
    // Sync even when the child already exists: an earlier attempt may have
    // created it but returned after the parent sync failed.
    sync(parent)?;
    Ok(child)
}

fn remove_file_and_sync_directory(
    path: &Path,
    sync: impl FnOnce(&Path) -> io::Result<()>,
) -> StoreResult<()> {
    fs::remove_file(path)?;
    let parent = path.parent().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "journal has no parent directory")
    })?;
    sync(parent)?;
    Ok(())
}

fn new_incarnation(path: &Path) -> [u8; 32] {
    let mut hash = blake3::Hasher::new_derive_key("tessera smart preview journal incarnation v1");
    hash.update(path.as_os_str().to_string_lossy().as_bytes());
    hash.update(&std::process::id().to_le_bytes());
    hash.update(&TEMP_ID.fetch_add(1, Ordering::Relaxed).to_le_bytes());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    hash.update(&nanos.to_le_bytes());
    *hash.finalize().as_bytes()
}

fn write_record(path: &Path, record: &Record) -> StoreResult<()> {
    let bytes = serde_json::to_vec(record).map_err(|e| StoreError::Corrupt(e.to_string()))?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(StoreError::TooLarge);
    }
    let (temp, mut file) = loop {
        let id = TEMP_ID.fetch_add(1, Ordering::Relaxed);
        let temp = path.with_extension(format!("{}.{}.tmp", std::process::id(), id));
        match OpenOptions::new().write(true).create_new(true).open(&temp) {
            Ok(file) => break (temp, file),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    };
    let result = (|| -> io::Result<()> {
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temp, path)?;
        if let Some(parent) = path.parent() {
            sync_directory(parent)?;
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result?;
    Ok(())
}

/// Read-only bounded declaration admission. This does not validate pixel bytes
/// or original identity. Never creates directories or opens original paths.
pub(crate) fn has_local_declaration(root: &Path, id: ImageId) -> bool {
    let previews = root.join("smart-previews");
    let directory = previews.join(id.to_string());
    for path in [&previews, &directory] {
        if !fs::symlink_metadata(path).is_ok_and(|m| m.file_type().is_dir()) {
            return false;
        }
    }
    if !fs::symlink_metadata(directory.join("pixels.tsp"))
        .is_ok_and(|m| m.file_type().is_file() && m.len() > 0)
    {
        return false;
    }
    if !fs::symlink_metadata(directory.join("journal.json")).is_ok_and(|m| m.file_type().is_file())
    {
        return false;
    }
    read_record(&directory.join("journal.json"), id).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::Recipe;
    use tempfile::tempdir;

    fn recipe(id: ImageId) -> Vec<u8> {
        let mut recipe = Recipe::new(id);
        recipe
            .unknown
            .insert("future_field".into(), serde_json::json!({"kept": true}));
        serde_json::to_vec(&RecipeDocument {
            recipe,
            ..Default::default()
        })
        .unwrap()
    }

    #[test]
    fn save_reopens_with_exact_document_baseline_and_unknown_fields() {
        let dir = tempdir().unwrap();
        let id = ImageId(17);
        let original = recipe(id);
        let baseline = recipe(id);
        let mut journal = SmartPreviewJournal::create(
            dir.path(),
            id,
            [9; 32],
            123,
            original.clone(),
            Some(baseline.clone()),
            Some(b"base xmp".to_vec()),
        )
        .unwrap();
        let edited = recipe(id);
        journal.save_recipe(edited.clone()).unwrap();
        drop(journal);
        let (journal, snapshot) = SmartPreviewJournal::open(dir.path(), id).unwrap();
        assert_eq!(snapshot.recipe, edited);
        assert_eq!(
            snapshot.baseline_recipe.as_deref(),
            Some(baseline.as_slice())
        );
        assert_eq!(
            snapshot.baseline_recipe_digest,
            Some(*blake3::hash(&baseline).as_bytes())
        );
        assert_eq!(snapshot.baseline_xmp.as_deref(), Some(&b"base xmp"[..]));
        assert_eq!(snapshot.source_digest, [9; 32]);
        assert_eq!(snapshot.source_len, 123);
        assert!(snapshot.dirty);
        let decoded: RecipeDocument = serde_json::from_slice(&snapshot.recipe).unwrap();
        assert!(decoded.recipe.unknown.contains_key("future_field"));
        assert_eq!(journal.snapshot().unwrap().generation, 2);
    }

    #[test]
    fn wrong_owner_corruption_and_dirty_discard_are_rejected_without_destroying_record() {
        let dir = tempdir().unwrap();
        let id = ImageId(21);
        let other = ImageId(22);
        let mut journal =
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let path = journal.path();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["image_id"] = serde_json::to_value(other).unwrap();
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(matches!(
            SmartPreviewJournal::open(dir.path(), id),
            Err(StoreError::WrongImage)
        ));
        fs::write(
            &path,
            serde_json::to_vec(&Record {
                version: VERSION,
                image_id: id,
                generation: 1,
                incarnation: journal.incarnation,
                source_digest: [0; 32],
                source_len: 1,
                recipe: recipe(id),
                recipe_digest: *blake3::hash(&recipe(id)).as_bytes(),
                baseline_recipe: None,
                baseline_recipe_digest: None,
                baseline_xmp: None,
                baseline_xmp_digest: None,
                dirty: false,
            })
            .unwrap(),
        )
        .unwrap();
        journal.save_recipe(recipe(id)).unwrap();
        assert!(matches!(
            journal.discard_clean(),
            Err(StoreError::DirtyDiscard)
        ));
        assert!(journal.path().exists());
        fs::write(journal.path(), b"not json").unwrap();
        assert!(matches!(journal.snapshot(), Err(StoreError::Corrupt(_))));
    }

    #[test]
    fn clean_discard_and_stale_handle_preserve_newer_generation() {
        let dir = tempdir().unwrap();
        let id = ImageId(31);
        let stale = SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
            .unwrap();
        let (mut current, _) = SmartPreviewJournal::open(dir.path(), id).unwrap();
        current.save_recipe(recipe(id)).unwrap();
        let before = fs::read(current.path()).unwrap();
        assert!(matches!(
            stale.discard_clean(),
            Err(StoreError::RevisionConflict)
        ));
        assert_eq!(fs::read(current.path()).unwrap(), before);
        drop(current);
        let clean_id = ImageId(32);
        let clean = SmartPreviewJournal::create(
            dir.path(),
            clean_id,
            [0; 32],
            1,
            recipe(clean_id),
            None,
            None,
        )
        .unwrap();
        clean.discard_clean().unwrap();
        assert!(!clean.path().exists());
    }

    #[test]
    fn recreated_record_has_new_identity_and_alias_roots_share_lock() {
        let dir = tempdir().unwrap();
        let id = ImageId(33);
        let mut stale =
            SmartPreviewJournal::create(dir.path(), id, [3; 32], 1, recipe(id), None, None)
                .unwrap();
        let alias_root = dir.path().join(".");
        let current = SmartPreviewJournal::open(alias_root, id).unwrap().0;
        assert!(Arc::ptr_eq(&stale._guard, &current._guard));
        stale.discard_clean().unwrap();
        let replacement =
            SmartPreviewJournal::create(dir.path(), id, [4; 32], 2, recipe(id), None, None)
                .unwrap();
        assert_eq!(replacement.revision, stale.revision);
        assert_ne!(replacement.incarnation, stale.incarnation);
        assert!(matches!(
            stale.save_recipe(recipe(id)),
            Err(StoreError::RevisionConflict)
        ));
        assert_eq!(
            SmartPreviewJournal::open(dir.path(), id)
                .unwrap()
                .1
                .source_digest,
            [4; 32]
        );
    }

    #[test]
    fn missing_recipe_owner_and_zero_source_length_are_rejected() {
        let dir = tempdir().unwrap();
        let id = ImageId(34);
        let mut missing_owner: RecipeDocument = serde_json::from_slice(&recipe(id)).unwrap();
        missing_owner.recipe.image_id = None;
        let bytes = serde_json::to_vec(&missing_owner).unwrap();
        assert!(matches!(
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, bytes, None, None),
            Err(StoreError::WrongImage)
        ));
        assert!(matches!(
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 0, recipe(id), None, None),
            Err(StoreError::Corrupt(_))
        ));
    }

    #[test]
    fn current_schema_is_accepted_and_future_schema_cannot_replace_a_record() {
        let dir = tempdir().unwrap();
        let id = ImageId(38);
        let mut journal =
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let path = journal.path();
        let original_record = fs::read(&path).unwrap();

        let mut future: serde_json::Value = serde_json::from_slice(&recipe(id)).unwrap();
        future["recipe"]["schema_version"] = serde_json::json!(u32::MAX);
        let future_bytes = serde_json::to_vec(&future).unwrap();
        assert!(matches!(
            journal.save_recipe(future_bytes.clone()),
            Err(StoreError::Corrupt(_))
        ));
        assert_eq!(fs::read(&path).unwrap(), original_record);

        let future_id = ImageId(39);
        let mut future: serde_json::Value = serde_json::from_slice(&recipe(future_id)).unwrap();
        future["recipe"]["schema_version"] = serde_json::json!(u32::MAX);
        assert!(matches!(
            SmartPreviewJournal::create(
                dir.path(),
                future_id,
                [0; 32],
                1,
                serde_json::to_vec(&future).unwrap(),
                None,
                None
            ),
            Err(StoreError::Corrupt(_))
        ));
        assert!(
            !dir.path()
                .join("smart-previews")
                .join(future_id.to_string())
                .join("journal.json")
                .exists()
        );
        assert_eq!(journal.snapshot().unwrap().generation, 1);
    }

    #[test]
    fn recipe_digest_tampering_is_rejected() {
        let dir = tempdir().unwrap();
        let id = ImageId(35);
        let journal =
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let path = journal.path();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["recipe_digest"][0] = serde_json::json!(255);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(matches!(
            SmartPreviewJournal::open(dir.path(), id),
            Err(StoreError::Corrupt(message)) if message == "recipe digest mismatch"
        ));
    }

    #[test]
    fn generation_exhaustion_does_not_replace_the_record() {
        let dir = tempdir().unwrap();
        let id = ImageId(36);
        let mut journal =
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let path = journal.path();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["generation"] = serde_json::json!(u64::MAX);
        let bytes = serde_json::to_vec(&record).unwrap();
        fs::write(&path, &bytes).unwrap();
        journal.revision = u64::MAX;
        assert!(matches!(
            journal.save_recipe(recipe(id)),
            Err(StoreError::Corrupt(_))
        ));
        assert_eq!(fs::read(path).unwrap(), bytes);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_store_roots_share_the_same_process_lock() {
        use std::os::unix::fs::symlink;
        let dir = tempdir().unwrap();
        let real_root = dir.path().join("actual");
        let alias_root = dir.path().join("alias");
        fs::create_dir(&real_root).unwrap();
        symlink(&real_root, &alias_root).unwrap();
        let id = ImageId(37);
        let alias =
            SmartPreviewJournal::create(&alias_root, id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let real = SmartPreviewJournal::open(&real_root, id).unwrap().0;
        assert!(Arc::ptr_eq(&real._guard, &alias._guard));
    }

    #[test]
    fn existing_generated_directory_is_resynced_after_create_sync_failure() {
        let dir = tempdir().unwrap();
        let mut fail_once = true;
        let first = ensure_child_dir_durable_with(dir.path(), "generated", |_| {
            if fail_once {
                fail_once = false;
                Err(io::Error::other("injected directory sync failure"))
            } else {
                Ok(())
            }
        });
        assert_eq!(first.unwrap_err().kind(), io::ErrorKind::Other);
        assert!(dir.path().join("generated").is_dir());

        let mut sync_count = 0;
        let retried = ensure_child_dir_durable_with(dir.path(), "generated", |parent| {
            assert_eq!(parent, dir.path());
            sync_count += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(retried, dir.path().join("generated"));
        assert_eq!(sync_count, 1, "retry must sync the existing entry");

        let child_failure = ensure_child_dir_durable_with(&retried, "child", |_| {
            Err(io::Error::other("injected directory sync failure"))
        });
        assert!(
            child_failure.is_err(),
            "the parent fsync failure must remain visible to callers"
        );
        assert!(retried.join("child").is_dir());
    }

    #[test]
    fn discard_reports_directory_sync_failure_after_unlink() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("journal.json");
        fs::write(&path, b"record").unwrap();
        let result = remove_file_and_sync_directory(&path, |_| {
            Err(io::Error::other("injected directory sync failure"))
        });
        assert_eq!(
            result.unwrap_err().to_string(),
            "Smart Preview journal I/O: injected directory sync failure"
        );
        assert!(!path.exists(), "unlink may be visible despite sync failure");
    }

    #[test]
    fn adjacent_image_journals_do_not_collide_or_get_removed_together() {
        let dir = tempdir().unwrap();
        let first = ImageId(41);
        let second = ImageId(42);
        let one =
            SmartPreviewJournal::create(dir.path(), first, [1; 32], 1, recipe(first), None, None)
                .unwrap();
        let two =
            SmartPreviewJournal::create(dir.path(), second, [2; 32], 2, recipe(second), None, None)
                .unwrap();
        one.discard_clean().unwrap();
        assert!(!one.path().exists());
        assert!(two.path().exists());
        assert_eq!(
            SmartPreviewJournal::open(dir.path(), second)
                .unwrap()
                .1
                .source_digest,
            [2; 32]
        );
    }

    #[test]
    fn unsupported_version_and_oversized_record_are_rejected() {
        let dir = tempdir().unwrap();
        let id = ImageId(51);
        let journal =
            SmartPreviewJournal::create(dir.path(), id, [0; 32], 1, recipe(id), None, None)
                .unwrap();
        let path = journal.path();
        let mut record: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        record["version"] = serde_json::json!(VERSION + 1);
        fs::write(&path, serde_json::to_vec(&record).unwrap()).unwrap();
        assert!(matches!(
            SmartPreviewJournal::open(dir.path(), id),
            Err(StoreError::Corrupt(_))
        ));
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_RECORD_BYTES + 1)
            .unwrap();
        assert!(matches!(
            SmartPreviewJournal::open(dir.path(), id),
            Err(StoreError::TooLarge)
        ));
    }
}
