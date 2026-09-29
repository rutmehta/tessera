//! Test guard for the shared static [`ARENA`] (rev7 3.1a, R2). Feature-only;
//! used by image-core and tessera-ffi tests. It uses only the transport API.
use super::transport::SLOTS;
use super::{ARENA, Counts, Epoch, Error, SlotMeta, State};

/// Why [`EpochGuard::open`] refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuardError {
    /// The test binary was not launched with `--test-threads=1` (or
    /// `RUST_TEST_THREADS=1`). The shared arena is only correct serially.
    NotSingleThreaded,
    /// The arena is not Idle: an earlier test leaked a live lease.
    NotIdle(State),
    /// The arena itself refused.
    Arena(Error),
}

/// N-3: the shared static ARENA is correct only when one test runs at a time.
fn single_threaded_test_run() -> bool {
    // Test-helper allocation, outside the storage ledger.
    // libtest gives a `--test-threads` argument precedence over the
    // environment (Stage B review NB-2): decide on the last one if present.
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let mut cli = None;
    for (i, a) in args.iter().enumerate() {
        if let Some(v) = a.to_str().and_then(|a| a.strip_prefix("--test-threads=")) {
            cli = Some(v == "1");
        } else if a == "--test-threads" {
            cli = Some(args.get(i + 1).is_some_and(|v| v == "1"));
        }
    }
    cli.unwrap_or_else(|| std::env::var_os("RUST_TEST_THREADS").is_some_and(|v| v == "1"))
}

/// Opens one epoch of [`ARENA`] and, on drop (including unwind), closes it,
/// drains and discards what it can, and acknowledges it.
pub struct EpochGuard {
    epoch: Option<Epoch<'static>>,
    id: u64,
}

impl EpochGuard {
    /// Refuses unless single-threaded, then requires Idle, then opens.
    pub fn open() -> Result<Self, GuardError> {
        if !single_threaded_test_run() {
            return Err(GuardError::NotSingleThreaded);
        }
        let state = ARENA.state().map_err(GuardError::Arena)?;
        if state != State::Idle {
            return Err(GuardError::NotIdle(state));
        }
        let epoch = ARENA.open().map_err(GuardError::Arena)?;
        let id = epoch.epoch();
        Ok(Self {
            epoch: Some(epoch),
            id,
        })
    }
    pub fn epoch(&self) -> &Epoch<'static> {
        self.epoch
            .as_ref()
            .expect("EpochGuard epoch is present until drop")
    }
}

impl Drop for EpochGuard {
    fn drop(&mut self) {
        if let Some(e) = self.epoch.take() {
            let _ = e.close();
            // N-2: bounded drain-and-discard; at most SLOTS completed slots.
            for _ in 0..=SLOTS {
                if !ARENA.counts().is_ok_and(|c| c.completed > 0) {
                    break;
                }
                match e.drain() {
                    Ok(d) => drop(d),
                    Err(_) => break,
                }
            }
            drop(e);
            // Succeeds iff quiescent; otherwise the next open reports NotIdle.
            let _ = ARENA.acknowledge(self.id);
        }
    }
}

/// Stage A review NB-1: an epoch with loss, refused work or a disabled arena
/// is inconclusive.
pub fn epoch_inconclusive(c: &Counts) -> bool {
    c.loss > 0 || c.disabled > 0 || c.disabled_sticky || c.attribution_rejected > 0
}

/// Stage A review NB-6: records carry the reservation's expected operator, so
/// the Begin operator (the renderer's tag) must be compared explicitly.
pub fn operator_matches(meta: &SlotMeta) -> bool {
    meta.begin
        .is_some_and(|b| b.operator == meta.request.expected_operator)
}
