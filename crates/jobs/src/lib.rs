//! Cooperative, priority-ordered background work (spec 08 §2).
//!
//! Dequeue and transition to Running are atomic under one lock: a worker never
//! picks a less urgent job while a more urgent job is ready. Equal priorities
//! use submission order, including after reprioritization. Running jobs are not
//! pre-empted. Long jobs must poll cancellation and yield per tile; to yield a
//! worker to higher-priority work, split work into separately submitted tiles.
//! Never synchronously wait for lower-priority work on this pool.

use engine_api::error::{EngineError, EngineResult};
use engine_api::id::{JobGroupId, JobId};
use engine_api::jobs::{
    CancellationToken, Job, JobContext, JobHandle, JobStatus, JobTarget, Priority, ProgressSink,
    Scheduler,
};
use std::collections::{BTreeSet, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};

/// Runs inline for tests/CLI without creating a worker pool. The context has
/// reserved id zero, a fresh token and no progress sink. Returns the job's error
/// unchanged; unlike the pool, panics propagate to the caller.
pub fn blocking_run(job: Box<dyn Job>) -> EngineResult<()> {
    job.run(&JobContext::new(JobId(0), CancellationToken::new(), None))
}

struct Record {
    priority: Priority,
    group: Option<JobGroupId>,
    token: CancellationToken,
    status: JobStatus,
    job: Option<Box<dyn Job>>,
}

#[derive(Default)]
struct State {
    next: u64,
    ready: BTreeSet<(Priority, JobId)>,
    records: HashMap<JobId, Record>,
    shutdown: bool,
}

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    wake: Condvar,
}

/// Fixed-size worker pool. Completed statuses are retained for its lifetime.
/// Dropping the pool cancels queued/running jobs and joins workers. Thus running
/// jobs must cooperate with cancellation. If dropped from one of its own jobs,
/// workers are detached instead to avoid self/cross-worker join deadlocks.
pub struct ThreadPoolScheduler {
    shared: Arc<Shared>,
    workers: Vec<JoinHandle<()>>,
}

impl ThreadPoolScheduler {
    /// Starts `worker_count` workers. Panics for zero or thread creation failure.
    pub fn new(worker_count: usize) -> Self {
        assert!(worker_count > 0, "a scheduler needs at least one worker");
        let mut pool = Self {
            shared: Arc::new(Shared::default()),
            workers: Vec::with_capacity(worker_count),
        };
        for index in 0..worker_count {
            let shared = pool.shared.clone();
            pool.workers.push(
                thread::Builder::new()
                    .name(format!("jobs-{index}"))
                    .spawn(move || worker(shared))
                    .expect("could not spawn job worker"),
            );
        }
        pool
    }

    /// Number of configured workers.
    pub fn worker_count(&self) -> usize {
        self.workers.len()
    }
}

impl Default for ThreadPoolScheduler {
    fn default() -> Self {
        Self::new(num_cpus::get_physical().max(1))
    }
}

impl Scheduler for ThreadPoolScheduler {
    fn submit(&self, job: Box<dyn Job>, parent: Option<&CancellationToken>) -> JobHandle {
        // Call user code outside the scheduler lock.
        let priority = job.priority();
        let group = job.group();
        let token = parent.map(CancellationToken::child).unwrap_or_default();
        let mut state = self.shared.state.lock().unwrap();
        state.next = state.next.checked_add(1).expect("job id space exhausted");
        let id = JobId(state.next);
        state.ready.insert((priority, id));
        state.records.insert(
            id,
            Record {
                priority,
                group,
                token: token.clone(),
                status: JobStatus::Queued,
                job: Some(job),
            },
        );
        drop(state);
        self.shared.wake.notify_one();
        JobHandle {
            id,
            cancellation: token,
        }
    }

    fn reprioritize(&self, target: JobTarget, priority: Priority) {
        let mut state = self.shared.state.lock().unwrap();
        let State { records, ready, .. } = &mut *state;
        for (&id, record) in records.iter_mut() {
            if matches_target(target, id, record) && record.status == JobStatus::Queued {
                ready.remove(&(record.priority, id));
                record.priority = priority;
                ready.insert((priority, id));
            }
        }
    }

    fn cancel(&self, target: JobTarget) {
        let state = self.shared.state.lock().unwrap();
        for (&id, record) in &state.records {
            if matches_target(target, id, record) {
                record.token.cancel();
            }
        }
        drop(state);
        self.shared.wake.notify_all();
    }

    fn status(&self, id: JobId) -> JobStatus {
        let state = self.shared.state.lock().unwrap();
        match state.records.get(&id) {
            Some(record) if record.status == JobStatus::Queued && record.token.is_cancelled() => {
                JobStatus::Cancelled
            }
            Some(record) => record.status.clone(),
            None => JobStatus::Unknown,
        }
    }
}

/// A reusable batch handle, for example prefetching a view's neighbours.
/// Choose a group id unique within the scheduler. Clones share cancellation;
/// dropping a handle does not cancel. `cancel()` also cancels future submissions
/// through this handle. Scheduler `cancel(Group(id))` affects existing jobs only.
#[derive(Clone, Debug)]
pub struct JobGroup {
    id: JobGroupId,
    cancellation: CancellationToken,
}

impl JobGroup {
    /// Creates a batch with a caller-assigned identity.
    pub fn new(id: JobGroupId) -> Self {
        Self {
            id,
            cancellation: CancellationToken::new(),
        }
    }

    /// Identity usable with scheduler group operations.
    pub fn id(&self) -> JobGroupId {
        self.id
    }

    /// Submits work with this batch's group id and parent cancellation token.
    /// This deliberately overrides the wrapped job's original group id.
    pub fn submit(&self, scheduler: &dyn Scheduler, job: Box<dyn Job>) -> JobHandle {
        scheduler.submit(
            Box::new(GroupedJob { id: self.id, job }),
            Some(&self.cancellation),
        )
    }

    /// Cancels all work submitted through this handle or any of its clones.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
}

struct GroupedJob {
    id: JobGroupId,
    job: Box<dyn Job>,
}

impl Job for GroupedJob {
    fn label(&self) -> &str {
        self.job.label()
    }
    fn priority(&self) -> Priority {
        self.job.priority()
    }
    fn group(&self) -> Option<JobGroupId> {
        Some(self.id)
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        self.job.run(ctx)
    }
}

fn matches_target(target: JobTarget, id: JobId, record: &Record) -> bool {
    match target {
        JobTarget::Job(wanted) => id == wanted,
        JobTarget::Group(group) => record.group == Some(group),
    }
}

struct Progress {
    shared: Arc<Shared>,
    id: JobId,
}

impl ProgressSink for Progress {
    fn report(&self, fraction: f32, _: Option<&str>) {
        let mut state = self.shared.state.lock().unwrap();
        if let Some(Record {
            status: JobStatus::Running { progress },
            ..
        }) = state.records.get_mut(&self.id)
        {
            *progress = if fraction.is_finite() {
                fraction.clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
    }
}

fn worker(shared: Arc<Shared>) {
    loop {
        let (id, job, token, cancelled) = {
            let mut state = shared.state.lock().unwrap();
            while state.ready.is_empty() && !state.shutdown {
                state = shared.wake.wait(state).unwrap();
            }
            if state.shutdown {
                return;
            }
            let (_, id) = state.ready.pop_first().unwrap();
            let record = state.records.get_mut(&id).unwrap();
            let cancelled = record.token.is_cancelled();
            record.status = if cancelled {
                JobStatus::Cancelled
            } else {
                JobStatus::Running { progress: 0.0 }
            };
            (
                id,
                record.job.take().unwrap(),
                record.token.clone(),
                cancelled,
            )
        };
        // Never execute (or drop) a user job while holding the scheduler lock.
        let result = catch_unwind(AssertUnwindSafe(|| {
            if cancelled {
                drop(job);
                return Err(EngineError::Cancelled);
            }
            let ctx = JobContext::new(
                id,
                token,
                Some(Arc::new(Progress {
                    shared: shared.clone(),
                    id,
                })),
            );
            ctx.check_cancelled()?;
            job.run(&ctx)
        }))
        .unwrap_or_else(|_| Err(EngineError::internal("job panicked")));
        shared
            .state
            .lock()
            .unwrap()
            .records
            .get_mut(&id)
            .unwrap()
            .status = match result {
            Ok(()) => JobStatus::Succeeded,
            Err(EngineError::Cancelled) => JobStatus::Cancelled,
            Err(error) => JobStatus::Failed { error },
        };
    }
}

impl Drop for ThreadPoolScheduler {
    fn drop(&mut self) {
        let pending = {
            let mut state = self.shared.state.lock().unwrap();
            state.shutdown = true;
            state.ready.clear();
            let mut pending = Vec::new();
            for record in state.records.values_mut() {
                record.token.cancel();
                if let Some(job) = record.job.take() {
                    record.status = JobStatus::Cancelled;
                    pending.push(job);
                }
            }
            pending
        };
        self.shared.wake.notify_all();
        if !self
            .workers
            .iter()
            .any(|worker| worker.thread().id() == thread::current().id())
        {
            for worker in self.workers.drain(..) {
                let _ = worker.join();
            }
        }
        drop(pending);
    }
}
