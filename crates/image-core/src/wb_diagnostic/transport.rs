//! Explicit-token WB diagnostic transport (rev5 Stage A). No renderer hooks.
//! Attribution travels only as a by-value `Copy` [`Token`]: no thread-local
//! storage and no thread identity. All storage is one const-initialised arena
//! whose epoch protocol lives under a single leaf mutex.
#![allow(dead_code)]
use super::{Bucket, CAPACITY, Context, Error, Identity, Outcome, Record, Snapshot};
use engine_api::{
    id::{Digest, ImageId},
    stage::{MemoKey, ParamHash, StageId},
    tile::TileCoord,
};
use std::{
    mem::{ManuallyDrop, needs_drop},
    sync::{
        Mutex, MutexGuard, PoisonError, TryLockError,
        atomic::{AtomicBool, Ordering},
    },
};

pub(crate) const SLOTS: usize = 8;
pub const ACTIVE: usize = 2;
const ARENA_BYTES: usize = 296 * 1024;
const METADATA_BYTES: usize = 8 * 1024;
const FIXED_BYTES: usize = 328 * 1024;

/// The process-wide diagnostic arena used by the live call sites (rev7 3.1a).
pub static ARENA: Arena = Arena::EMPTY;

/// Attribution for one render transaction. Copied by value into render state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Token {
    epoch: u64,
    phase: u64,
    transaction: u64,
    slot: u8,
}
/// Terminal route reported by a finishing lease.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Delivered,
    Declined,
}
/// Route as drained. `Aborted` (dropped unfinished) and `Unobserved` (finished
/// without Begin) are derived by the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteState {
    Delivered,
    Declined,
    Aborted,
    Unobserved,
}
/// Phase of a reservation (rev7 S3). Stored as `RequestContext.phase_kind`
/// (1 calibration, 2 develop, 3 test) plus `generation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhaseKind {
    Calibration { iteration: u8 },
    Develop { generation: u64 },
    Test,
}
/// Copied at reservation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RequestContext {
    pub recipe_fingerprint: [u8; 32],
    pub settings_fingerprint: [u8; 32],
    pub process_identity: u64,
    pub output_tag: u8,
    pub headroom_bits: u32,
    pub requested_level: u8,
    pub phase_kind: u8,
    pub expected_operator: u64,
    pub generation: u64,
}
/// Written only at binding (`begin`). `wb_bits` are the raw bits of the
/// resolved WB matrix (row-major); `wb_digest` is their S1 digest, computed by
/// the caller outside the arena lock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeginContext {
    pub wb_digest: [u8; 32],
    pub wb_bits: [u64; 9],
    pub output_tag: u8,
    pub headroom_bits: u32,
    pub render_level: u8,
    pub operator: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotMeta {
    pub token: Token,
    pub request: RequestContext,
    pub begin: Option<BeginContext>,
    pub route: RouteState,
    pub loss_at_finish: u64,
}
pub struct DrainView<'a> {
    pub snapshot: &'a Snapshot,
    pub meta: &'a SlotMeta,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Idle,
    Open,
    Closing,
    Quiescent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Counts {
    pub reserved: usize,
    pub active: usize,
    pub completed: usize,
    pub scratch: bool,
    pub loss: u64,
    pub disabled: u64,
    /// Stage A review NB-1: set once capture has been disabled (poison or
    /// ordinal exhaustion); survives `open`. Any epoch observing it is
    /// inconclusive.
    pub disabled_sticky: bool,
    /// `live::begin` refused a nonfinite WB matrix for a live token.
    pub attribution_rejected: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SlotState {
    Free,
    Reserved,
    Active,
    Completed,
}
struct Slot {
    state: SlotState,
    meta: SlotMeta,
    payload: Snapshot,
}
struct Storage {
    slots: [Slot; SLOTS],
    counts: Counts,
    state: State,
    epoch: u64,
    next_epoch: u64,
    next_transaction: u64,
    next_operator: u64,
    armed_phase: Option<u64>,
    disabled: bool,
}
/// Drained copy. Owned by at most one `Drain` (`Counts.scratch`), so its
/// mutex is never contended and is never taken while blocking on `storage`.
struct Scratch {
    snapshot: Snapshot,
    meta: SlotMeta,
}
pub struct Arena {
    storage: Mutex<Storage>,
    scratch: Mutex<Scratch>,
    armed: AtomicBool,
}
/// Harness handle for one epoch. Dropping it closes that epoch.
pub struct Epoch<'a> {
    arena: &'a Arena,
    epoch: u64,
}
/// Reserved, not yet bound. Drop unbound records Abort.
pub struct Reservation {
    arena: &'static Arena,
    token: Token,
}
/// Bound transaction. Drop unfinished records Abort.
pub struct Lease {
    arena: &'static Arena,
    token: Token,
}
/// Exclusive owner of the scratch copy.
pub struct Drain<'a> {
    arena: &'a Arena,
}

const EMPTY_CONTEXT: Context = Context {
    recipe_fingerprint: [0; 32],
    resolved_wb: [0; 32],
    output_tag: 0,
    headroom_bits: 0,
    render_level: 0,
};
const EMPTY_TOKEN: Token = Token {
    epoch: 0,
    phase: 0,
    transaction: 0,
    slot: 0,
};
/// Placeholder key carried by Begin/End/Abort records.
const NO_KEY: MemoKey = MemoKey {
    image_id: ImageId(0),
    stage: StageId::Decode,
    params_hash: ParamHash(Digest([0; 32])),
    tile: TileCoord::new(0, 0, 0),
};
impl RequestContext {
    pub const EMPTY: RequestContext = RequestContext {
        recipe_fingerprint: [0; 32],
        settings_fingerprint: [0; 32],
        process_identity: 0,
        output_tag: 0,
        headroom_bits: 0,
        requested_level: 0,
        phase_kind: 0,
        expected_operator: 0,
        generation: 0,
    };
    /// FFI-facing constructor (rev7 S3); every parameter is a primitive, a
    /// byte array or [`PhaseKind`].
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        recipe_fingerprint: [u8; 32],
        settings_fingerprint: [u8; 32],
        process_identity: u64,
        output_tag: u8,
        headroom_bits: u32,
        requested_level: u8,
        phase: PhaseKind,
        expected_operator: u64,
    ) -> RequestContext {
        let (phase_kind, generation) = match phase {
            PhaseKind::Calibration { iteration } => (1, iteration as u64),
            PhaseKind::Develop { generation } => (2, generation),
            PhaseKind::Test => (3, 0),
        };
        RequestContext {
            recipe_fingerprint,
            settings_fingerprint,
            process_identity,
            output_tag,
            headroom_bits,
            requested_level,
            phase_kind,
            expected_operator,
            generation,
        }
    }
}
impl BeginContext {
    /// Constructor, so callers stay source-compatible when fields are added.
    /// Arity is kept from Stage A (review NB-3); `wb_bits` default to zero and
    /// are set with [`BeginContext::with_wb_bits`].
    pub const fn new(
        wb_digest: [u8; 32],
        output_tag: u8,
        headroom_bits: u32,
        render_level: u8,
        operator: u64,
    ) -> BeginContext {
        BeginContext {
            wb_digest,
            wb_bits: [0; 9],
            output_tag,
            headroom_bits,
            render_level,
            operator,
        }
    }
    pub const fn with_wb_bits(mut self, wb_bits: [u64; 9]) -> BeginContext {
        self.wb_bits = wb_bits;
        self
    }
}
impl SlotMeta {
    const EMPTY: SlotMeta = SlotMeta {
        token: EMPTY_TOKEN,
        request: RequestContext::EMPTY,
        begin: None,
        route: RouteState::Aborted,
        loss_at_finish: 0,
    };
}
impl Slot {
    const EMPTY: Slot = Slot {
        state: SlotState::Free,
        meta: SlotMeta::EMPTY,
        payload: Snapshot {
            context: EMPTY_CONTEXT,
            records: [None; CAPACITY],
            len: 0,
            overflow: 0,
        },
    };
}

/// Poison recovery (rev5 N11): the guard is recovered and capture disabled.
/// Terminal paths still update scalar state on the recovered guard.
fn lock(m: &Mutex<Storage>) -> MutexGuard<'_, Storage> {
    m.lock().unwrap_or_else(|poisoned| {
        let mut guard = poisoned.into_inner();
        guard.disabled = true;
        guard
    })
}
fn bump(counter: &mut u64) {
    *counter = counter.saturating_add(1);
}
fn stamp(meta: &SlotMeta, key: MemoKey, bucket: Bucket, outcome: Outcome) -> Record {
    Record {
        identity: Identity {
            operator: meta.request.expected_operator,
            phase: meta.token.phase,
            transaction: meta.token.transaction,
            generation: meta.request.generation,
        },
        key,
        bucket,
        outcome,
    }
}
/// Bounded append. The last record is reserved for the terminal record, so
/// End/Abort always fits; anything else past the bound counts as overflow.
fn append(slot: &mut Slot, record: Record, terminal: bool) {
    let limit = if terminal { CAPACITY } else { CAPACITY - 1 };
    let payload = &mut slot.payload;
    if payload.len >= limit {
        payload.overflow = payload.overflow.saturating_add(1);
        return;
    }
    if let Some(dst) = payload.records.get_mut(payload.len) {
        *dst = Some(record);
        payload.len += 1;
    }
}
impl Storage {
    /// Closing becomes Quiescent once no reservation or lease is outstanding.
    fn settle(&mut self) {
        if self.state == State::Closing && self.counts.reserved == 0 && self.counts.active == 0 {
            self.state = State::Quiescent;
        }
    }
    /// Applies `f` to the slot a live, bound token names. Refused while
    /// disabled; a stale or unknown token is counted as loss.
    fn with_active(&mut self, t: Token, f: impl FnOnce(&mut Slot) -> bool) {
        if self.disabled {
            bump(&mut self.counts.disabled);
            return;
        }
        let epoch = self.epoch;
        let accepted = match self.slots.get_mut(usize::from(t.slot)) {
            Some(slot)
                if slot.state == SlotState::Active && slot.meta.token == t && t.epoch == epoch =>
            {
                f(slot)
            }
            _ => false,
        };
        if !accepted {
            bump(&mut self.counts.loss);
        }
    }
    /// Terminal transition for a reservation (`from == Reserved`) or lease
    /// (`from == Active`). Never panics; runs on a recovered guard too.
    fn terminate(&mut self, t: Token, from: SlotState, route: Option<Route>) {
        let epoch = self.epoch;
        let disabled = self.disabled;
        let loss = self.counts.loss;
        let Some(slot) = self.slots.get_mut(usize::from(t.slot)) else {
            bump(&mut self.counts.loss);
            return;
        };
        if slot.state != from || slot.meta.token != t || t.epoch != epoch {
            bump(&mut self.counts.loss);
            return;
        }
        let outcome = if route.is_some() {
            Outcome::End
        } else {
            Outcome::Abort
        };
        if disabled {
            bump(&mut self.counts.disabled);
        } else {
            let record = stamp(&slot.meta, NO_KEY, Bucket::Detail, outcome);
            append(slot, record, true);
        }
        slot.meta.route = match route {
            None => RouteState::Aborted,
            Some(_) if slot.meta.begin.is_none() => RouteState::Unobserved,
            Some(Route::Delivered) => RouteState::Delivered,
            Some(Route::Declined) => RouteState::Declined,
        };
        slot.meta.loss_at_finish = loss;
        slot.state = SlotState::Completed;
        if from == SlotState::Reserved {
            self.counts.reserved = self.counts.reserved.saturating_sub(1);
        } else {
            self.counts.active = self.counts.active.saturating_sub(1);
        }
        self.counts.completed = self.counts.completed.saturating_add(1);
        self.settle();
    }
}
impl Arena {
    /// Const initialiser for a process `static` (rev5 section 3.1 spells this
    /// `const EMPTY`); only ever used as `static A: Arena = Arena::EMPTY;`.
    #[allow(clippy::declare_interior_mutable_const)]
    pub const EMPTY: Arena = Arena {
        storage: Mutex::new(Storage {
            slots: [const { Slot::EMPTY }; SLOTS],
            counts: Counts {
                reserved: 0,
                active: 0,
                completed: 0,
                scratch: false,
                loss: 0,
                disabled: 0,
                disabled_sticky: false,
                attribution_rejected: 0,
            },
            state: State::Idle,
            epoch: 0,
            next_epoch: 1,
            next_transaction: 1,
            next_operator: 1,
            armed_phase: None,
            disabled: false,
        }),
        scratch: Mutex::new(Scratch {
            snapshot: Slot::EMPTY.payload,
            meta: SlotMeta::EMPTY,
        }),
        armed: AtomicBool::new(false),
    };
    /// Idle -> Open. Refused unless the previous epoch was acknowledged, and
    /// on epoch-ordinal exhaustion (never wraps).
    pub fn open(&self) -> Result<Epoch<'_>, Error> {
        let mut s = lock(&self.storage);
        if s.state != State::Idle {
            return Err(Error::Incomplete);
        }
        if s.next_epoch == u64::MAX {
            return Err(Error::Overflow);
        }
        let epoch = s.next_epoch;
        s.next_epoch += 1;
        s.epoch = epoch;
        s.state = State::Open;
        s.armed_phase = None;
        s.counts = Counts {
            disabled_sticky: s.disabled,
            ..Counts::default()
        };
        self.armed.store(false, Ordering::Relaxed);
        Ok(Epoch { arena: self, epoch })
    }
    pub fn counts(&self) -> Result<Counts, Error> {
        let s = lock(&self.storage);
        Ok(Counts {
            disabled_sticky: s.disabled,
            ..s.counts
        })
    }
    pub fn state(&self) -> Result<State, Error> {
        Ok(lock(&self.storage).state)
    }
    /// Quiescent -> Idle for `epoch`, discarding undrained slots. Requires the
    /// scratch to be free.
    pub fn acknowledge(&self, epoch: u64) -> Result<(), Error> {
        let mut s = lock(&self.storage);
        if s.epoch != epoch || s.state != State::Quiescent || s.counts.scratch {
            return Err(Error::Incomplete);
        }
        for slot in &mut s.slots {
            slot.state = SlotState::Free;
        }
        s.counts.completed = 0;
        s.state = State::Idle;
        Ok(())
    }
    /// Product call site: infallible; `None` unless a phase is armed. Refusals
    /// while armed (capacity, closing, disabled, ordinal exhaustion) are
    /// counted, never returned.
    pub fn reserve_armed(&'static self, ctx: RequestContext) -> Option<Reservation> {
        if !self.armed.load(Ordering::Relaxed) {
            return None;
        }
        let mut guard = lock(&self.storage);
        let s = &mut *guard;
        let phase = s.armed_phase?;
        if s.disabled {
            bump(&mut s.counts.disabled);
            return None;
        }
        if s.state != State::Open || s.counts.reserved + s.counts.active >= ACTIVE {
            bump(&mut s.counts.loss);
            return None;
        }
        let Some(index) = s.slots.iter().position(|x| x.state == SlotState::Free) else {
            bump(&mut s.counts.loss);
            return None;
        };
        if s.next_transaction == u64::MAX {
            s.disabled = true;
            bump(&mut s.counts.disabled);
            return None;
        }
        let transaction = s.next_transaction;
        s.next_transaction += 1;
        let token = Token {
            epoch: s.epoch,
            phase,
            transaction,
            slot: index as u8,
        };
        let slot = s.slots.get_mut(index)?;
        slot.state = SlotState::Reserved;
        slot.meta = SlotMeta {
            token,
            request: ctx,
            ..SlotMeta::EMPTY
        };
        slot.payload.context = EMPTY_CONTEXT;
        slot.payload.len = 0;
        slot.payload.overflow = 0;
        s.counts.reserved += 1;
        Some(Reservation { arena: self, token })
    }
    /// Runs `lookup` exactly once with no lock held and returns its result.
    pub fn observe<T, E>(
        &self,
        t: Token,
        rec: Record,
        lookup: impl FnOnce() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        let result = lookup();
        let outcome = match &result {
            Ok(Some(_)) => Outcome::Some,
            Ok(None) => Outcome::None,
            Err(_) => Outcome::Error,
        };
        lock(&self.storage).with_active(t, |slot| {
            let record = stamp(&slot.meta, rec.key, rec.bucket, outcome);
            append(slot, record, false);
            true
        });
        result
    }
    pub fn request(&self, t: Token, key: MemoKey, bucket: Bucket) {
        lock(&self.storage).with_active(t, |slot| {
            let record = stamp(&slot.meta, key, bucket, Outcome::Request);
            append(slot, record, false);
            true
        });
    }
    /// Records Begin once per transaction; a repeated Begin counts as loss.
    pub fn begin(&self, t: Token, ctx: BeginContext) {
        lock(&self.storage).with_active(t, |slot| {
            if slot.meta.begin.is_some() {
                return false;
            }
            slot.meta.begin = Some(ctx);
            let record = stamp(&slot.meta, NO_KEY, Bucket::Detail, Outcome::Begin);
            append(slot, record, false);
            true
        });
    }
    /// `live::begin` saw a nonfinite WB matrix: count it for a live token and
    /// record nothing else, so the transaction stays without Begin.
    pub fn reject_attribution(&self, t: Token) {
        let mut guard = lock(&self.storage);
        let s = &mut *guard;
        let mut accepted = false;
        s.with_active(t, |_| {
            accepted = true;
            true
        });
        if accepted {
            bump(&mut s.counts.attribution_rejected);
        }
    }
    /// 0 = exhausted/unknown.
    pub fn next_operator(&self) -> u64 {
        let mut s = lock(&self.storage);
        if s.next_operator == u64::MAX {
            return 0;
        }
        let operator = s.next_operator;
        s.next_operator += 1;
        operator
    }
    #[cfg(test)]
    fn set_next_epoch_for_test(&self, n: u64) -> Result<(), Error> {
        lock(&self.storage).next_epoch = n;
        Ok(())
    }
    #[cfg(test)]
    fn set_next_transaction_for_test(&self, n: u64) -> Result<(), Error> {
        lock(&self.storage).next_transaction = n;
        Ok(())
    }
    #[cfg(test)]
    fn set_next_operator_for_test(&self, n: u64) -> Result<(), Error> {
        lock(&self.storage).next_operator = n;
        Ok(())
    }
    /// Test accessor (B12): runs `f` while holding the storage lock.
    #[cfg(test)]
    pub(crate) fn hold_lock_for_test<R>(&self, f: impl FnOnce() -> R) -> R {
        let _guard = lock(&self.storage);
        f()
    }
    /// Test accessor (B5): the last record appended to a live token's slot.
    #[cfg(test)]
    pub(crate) fn last_record_for_test(&self, t: Token) -> Option<Record> {
        let s = lock(&self.storage);
        let slot = s.slots.get(usize::from(t.slot))?;
        if slot.state != SlotState::Active || slot.meta.token != t {
            return None;
        }
        let len = slot.payload.len;
        slot.payload
            .records
            .get(len.checked_sub(1)?)
            .copied()
            .flatten()
    }
}
impl<'a> Epoch<'a> {
    /// Epoch number, for `Arena::acknowledge` outside this module (NB-2).
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
    fn current(&self, s: &Storage) -> Result<(), Error> {
        if s.epoch == self.epoch && s.state != State::Idle {
            Ok(())
        } else {
            Err(Error::Incomplete)
        }
    }
    pub fn arm(&self, phase: u64) -> Result<(), Error> {
        let mut s = lock(&self.arena.storage);
        self.current(&s)?;
        if s.state != State::Open {
            return Err(Error::Incomplete);
        }
        s.armed_phase = Some(phase);
        self.arena.armed.store(true, Ordering::Relaxed);
        Ok(())
    }
    pub fn disarm(&self) -> Result<(), Error> {
        let mut s = lock(&self.arena.storage);
        self.current(&s)?;
        s.armed_phase = None;
        self.arena.armed.store(false, Ordering::Relaxed);
        Ok(())
    }
    /// Open -> Closing (-> Quiescent once nothing is outstanding). Idempotent.
    pub fn close(&self) -> Result<(), Error> {
        let mut s = lock(&self.arena.storage);
        self.current(&s)?;
        if s.state == State::Open {
            s.state = State::Closing;
            s.armed_phase = None;
            self.arena.armed.store(false, Ordering::Relaxed);
            s.settle();
        }
        Ok(())
    }
    /// Copies the lowest completed slot into the scratch in place and frees
    /// the slot. Refused while the scratch is owned by another `Drain`.
    pub fn drain(&self) -> Result<Drain<'a>, Error> {
        let mut guard = lock(&self.arena.storage);
        let s = &mut *guard;
        self.current(s)?;
        if s.counts.scratch {
            return Err(Error::Incomplete);
        }
        let Some(slot) = s.slots.iter_mut().find(|x| x.state == SlotState::Completed) else {
            return Err(Error::Incomplete);
        };
        // Never blocks: with `scratch == false` no `Drain` (hence no inspect)
        // can hold this mutex, so the storage mutex stays a leaf.
        let mut scratch = match self.arena.scratch.try_lock() {
            Ok(g) => g,
            Err(TryLockError::Poisoned(p)) => p.into_inner(),
            Err(TryLockError::WouldBlock) => return Err(Error::Incomplete),
        };
        let len = slot.payload.len.min(CAPACITY);
        let dst = &mut scratch.snapshot;
        dst.records[..len].copy_from_slice(&slot.payload.records[..len]);
        dst.records[len..].fill(None);
        dst.len = len;
        dst.overflow = slot.payload.overflow;
        dst.context = match slot.meta.begin {
            Some(b) => Context {
                recipe_fingerprint: slot.meta.request.recipe_fingerprint,
                resolved_wb: b.wb_digest,
                output_tag: b.output_tag,
                headroom_bits: b.headroom_bits,
                render_level: b.render_level,
            },
            None => EMPTY_CONTEXT,
        };
        scratch.meta = slot.meta;
        slot.state = SlotState::Free;
        s.counts.completed = s.counts.completed.saturating_sub(1);
        s.counts.scratch = true;
        Ok(Drain { arena: self.arena })
    }
}
impl Reservation {
    pub fn token(&self) -> Token {
        self.token
    }
    /// Product call site: infallible. A stale reservation still yields a
    /// lease whose records are counted as loss.
    pub fn bind(self) -> Lease {
        let this = ManuallyDrop::new(self);
        let (arena, token) = (this.arena, this.token);
        let mut guard = lock(&arena.storage);
        let s = &mut *guard;
        let epoch = s.epoch;
        match s.slots.get_mut(usize::from(token.slot)) {
            Some(slot)
                if slot.state == SlotState::Reserved
                    && slot.meta.token == token
                    && token.epoch == epoch =>
            {
                slot.state = SlotState::Active;
                s.counts.reserved = s.counts.reserved.saturating_sub(1);
                s.counts.active += 1;
            }
            _ => bump(&mut s.counts.loss),
        }
        Lease { arena, token }
    }
}
impl Lease {
    pub fn token(&self) -> Token {
        self.token
    }
    /// Product call site: infallible.
    pub fn finish(self, route: Route) {
        let this = ManuallyDrop::new(self);
        lock(&this.arena.storage).terminate(this.token, SlotState::Active, Some(route));
    }
}
impl Drain<'_> {
    /// Borrowed scratch view; the arena (storage) mutex is not held.
    pub fn inspect<R>(&self, f: impl FnOnce(&DrainView<'_>) -> R) -> R {
        let scratch = self
            .arena
            .scratch
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        f(&DrainView {
            snapshot: &scratch.snapshot,
            meta: &scratch.meta,
        })
    }
}
impl Drop for Epoch<'_> {
    fn drop(&mut self) {
        let mut s = lock(&self.arena.storage);
        if s.epoch == self.epoch && s.state == State::Open {
            s.state = State::Closing;
            s.armed_phase = None;
            self.arena.armed.store(false, Ordering::Relaxed);
            s.settle();
        }
    }
}
impl Drop for Reservation {
    fn drop(&mut self) {
        lock(&self.arena.storage).terminate(self.token, SlotState::Reserved, None);
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        lock(&self.arena.storage).terminate(self.token, SlotState::Active, None);
    }
}
impl Drop for Drain<'_> {
    fn drop(&mut self) {
        lock(&self.arena.storage).counts.scratch = false;
    }
}

const _: () = assert!(size_of::<Arena>() <= ARENA_BYTES);
const _: () = assert!(size_of::<Arena>() - (SLOTS + 1) * size_of::<Snapshot>() <= METADATA_BYTES);
const _: () = assert!(size_of::<Arena>() + size_of::<Snapshot>() <= FIXED_BYTES);
const _: () = assert!(size_of::<Option<Token>>() <= 40);
const _: () = assert!(size_of::<Lease>() <= 48 && size_of::<Reservation>() <= 48);
const _: () = assert!(size_of::<Record>() <= 128);
const _: () = assert!(!needs_drop::<Storage>() && !needs_drop::<Scratch>());

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wb_diagnostic::{Identity, Outcome};
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::mpsc,
        time::Duration,
    };
    const WAIT: Duration = Duration::from_secs(2);
    fn record(n: u64) -> Record {
        Record {
            identity: Identity {
                operator: 1,
                phase: 1,
                transaction: 1,
                generation: 1,
            },
            key: MemoKey {
                image_id: ImageId(n as u128),
                stage: StageId::Detail,
                params_hash: ParamHash::of(StageId::Detail, &0u32),
                tile: TileCoord::new(0, 0, 0),
            },
            bucket: Bucket::Detail,
            outcome: Outcome::None,
        }
    }
    fn ctx(generation: u64) -> RequestContext {
        RequestContext {
            expected_operator: 1,
            generation,
            ..RequestContext::EMPTY
        }
    }
    const BEGIN: BeginContext = BeginContext::new([1; 32], 1, 0, 0, 1);
    fn lease(a: &'static Arena, generation: u64) -> Lease {
        a.reserve_armed(ctx(generation))
            .expect("armed reservation")
            .bind()
    }
    fn emit(a: &Arena, t: Token, n: u64) {
        assert_eq!(
            a.observe(t, record(n), || Ok::<_, ()>(Some(n))),
            Ok(Some(n))
        );
    }
    fn lookups(v: &DrainView<'_>) -> Vec<u128> {
        v.snapshot.records[..v.snapshot.len]
            .iter()
            .flatten()
            .filter(|r| matches!(r.outcome, Outcome::Some | Outcome::None | Outcome::Error))
            .map(|r| r.key.image_id.0)
            .collect()
    }
    fn images(d: &Drain<'_>) -> Vec<u128> {
        d.inspect(lookups)
    }
    fn last_outcome(d: &Drain<'_>) -> Outcome {
        d.inspect(|v| v.snapshot.records[v.snapshot.len - 1].unwrap().outcome)
    }
    #[test]
    fn fixed_storage_and_scalar_handle_layout_do_not_require_guard_implementation() {
        assert!(size_of::<Snapshot>() <= 32 * 1024);
        assert!(size_of::<Arena>() <= 296 * 1024);
        assert!(size_of::<Arena>() - 9 * size_of::<Snapshot>() <= 8 * 1024);
        assert!(size_of::<Arena>() + size_of::<Snapshot>() <= 328 * 1024);
        assert!(size_of::<Option<Token>>() <= 40);
        assert!(size_of::<Lease>() <= 48 && size_of::<Reservation>() <= 48);
        assert!(!needs_drop::<Storage>());
        assert_eq!((SLOTS, ACTIVE), (8, 2));
    }
    #[test]
    fn const_static_arena_compiles() {
        static A: Arena = Arena::EMPTY;
        assert_eq!(size_of_val(&A), size_of::<Arena>());
    }
    #[test]
    fn successful_terminal_and_drain_are_single_owner() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        let t = w.token();
        emit(&A, t, 1);
        w.finish(Route::Delivered);
        let d = e.drain().unwrap();
        assert_eq!(images(&d), [1]);
        assert!(e.drain().is_err());
        assert!(A.counts().unwrap().scratch);
        assert_eq!(last_outcome(&d), Outcome::End);
        d.inspect(|v| {
            assert_eq!(v.meta.token, t);
            assert_eq!(v.meta.route, RouteState::Unobserved);
        });
        drop(d);
        assert!(e.drain().is_err());
        e.close().unwrap();
        A.acknowledge(e.epoch).unwrap();
        assert!(A.open().is_ok());
    }
    #[test]
    fn error_unwind_and_unstarted_job_drop_abort_and_release() {
        static A: Arena = Arena::EMPTY;
        for mode in 0..3 {
            let e = A.open().unwrap();
            e.arm(1).unwrap();
            let r = A.reserve_armed(ctx(1)).unwrap();
            if mode == 0 {
                drop(r);
            } else {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    let w = r.bind();
                    assert_eq!(
                        A.observe(w.token(), record(1), || Err::<Option<u8>, _>(7)),
                        Err(7)
                    );
                    if mode == 2 {
                        panic!("synthetic unwind");
                    }
                }));
            }
            let c = A.counts().unwrap();
            assert_eq!(c.active + c.reserved, 0);
            {
                let d = e.drain().unwrap();
                assert_eq!(last_outcome(&d), Outcome::Abort);
                d.inspect(|v| assert_eq!(v.meta.route, RouteState::Aborted));
                assert_eq!(images(&d), if mode == 0 { vec![] } else { vec![1] });
            }
            e.close().unwrap();
            A.acknowledge(e.epoch).unwrap();
        }
    }
    #[test]
    fn overflow_preserves_original_lookup_exactly_once() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        for n in 0..300 {
            emit(&A, w.token(), n);
        }
        let calls = Cell::new(0);
        assert_eq!(
            A.observe(w.token(), record(999), || {
                calls.set(calls.get() + 1);
                Err::<Option<u8>, _>(17)
            }),
            Err(17)
        );
        assert_eq!(calls.get(), 1);
        w.finish(Route::Delivered);
        assert!(e.drain().unwrap().inspect(|v| v.snapshot.overflow > 0));
    }
    #[test]
    fn two_workers_isolate_records_and_third_reservation_marks_loss() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        // Reserved on main; bound and used on other threads.
        let r1 = A.reserve_armed(ctx(1)).unwrap();
        let r2 = A.reserve_armed(ctx(2)).unwrap();
        assert!(A.reserve_armed(ctx(3)).is_none());
        assert_eq!(A.counts().unwrap().loss, 1);
        let (ready_tx, ready_rx) = mpsc::channel();
        let (token_tx, token_rx) = mpsc::channel::<Token>();
        let (emitted_tx, emitted_rx) = mpsc::channel();
        let (go1, wait1) = mpsc::channel();
        let (go2, wait2) = mpsc::channel();
        std::thread::scope(|s| {
            let ready1 = ready_tx.clone();
            // Thread A binds r1; thread B observes with A's token.
            s.spawn(move || {
                let w = r1.bind();
                token_tx.send(w.token()).unwrap();
                ready1.send(()).unwrap();
                emitted_rx.recv_timeout(WAIT).unwrap();
                w.finish(Route::Delivered);
            });
            s.spawn(move || {
                let t = token_rx.recv_timeout(WAIT).unwrap();
                wait1.recv_timeout(WAIT).unwrap();
                emit(&A, t, 11);
                emitted_tx.send(()).unwrap();
            });
            s.spawn(move || {
                let w = r2.bind();
                ready_tx.send(()).unwrap();
                wait2.recv_timeout(WAIT).unwrap();
                emit(&A, w.token(), 22);
                w.finish(Route::Delivered);
            });
            ready_rx.recv_timeout(WAIT).unwrap();
            ready_rx.recv_timeout(WAIT).unwrap();
            assert_eq!(A.counts().unwrap().active, 2);
            go1.send(()).unwrap();
            go2.send(()).unwrap();
        });
        let x = images(&e.drain().unwrap());
        let y = images(&e.drain().unwrap());
        assert!(x == [11] && y == [22] || x == [22] && y == [11]);
        assert_eq!(A.counts().unwrap().loss, 1);
    }
    #[test]
    fn sequential_threads_and_rejections_leave_no_persistent_worker_context() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        for n in 0..20 {
            e.arm(n).unwrap();
            std::thread::scope(|s| {
                s.spawn(|| {
                    let w = lease(&A, n);
                    emit(&A, w.token(), n);
                    w.finish(Route::Delivered);
                })
                .join()
                .unwrap();
            });
            e.disarm().unwrap();
            assert!(A.reserve_armed(ctx(n)).is_none());
            {
                let d = e.drain().unwrap();
                assert_eq!(images(&d), [n as u128]);
            }
            assert_eq!(A.counts().unwrap(), Counts::default());
        }
    }
    #[test]
    fn independent_leases_never_cross_attribute() {
        static A: Arena = Arena::EMPTY;
        for unwind in [false, true] {
            let e = A.open().unwrap();
            e.arm(1).unwrap();
            let l1 = lease(&A, 1);
            let l2 = lease(&A, 2);
            let (t1, t2) = (l1.token(), l2.token());
            assert_ne!(t1, t2);
            A.begin(t1, BEGIN);
            A.begin(t2, BEGIN);
            emit(&A, t1, 1);
            emit(&A, t2, 2);
            assert_eq!(A.observe(t1, record(3), || Err::<Option<u8>, _>(9)), Err(9));
            let r = catch_unwind(AssertUnwindSafe(|| {
                let l2 = l2;
                emit(&A, t2, 4);
                if unwind {
                    panic!("lease unwind");
                }
                l2.finish(Route::Delivered);
            }));
            assert_eq!(r.is_err(), unwind);
            // A dead lease's copied token is stale, never re-attributed.
            emit(&A, t2, 6);
            emit(&A, t1, 5);
            l1.finish(Route::Declined);
            let mut seen = Vec::new();
            for _ in 0..2 {
                let d = e.drain().unwrap();
                seen.push(d.inspect(|v| (v.meta.token, lookups(v), v.meta.route)));
            }
            seen.sort_by_key(|(t, _, _)| t.transaction);
            assert_eq!(seen[0], (t1, vec![1, 3, 5], RouteState::Declined));
            let t2_route = if unwind {
                RouteState::Aborted
            } else {
                RouteState::Delivered
            };
            assert_eq!(seen[1], (t2, vec![2, 4], t2_route));
            assert_eq!(A.counts().unwrap().loss, 1);
            e.close().unwrap();
            A.acknowledge(e.epoch).unwrap();
        }
    }
    #[test]
    fn stale_token_runs_lookup_once_and_appends_nothing() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        let t = w.token();
        emit(&A, t, 1);
        w.finish(Route::Delivered);
        // After finish.
        let calls = Cell::new(0);
        assert_eq!(
            A.observe(t, record(2), || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(Some(2u8))
            }),
            Ok(Some(2))
        );
        assert_eq!(calls.get(), 1);
        A.request(t, record(3).key, Bucket::Detail);
        A.begin(t, BEGIN);
        assert_eq!(A.counts().unwrap().loss, 3);
        {
            let d = e.drain().unwrap();
            assert_eq!(images(&d), [1]);
            d.inspect(|v| {
                assert_eq!(v.snapshot.len, 2);
                assert!(v.meta.begin.is_none());
            });
        }
        e.close().unwrap();
        A.acknowledge(e.epoch).unwrap();
        // After a new epoch, even when the slot index is reused.
        let e2 = A.open().unwrap();
        e2.arm(1).unwrap();
        let w2 = lease(&A, 2);
        assert_eq!(
            A.observe(t, record(4), || {
                calls.set(calls.get() + 1);
                Err::<Option<u8>, _>(5)
            }),
            Err(5)
        );
        assert_eq!(calls.get(), 2);
        w2.finish(Route::Delivered);
        assert_eq!(A.counts().unwrap().loss, 1);
        let d = e2.drain().unwrap();
        assert!(images(&d).is_empty());
        d.inspect(|v| assert_eq!(v.snapshot.len, 1));
    }
    #[test]
    fn outer_lease_dropped_while_other_lease_live_blocks_epoch_reuse() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        let old_epoch = e.epoch;
        e.arm(1).unwrap();
        let outer = lease(&A, 1);
        let other = lease(&A, 2);
        let token = outer.token();
        drop(outer);
        drop(e);
        assert_eq!(A.state().unwrap(), State::Closing);
        assert_eq!(A.counts().unwrap().active, 1);
        assert!(A.open().is_err());
        assert!(A.acknowledge(old_epoch).is_err());
        drop(other);
        assert_eq!(A.state().unwrap(), State::Quiescent);
        A.acknowledge(old_epoch).unwrap();
        let next = A.open().unwrap();
        assert_ne!(next.epoch, old_epoch);
        emit(&A, token, 9);
        let c = A.counts().unwrap();
        assert_eq!((c.loss, c.active, c.reserved), (1, 0, 0));
    }
    #[test]
    fn closing_waits_for_delayed_worker_and_final_notification_is_not_terminal() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let r = A.reserve_armed(ctx(1)).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (go, wait) = mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(move || {
                let w = r.bind();
                ready_tx.send(()).unwrap();
                wait.recv_timeout(WAIT).unwrap();
                w.finish(Route::Delivered);
            });
            ready_rx.recv_timeout(WAIT).unwrap();
            e.close().unwrap(); // simulated final callback/owner timeout
            assert!(A.open().is_err());
            assert_eq!(A.counts().unwrap().active, 1);
            assert!(A.acknowledge(e.epoch).is_err());
            go.send(()).unwrap();
        });
        A.acknowledge(e.epoch).unwrap();
        assert!(A.open().is_ok());
    }
    #[test]
    fn saturated_slots_active_workers_and_scratch_refuse_without_overwrite() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        for n in 0..6 {
            let w = lease(&A, n);
            emit(&A, w.token(), n);
            w.finish(Route::Delivered);
        }
        let r1 = A.reserve_armed(ctx(6)).unwrap();
        let r2 = A.reserve_armed(ctx(7)).unwrap();
        assert!(A.reserve_armed(ctx(8)).is_none());
        let d = e.drain().unwrap();
        assert_eq!(images(&d), [0]);
        assert!(e.drain().is_err());
        assert!(A.reserve_armed(ctx(9)).is_none());
        drop(r1);
        drop(r2);
        assert_eq!(images(&d), [0]);
        drop(d);
        assert!(A.counts().unwrap().loss > 0);
    }
    #[test]
    fn epoch_counter_overflow_refuses_without_wrap() {
        static A: Arena = Arena::EMPTY;
        A.set_next_epoch_for_test(u64::MAX).unwrap();
        assert!(A.open().is_err());
        assert!(A.open().is_err());
        // Transaction ordinal exhaustion disables capture without wrapping.
        static B: Arena = Arena::EMPTY;
        let e = B.open().unwrap();
        e.arm(1).unwrap();
        B.set_next_transaction_for_test(u64::MAX).unwrap();
        assert!(B.reserve_armed(ctx(1)).is_none());
        assert!(B.reserve_armed(ctx(2)).is_none());
        let c = B.counts().unwrap();
        assert_eq!((c.reserved, c.active), (0, 0));
        assert!(c.disabled >= 2);
        // Operator ordinal exhaustion yields 0 (unknown) without wrapping.
        static C: Arena = Arena::EMPTY;
        assert_eq!(C.next_operator(), 1);
        assert_eq!(C.next_operator(), 2);
        C.set_next_operator_for_test(u64::MAX - 1).unwrap();
        assert_eq!(C.next_operator(), u64::MAX - 1);
        assert_eq!(C.next_operator(), 0);
        assert_eq!(C.next_operator(), 0);
    }
    #[test]
    fn exporter_unwind_releases_scratch_without_retaining_snapshot() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        w.finish(Route::Delivered);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _d = e.drain().unwrap();
            panic!("exporter");
        }));
        assert!(!A.counts().unwrap().scratch);
        e.close().unwrap();
        A.acknowledge(e.epoch).unwrap();
    }
    #[test]
    fn actual_lookup_and_export_inspection_never_hold_arena_mutex() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        assert_eq!(
            A.observe(w.token(), record(1), || {
                assert!(A.storage.try_lock().is_ok());
                Ok::<_, ()>(Some(1))
            }),
            Ok(Some(1))
        );
        w.finish(Route::Delivered);
        let d = e.drain().unwrap();
        d.inspect(|_| assert!(A.storage.try_lock().is_ok()));
    }
    #[test]
    fn context_is_copied_from_admission_not_replaced_by_drain_defaults() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(7).unwrap();
        let request = RequestContext {
            recipe_fingerprint: [3; 32],
            settings_fingerprint: [4; 32],
            process_identity: 5,
            output_tag: 2,
            headroom_bits: 4f32.to_bits(),
            requested_level: 1,
            phase_kind: 6,
            expected_operator: 8,
            generation: 9,
        };
        let begin = BeginContext::new([9; 32], 2, 4f32.to_bits(), 1, 8);
        let bound = A.reserve_armed(request).unwrap().bind();
        let bound_token = bound.token();
        A.begin(bound_token, begin);
        emit(&A, bound_token, 1);
        bound.finish(Route::Delivered);
        let unbegun = A.reserve_armed(request).unwrap().bind();
        emit(&A, unbegun.token(), 2);
        unbegun.finish(Route::Delivered);
        for _ in 0..2 {
            let d = e.drain().unwrap();
            d.inspect(|v| {
                // Request context always comes from reservation.
                assert_eq!(v.meta.request, request);
                let reachable = v.meta.route == RouteState::Delivered && v.meta.begin.is_some();
                if v.meta.token == bound_token {
                    assert_eq!(v.meta.begin, Some(begin));
                    assert!(reachable);
                    assert_eq!(
                        v.snapshot.context,
                        Context {
                            recipe_fingerprint: [3; 32],
                            resolved_wb: [9; 32],
                            output_tag: 2,
                            headroom_bits: 4f32.to_bits(),
                            render_level: 1,
                        }
                    );
                    let identity = Identity {
                        operator: 8,
                        phase: 7,
                        transaction: bound_token.transaction,
                        generation: 9,
                    };
                    assert_eq!(
                        super::super::reach(v.snapshot, identity, Bucket::Detail),
                        Ok(super::super::Reach::Hit)
                    );
                } else {
                    // No Begin: Unobserved, empty context, harness skips reach.
                    assert_eq!(v.meta.begin, None);
                    assert_eq!(v.meta.route, RouteState::Unobserved);
                    assert!(!reachable);
                    assert_eq!(v.snapshot.context, Context::default());
                }
            });
        }
    }
    #[test]
    fn poisoned_arena_disables_but_still_quiesces() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        let t = w.token();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = A.storage.lock();
            panic!("poison");
        }));
        assert!(A.storage.is_poisoned());
        let calls = Cell::new(0);
        assert_eq!(
            A.observe(t, record(1), || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(Some(1u8))
            }),
            Ok(Some(1))
        );
        assert_eq!(calls.get(), 1);
        assert!(A.reserve_armed(ctx(2)).is_none());
        drop(w);
        let c = A.counts().unwrap();
        assert_eq!((c.active, c.reserved), (0, 0));
        assert!(c.disabled >= 2);
        e.close().unwrap();
        assert_eq!(A.state().unwrap(), State::Quiescent);
        {
            let d = e.drain().unwrap();
            d.inspect(|v| {
                assert_eq!(v.meta.route, RouteState::Aborted);
                assert_eq!(v.snapshot.len, 0);
            });
        }
        A.acknowledge(e.epoch).unwrap();
    }
    // Stage A review NB-4: coverage for existing Stage A behaviour.
    #[test]
    fn acknowledge_discards_undrained_completed_slot() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        emit(&A, w.token(), 1);
        w.finish(Route::Delivered);
        assert_eq!(A.counts().unwrap().completed, 1);
        e.close().unwrap();
        A.acknowledge(e.epoch()).unwrap();
        let next = A.open().unwrap();
        assert_eq!(A.counts().unwrap().completed, 0);
        assert!(next.drain().is_err());
    }
    #[test]
    fn reopening_disabled_arena_keeps_sticky_flag_and_refuses_reservations() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = A.storage.lock();
            panic!("poison");
        }));
        e.close().unwrap();
        A.acknowledge(e.epoch()).unwrap();
        let next = A.open().unwrap();
        let c = A.counts().unwrap();
        assert!(c.disabled_sticky);
        assert_eq!((c.disabled, c.loss), (0, 0));
        next.arm(1).unwrap();
        assert!(A.reserve_armed(ctx(1)).is_none());
        let c = A.counts().unwrap();
        assert_eq!((c.disabled, c.reserved), (1, 0));
        assert!(c.disabled_sticky);
    }
    #[test]
    fn repeated_begin_counts_loss_and_keeps_first_context() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let w = lease(&A, 1);
        let first = BeginContext::new([5; 32], 1, 0, 0, 1);
        let second = BeginContext::new([6; 32], 2, 0, 1, 2);
        A.begin(w.token(), first);
        A.begin(w.token(), second);
        assert_eq!(A.counts().unwrap().loss, 1);
        w.finish(Route::Delivered);
        let d = e.drain().unwrap();
        d.inspect(|v| {
            assert_eq!(v.meta.begin, Some(first));
            let begins = v.snapshot.records[..v.snapshot.len]
                .iter()
                .flatten()
                .filter(|r| r.outcome == Outcome::Begin)
                .count();
            assert_eq!(begins, 1);
        });
    }
    #[test]
    fn drain_while_open_with_other_lease_live() {
        static A: Arena = Arena::EMPTY;
        let e = A.open().unwrap();
        e.arm(1).unwrap();
        let done = lease(&A, 1);
        let live = lease(&A, 2);
        emit(&A, done.token(), 1);
        done.finish(Route::Delivered);
        {
            let d = e.drain().unwrap();
            assert_eq!(images(&d), [1]);
        }
        assert_eq!(A.state().unwrap(), State::Open);
        assert_eq!(A.counts().unwrap().active, 1);
        emit(&A, live.token(), 2);
        live.finish(Route::Delivered);
        let d = e.drain().unwrap();
        assert_eq!(images(&d), [2]);
        assert_eq!(A.counts().unwrap().loss, 0);
    }
}
