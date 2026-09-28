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
    sync::{
        Arc, Mutex, MutexGuard, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
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
        owner: Mutex::new(None),
        next_owner: AtomicU64::new(1),
        #[cfg(test)]
        contended_observer: Mutex::new(None),
    });
    table.insert(key, Arc::downgrade(&state));
    Ok(state)
}

pub(crate) struct GateState {
    key: PathBuf,
    epoch: Mutex<u64>,
    // Accessed only while `epoch` is held, preserving one lock order for
    // ordinary writers, admission, and lease release.
    owner: Mutex<Option<u64>>,
    next_owner: AtomicU64,
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

/// Exclusive authority for a visible Develop session. Clones are held by the
/// session and save worker so self-thread Drop cannot release admission early.
#[derive(Clone)]
pub(crate) struct DevelopLease(Arc<LeaseReservation>);

#[derive(Clone)]
pub(crate) struct DevelopAuthority {
    state: Arc<GateState>,
    id: u64,
}

impl DevelopLease {
    pub(crate) fn authority(&self) -> DevelopAuthority {
        DevelopAuthority {
            state: self.0.state.clone(),
            id: self.0.id,
        }
    }
}

struct LeaseReservation {
    state: Arc<GateState>,
    id: u64,
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

    fn check_image(&self, image: &Path) -> Result<()> {
        if destination_key(image)? != self.key {
            return Err(failure("image does not use this recipe destination"));
        }
        Ok(())
    }

    pub(crate) fn begin_write(&self) -> Result<WriteGuard<'_>> {
        let epoch = self.lock()?;
        if self.owner.lock().map_err(failure)?.is_some() {
            return Err(failure(
                "conflict: Develop destination already has an active editor",
            ));
        }
        if *epoch == u64::MAX {
            return Err(failure("recipe write epoch exhausted"));
        }
        Ok(WriteGuard { state: self, epoch })
    }

    /// Read an effective document and its raw sidecar under the same
    /// destination lock as participating writers, without advancing the
    /// write epoch merely because an editor opened.
    pub(crate) fn begin_read(&self) -> Result<MutexGuard<'_, u64>> {
        self.lock()
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

    /// Selection is orthogonal to Develop and remains writable during an edit.
    pub(crate) fn begin_selection_write(&self) -> Result<WriteGuard<'_>> {
        let epoch = self.lock()?;
        if *epoch == u64::MAX {
            return Err(failure("recipe write epoch exhausted"));
        }
        Ok(WriteGuard { state: self, epoch })
    }

    #[cfg(test)]
    pub(crate) fn exhaust_owner_ids_for_test(&self) {
        self.next_owner.store(u64::MAX, Ordering::Relaxed);
    }

    pub(crate) fn reserve_develop<T>(
        self: &Arc<Self>,
        image: &Path,
        snapshot: impl FnOnce() -> Result<T>,
    ) -> Result<(DevelopLease, T)> {
        let _epoch = self.lock()?;
        self.check_image(image)?;
        let mut owner = self.owner.lock().map_err(failure)?;
        if owner.is_some() {
            return Err(failure(
                "conflict: Develop destination already has an active editor",
            ));
        }
        let id = self
            .next_owner
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.checked_add(1)
            })
            .map_err(|_| failure("Develop lease identifier exhausted"))?;
        *owner = Some(id);
        match snapshot() {
            Ok(value) => Ok((
                DevelopLease(Arc::new(LeaseReservation {
                    state: self.clone(),
                    id,
                })),
                value,
            )),
            Err(error) => {
                *owner = None;
                Err(error)
            }
        }
    }

    pub(crate) fn begin_develop_write<'a>(
        self: &'a Arc<Self>,
        authority: &DevelopAuthority,
        image: &Path,
    ) -> Result<WriteGuard<'a>> {
        let epoch = self.lock()?;
        self.check_image(image)?;
        if !Arc::ptr_eq(&authority.state, self) {
            return Err(failure(
                "conflict: Develop lease belongs to a different destination",
            ));
        }
        if *self.owner.lock().map_err(failure)? != Some(authority.id) {
            return Err(failure("conflict: Develop lease is no longer active"));
        }
        if *epoch == u64::MAX {
            return Err(failure("recipe write epoch exhausted"));
        }
        Ok(WriteGuard { state: self, epoch })
    }
}

impl Drop for LeaseReservation {
    fn drop(&mut self) {
        // Do not panic during teardown. The gate is retained by this token,
        // and owner is always cleared only when the matching ID still owns it.
        if let Ok(_epoch) = self.state.epoch.lock() {
            if let Ok(mut owner) = self.state.owner.lock()
                && *owner == Some(self.id)
            {
                *owner = None;
            }
        }
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
