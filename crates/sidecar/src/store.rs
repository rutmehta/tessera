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

fn read_alias(alias: &Path) -> Option<Alias> {
    registry()
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .aliases
        .get(alias)
        .map(|(_, key)| key.clone())
        .or_else(|| {
            fs::read(alias)
                .ok()
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

/// Fix a protected source's recipe identity to `key` (64 hex characters).
/// Lookup-only until the next write to that recipe publishes the alias.
pub(super) fn pin(image: &Path, key: &str) {
    let image = resolved_path(image);
    let store = support(&image).join(".edits/lightroom");
    let path_key = blake3::hash(image.as_os_str().as_encoded_bytes())
        .to_hex()
        .to_string();
    let alias = store.join("paths").join(format!("{path_key}.json"));
    let recipe = recipe_path(&store, key);
    register_aliases(
        &recipe,
        vec![(
            alias,
            Alias::Pinned {
                pinned: key.to_owned(),
            },
        )],
    );
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
