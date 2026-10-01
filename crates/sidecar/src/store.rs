//! App-owned protected-source metadata; no writes are performed by path lookup.
use super::{EngineResult, Path, PathBuf, SidecarPaths, atomic_write, resolved_path};
use std::{
    collections::BTreeMap,
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
    let mut file = fs::File::open(image).ok()?;
    let mut hash = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hash).ok()?;
    // Do not associate a changing source with a cached digest.
    if FileVersion::read(image).as_ref() != Some(&version) {
        return None;
    }
    let key = hash.finalize().to_hex().to_string();
    let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
    if registry.hashes.len() >= 8192 {
        registry.hashes.clear();
    }
    registry
        .hashes
        .insert(image.to_path_buf(), (version, key.clone()));
    Some(key)
}

#[derive(Default)]
struct Registry {
    roots: BTreeMap<PathBuf, PathBuf>,
    hashes: BTreeMap<PathBuf, (FileVersion, String)>,
    aliases: BTreeMap<PathBuf, (PathBuf, String)>,
    destinations: BTreeMap<PathBuf, BTreeMap<PathBuf, String>>,
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

pub(super) fn paths(image: &Path) -> SidecarPaths {
    let image = resolved_path(image);
    let store = support(&image).join(".edits/lightroom");
    let path_key = blake3::hash(image.as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    let alias = store.join("paths").join(format!("{path_key}.json"));
    let content = content_key(&image);
    let remembered = registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .aliases
        .get(&alias)
        .map(|(_, key)| key.clone());
    let key = content
        .or(remembered)
        .or_else(|| {
            fs::read(&alias)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<String>(&bytes).ok())
                .filter(|key| key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()))
        })
        .unwrap_or(path_key);
    let directory = store.join("objects").join(&key[..2]);
    let recipe = directory.join(format!("{key}.json"));
    {
        let mut registry = registry().lock().unwrap_or_else(|e| e.into_inner());
        if let Some((previous, _)) = registry
            .aliases
            .insert(alias.clone(), (recipe.clone(), key.clone()))
            && previous != recipe
            && let Some(aliases) = registry.destinations.get_mut(&previous)
        {
            aliases.remove(&alias);
        }
        registry
            .destinations
            .entry(recipe.clone())
            .or_default()
            .insert(alias, key.clone());
    }
    SidecarPaths {
        recipe,
        xmp: directory.join(format!("{key}.xmp")),
    }
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
