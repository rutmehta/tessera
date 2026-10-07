//! Persistent dHashes. Values are hints, keyed by image identity and nanosecond
//! source/sidecar revisions, never a path-only or image-id-only cache hit.
use crate::{PreviewProvider, persistence};
use engine_api::{EngineError, EngineResult};
use index::ImageInfo;
use previews::PreviewKey;
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Serialize, Deserialize, PartialEq, Eq)]
struct Fingerprint {
    source: [u8; 32],
    recipe: Option<[u8; 32]>,
}
#[derive(Serialize, Deserialize)]
struct CachedHash {
    fingerprint: Fingerprint,
    hash: Option<u64>,
}
fn revision(path: &Path) -> EngineResult<[u8; 32]> {
    PreviewKey::for_source(path, 256, 0, [0; 32])
        .map(|key| key.file_hash)
        .map_err(|e| EngineError::invalid("cull fingerprint", e.to_string()))
}
fn fingerprint(info: &ImageInfo) -> EngineResult<Fingerprint> {
    let recipe = sidecar::Sidecar::paths(&info.path).recipe;
    Ok(Fingerprint {
        source: revision(&info.path)?,
        recipe: match std::fs::metadata(&recipe) {
            Ok(_) => Some(revision(&recipe)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(EngineError::io_at(&recipe, &e)),
        },
    })
}

pub(crate) fn persistent(
    policy: Option<HashCachePolicy>,
    provider: PreviewProvider,
) -> PreviewProvider {
    let Some(policy) = policy else {
        return provider;
    };
    // Bump the namespace whenever the pixel/hash policy changes. Cache creation
    // and validation happen on the worker, never during session construction.
    let legacy_removed = std::sync::atomic::AtomicBool::new(false);
    Arc::new(move |info| {
        if !legacy_removed.swap(true, std::sync::atomic::Ordering::AcqRel) {
            policy.remove_legacy();
        }
        let Some(root) = policy.directory(info) else {
            return provider(info);
        };
        let before = fingerprint(info)?;
        let path = root.join(format!("{}.json", info.id));
        if let Ok(file) = std::fs::File::open(&path)
            && let Ok(cached) = serde_json::from_reader::<_, CachedHash>(file.take(4096))
            && cached.fingerprint == before
        {
            return Ok(cached.hash);
        }
        let hash = provider(info)?;
        if before != fingerprint(info)? {
            return Err(EngineError::invalid("cull source", "changed while hashing"));
        }
        let bytes = serde_json::to_vec(&CachedHash {
            fingerprint: before,
            hash,
        })?;
        // A provider can spend time decoding. Revalidate after it returns so a
        // moved/symlinked ancestor cannot redirect this write into a library.
        // A read-only/full cache must not prevent manual culling or grouping.
        if policy.directory(info).as_ref() == Some(&root) {
            let _ = persistence::atomic_write(&path, &bytes);
        }
        Ok(hash)
    })
}
/// Explicit host approval for persistent hashes. Pixel providers must change
/// their identity or version whenever their comparison-pixel policy changes.
#[derive(Clone, Debug)]
pub struct HashCachePolicy {
    root: PathBuf,
    protected_roots: Vec<PathBuf>,
    namespace: String,
}

fn resolved(path: &Path) -> EngineResult<PathBuf> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return Err(EngineError::invalid(
            "hash cache root",
            "expected absolute path without ..",
        ));
    }
    let mut ancestor = path;
    let mut suffix = Vec::new();
    while !ancestor.exists() {
        suffix.push(
            ancestor
                .file_name()
                .ok_or_else(|| EngineError::invalid("hash cache root", "unresolvable path"))?,
        );
        ancestor = ancestor
            .parent()
            .ok_or_else(|| EngineError::invalid("hash cache root", "unresolvable parent"))?;
    }
    let mut result = ancestor
        .canonicalize()
        .map_err(|e| EngineError::io_at(ancestor, &e))?;
    for component in suffix.into_iter().rev() {
        result.push(component);
    }
    Ok(result)
}

fn protected_name(path: &Path) -> bool {
    path.components().any(|c| {
        let name = c.as_os_str().to_string_lossy().to_ascii_lowercase();
        matches!(name.as_str(), "pictures" | "tessera library" | "lightroom")
            || name.ends_with(".lrcat")
            || name.ends_with(".lrdata")
    })
}

impl HashCachePolicy {
    /// Approve a host Application Support directory, excluding catalog/library
    /// roots supplied by the host. No directory is created during approval.
    pub fn application_support(
        root: PathBuf,
        provider_id: &str,
        pixel_policy_version: u32,
        protected_roots: &[PathBuf],
    ) -> EngineResult<Self> {
        if provider_id.is_empty()
            || provider_id.len() > 80
            || !provider_id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(EngineError::invalid(
                "hash provider identity",
                "expected 1-80 ASCII letters, digits, hyphens or underscores",
            ));
        }
        if protected_name(&root) {
            return Err(EngineError::invalid(
                "hash cache root",
                "protected library location",
            ));
        }
        let root = resolved(&root)?;
        if protected_name(&root)
            || !root
                .components()
                .any(|c| c.as_os_str() == "Application Support")
        {
            return Err(EngineError::invalid(
                "hash cache root",
                "expected Application Support location",
            ));
        }
        let protected_roots = protected_roots
            .iter()
            .map(|p| resolved(p))
            .collect::<EngineResult<Vec<_>>>()?;
        if protected_roots.iter().any(|p| root.starts_with(p)) {
            return Err(EngineError::invalid(
                "hash cache root",
                "inside protected catalog or library",
            ));
        }
        Ok(Self {
            root,
            protected_roots,
            namespace: format!("{provider_id}-v{pixel_policy_version}"),
        })
    }

    /// Removes the LR-13c `cull-hashes-v1` directory directly inside this
    /// approved root, once per session on the worker. A symlink is never
    /// followed or removed, and a redirected root is left alone.
    fn remove_legacy(&self) {
        let legacy = self.root.join("cull-hashes-v1");
        let Ok(meta) = std::fs::symlink_metadata(&legacy) else {
            return;
        };
        if !meta.file_type().is_dir()
            || resolved(&self.root).ok().as_ref() != Some(&self.root)
            || protected_name(&legacy)
            || self.protected_roots.iter().any(|p| legacy.starts_with(p))
        {
            return;
        }
        // Best-effort: std's remove_dir_all does not follow inner symlinks.
        let _ = std::fs::remove_dir_all(&legacy);
    }

    fn directory(&self, info: &ImageInfo) -> Option<PathBuf> {
        let path = resolved(&self.root.join("cull-hashes-v2").join(&self.namespace)).ok()?;
        let source = resolved(&info.path).ok()?;
        // Revalidate at use time, including symlinks introduced after approval.
        if !path.starts_with(&self.root)
            || protected_name(&path)
            || self
                .protected_roots
                .iter()
                .any(|root| path.starts_with(root))
            || source
                .parent()
                .is_some_and(|parent| path.starts_with(parent))
        {
            return None;
        }
        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lr13d_rejects_protected_and_unapproved_roots() {
        let dir = tempfile::tempdir().unwrap();
        let approve = |root| HashCachePolicy::application_support(root, "synthetic", 1, &[]);
        assert!(approve(dir.path().join("catalog")).is_err());
        assert!(approve(dir.path().join("Pictures/Application Support/App")).is_err());
        assert!(approve(dir.path().join("Tessera Library/Application Support/App")).is_err());
        assert!(approve(dir.path().join("catalog.lrdata/Application Support/App")).is_err());
        let protected = dir.path().join("catalog");
        assert!(
            HashCachePolicy::application_support(
                protected.join("Application Support/App"),
                "synthetic",
                1,
                &[protected]
            )
            .is_err()
        );
        assert!(approve(dir.path().join("Application Support/App")).is_ok());
    }

    #[cfg(unix)]
    #[test]
    fn lr13d_revalidates_cache_destination_after_provider_returns() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Application Support/App");
        let protected = dir.path().join("protected-photos");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir(&protected).unwrap();
        let source = dir.path().join("sources/source.dng");
        std::fs::create_dir(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"synthetic source pixels").unwrap();
        let info = ImageInfo {
            id: engine_api::id::ImageId(1),
            path: source,
            size: 23,
            capture_seconds: None,
        };
        let policy = HashCachePolicy::application_support(
            root.clone(),
            "synthetic",
            1,
            std::slice::from_ref(&protected),
        )
        .unwrap();
        let destination = protected.clone();
        // The provider runs after the initial cache validation. Change the
        // ancestor here to deterministically model a move during a slow decode.
        let provider = persistent(
            Some(policy),
            Arc::new(move |_| {
                std::fs::rename(&root, root.with_file_name("retired-app")).unwrap();
                std::os::unix::fs::symlink(&destination, &root).unwrap();
                Ok(Some(17))
            }),
        );
        assert_eq!(provider(&info).unwrap(), Some(17));
        assert_eq!(
            std::fs::read_dir(&protected).unwrap().count(),
            0,
            "changed destination must not receive cache directories or files"
        );
    }

    #[cfg(unix)]
    #[test]
    fn lr13d_rejects_symlink_into_protected_root() {
        let dir = tempfile::tempdir().unwrap();
        let protected = dir.path().join("photos");
        std::fs::create_dir(&protected).unwrap();
        let support = dir.path().join("Application Support");
        std::os::unix::fs::symlink(&protected, &support).unwrap();
        assert!(
            HashCachePolicy::application_support(support.join("App"), "synthetic", 1, &[protected])
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn lr13e_removes_v1_cache_inside_approved_root_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Application Support/App");
        std::fs::create_dir_all(root.join("cull-hashes-v1")).unwrap();
        std::fs::write(root.join("cull-hashes-v1/1.json"), b"{}").unwrap();
        let elsewhere = dir.path().join("elsewhere/cull-hashes-v1");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("1.json"), b"{}").unwrap();
        let source = dir.path().join("sources/source.dng");
        std::fs::create_dir(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"synthetic source pixels").unwrap();
        let info = ImageInfo {
            id: engine_api::id::ImageId(1),
            path: source,
            size: 23,
            capture_seconds: None,
        };
        let policy =
            HashCachePolicy::application_support(root.clone(), "synthetic", 1, &[]).unwrap();
        let provider = persistent(Some(policy), Arc::new(|_| Ok(Some(3))));
        assert_eq!(provider(&info).unwrap(), Some(3));
        assert!(
            !root.join("cull-hashes-v1").exists(),
            "stale v1 cache remains"
        );
        assert!(
            elsewhere.join("1.json").exists(),
            "only the approved root is cleaned"
        );
        assert!(root.join("cull-hashes-v2").exists());

        // A v1 symlink is never followed: its target survives.
        let other = dir.path().join("Application Support/Other");
        std::fs::create_dir_all(&other).unwrap();
        std::os::unix::fs::symlink(&elsewhere, other.join("cull-hashes-v1")).unwrap();
        let policy =
            HashCachePolicy::application_support(other.clone(), "synthetic", 1, &[]).unwrap();
        let provider = persistent(Some(policy), Arc::new(|_| Ok(Some(3))));
        assert_eq!(provider(&info).unwrap(), Some(3));
        assert!(
            elsewhere.join("1.json").exists(),
            "symlink target was deleted"
        );
    }
}
