//! Process-wide interactive pressure (own test binary: the count is global).
use engine_api::error::EngineResult;
use engine_api::jobs::{
    CancellationToken, Job, JobContext, JobStatus, JobTarget, Priority, Scheduler,
};
use jobs::ThreadPoolScheduler;
use std::sync::mpsc;
use std::time::{Duration, Instant};

struct Task<F>(Priority, F);
impl<F: FnOnce(&JobContext) -> EngineResult<()> + Send + 'static> Job for Task<F> {
    fn label(&self) -> &str {
        "pressure test"
    }
    fn priority(&self) -> Priority {
        self.0
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        (self.1)(ctx)
    }
}

fn wait_done(pool: &ThreadPoolScheduler, id: engine_api::id::JobId) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while matches!(
        pool.status(id),
        JobStatus::Queued | JobStatus::Running { .. }
    ) {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn bulk_work_yields_to_queued_and_running_interactive_jobs() {
    let pool = ThreadPoolScheduler::new(1);
    let quiet = Duration::from_millis(30);
    let cancel = CancellationToken::new();
    assert_eq!(jobs::interactive_pending(), 0);
    // Idle: no wait at all.
    assert!(
        jobs::yield_to_interactive(&cancel, Duration::ZERO, Duration::from_secs(5)).unwrap()
            < Duration::from_millis(20)
    );

    // A running viewport job holds bulk work until it ends, plus the quiet period.
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel::<()>();
    let running = pool.submit(
        Box::new(Task(Priority::Viewport, move |_: &JobContext| {
            started.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        })),
        None,
    );
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    // A queued Ui job behind it counts too; Export jobs never do.
    let queued = pool.submit(Box::new(Task(Priority::Ui, |_: &JobContext| Ok(()))), None);
    let export = pool.submit(
        Box::new(Task(Priority::Export, |_: &JobContext| Ok(()))),
        None,
    );
    assert_eq!(jobs::interactive_pending(), 2);
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(150));
        release.send(()).unwrap();
    });
    let waited = jobs::yield_to_interactive(&cancel, quiet, Duration::from_secs(5)).unwrap();
    assert!(waited >= Duration::from_millis(150) + quiet, "{waited:?}");
    releaser.join().unwrap();
    for id in [running.id, queued.id, export.id] {
        wait_done(&pool, id);
    }
    assert_eq!(jobs::interactive_pending(), 0);

    // Bounded: a never-idle viewport cannot stall bulk work past max_wait.
    let (release, blocked) = mpsc::channel::<()>();
    let busy = pool.submit(
        Box::new(Task(Priority::Viewport, move |_: &JobContext| {
            let _ = blocked.recv_timeout(Duration::from_secs(10));
            Ok(())
        })),
        None,
    );
    let waited = jobs::yield_to_interactive(&cancel, quiet, Duration::from_millis(100)).unwrap();
    assert!(waited >= Duration::from_millis(100) && waited < Duration::from_secs(2));
    // Cancellation ends a wait promptly.
    cancel.cancel();
    assert!(jobs::yield_to_interactive(&cancel, quiet, Duration::from_secs(5)).is_err());
    release.send(()).unwrap();
    wait_done(&pool, busy.id);

    // Cancelled queued jobs and reprioritized jobs keep the count exact.
    let pool = ThreadPoolScheduler::new(1);
    let (release, blocked) = mpsc::channel::<()>();
    let (started, ready) = mpsc::channel();
    let hold = pool.submit(
        Box::new(Task(Priority::Export, move |_: &JobContext| {
            started.send(()).unwrap();
            let _ = blocked.recv_timeout(Duration::from_secs(10));
            Ok(())
        })),
        None,
    );
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    let a = pool.submit(
        Box::new(Task(Priority::Viewport, |_: &JobContext| Ok(()))),
        None,
    );
    let b = pool.submit(
        Box::new(Task(Priority::Prefetch, |_: &JobContext| Ok(()))),
        None,
    );
    assert_eq!(jobs::interactive_pending(), 1);
    pool.reprioritize(JobTarget::Job(a.id), Priority::Prefetch);
    pool.reprioritize(JobTarget::Job(b.id), Priority::Ui);
    assert_eq!(jobs::interactive_pending(), 1);
    pool.cancel(JobTarget::Job(b.id));
    release.send(()).unwrap();
    for id in [hold.id, a.id, b.id] {
        wait_done(&pool, id);
    }
    assert_eq!(jobs::interactive_pending(), 0);
    // Dropping a pool with queued interactive work releases its count.
    let pool = ThreadPoolScheduler::new(1);
    let (release, blocked) = mpsc::channel::<()>();
    pool.submit(
        Box::new(Task(Priority::Export, move |_: &JobContext| {
            let _ = blocked.recv_timeout(Duration::from_millis(200));
            Ok(())
        })),
        None,
    );
    pool.submit(Box::new(Task(Priority::Ui, |_: &JobContext| Ok(()))), None);
    drop(release);
    drop(pool);
    assert_eq!(jobs::interactive_pending(), 0);
}
