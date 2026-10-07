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

pub(crate) fn persistent(root: Option<PathBuf>, provider: PreviewProvider) -> PreviewProvider {
    let Some(root) = root else {
        return provider;
    };
    // Bump the namespace whenever the pixel/hash policy changes. Cache creation
    // and validation happen on the worker, never during session construction.
    let root = root.join("cull-hashes-v1");
    Arc::new(move |info| {
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
        // A read-only/full cache must not prevent manual culling or grouping.
        let _ = persistence::atomic_write(&path, &bytes);
        Ok(hash)
    })
}
