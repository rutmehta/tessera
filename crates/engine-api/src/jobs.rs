//! Background work: jobs, priority classes, cancellation and the scheduler
//! contract (spec 08 §2).
//!
//! Everything that is not an immediate UI reaction — tile renders, prefetch,
//! preview generation, AI scoring, export — is a [`Job`] submitted to a
//! [`Scheduler`]. Jobs are cooperative: they poll their
//! [`CancellationToken`] between units of work (typically per tile).

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::error::{EngineError, EngineResult};
use crate::id::{JobGroupId, JobId};

/// Priority class of a job. Declared most urgent first; the derived `Ord`
/// therefore sorts urgent work first (`Ui < Viewport < … < Export`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    /// Work the user is waiting on this frame (screen-resolution slider updates).
    Ui,
    /// Refinement of tiles under the visible viewport.
    Viewport,
    /// Neighbouring images in the filmstrip, adjacent zoom levels.
    Prefetch,
    /// Library preview and thumbnail generation.
    Preview,
    /// AI scores, embeddings, faces.
    Score,
    /// Exports and other bulk output.
    Export,
}

impl Priority {
    /// All classes, most urgent first.
    pub const ALL: [Priority; 6] = [
        Self::Ui,
        Self::Viewport,
        Self::Prefetch,
        Self::Preview,
        Self::Score,
        Self::Export,
    ];

    /// True if `self` must be scheduled ahead of `other`.
    pub fn is_more_urgent_than(self, other: Priority) -> bool {
        self < other
    }

    /// True for classes that directly back something on screen; schedulers
    /// may preempt lower classes for these.
    pub fn is_interactive(self) -> bool {
        matches!(self, Self::Ui | Self::Viewport)
    }
}

struct TokenInner {
    cancelled: AtomicBool,
    parent: Option<CancellationToken>,
}

/// Cooperative cancellation flag, cheap to clone and check.
///
/// Tokens form a tree: cancelling a token cancels every token derived from it
/// with [`child`](CancellationToken::child), but not its parent. A typical
/// shape is one token per image view with child tokens per render pass.
#[derive(Clone)]
pub struct CancellationToken(Arc<TokenInner>);

impl CancellationToken {
    /// A fresh, uncancelled root token.
    pub fn new() -> Self {
        Self(Arc::new(TokenInner {
            cancelled: AtomicBool::new(false),
            parent: None,
        }))
    }

    /// A token cancelled when either it or `self` is cancelled.
    pub fn child(&self) -> Self {
        Self(Arc::new(TokenInner {
            cancelled: AtomicBool::new(false),
            parent: Some(self.clone()),
        }))
    }

    /// Requests cancellation. Idempotent.
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Release);
    }

    /// True if this token or any ancestor has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        let mut cur = Some(self);
        while let Some(t) = cur {
            if t.0.cancelled.load(Ordering::Acquire) {
                return true;
            }
            cur = t.0.parent.as_ref();
        }
        false
    }

    /// `Err(EngineError::Cancelled)` if cancelled; use with `?` between units of work.
    pub fn check(&self) -> EngineResult<()> {
        if self.is_cancelled() {
            Err(EngineError::Cancelled)
        } else {
            Ok(())
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for CancellationToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CancellationToken")
            .field("cancelled", &self.is_cancelled())
            .finish()
    }
}

/// Receives progress reports from a running job.
pub trait ProgressSink: Send + Sync {
    /// `fraction` in `[0, 1]`; `message` is optional human-readable status.
    fn report(&self, fraction: f32, message: Option<&str>);
}

/// What a running job can see of its environment.
pub struct JobContext {
    /// The job's id as assigned by the scheduler.
    pub id: JobId,
    /// Cancellation token; poll between units of work.
    pub cancellation: CancellationToken,
    progress: Option<Arc<dyn ProgressSink>>,
}

impl JobContext {
    /// Creates a context. Schedulers call this; jobs only consume it.
    pub fn new(
        id: JobId,
        cancellation: CancellationToken,
        progress: Option<Arc<dyn ProgressSink>>,
    ) -> Self {
        Self {
            id,
            cancellation,
            progress,
        }
    }

    /// Shorthand for `self.cancellation.check()`.
    pub fn check_cancelled(&self) -> EngineResult<()> {
        self.cancellation.check()
    }

    /// Reports progress, clamped to `[0, 1]`. No-op without a sink.
    pub fn report_progress(&self, fraction: f32, message: Option<&str>) {
        if let Some(sink) = &self.progress {
            sink.report(fraction.clamp(0.0, 1.0), message);
        }
    }
}

impl fmt::Debug for JobContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobContext")
            .field("id", &self.id)
            .field("cancellation", &self.cancellation)
            .finish()
    }
}

/// A unit of background work.
///
/// Jobs deliver their results through side effects they own (writing into a
/// tile cache, sending on a channel, updating the index), which keeps the
/// trait object-safe and the scheduler type-agnostic.
pub trait Job: Send {
    /// Short label for logs and the activity UI, e.g. `"render L0 tiles"`.
    fn label(&self) -> &str;

    /// Priority class. Read once at submission.
    fn priority(&self) -> Priority;

    /// Optional group for bulk cancellation and reprioritisation.
    fn group(&self) -> Option<JobGroupId> {
        None
    }

    /// Does the work. Must return `Err(EngineError::Cancelled)` promptly after
    /// the context's token is cancelled.
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()>;
}

/// Lifecycle state of a submitted job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum JobStatus {
    /// Waiting for a worker.
    Queued,
    /// Executing.
    Running {
        /// Last reported progress in `[0, 1]`.
        progress: f32,
    },
    /// Finished successfully.
    Succeeded,
    /// Finished with an error.
    Failed {
        /// The error the job returned.
        error: EngineError,
    },
    /// Cancelled before or during execution.
    Cancelled,
    /// The scheduler no longer tracks this id (finished and evicted, or never existed).
    Unknown,
}

/// Handle returned by [`Scheduler::submit`].
#[derive(Debug, Clone)]
pub struct JobHandle {
    /// Assigned id.
    pub id: JobId,
    /// Token that cancels this job (a child of any token given at submission).
    pub cancellation: CancellationToken,
}

impl JobHandle {
    /// Cancels the job.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

/// Runs [`Job`]s by priority on a worker pool.
///
/// Contract: within a priority class jobs start in submission order; a job of
/// a more urgent class never waits behind a less urgent queued job; cancelled
/// queued jobs never start; `status` is eventually consistent.
pub trait Scheduler: Send + Sync {
    /// Queues a job. `parent` (if any) becomes the parent of the job's token.
    fn submit(&self, job: Box<dyn Job>, parent: Option<&CancellationToken>) -> JobHandle;

    /// Moves a queued job (or every queued job in a group) to another class.
    fn reprioritize(&self, target: JobTarget, priority: Priority);

    /// Cancels a job or every job in a group.
    fn cancel(&self, target: JobTarget);

    /// Current status of a job.
    fn status(&self, id: JobId) -> JobStatus;
}

/// Addresses one job or a group of jobs in [`Scheduler`] calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobTarget {
    /// A single job.
    Job(JobId),
    /// Every job carrying this group id.
    Group(JobGroupId),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn priority_order() {
        assert!(Priority::Ui.is_more_urgent_than(Priority::Viewport));
        assert!(Priority::Score.is_more_urgent_than(Priority::Export));
        let mut v = vec![Priority::Export, Priority::Ui, Priority::Preview];
        v.sort();
        assert_eq!(v, [Priority::Ui, Priority::Preview, Priority::Export]);
        assert!(Priority::Viewport.is_interactive() && !Priority::Prefetch.is_interactive());
    }

    #[test]
    fn cancellation_propagates_down_not_up() {
        let root = CancellationToken::new();
        let child = root.child();
        let grandchild = child.child();
        child.cancel();
        assert!(!root.is_cancelled());
        assert!(grandchild.is_cancelled());
        assert_eq!(grandchild.check(), Err(EngineError::Cancelled));
        let other = root.child();
        root.cancel();
        assert!(other.is_cancelled());
    }

    /// Minimal inline scheduler proving the traits are object-safe and usable.
    #[derive(Default)]
    struct InlineScheduler {
        next: Mutex<u64>,
        statuses: Mutex<HashMap<JobId, JobStatus>>,
    }

    impl Scheduler for InlineScheduler {
        fn submit(&self, job: Box<dyn Job>, parent: Option<&CancellationToken>) -> JobHandle {
            let id = {
                let mut n = self.next.lock().unwrap();
                *n += 1;
                JobId(*n)
            };
            let token = parent.map(CancellationToken::child).unwrap_or_default();
            let ctx = JobContext::new(id, token.clone(), None);
            let status = match job.run(&ctx) {
                Ok(()) => JobStatus::Succeeded,
                Err(EngineError::Cancelled) => JobStatus::Cancelled,
                Err(error) => JobStatus::Failed { error },
            };
            self.statuses.lock().unwrap().insert(id, status);
            JobHandle {
                id,
                cancellation: token,
            }
        }
        fn reprioritize(&self, _: JobTarget, _: Priority) {}
        fn cancel(&self, _: JobTarget) {}
        fn status(&self, id: JobId) -> JobStatus {
            self.statuses
                .lock()
                .unwrap()
                .get(&id)
                .cloned()
                .unwrap_or(JobStatus::Unknown)
        }
    }

    struct CountTiles(Arc<Mutex<u32>>);
    impl Job for CountTiles {
        fn label(&self) -> &str {
            "count tiles"
        }
        fn priority(&self) -> Priority {
            Priority::Viewport
        }
        fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
            for i in 0..4 {
                ctx.check_cancelled()?;
                *self.0.lock().unwrap() += 1;
                ctx.report_progress(i as f32 / 4.0, None);
            }
            Ok(())
        }
    }

    #[test]
    fn scheduler_runs_and_honours_cancellation() {
        let sched: Box<dyn Scheduler> = Box::new(InlineScheduler::default());
        let count = Arc::new(Mutex::new(0));
        let h = sched.submit(Box::new(CountTiles(count.clone())), None);
        assert_eq!(sched.status(h.id), JobStatus::Succeeded);
        assert_eq!(*count.lock().unwrap(), 4);

        let parent = CancellationToken::new();
        parent.cancel();
        let h2 = sched.submit(Box::new(CountTiles(count.clone())), Some(&parent));
        assert_eq!(sched.status(h2.id), JobStatus::Cancelled);
        assert_eq!(*count.lock().unwrap(), 4);
        assert_eq!(sched.status(JobId(99)), JobStatus::Unknown);
    }
}
