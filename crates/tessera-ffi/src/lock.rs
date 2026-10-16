//! Instrumented mutexes (ST-1). The catalog and each cull session are guarded
//! by one `Traced` lock: every acquisition records how long it waited and how
//! long it was held, and in debug builds (or with `TESSERA_LOCK_TRACE=1`) a
//! wait or hold over [`WARN`] is reported on stderr with the calling function,
//! so a regression that holds the lock across I/O or long compute is visible
//! in a test log or a Console sample. `lock_for` is the main thread's entry:
//! it gives up with [`BridgeError::Busy`] instead of waiting out a worker.
use crate::{BridgeError, Result, failure};
use std::{
    ops::{Deref, DerefMut},
    panic::Location,
    sync::{
        Mutex, MutexGuard, OnceLock, TryLockError,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Holds and waits at least this long are reported when tracing is on.
pub(crate) const WARN: Duration = Duration::from_millis(50);
/// How long a UI-safe entry point waits before reporting `Busy`.
pub(crate) const UI_BUDGET: Duration = Duration::from_millis(20);

fn tracing() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        cfg!(debug_assertions) || std::env::var_os("TESSERA_LOCK_TRACE").is_some_and(|v| v == "1")
    })
}

/// Longest wait and hold seen on one lock, in microseconds. Tests assert on
/// them (`reset` starts a measurement); the trace lines are the production view.
#[derive(Default)]
pub(crate) struct LockStats {
    max_wait_us: AtomicU64,
    max_hold_us: AtomicU64,
}

impl LockStats {
    fn note(slot: &AtomicU64, took: Duration) {
        slot.fetch_max(
            took.as_micros().min(u64::MAX as u128) as u64,
            Ordering::Relaxed,
        );
    }
    #[cfg(test)]
    pub(crate) fn max_wait(&self) -> Duration {
        Duration::from_micros(self.max_wait_us.load(Ordering::Relaxed))
    }
    #[cfg(test)]
    pub(crate) fn max_hold(&self) -> Duration {
        Duration::from_micros(self.max_hold_us.load(Ordering::Relaxed))
    }
    #[cfg(test)]
    pub(crate) fn reset(&self) {
        self.max_wait_us.store(0, Ordering::Relaxed);
        self.max_hold_us.store(0, Ordering::Relaxed);
    }
}

/// A mutex that measures its waits and holds.
pub(crate) struct Traced<T> {
    name: &'static str,
    inner: Mutex<T>,
    stats: LockStats,
}

impl<T> Traced<T> {
    pub(crate) fn new(name: &'static str, value: T) -> Self {
        Self {
            name,
            inner: Mutex::new(value),
            stats: LockStats::default(),
        }
    }
    #[cfg(test)]
    pub(crate) fn stats(&self) -> &LockStats {
        &self.stats
    }
    fn held<'a>(
        &'a self,
        guard: MutexGuard<'a, T>,
        waited: Duration,
        caller: &'static Location<'static>,
    ) -> Guard<'a, T> {
        LockStats::note(&self.stats.max_wait_us, waited);
        if waited >= WARN && tracing() {
            eprintln!(
                "{} lock: {} waited {} ms",
                self.name,
                caller,
                waited.as_millis()
            );
        }
        Guard {
            guard: Some(guard),
            lock: self,
            since: Instant::now(),
            caller,
        }
    }
    /// Waits for the lock.
    #[track_caller]
    pub(crate) fn lock(&self) -> Result<Guard<'_, T>> {
        let caller = Location::caller();
        let started = Instant::now();
        let guard = self.inner.lock().map_err(failure)?;
        Ok(self.held(guard, started.elapsed(), caller))
    }
    /// Waits at most `budget` for the lock, then reports `Busy` so a caller
    /// on the main thread can retry later instead of freezing.
    #[track_caller]
    pub(crate) fn lock_for(&self, budget: Duration) -> Result<Guard<'_, T>> {
        let caller = Location::caller();
        let started = Instant::now();
        loop {
            match self.inner.try_lock() {
                Ok(guard) => return Ok(self.held(guard, started.elapsed(), caller)),
                Err(TryLockError::Poisoned(e)) => return Err(failure(e)),
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= budget {
                        if tracing() {
                            eprintln!(
                                "{} lock: {} busy after {} ms",
                                self.name,
                                caller,
                                budget.as_millis()
                            );
                        }
                        return Err(BridgeError::Busy {
                            message: format!("{} is busy; retry", self.name),
                        });
                    }
                    std::thread::sleep(Duration::from_micros(250));
                }
            }
        }
    }
}

/// A held `Traced` lock; releasing it records the hold.
pub(crate) struct Guard<'a, T> {
    guard: Option<MutexGuard<'a, T>>,
    lock: &'a Traced<T>,
    since: Instant,
    caller: &'static Location<'static>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        self.guard.as_ref().expect("guard is held until drop")
    }
}
impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.guard.as_mut().expect("guard is held until drop")
    }
}
impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        // Release first so the report never extends the hold.
        drop(self.guard.take());
        let held = self.since.elapsed();
        LockStats::note(&self.lock.stats.max_hold_us, held);
        if held >= WARN && tracing() {
            eprintln!(
                "{} lock: {} held {} ms",
                self.lock.name,
                self.caller,
                held.as_millis()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn holds_are_measured_and_busy_is_reported_within_budget() {
        let lock = Traced::new("test", 0u32);
        {
            let mut g = lock.lock().unwrap();
            *g += 1;
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(lock.stats().max_hold() >= Duration::from_millis(5));
        std::thread::scope(|s| {
            let held = lock.lock().unwrap();
            s.spawn(|| {
                let started = Instant::now();
                let err = lock.lock_for(Duration::from_millis(10)).err().unwrap();
                assert!(matches!(err, BridgeError::Busy { .. }), "{err}");
                assert!(started.elapsed() < Duration::from_millis(500));
            });
            std::thread::sleep(Duration::from_millis(30));
            drop(held);
        });
        lock.stats().reset();
        assert_eq!(lock.stats().max_hold(), Duration::ZERO);
        assert_eq!(*lock.lock_for(UI_BUDGET).unwrap(), 1);
    }
}
