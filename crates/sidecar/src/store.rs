//! App-owned protected-source metadata; no writes are performed by path lookup.
use super::{EngineResult, Path, PathBuf, SidecarPaths, atomic_write, resolved_path};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    sync::{Mutex, OnceLock},
};

#[derive(Clone, PartialEq, Eq)]
struct FileVersion {
    len: u64,
    modified: Option<std::time::SystemTime>,
    #[cfg(unix)]
    identity: (u64, u64, i64, i64),
}
impl FileVersion {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            identity: (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            ),
        })
    }
}

/// Content hashes computed (file reads), for cache tests.
pub(super) static CONTENT_HASHES: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(0);

fn content_key(image: &Path) -> Option<String> {
    let version = FileVersion::read(image)?;
    {
        let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((previous, key)) = registry.hashes.get(image)
            && previous == &version
        {
            return Some(key.clone());
        }
    }
    CONTENT_HASHES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut file = fs::File::open(image).ok()?;
    let mut hash = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hash).ok()?;
    // Do not associate a changing source with a cached digest.
    if FileVersion::read(image).as_ref() != Some(&version) {
        return None;
    }
    let key = hash.finalize().to_hex().to_string();
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    // Sized for whole libraries (REV4-SP N8): a plan refresh over ~20k
    // in-place Smart Previews must not re-read every file. Each entry is a
    // path, a file version and a 64-character key (~200 bytes; 128k entries
    // stay well under 32 MB). Keyed by file version, so a change re-hashes.
    if registry.hashes.len() >= 131_072 {
        registry.hashes.clear();
    }
    registry
        .hashes
        .insert(image.to_path_buf(), (version, key.clone()));
    Some(key)
}

// Accept the original string path aliases as well as the explicit stable
// recipe reference. A content hash can be shared; an established path must not
// lose its recipe when another path updates that hash's discovery alias.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
enum Alias {
    Path {
        content_hash: String,
        recipe_key: String,
    },
    /// A path whose recipe identity was fixed by its owner (an in-place
    /// Lightroom Smart Preview keyed by catalog image): never content-keyed,
    /// so byte-identical sources stay separate recipes.
    Pinned {
        pinned: String,
    },
    Key(String),
}
impl Alias {
    fn recipe_key(&self) -> &str {
        match self {
            Self::Path { recipe_key, .. } => recipe_key,
            Self::Pinned { pinned } => pinned,
            Self::Key(key) => key,
        }
    }
    fn content_hash(&self) -> &str {
        match self {
            Self::Path { content_hash, .. } => content_hash,
            Self::Pinned { pinned } => pinned,
            Self::Key(key) => key,
        }
    }
    fn valid(&self) -> bool {
        [self.recipe_key(), self.content_hash()]
            .into_iter()
            .all(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()))
    }
}

#[derive(Default)]
struct Registry {
    read_only: BTreeSet<PathBuf>,
    roots: BTreeMap<PathBuf, PathBuf>,
    hashes: BTreeMap<PathBuf, (FileVersion, String)>,
    aliases: BTreeMap<PathBuf, (PathBuf, Alias)>,
    destinations: BTreeMap<PathBuf, BTreeMap<PathBuf, Alias>>,
}
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();
fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(Default::default)
}

pub(super) fn register(source: &Path, support: &Path) {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .roots
        .insert(source.to_path_buf(), resolved_path(support));
}

pub(super) fn register_read_only(source: &Path, support: &Path) {
    let source = resolved_path(source);
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    registry
        .roots
        .insert(source.clone(), resolved_path(support));
    registry.read_only.insert(source);
}

pub(super) fn is_read_only(image: &Path) -> bool {
    let image = resolved_path(image);
    let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    image.ancestors().any(|p| registry.read_only.contains(p))
}

fn support(image: &Path) -> PathBuf {
    let registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    for ancestor in image.ancestors() {
        if let Some(support) = registry.roots.get(ancestor) {
            return support.clone();
        }
    }
    // Same environment override and macOS default as EngineLibrary.supportDirectory.
    std::env::var_os("TESSERA_APP_DIR")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
                .join("Library/Application Support/Tessera")
        })
}

/// Alias files read from disk (scan entries and lookups), for linearity tests.
pub(super) static ALIAS_READS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn read_alias_file(path: &Path) -> Option<Vec<u8>> {
    ALIAS_READS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    fs::read(path).ok()
}

fn read_alias(alias: &Path) -> Option<Alias> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .aliases
        .get(alias)
        .map(|(_, key)| key.clone())
        .or_else(|| {
            read_alias_file(alias)
                .and_then(|bytes| serde_json::from_slice::<Alias>(&bytes).ok())
                .filter(Alias::valid)
        })
}

fn content_alias(store: &Path, key: &str) -> PathBuf {
    store.join("content").join(format!("{key}.json"))
}

fn recipe_path(store: &Path, key: &str) -> PathBuf {
    store
        .join("objects")
        .join(&key[..2])
        .join(format!("{key}.json"))
}

pub(super) fn paths(image: &Path) -> SidecarPaths {
    let image = resolved_path(image);
    let store = support(&image).join(".edits/lightroom");
    let path_key = blake3::hash(image.as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    let alias = store.join("paths").join(format!("{path_key}.json"));
    let previous = read_alias(&alias);
    if let Some(pinned @ Alias::Pinned { .. }) = previous {
        let recipe = recipe_path(&store, pinned.recipe_key());
        register_aliases(&recipe, vec![(alias, pinned)]);
        return SidecarPaths {
            xmp: recipe.with_extension("xmp"),
            recipe,
        };
    }
    // A path already owning a recipe wins over changed source bytes. Content
    // aliases point directly to stable object keys (never to another alias).
    let existing = previous
        .as_ref()
        .map(|alias| alias.recipe_key().to_owned())
        .filter(|key| recipe_path(&store, key).is_file());
    let content = content_key(&image);
    let key = existing
        .or_else(|| {
            content.as_ref().map(|key| {
                read_alias(&content_alias(&store, key))
                    .map(|alias| alias.recipe_key().to_owned())
                    .unwrap_or_else(|| key.clone())
            })
        })
        .or_else(|| previous.as_ref().map(|alias| alias.recipe_key().to_owned()))
        .unwrap_or_else(|| path_key.clone());
    let recipe = recipe_path(&store, &key);
    // Keep lookup read-only. Record the new hash immediately in memory and
    // durably publish its object alias before the path alias on the next save.
    let current = content
        .clone()
        .or_else(|| previous.map(|alias| alias.content_hash().to_owned()))
        .unwrap_or(path_key);
    let mut aliases = vec![(
        alias,
        Alias::Path {
            content_hash: current,
            recipe_key: key.clone(),
        },
    )];
    if let Some(content) = content {
        aliases.push((content_alias(&store, &content), Alias::Key(key)));
    }
    register_aliases(&recipe, aliases);
    SidecarPaths {
        xmp: recipe.with_extension("xmp"),
        recipe,
    }
}

fn register_aliases(recipe: &Path, aliases: Vec<(PathBuf, Alias)>) {
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    for (alias, key) in aliases {
        if let Some((previous, _)) = registry
            .aliases
            .insert(alias.clone(), (recipe.to_path_buf(), key.clone()))
            && previous != recipe
            && let Some(aliases) = registry.destinations.get_mut(&previous)
        {
            aliases.remove(&alias);
        }
        registry
            .destinations
            .entry(recipe.to_path_buf())
            .or_default()
            .insert(alias, key);
    }
}

/// What re-keying one protected source did (SP-INT4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PinOutcome {
    /// Not a protected source; ordinary sidecars are unaffected.
    Unprotected,
    /// Already on its key; nothing changed.
    AlreadyPinned,
    /// No recipe yet; the key is published for future writes.
    Fresh,
    /// Its recipe was copied to the new key.
    Migrated,
    /// The key already held byte-identical edits from another source with the
    /// same identity (another catalog's copy of the photo): they now share it.
    Shared,
    /// The key already holds *different* edits owned by another source: this
    /// source keeps its own recipe and key. Nothing is merged or deleted.
    Conflict,
    /// An interrupted migration left a copy that diverged: the newer of the
    /// two is now on the key and the other is kept as a backup.
    Recovered,
}

#[derive(Default)]
struct StoreRefs {
    /// Non-pinned path aliases per recipe key.
    legacy: std::collections::HashMap<String, usize>,
    /// Content aliases per recipe key.
    content: std::collections::HashMap<String, usize>,
    /// Pinned path aliases (alias file names) per recipe key.
    pinned: std::collections::HashMap<String, BTreeSet<String>>,
}

/// One re-keying pass. Each store's alias directories are read once into
/// reference counts (linear in the number of recipes); legacy objects are
/// removed in [`PinBatch::finish`] only when nothing references them any more
/// and the destination holds byte-identical content.
#[derive(Default)]
pub struct PinBatch {
    /// Classify only (plan preview): no copy, alias, backup or delete.
    pub(super) dry_run: bool,
    stores: std::collections::HashMap<PathBuf, StoreRefs>,
    /// (legacy object, destination, store, legacy key)
    candidates: Vec<(PathBuf, PathBuf, PathBuf, String)>,
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (fs::read(a), fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// The recorded write stamp of a recipe document, when it has one.
fn recorded(path: &Path) -> Option<(i64, u64, String)> {
    let doc: super::RecipeDocument = serde_json::from_slice(&fs::read(path).ok()?).ok()?;
    let stamp = doc.last_writer;
    (stamp.timestamp_ms > 0).then_some((stamp.timestamp_ms, stamp.counter, stamp.machine_id))
}

/// Whether `a` holds the newer edit: recorded edit time when both documents
/// have one (as the sidecar merge decides), file mtime otherwise.
fn newer(a: &Path, b: &Path) -> bool {
    match (recorded(a), recorded(b)) {
        (Some(a), Some(b)) => a > b,
        _ => {
            let modified = |p: &Path| fs::metadata(p).and_then(|m| m.modified()).ok();
            modified(a) > modified(b)
        }
    }
}

/// Move a recipe object (and its XMP) to `<key>.backup-<ns>.json` beside it.
/// Lookups only ever resolve `<key>.json`, so nothing reads or overwrites it.
fn backup(object: &Path, key: &str) -> EngineResult<()> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    for extension in ["json", "xmp"] {
        let from = object.with_extension(extension);
        if from.is_file() {
            let to = object.with_file_name(format!("{key}.backup-{stamp}.{extension}"));
            fs::rename(&from, &to).map_err(|e| engine_api::EngineError::io_at(&from, &e))?;
        }
    }
    Ok(())
}

fn alias_key_of(path: &Path) -> Option<String> {
    path.file_stem()?.to_str().map(str::to_owned)
}

impl PinBatch {
    pub(super) fn preview() -> Self {
        Self {
            dry_run: true,
            ..Default::default()
        }
    }
    fn refs(&mut self, store: &Path) -> &mut StoreRefs {
        self.stores.entry(store.to_path_buf()).or_insert_with(|| {
            let mut refs = StoreRefs::default();
            let read = |dir: PathBuf| {
                fs::read_dir(dir)
                    .into_iter()
                    .flatten()
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        let name = alias_key_of(&e.path())?;
                        let alias =
                            serde_json::from_slice::<Alias>(&read_alias_file(&e.path())?).ok()?;
                        Some((name, alias))
                    })
                    .collect::<Vec<_>>()
            };
            for (name, alias) in read(store.join("paths")) {
                match alias {
                    Alias::Pinned { pinned } => {
                        refs.pinned.entry(pinned).or_default().insert(name);
                    }
                    other => {
                        *refs
                            .legacy
                            .entry(other.recipe_key().to_owned())
                            .or_default() += 1
                    }
                }
            }
            for (_, alias) in read(store.join("content")) {
                *refs
                    .content
                    .entry(alias.recipe_key().to_owned())
                    .or_default() += 1;
            }
            refs
        })
    }

    fn copy_object(from: &Path, to: &Path) -> EngineResult<()> {
        let read = |p: &Path| fs::read(p).map_err(|e| engine_api::EngineError::io_at(p, &e));
        atomic_write(to, &read(from)?)?;
        let xmp = from.with_extension("xmp");
        if xmp.is_file() {
            atomic_write(&to.with_extension("xmp"), &read(&xmp)?)?;
        }
        Ok(())
    }

    /// Re-key one protected source to `key` (64 hex characters).
    pub(super) fn pin(&mut self, image: &Path, key: &str) -> EngineResult<PinOutcome> {
        let image = resolved_path(image);
        let store = support(&image).join(".edits/lightroom");
        let path_key = blake3::hash(image.as_os_str().as_encoded_bytes())
            .to_hex()
            .to_string();
        let alias = store.join("paths").join(format!("{path_key}.json"));
        let destination = recipe_path(&store, key);
        let previous = read_alias(&alias);
        if let Some(Alias::Pinned { pinned }) = &previous
            && pinned == key
        {
            register_aliases(
                &destination,
                vec![(
                    alias,
                    Alias::Pinned {
                        pinned: key.to_owned(),
                    },
                )],
            );
            return Ok(PinOutcome::AlreadyPinned);
        }
        // The recipe this source resolves to now (path alias or content key).
        let current = paths(&image).recipe;
        let legacy_key = alias_key_of(&current).unwrap_or_default();
        let others: usize = self
            .refs(&store)
            .pinned
            .get(key)
            .map_or(0, |set| set.iter().filter(|n| **n != path_key).count());
        let mut recovered: Option<bool> = None;
        let outcome = if current == destination {
            // Found through a content alias already pointing at the key: the
            // same photo another source (catalog) is pinned to.
            if others > 0 {
                PinOutcome::Shared
            } else {
                PinOutcome::AlreadyPinned
            }
        } else if !current.is_file() {
            PinOutcome::Fresh
        } else if !destination.is_file() {
            if !self.dry_run {
                Self::copy_object(&current, &destination)?;
            }
            PinOutcome::Migrated
        } else if same_bytes(&current, &destination) {
            let xmp = current.with_extension("xmp");
            if !self.dry_run && xmp.is_file() && !destination.with_extension("xmp").is_file() {
                atomic_write(
                    &destination.with_extension("xmp"),
                    &fs::read(&xmp).map_err(|e| engine_api::EngineError::io_at(&xmp, &e))?,
                )?;
            }
            if others > 0 {
                PinOutcome::Shared
            } else {
                PinOutcome::Migrated
            }
        } else if others > 0 {
            // Another source owns different edits under this key: keep both,
            // leave this source on its own recipe, report it.
            return Ok(PinOutcome::Conflict);
        } else {
            // A copy no source references diverged from its legacy object: an
            // interrupted migration (REV4-SP S1). The edit recorded as newer
            // wins (file mtime only when a document has no recorded time); the
            // loser is moved to a labelled backup no lookup resolves to.
            let legacy_newer = newer(&current, &destination);
            let legacy_shared = self
                .refs(&store)
                .legacy
                .get(&legacy_key)
                .copied()
                .unwrap_or(0)
                > usize::from(
                    matches!(&previous, Some(a) if !matches!(a, Alias::Pinned { .. })
                    && a.recipe_key() == legacy_key && fs::metadata(&alias).is_ok()),
                );
            if !self.dry_run {
                if legacy_newer {
                    backup(&destination, key)?;
                    Self::copy_object(&current, &destination)?;
                } else if !legacy_shared {
                    // Only this photo used the legacy object: label it.
                    backup(&current, &legacy_key)?;
                }
            }
            recovered = Some(legacy_shared);
            PinOutcome::Recovered
        };
        if self.dry_run {
            return Ok(outcome);
        }
        // Publish the key durably, then account for references.
        let pinned = Alias::Pinned {
            pinned: key.to_owned(),
        };
        atomic_write(&alias, &serde_json::to_vec(&pinned)?)?;
        register_aliases(&destination, vec![(alias.clone(), pinned)]);
        let on_disk = fs::metadata(&alias).is_ok();
        let refs = self.refs(&store);
        if let Some(previous) = &previous
            && !matches!(previous, Alias::Pinned { .. })
            && on_disk
            && let Some(count) = refs.legacy.get_mut(previous.recipe_key())
        {
            *count = count.saturating_sub(1);
        }
        refs.pinned
            .entry(key.to_owned())
            .or_default()
            .insert(path_key);
        if recovered == Some(false)
            || (recovered.is_none()
                && current != destination
                && current.is_file()
                && same_bytes(&current, &destination))
        {
            // Unmigrated sources with these bytes now find the identical copy.
            if let Some(content) = content_key(&image) {
                let content_alias = content_alias(&store, &content);
                if read_alias(&content_alias).is_some_and(|a| a.recipe_key() == legacy_key) {
                    let alias = Alias::Key(key.to_owned());
                    atomic_write(&content_alias, &serde_json::to_vec(&alias)?)?;
                    register_aliases(&destination, vec![(content_alias, alias)]);
                    if let Some(count) = refs.content.get_mut(&legacy_key) {
                        *count = count.saturating_sub(1);
                    }
                }
            }
            if current.is_file() && same_bytes(&current, &destination) {
                self.candidates
                    .push((current, destination, store, legacy_key));
            }
        }
        Ok(outcome)
    }

    /// Remove legacy objects that nothing references any more and whose bytes
    /// equal their destination's. Anything else is kept.
    pub(super) fn finish(mut self) -> EngineResult<()> {
        let candidates = std::mem::take(&mut self.candidates);
        let mut seen = BTreeSet::new();
        for (legacy, destination, store, legacy_key) in candidates {
            if !seen.insert(legacy.clone()) {
                continue;
            }
            let refs = self.refs(&store);
            let referenced = refs.legacy.get(&legacy_key).copied().unwrap_or(0) > 0
                || refs.content.get(&legacy_key).copied().unwrap_or(0) > 0;
            if referenced || !same_bytes(&legacy, &destination) {
                continue;
            }
            let xmp = legacy.with_extension("xmp");
            if xmp.is_file() {
                if !same_bytes(&xmp, &destination.with_extension("xmp")) {
                    continue;
                }
                let _ = fs::remove_file(&xmp);
            }
            let _ = fs::remove_file(&legacy);
        }
        Ok(())
    }
}

/// The recipe object a protected source would use under `key`.
pub(super) fn keyed_recipe(image: &Path, key: &str) -> PathBuf {
    let image = resolved_path(image);
    recipe_path(&support(&image).join(".edits/lightroom"), key)
}

pub(super) fn persist_aliases(destination: &Path) -> EngineResult<()> {
    let recipe = destination.with_extension("json");
    let aliases = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .destinations
        .get(&recipe)
        .cloned()
        .unwrap_or_default();
    for alias in aliases.keys() {
        super::Sidecar::ensure_writable_destination(alias)?;
    }
    for (alias, key) in aliases {
        let bytes = serde_json::to_vec(&key)?;
        if fs::read(&alias).ok().as_ref() != Some(&bytes) {
            atomic_write(&alias, &bytes)?;
        }
    }
    Ok(())
}
