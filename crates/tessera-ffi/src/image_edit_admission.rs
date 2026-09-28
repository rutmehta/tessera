//! Stable, process-wide edit admission keyed only by catalog image identity.
//!
//! Lock order for participating callers is this image operation guard, then
//! the existing recipe destination guard. Never acquire in the reverse order.
//! No path or filesystem access occurs here, including when originals are offline.
//! This is in-process admission, not a cross-process lock or publication CAS.
use crate::{Result, failure};
use engine_api::id::ImageId;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

type GateTable = HashMap<ImageId, Weak<ImageEditGate>>;
static GATES: OnceLock<Mutex<GateTable>> = OnceLock::new();

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditSource {
    Original,
    SmartPreview,
    ExternalWriter,
}

/// Reuses the same gate across Engines and source routes. Weak entries are
/// pruned on every lookup so visited images do not accumulate retained gates.
pub(crate) fn gate_for(image_id: ImageId) -> Result<Arc<ImageEditGate>> {
    let mut gates = GATES
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .map_err(failure)?;
    gates.retain(|_, state| state.strong_count() != 0);
    if let Some(state) = gates.get(&image_id).and_then(Weak::upgrade) {
        return Ok(state);
    }
    let state = Arc::new(ImageEditGate {
        owner: Mutex::new(None),
    });
    gates.insert(image_id, Arc::downgrade(&state));
    Ok(state)
}

pub(crate) struct ImageEditGate {
    // The mutex serializes admission, inspection and all writer operations.
    // Expired reservations are logically absent, so final lease drop needs no
    // mutex (and is safe even on a worker currently holding an operation guard).
    owner: Mutex<Option<Weak<Reservation>>>,
}

/// Clone into every save worker. Admission ends only after the final lease
/// and any validated in-flight save guard have dropped.
#[derive(Clone)]
pub(crate) struct ImageEditLease(Arc<Reservation>);

/// Opaque capability that cannot prolong admission or authorize a new owner.
/// A Weak pointer pins the allocation identity after release, preventing ABA.
#[derive(Clone)]
pub(crate) struct ImageEditAuthority {
    reservation: Weak<Reservation>,
}

struct Reservation {
    gate: Arc<ImageEditGate>,
    source: EditSource,
}

/// Hold across the entire participating operation, including destination
/// baseline validation and publication. Do not recursively acquire this gate.
pub(crate) struct ImageOperationGuard<'a> {
    _owner: MutexGuard<'a, Option<Weak<Reservation>>>,
    // A validated save must finish before another editor can be admitted.
    _lease: Option<Arc<Reservation>>,
}

impl ImageEditLease {
    pub(crate) fn authority(&self) -> ImageEditAuthority {
        ImageEditAuthority {
            reservation: Arc::downgrade(&self.0),
        }
    }
}

impl ImageEditGate {
    /// Reserve before any destination gate/snapshot or source decoding. Drop
    /// the returned lease on an open failure; retain clones for save workers.
    pub(crate) fn reserve_develop(self: &Arc<Self>, source: EditSource) -> Result<ImageEditLease> {
        let mut owner = self.owner.lock().map_err(failure)?;
        if let Some(reservation) = owner.as_ref().and_then(Weak::upgrade) {
            return Err(failure(match reservation.source {
                EditSource::Original => {
                    "conflict: Develop destination already has an active editor"
                }
                EditSource::SmartPreview => {
                    "conflict: close the active Smart Preview editor before changing originals"
                }
                EditSource::ExternalWriter => "conflict: original sidecars have an active writer",
            }));
        }
        let reservation = Arc::new(Reservation {
            gate: self.clone(),
            source,
        });
        *owner = Some(Arc::downgrade(&reservation));
        Ok(ImageEditLease(reservation))
    }

    /// Direct recipe mutation is forbidden during either kind of editor.
    pub(crate) fn begin_write(&self) -> Result<ImageOperationGuard<'_>> {
        let owner = self.owner.lock().map_err(failure)?;
        if let Some(reservation) = owner.as_ref().and_then(Weak::upgrade) {
            return Err(failure(match reservation.source {
                EditSource::Original => {
                    "conflict: Develop destination already has an active editor"
                }
                EditSource::SmartPreview => {
                    "conflict: close the active Smart Preview editor before changing originals"
                }
                EditSource::ExternalWriter => "conflict: original sidecars have an active writer",
            }));
        }
        Ok(ImageOperationGuard {
            _owner: owner,
            _lease: None,
        })
    }

    /// Preserve existing online selection behavior. Smart Preview selection
    /// requires a journal-aware merge path and is intentionally rejected here.
    pub(crate) fn begin_selection_write(&self) -> Result<ImageOperationGuard<'_>> {
        let owner = self.owner.lock().map_err(failure)?;
        if owner
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|reservation| reservation.source != EditSource::Original)
        {
            return Err(failure(
                "conflict: close the active Smart Preview or original-sidecar writer before changing selection",
            ));
        }
        Ok(ImageOperationGuard {
            _owner: owner,
            _lease: None,
        })
    }

    /// Validates both image identity and current reservation while locking out
    /// other writes and admission for the entire caller operation.
    pub(crate) fn begin_develop_write(
        self: &Arc<Self>,
        authority: &ImageEditAuthority,
    ) -> Result<ImageOperationGuard<'_>> {
        let owner = self.owner.lock().map_err(failure)?;
        let reservation = authority
            .reservation
            .upgrade()
            .ok_or_else(|| failure("conflict: image editor authority is no longer active"))?;
        if !Arc::ptr_eq(&reservation.gate, self)
            || !owner
                .as_ref()
                .is_some_and(|current| Weak::ptr_eq(current, &authority.reservation))
        {
            return Err(failure(
                "conflict: image editor authority belongs to another reservation",
            ));
        }
        Ok(ImageOperationGuard {
            _owner: owner,
            _lease: Some(reservation),
        })
    }

    /// Consistent read-only inspection never reserves an editor or rejects an
    /// existing one. Callers still need destination protection for sidecars.
    pub(crate) fn begin_read(&self) -> Result<ImageOperationGuard<'_>> {
        Ok(ImageOperationGuard {
            _owner: self.owner.lock().map_err(failure)?,
            _lease: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn image() -> ImageId {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        ImageId(u128::MAX - u128::from(NEXT.fetch_add(1, Ordering::Relaxed)))
    }

    #[test]
    fn original_and_proxy_reservations_are_mutually_exclusive() {
        for source in [EditSource::Original, EditSource::SmartPreview] {
            let id = image();
            let gate = gate_for(id).unwrap();
            let lease = gate.reserve_develop(source).unwrap();
            let second_lookup = gate_for(id).unwrap();
            assert!(second_lookup.reserve_develop(EditSource::Original).is_err());
            assert!(
                second_lookup
                    .reserve_develop(EditSource::SmartPreview)
                    .is_err()
            );
            assert!(second_lookup.begin_write().is_err());
            // Read-only inspection must not claim another editor.
            drop(second_lookup.begin_read().unwrap());
            drop(lease);
            drop(second_lookup.begin_write().unwrap());
            assert!(second_lookup.reserve_develop(EditSource::Original).is_ok());
        }
    }

    #[test]
    fn selection_is_allowed_only_without_editor_or_with_original_editor() {
        let gate = gate_for(image()).unwrap();
        drop(gate.begin_selection_write().unwrap());
        let original = gate.reserve_develop(EditSource::Original).unwrap();
        drop(gate.begin_selection_write().unwrap());
        drop(original);
        let proxy = gate.reserve_develop(EditSource::SmartPreview).unwrap();
        assert!(gate.begin_selection_write().is_err());
        drop(proxy);
        assert!(gate.begin_selection_write().is_ok());
    }

    #[test]
    fn worker_clone_and_in_flight_guard_drain_before_readmission() {
        let gate = gate_for(image()).unwrap();
        let session = gate.reserve_develop(EditSource::Original).unwrap();
        let worker = session.clone();
        let authority = session.authority();
        drop(session);
        assert!(gate.begin_write().is_err());
        let operation = gate.begin_develop_write(&authority).unwrap();
        drop(worker);
        // Operation retains the reservation, even when the last worker drops.
        assert!(authority.reservation.upgrade().is_some());
        drop(operation);
        assert!(gate.begin_develop_write(&authority).is_err());
        assert!(gate.reserve_develop(EditSource::SmartPreview).is_ok());
    }

    #[test]
    fn stale_and_wrong_image_authorities_never_authorize_a_save() {
        let id = image();
        let gate = gate_for(id).unwrap();
        let other_gate = gate_for(image()).unwrap();
        let lease = gate.reserve_develop(EditSource::Original).unwrap();
        let authority = lease.authority();
        assert!(other_gate.begin_develop_write(&authority).is_err());
        drop(lease);
        let replacement = gate.reserve_develop(EditSource::SmartPreview).unwrap();
        assert!(gate.begin_develop_write(&authority).is_err());
        drop(gate.begin_develop_write(&replacement.authority()).unwrap());
        drop(replacement);
        drop(gate);
        let recreated = gate_for(id).unwrap();
        let current = recreated.reserve_develop(EditSource::Original).unwrap();
        assert!(recreated.begin_develop_write(&authority).is_err());
        assert!(recreated.begin_develop_write(&current.authority()).is_ok());
    }

    #[test]
    fn failed_open_and_failed_operation_release_admission() {
        let gate = gate_for(image()).unwrap();
        let failed_open = || -> Result<()> {
            let _lease = gate.reserve_develop(EditSource::SmartPreview)?;
            Err(failure("snapshot unavailable"))
        };
        assert!(failed_open().is_err());
        let failed_write = || -> Result<()> {
            let _operation = gate.begin_write()?;
            Err(failure("write failed"))
        };
        assert!(failed_write().is_err());
        assert!(gate.reserve_develop(EditSource::Original).is_ok());
    }

    #[test]
    fn operations_serialize_admission_but_not_independent_images() {
        let gate = gate_for(image()).unwrap();
        let independent = gate_for(image()).unwrap();
        let operation = gate.begin_write().unwrap();
        assert!(matches!(
            gate.owner.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        let independent_lease = independent
            .reserve_develop(EditSource::SmartPreview)
            .unwrap();
        drop(
            independent
                .begin_develop_write(&independent_lease.authority())
                .unwrap(),
        );
        drop(operation);
        let lease = gate.reserve_develop(EditSource::Original).unwrap();
        let save = gate.begin_develop_write(&lease.authority()).unwrap();
        assert!(matches!(
            gate.owner.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        drop(save);
        let selection = gate.begin_selection_write().unwrap();
        assert!(matches!(
            gate.owner.try_lock(),
            Err(std::sync::TryLockError::WouldBlock)
        ));
        drop(selection);
    }
}
