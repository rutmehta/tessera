//! In-process serialization for the two adopted Engine recipe writers.
//!
//! This is a destination gate and an internal full-byte revision, not a
//! filesystem CAS, batch API, or transaction across recipe, XMP, and index.
use crate::{Result, catalog, failure};
use sidecar::Sidecar;
#[cfg(test)]
use std::sync::{TryLockError, mpsc};
use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

type GateTable = HashMap<PathBuf, Weak<GateState>>;
static GATES: OnceLock<Mutex<GateTable>> = OnceLock::new();

/// The key is the actual recipe destination, including the existing same-stem
/// collision between different image extensions.
fn destination_key(image: &Path) -> Result<PathBuf> {
    let recipe = Sidecar::paths(image).recipe;
    let edits = recipe
        .parent()
        .ok_or_else(|| failure("recipe has no parent directory"))?;
    let resolved_dir = match fs::symlink_metadata(edits) {
        Ok(_) => fs::canonicalize(edits)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let image_dir = image
                .parent()
                .ok_or_else(|| failure("image has no parent directory"))?;
            fs::canonicalize(image_dir)?.join(".edits")
        }
        Err(e) => return Err(e.into()),
    };
    let filename = recipe
        .file_name()
        .ok_or_else(|| failure("recipe has no filename"))?;
    Ok(resolved_dir.join(filename))
}

pub(crate) fn gate_for(image: &Path) -> Result<Arc<GateState>> {
    let key = destination_key(image)?;
    let mut table = GATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(failure)?;
    table.retain(|_, weak| weak.strong_count() != 0);
    if let Some(existing) = table.get(&key).and_then(Weak::upgrade) {
        return Ok(existing);
    }
    let state = Arc::new(GateState {
        key: key.clone(),
        epoch: Mutex::new(0),
        #[cfg(test)]
        contended_observer: Mutex::new(None),
    });
    table.insert(key, Arc::downgrade(&state));
    Ok(state)
}

pub(crate) struct GateState {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "retained for the internal full-byte revision")
    )]
    key: PathBuf,
    epoch: Mutex<u64>,
    #[cfg(test)]
    contended_observer: Mutex<Option<mpsc::Sender<()>>>,
}

/// Keeps its gate state alive so a weak-table prune/recreate cannot reset the
/// epoch while a caller still holds an old revision.
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "internal revision awaits a guarded write caller")
)]
pub(crate) struct RecipeRevision {
    state: Arc<GateState>,
    epoch: u64,
    bytes: blake3::Hash,
}

pub(crate) struct WriteGuard<'a> {
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "held-guard revision comparison awaits a caller")
    )]
    state: &'a GateState,
    epoch: MutexGuard<'a, u64>,
}

impl GateState {
    fn lock(&self) -> Result<MutexGuard<'_, u64>> {
        #[cfg(test)]
        match self.epoch.try_lock() {
            Ok(guard) => return Ok(guard),
            Err(TryLockError::WouldBlock) => {
                if let Some(observer) = self.contended_observer.lock().map_err(failure)?.take() {
                    let _ = observer.send(());
                }
            }
            Err(TryLockError::Poisoned(_)) => return Err(failure("recipe write gate poisoned")),
        }
        self.epoch.lock().map_err(failure)
    }

    #[cfg(test)]
    pub(crate) fn observe_next_contended_gate_attempt(&self) -> mpsc::Receiver<()> {
        let (sender, receiver) = mpsc::channel();
        *self.contended_observer.lock().unwrap() = Some(sender);
        receiver
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "internal revision awaits a caller")
    )]
    fn check_image(&self, image: &Path) -> Result<()> {
        if destination_key(image)? != self.key {
            return Err(failure("image does not use this recipe destination"));
        }
        Ok(())
    }

    pub(crate) fn begin_write(&self) -> Result<WriteGuard<'_>> {
        let epoch = self.lock()?;
        if *epoch == u64::MAX {
            return Err(failure("recipe write epoch exhausted"));
        }
        Ok(WriteGuard { state: self, epoch })
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "internal revision awaits a caller")
    )]
    pub(crate) fn capture_revision(self: &Arc<Self>, image: &Path) -> Result<RecipeRevision> {
        let epoch = self.lock()?;
        self.check_image(image)?;
        Ok(RecipeRevision {
            state: self.clone(),
            epoch: *epoch,
            bytes: raw_revision(image)?,
        })
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "internal revision awaits a caller")
    )]
    pub(crate) fn matches_revision(&self, revision: &RecipeRevision, image: &Path) -> Result<bool> {
        let epoch = self.lock()?;
        revision_matches(self, *epoch, revision, image)
    }
}

impl WriteGuard<'_> {
    /// Must use this held guard: calling `GateState::matches_revision` here
    /// would recursively acquire the same mutex.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "internal guarded comparison awaits a caller")
    )]
    pub(crate) fn matches_revision(&self, revision: &RecipeRevision, image: &Path) -> Result<bool> {
        revision_matches(self.state, *self.epoch, revision, image)
    }
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        // `begin_write` rejected MAX while holding the mutex. Even an errored
        // or same-byte attempt invalidates a captured token conservatively.
        *self.epoch += 1;
    }
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "internal revision awaits a caller")
)]
fn revision_matches(
    state: &GateState,
    epoch: u64,
    revision: &RecipeRevision,
    image: &Path,
) -> Result<bool> {
    if !std::ptr::eq(Arc::as_ptr(&revision.state), state) || revision.epoch != epoch {
        return Ok(false);
    }
    state.check_image(image)?;
    Ok(revision.bytes == raw_revision(image)?)
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "internal revision awaits a caller")
)]
fn hash_path(hasher: &mut blake3::Hasher, path: &Path) -> Result<()> {
    let encoded = path.as_os_str().as_encoded_bytes();
    hasher.update(&u64::try_from(encoded.len()).map_err(failure)?.to_le_bytes());
    hasher.update(encoded);
    Ok(())
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "internal revision awaits a caller")
)]
fn hash_file(hasher: &mut blake3::Hasher, path: &Path) -> Result<()> {
    match fs::read(path) {
        Ok(bytes) => {
            hasher.update(&[1]);
            hasher.update(&u64::try_from(bytes.len()).map_err(failure)?.to_le_bytes());
            hasher.update(&bytes);
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            hasher.update(&[0]);
        }
        Err(e) => return Err(e.into()),
    }
    Ok(())
}

#[cfg_attr(
    not(test),
    expect(dead_code, reason = "internal revision awaits a caller")
)]
fn raw_revision(image: &Path) -> Result<blake3::Hash> {
    let paths = Sidecar::paths(image);
    let selected_xmp = catalog::xmp_path(image);
    let mut hasher = blake3::Hasher::new_derive_key("tessera raw recipe write revision v1");
    hash_path(&mut hasher, &paths.recipe)?;
    hash_file(&mut hasher, &paths.recipe)?;
    hash_path(&mut hasher, &selected_xmp)?;
    hash_file(&mut hasher, &selected_xmp)?;
    Ok(hasher.finalize())
}
