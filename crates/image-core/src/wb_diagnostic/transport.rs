//! Explicit-token WB diagnostic transport (rev5 Stage A). No renderer hooks.
//! Attribution travels only as a by-value `Copy` [`Token`]: no thread-local
//! storage and no thread identity. All storage is one const-initialised arena
//! whose epoch protocol lives under a single leaf mutex.
#![allow(dead_code)]
use super::{Bucket, CAPACITY, Context, Error, Record, Snapshot};
use engine_api::{
    id::{Digest, ImageId},
    stage::{MemoKey, ParamHash, StageId},
    tile::TileCoord,
};
use std::{
    mem::{ManuallyDrop, needs_drop},
    sync::{Mutex, PoisonError, atomic::AtomicBool},
};

pub const SLOTS: usize = 8;
pub const ACTIVE: usize = 2;
const ARENA_BYTES: usize = 296 * 1024;
const METADATA_BYTES: usize = 8 * 1024;
const FIXED_BYTES: usize = 328 * 1024;

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
/// Written only at binding (`begin`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeginContext {
    pub resolved_wb: [u8; 32],
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
const EMPTY_SNAPSHOT: Snapshot = Snapshot {
    context: EMPTY_CONTEXT,
    records: [None; CAPACITY],
    len: 0,
    overflow: 0,
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
        payload: EMPTY_SNAPSHOT,
    };
}

fn unsupported<T>() -> Result<T, Error> {
    Err(Error::Unsupported)
}
impl Arena {
    /// Const initialiser for a process `static`; never used as a value.
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
            snapshot: EMPTY_SNAPSHOT,
            meta: SlotMeta::EMPTY,
        }),
        armed: AtomicBool::new(false),
    };
    pub fn open(&self) -> Result<Epoch<'_>, Error> {
        unsupported()
    }
    pub fn counts(&self) -> Result<Counts, Error> {
        unsupported()
    }
    pub fn state(&self) -> Result<State, Error> {
        unsupported()
    }
    pub fn acknowledge(&self, _epoch: u64) -> Result<(), Error> {
        unsupported()
    }
    /// Product call site: infallible; `None` unless a phase is armed.
    pub fn reserve_armed(&'static self, _ctx: RequestContext) -> Option<Reservation> {
        None
    }
    /// Runs `lookup` exactly once with no lock held and returns its result.
    pub fn observe<T, E>(
        &self,
        _t: Token,
        _rec: Record,
        lookup: impl FnOnce() -> Result<Option<T>, E>,
    ) -> Result<Option<T>, E> {
        lookup()
    }
    pub fn request(&self, _t: Token, _key: MemoKey, _bucket: Bucket) {}
    pub fn begin(&self, _t: Token, _ctx: BeginContext) {}
    /// 0 = exhausted/unknown.
    pub fn next_operator(&self) -> u64 {
        0
    }
    #[cfg(test)]
    fn set_next_epoch_for_test(&self, _n: u64) -> Result<(), Error> {
        unsupported()
    }
    #[cfg(test)]
    fn set_next_transaction_for_test(&self, _n: u64) -> Result<(), Error> {
        unsupported()
    }
    #[cfg(test)]
    fn set_next_operator_for_test(&self, _n: u64) -> Result<(), Error> {
        unsupported()
    }
}
impl<'a> Epoch<'a> {
    pub fn arm(&self, _phase: u64) -> Result<(), Error> {
        unsupported()
    }
    pub fn disarm(&self) -> Result<(), Error> {
        unsupported()
    }
    pub fn close(&self) -> Result<(), Error> {
        unsupported()
    }
    pub fn drain(&self) -> Result<Drain<'a>, Error> {
        unsupported()
    }
}
impl Reservation {
    pub fn token(&self) -> Token {
        self.token
    }
    pub fn bind(self) -> Lease {
        let this = ManuallyDrop::new(self);
        Lease {
            arena: this.arena,
            token: this.token,
        }
    }
}
impl Lease {
    pub fn token(&self) -> Token {
        self.token
    }
    pub fn finish(self, _route: Route) {
        let _this = ManuallyDrop::new(self);
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
    fn drop(&mut self) {}
}
impl Drop for Reservation {
    fn drop(&mut self) {}
}
impl Drop for Lease {
    fn drop(&mut self) {}
}
impl Drop for Drain<'_> {
    fn drop(&mut self) {}
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
    const BEGIN: BeginContext = BeginContext {
        resolved_wb: [1; 32],
        output_tag: 1,
        headroom_bits: 0,
        render_level: 0,
        operator: 1,
    };
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
        let begin = BeginContext {
            resolved_wb: [9; 32],
            output_tag: 2,
            headroom_bits: 4f32.to_bits(),
            render_level: 1,
            operator: 8,
        };
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
}
