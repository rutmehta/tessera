//! Process-wide interactive pressure (spec 08 §2: UI > viewport > … > exports).
//!
//! Every [`crate::ThreadPoolScheduler`] counts its queued and running
//! interactive jobs (`Priority::Ui`/`Viewport`) here. Bulk work that shares a
//! device with the viewport but cannot be pre-empted mid-dispatch (GPU export
//! bands) calls [`yield_to_interactive`] at its natural boundaries, so it never
//! starts a unit of work while interactive work is pending. Only scheduling
//! order changes: the bulk work keeps its own state and resumes afterwards.
use engine_api::{EngineResult, jobs::CancellationToken};
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

struct State {
    active: usize,
    last: Option<Instant>,
}

static STATE: Mutex<State> = Mutex::new(State {
    active: 0,
    last: None,
});
static IDLE: Condvar = Condvar::new();

pub(crate) fn begin() {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    s.active += 1;
    s.last = Some(Instant::now());
}

pub(crate) fn end() {
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    s.active = s.active.saturating_sub(1);
    s.last = Some(Instant::now());
    drop(s);
    IDLE.notify_all();
}

/// Interactive jobs currently queued or running on any scheduler.
pub fn interactive_pending() -> usize {
    STATE.lock().unwrap_or_else(|e| e.into_inner()).active
}

/// Waits while interactive work is queued or running, or finished less than
/// `quiet` ago (a drag's next event usually follows within one display
/// frame). Returns the time spent waiting. After `max_wait` it returns anyway,
/// so a continuously busy viewport cannot starve bulk work indefinitely.
/// Cancellation is observed at least every 10 ms.
pub fn yield_to_interactive(
    cancel: &CancellationToken,
    quiet: Duration,
    max_wait: Duration,
) -> EngineResult<Duration> {
    let start = Instant::now();
    let mut s = STATE.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        cancel.check()?;
        let now = Instant::now();
        let waited = now - start;
        if waited >= max_wait {
            return Ok(waited);
        }
        let settle = s.last.map_or(Duration::ZERO, |t| {
            (t + quiet).saturating_duration_since(now)
        });
        if s.active == 0 && settle.is_zero() {
            return Ok(waited);
        }
        let timeout = if s.active == 0 {
            settle
        } else {
            Duration::from_millis(10)
        }
        .min(Duration::from_millis(10))
        .min(max_wait - waited);
        s = IDLE
            .wait_timeout(s, timeout)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}
