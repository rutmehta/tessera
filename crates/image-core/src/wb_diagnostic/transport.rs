//! SOURCE-ONLY Unsupported guard/transport contracts. No renderer hooks.
//! Fixed arena storage is separate from recursive scalar caller guards/RSS.
#![allow(dead_code)]
use super::{Context, Error, Record, Snapshot};
use std::{marker::PhantomData, rc::Rc, sync::Mutex, thread::ThreadId};

const SLOTS: usize = 8;
const ACTIVE: usize = 2;
const FIXED_BYTES: usize = 328 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Token {
    epoch: u64,
    phase: u64,
    transaction: u64,
    slot: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Idle,
    Open,
    Closing,
    Quiescent,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
struct Counts {
    reserved: usize,
    active: usize,
    suppression: usize,
    completed: usize,
    scratch: bool,
    loss: u64,
}
struct Slot {
    token: Option<Token>,
    payload: Snapshot,
    state: u8,
}
struct WorkerSlot {
    thread: Option<ThreadId>,
    token: Option<Token>,
    depth: u64,
    latched: bool,
}
struct Storage {
    slots: [Slot; SLOTS],
    workers: [WorkerSlot; ACTIVE],
    scratch: Snapshot,
    counts: Counts,
    epoch: u64,
    ordinal: u64,
    state: State,
}
struct Arena {
    storage: Mutex<Storage>,
}
// All handles borrow stable arena lifetime, never each other or render objects.
// Actual live wiring must use process-static Arena, with in-place initialization.
struct Epoch<'a> {
    arena: &'a Arena,
    epoch: u64,
}
struct Reservation<'a> {
    arena: &'a Arena,
    token: Token,
}
struct Worker<'a> {
    arena: &'a Arena,
    token: Token,
    local: PhantomData<Rc<()>>,
}
struct Suppression<'a> {
    arena: &'a Arena,
    token: Token,
    local: PhantomData<Rc<()>>,
}
struct Drain<'a> {
    arena: &'a Arena,
    epoch: u64,
}
fn unsupported<T>() -> Result<T, Error> {
    Err(Error::Unsupported)
}
impl Arena {
    fn new() -> Result<Self, Error> {
        unsupported()
    }
    fn open(&self) -> Result<Epoch<'_>, Error> {
        unsupported()
    }
    fn counts(&self) -> Result<Counts, Error> {
        unsupported()
    }
    fn state(&self) -> Result<State, Error> {
        unsupported()
    }
    fn acknowledge(&self, _epoch: u64) -> Result<(), Error> {
        unsupported()
    }
    fn observe<T, E>(
        &self,
        _record: Record,
        _lookup: impl FnOnce() -> Result<Option<T>, E>,
    ) -> Result<Result<Option<T>, E>, Error> {
        unsupported()
    }
    fn stale_terminal(&self, _token: Token) -> Result<(), Error> {
        unsupported()
    }
    fn set_next_epoch_for_test(&self, _n: u64) -> Result<(), Error> {
        unsupported()
    }
}
impl<'a> Epoch<'a> {
    fn reserve(&self, _phase: u64, _context: Context) -> Result<Reservation<'a>, Error> {
        unsupported()
    }
    fn close(&self) -> Result<(), Error> {
        unsupported()
    }
    fn drain(&self) -> Result<Drain<'a>, Error> {
        unsupported()
    }
}
impl<'a> Reservation<'a> {
    // Result lifetime is arena lifetime, not a borrow of Epoch/Reservation.
    fn bind(self) -> Result<Worker<'a>, Error> {
        unsupported()
    }
}
impl<'a> Worker<'a> {
    fn finish(self) -> Result<(), Error> {
        unsupported()
    }
    fn suppress(&self) -> Result<Suppression<'a>, Error> {
        unsupported()
    }
    fn set_depth_for_test(&self, _depth: u64) -> Result<(), Error> {
        unsupported()
    }
}
impl Drain<'_> {
    // Exposes a borrowed scratch value to pure tests, not a by-value Snapshot.
    fn inspect<T>(&self, _f: impl FnOnce(&Snapshot) -> T) -> Result<T, Error> {
        unsupported()
    }
}
impl Drop for Epoch<'_> {
    fn drop(&mut self) {}
}
impl Drop for Reservation<'_> {
    fn drop(&mut self) {}
}
impl Drop for Worker<'_> {
    fn drop(&mut self) {}
}
impl Drop for Suppression<'_> {
    fn drop(&mut self) {}
}
impl Drop for Drain<'_> {
    fn drop(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wb_diagnostic::{Bucket, Identity, Outcome};
    use engine_api::{
        id::ImageId,
        stage::{MemoKey, ParamHash, StageId},
        tile::TileCoord,
    };
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        sync::mpsc,
        time::Duration,
    };
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
    fn emit(arena: &Arena, n: u64) {
        assert_eq!(
            arena.observe(record(n), || Ok::<_, ()>(Some(n))).unwrap(),
            Ok(Some(n))
        );
    }
    fn images(d: &Drain<'_>) -> Vec<u128> {
        d.inspect(|s| {
            s.records[..s.len]
                .iter()
                .flatten()
                .filter(|r| matches!(r.outcome, Outcome::Some | Outcome::None | Outcome::Error))
                .map(|r| r.key.image_id.0)
                .collect()
        })
        .unwrap()
    }
    #[test]
    fn fixed_storage_and_scalar_handle_layout_do_not_require_guard_implementation() {
        assert!(size_of::<Snapshot>() <= 32 * 1024);
        assert!(
            size_of::<Arena>()
                + size_of::<Snapshot>()
                + 2 * size_of::<Worker<'_>>()
                + 2 * size_of::<Reservation<'_>>()
                + size_of::<Drain<'_>>()
                + size_of::<Epoch<'_>>()
                <= FIXED_BYTES
        );
        assert!(size_of::<Worker<'_>>() <= 96 && size_of::<Suppression<'_>>() <= 96);
        assert!(!std::mem::needs_drop::<Storage>());
        assert_eq!((SLOTS, ACTIVE), (8, 2));
    }
    #[test]
    fn successful_terminal_and_drain_are_single_owner() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        emit(&a, 1);
        w.finish().unwrap();
        let d = e.drain().unwrap();
        assert_eq!(images(&d), [1]);
        assert!(e.drain().is_err());
        assert!(a.counts().unwrap().scratch);
        assert_eq!(
            d.inspect(|s| s.records[s.len - 1].unwrap().outcome)
                .unwrap(),
            Outcome::End
        );
        drop(d);
        e.close().unwrap();
        a.acknowledge(e.epoch).unwrap();
        assert!(a.open().is_ok());
    }
    #[test]
    fn error_unwind_and_unstarted_job_drop_abort_and_release() {
        for mode in 0..3 {
            let a = Arena::new().unwrap();
            let e = a.open().unwrap();
            let r = e.reserve(1, Context::default()).unwrap();
            if mode == 0 {
                drop(r);
            } else {
                let _ = catch_unwind(AssertUnwindSafe(|| {
                    let _w = r.bind().unwrap();
                    assert_eq!(
                        a.observe(record(1), || Err::<Option<u8>, _>(7)).unwrap(),
                        Err(7)
                    );
                    if mode == 2 {
                        panic!("synthetic unwind");
                    }
                }));
            }
            assert_eq!(a.counts().unwrap().active + a.counts().unwrap().reserved, 0);
            let d = e.drain().unwrap();
            assert_eq!(
                d.inspect(|s| s.records[s.len - 1].unwrap().outcome)
                    .unwrap(),
                Outcome::Abort
            );
        }
    }
    #[test]
    fn overflow_preserves_original_lookup_exactly_once() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        for n in 0..300 {
            emit(&a, n);
        }
        let calls = Cell::new(0);
        assert_eq!(
            a.observe(record(999), || {
                calls.set(calls.get() + 1);
                Err::<Option<u8>, _>(17)
            })
            .unwrap(),
            Err(17)
        );
        assert_eq!(calls.get(), 1);
        w.finish().unwrap();
        assert!(e.drain().unwrap().inspect(|s| s.overflow > 0).unwrap());
    }
    #[test]
    fn two_workers_isolate_records_and_third_reservation_marks_loss() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let r1 = e.reserve(1, Context::default()).unwrap();
        let r2 = e.reserve(2, Context::default()).unwrap();
        assert!(e.reserve(3, Context::default()).is_err());
        let (ready_tx, ready_rx) = mpsc::channel();
        let (go1, wait1) = mpsc::channel();
        let (go2, wait2) = mpsc::channel();
        std::thread::scope(|s| {
            let ready1 = ready_tx.clone();
            let arena = &a;
            s.spawn(move || {
                let w = r1.bind().unwrap();
                ready1.send(()).unwrap();
                wait1.recv_timeout(Duration::from_secs(2)).unwrap();
                emit(arena, 11);
                w.finish().unwrap();
            });
            s.spawn(move || {
                let w = r2.bind().unwrap();
                ready_tx.send(()).unwrap();
                wait2.recv_timeout(Duration::from_secs(2)).unwrap();
                emit(arena, 22);
                w.finish().unwrap();
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            assert_eq!(a.counts().unwrap().active, 2);
            go1.send(()).unwrap();
            go2.send(()).unwrap();
        });
        let x = {
            let d = e.drain().unwrap();
            images(&d)
        };
        let y = {
            let d = e.drain().unwrap();
            images(&d)
        };
        assert!(x == [11] && y == [22] || x == [22] && y == [11]);
        assert!(a.counts().unwrap().loss > 0);
    }
    #[test]
    fn sequential_threads_and_rejections_leave_no_persistent_worker_context() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        for n in 0..20 {
            std::thread::scope(|s| {
                s.spawn(|| {
                    let w = e.reserve(n, Context::default()).unwrap().bind().unwrap();
                    emit(&a, n);
                    w.finish().unwrap();
                })
                .join()
                .unwrap();
            });
            {
                let d = e.drain().unwrap();
                assert_eq!(images(&d), [n as u128]);
            }
            assert_eq!(a.counts().unwrap(), Counts::default());
        }
    }
    #[test]
    fn nested_suppression_restores_after_normal_error_and_unwind() {
        for unwind in [false, true] {
            let a = Arena::new().unwrap();
            let e = a.open().unwrap();
            let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
            emit(&a, 1);
            let _ = catch_unwind(AssertUnwindSafe(|| {
                let _s = w.suppress().unwrap();
                emit(&a, 2);
                {
                    let _nested = w.suppress().unwrap();
                    emit(&a, 3);
                }
                assert_eq!(
                    a.observe(record(4), || Err::<Option<u8>, _>(9)).unwrap(),
                    Err(9)
                );
                if unwind {
                    panic!("nested unwind");
                }
            }));
            emit(&a, 5);
            w.finish().unwrap();
            let d = e.drain().unwrap();
            assert_eq!(images(&d), [1, 5]);
            assert!(a.counts().unwrap().loss > 0);
        }
    }
    #[test]
    fn suppression_counter_overflow_never_restores_outer_attribution() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        w.set_depth_for_test(u64::MAX).unwrap();
        let result = w.suppress();
        assert!(result.is_err());
        drop(result);
        emit(&a, 99);
        w.finish().unwrap();
        assert!(images(&e.drain().unwrap()).is_empty());
    }
    #[test]
    fn outer_dropped_before_nested_guard_blocks_epoch_reuse() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let old_epoch = e.epoch;
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        let token = w.token;
        let child = w.suppress().unwrap();
        drop(w);
        drop(e);
        assert_eq!(a.state().unwrap(), State::Closing);
        assert_eq!(a.counts().unwrap().suppression, 1);
        assert!(a.open().is_err());
        assert!(a.acknowledge(old_epoch).is_err());
        drop(child);
        a.acknowledge(old_epoch).unwrap();
        let next = a.open().unwrap();
        assert_ne!(next.epoch, old_epoch);
        assert!(a.stale_terminal(token).is_err());
        assert_eq!(a.counts().unwrap().active, 0);
    }
    #[test]
    fn closing_waits_for_delayed_worker_and_final_notification_is_not_terminal() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let r = e.reserve(1, Context::default()).unwrap();
        let (ready_tx, ready_rx) = mpsc::channel();
        let (go, wait) = mpsc::channel();
        std::thread::scope(|s| {
            s.spawn(move || {
                let w = r.bind().unwrap();
                ready_tx.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(2)).unwrap();
                w.finish().unwrap();
            });
            ready_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            e.close().unwrap(); // simulated final callback/owner timeout
            assert!(a.open().is_err());
            assert_eq!(a.counts().unwrap().active, 1);
            assert!(a.acknowledge(e.epoch).is_err());
            go.send(()).unwrap();
        });
        a.acknowledge(e.epoch).unwrap();
        assert!(a.open().is_ok());
    }
    #[test]
    fn saturated_slots_active_workers_and_scratch_refuse_without_overwrite() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        for n in 0..6 {
            let w = e.reserve(n, Context::default()).unwrap().bind().unwrap();
            emit(&a, n);
            w.finish().unwrap();
        }
        let r1 = e.reserve(6, Context::default()).unwrap();
        let r2 = e.reserve(7, Context::default()).unwrap();
        assert!(e.reserve(8, Context::default()).is_err());
        let d = e.drain().unwrap();
        assert_eq!(images(&d), [0]);
        assert!(e.drain().is_err());
        assert!(e.reserve(9, Context::default()).is_err());
        drop(r1);
        drop(r2);
        assert_eq!(images(&d), [0]);
        drop(d);
        assert!(a.counts().unwrap().loss > 0);
    }
    #[test]
    fn epoch_counter_overflow_refuses_without_wrap() {
        let a = Arena::new().unwrap();
        a.set_next_epoch_for_test(u64::MAX).unwrap();
        assert!(a.open().is_err());
        assert!(a.open().is_err());
    }
    #[test]
    fn exporter_unwind_releases_scratch_without_retaining_snapshot() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        w.finish().unwrap();
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _d = e.drain().unwrap();
            panic!("exporter");
        }));
        assert!(!a.counts().unwrap().scratch);
        e.close().unwrap();
        a.acknowledge(e.epoch).unwrap();
    }
    #[test]
    fn actual_lookup_and_export_inspection_never_hold_arena_mutex() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let w = e.reserve(1, Context::default()).unwrap().bind().unwrap();
        assert_eq!(
            a.observe(record(1), || {
                assert!(a.storage.try_lock().is_ok());
                Ok::<_, ()>(Some(1))
            })
            .unwrap(),
            Ok(Some(1))
        );
        w.finish().unwrap();
        let d = e.drain().unwrap();
        d.inspect(|_| assert!(a.storage.try_lock().is_ok()))
            .unwrap();
    }
    #[test]
    fn context_is_copied_from_admission_not_replaced_by_drain_defaults() {
        let a = Arena::new().unwrap();
        let e = a.open().unwrap();
        let c = Context {
            recipe_fingerprint: [3; 32],
            resolved_wb: [9; 32],
            output_tag: 2,
            headroom_bits: 4f32.to_bits(),
            render_level: 1,
        };
        let w = e.reserve(7, c).unwrap().bind().unwrap();
        emit(&a, 1);
        w.finish().unwrap();
        assert_eq!(e.drain().unwrap().inspect(|s| s.context).unwrap(), c);
    }
}
