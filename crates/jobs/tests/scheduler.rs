use engine_api::error::{EngineError, EngineResult};
use engine_api::id::{JobGroupId, JobId};
use engine_api::jobs::{Job, JobContext, JobStatus, JobTarget, Priority, Scheduler};
use jobs::{JobGroup, ThreadPoolScheduler};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[test]
fn blocking_run_returns_result_on_calling_thread() {
    let caller = thread::current().id();
    assert_eq!(
        jobs::blocking_run(task(move |ctx| {
            assert_eq!(thread::current().id(), caller);
            ctx.check_cancelled()
        })),
        Ok(())
    );
    assert_eq!(
        jobs::blocking_run(task(|_| Err(EngineError::Cancelled))),
        Err(EngineError::Cancelled)
    );
    let error = EngineError::invalid("tile", "invalid size");
    let returned = error.clone();
    assert_eq!(jobs::blocking_run(task(move |_| Err(returned))), Err(error));
}

#[test]
fn cancelled_queued_jobs_never_start() {
    use engine_api::jobs::CancellationToken;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let pool = ThreadPoolScheduler::new(1);
    let release = occupy(&pool);
    let parent = CancellationToken::new();
    let uncancelled_parent = CancellationToken::new();
    let count = Arc::new(AtomicUsize::new(0));
    let mut handles = Vec::new();
    for index in 0..4 {
        let count = count.clone();
        handles.push(pool.submit(
            task(move |_| {
                count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }),
            Some(if index < 2 {
                &uncancelled_parent
            } else {
                &parent
            }),
        ));
    }
    handles[0].cancel();
    pool.cancel(JobTarget::Job(handles[1].id));
    assert!(!uncancelled_parent.is_cancelled());
    assert!(!parent.is_cancelled());
    parent.cancel();
    // Also cover a parent already cancelled at submission.
    let count_job = count.clone();
    handles.push(pool.submit(
        task(move |_| {
            count_job.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }),
        Some(&parent),
    ));
    for handle in &handles {
        assert_eq!(pool.status(handle.id), JobStatus::Cancelled);
    }
    let fence = pool.submit(task(|_| Ok(())), None);
    release.send(()).unwrap();
    assert_eq!(wait(&pool, fence.id), JobStatus::Succeeded);
    assert_eq!(count.load(Ordering::SeqCst), 0);
    for handle in handles {
        assert_eq!(pool.status(handle.id), JobStatus::Cancelled);
    }
}

#[test]
fn reprioritize_job_and_group_preserves_original_fifo() {
    let pool = ThreadPoolScheduler::new(1);
    let release = occupy(&pool);
    let group = JobGroup::new(JobGroupId(7));
    let (tx, rx) = mpsc::channel();
    let mut ids = Vec::new();
    for n in 0..5 {
        let tx = tx.clone();
        let job = Box::new(Task(Priority::Export, move |_: &JobContext| {
            tx.send(n).unwrap();
            Ok(())
        }));
        let h = if n == 1 || n == 3 {
            group.submit(&pool, job)
        } else {
            pool.submit(job, None)
        };
        ids.push(h.id);
    }
    pool.reprioritize(JobTarget::Job(ids[4]), Priority::Ui);
    pool.reprioritize(JobTarget::Group(group.id()), Priority::Ui);
    pool.reprioritize(JobTarget::Job(ids[3]), Priority::Export);
    pool.reprioritize(JobTarget::Job(ids[3]), Priority::Ui);
    pool.reprioritize(JobTarget::Job(JobId(u64::MAX)), Priority::Ui);
    release.send(()).unwrap();
    let order: Vec<_> = (0..5)
        .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap())
        .collect();
    assert_eq!(order, [1, 3, 4, 0, 2]);
    for id in ids {
        assert_eq!(wait(&pool, id), JobStatus::Succeeded);
    }
}

#[test]
fn scheduler_group_cancel_is_isolated() {
    let pool = ThreadPoolScheduler::new(1);
    let release = occupy(&pool);
    let group = JobGroup::new(JobGroupId(8));
    let other = JobGroup::new(JobGroupId(9));
    let a = group.submit(&pool, task(|_| panic!("cancelled job started")));
    let b = group.submit(&pool, task(|_| panic!("cancelled job started")));
    let c = other.submit(&pool, task(|_| Ok(())));
    pool.cancel(JobTarget::Group(group.id()));
    pool.cancel(JobTarget::Job(JobId(u64::MAX)));
    release.send(()).unwrap();
    assert_eq!(wait(&pool, c.id), JobStatus::Succeeded);
    for h in [a, b] {
        assert_eq!(pool.status(h.id), JobStatus::Cancelled);
        assert!(h.cancellation.is_cancelled());
    }
}

#[test]
fn running_cancel_reports_progress_and_reaches_descendants() {
    let pool = ThreadPoolScheduler::new(1);
    let parent = engine_api::jobs::CancellationToken::new();
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let h = pool.submit(
        task(move |ctx| {
            ctx.report_progress(0.75, Some("tile"));
            let child = ctx.cancellation.child();
            started.send(child).unwrap();
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            ctx.check_cancelled()
        }),
        Some(&parent),
    );
    let child = ready.recv_timeout(Duration::from_secs(10)).unwrap();
    assert_eq!(pool.status(h.id), JobStatus::Running { progress: 0.75 });
    pool.cancel(JobTarget::Job(h.id));
    assert!(child.is_cancelled());
    assert!(!parent.is_cancelled());
    release.send(()).unwrap();
    assert_eq!(wait(&pool, h.id), JobStatus::Cancelled);
}

#[test]
fn errors_and_panics_do_not_kill_worker() {
    let pool = ThreadPoolScheduler::new(1);
    let error = EngineError::invalid("job", "bad input");
    let returned = error.clone();
    let failed = pool.submit(task(move |_| Err(returned)), None);
    let panicked = pool.submit(task(|_| panic!("intentional panic")), None);
    let success = pool.submit(task(|_| Ok(())), None);
    assert_eq!(wait(&pool, success.id), JobStatus::Succeeded);
    assert_eq!(pool.status(failed.id), JobStatus::Failed { error });
    assert!(matches!(
        pool.status(panicked.id),
        JobStatus::Failed {
            error: EngineError::Internal { .. }
        }
    ));
    pool.cancel(JobTarget::Job(success.id));
    pool.reprioritize(JobTarget::Job(success.id), Priority::Export);
    assert_eq!(pool.status(success.id), JobStatus::Succeeded);
}

#[test]
fn default_uses_physical_cores() {
    assert_eq!(
        ThreadPoolScheduler::default().worker_count(),
        num_cpus::get_physical().max(1)
    );
}

#[test]
fn four_workers_pick_ready_ui_before_export() {
    let pool = ThreadPoolScheduler::new(4);
    let occupied: Vec<_> = (0..4).map(|_| occupy(&pool)).collect();
    let (tx, rx) = mpsc::channel();
    let export_tx = tx.clone();
    let export = pool.submit(
        Box::new(Task(Priority::Export, move |_: &JobContext| {
            export_tx.send(false).unwrap();
            Ok(())
        })),
        None,
    );
    let mut releases = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..4 {
        let tx = tx.clone();
        let (release, blocked) = mpsc::channel();
        releases.push(release);
        ids.push(
            pool.submit(
                task(move |_| {
                    tx.send(true).unwrap();
                    blocked.recv_timeout(Duration::from_secs(10)).unwrap();
                    Ok(())
                }),
                None,
            )
            .id,
        );
    }
    for release in occupied {
        release.send(()).unwrap();
    }
    for _ in 0..4 {
        assert!(rx.recv_timeout(Duration::from_secs(10)).unwrap());
    }
    assert_eq!(pool.status(export.id), JobStatus::Queued);
    for release in releases {
        release.send(()).unwrap();
    }
    assert_eq!(wait(&pool, export.id), JobStatus::Succeeded);
    for id in ids {
        assert_eq!(wait(&pool, id), JobStatus::Succeeded);
    }
}

#[test]
fn last_pool_owner_can_be_dropped_inside_job() {
    use std::sync::Arc;
    let pool = Arc::new(ThreadPoolScheduler::new(2));
    let owner = pool.clone();
    let (release, blocked) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    pool.submit(
        task(move |_| {
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            drop(owner);
            done.send(()).unwrap();
            Ok(())
        }),
        None,
    );
    drop(pool);
    release.send(()).unwrap();
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
}

#[test]
#[should_panic(expected = "at least one worker")]
fn zero_workers_rejected() {
    ThreadPoolScheduler::new(0);
}

#[test]
fn ten_thousand_tiny_jobs_complete() {
    run_batch(10_000);
}

fn run_batch(n: usize) {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    let pool = ThreadPoolScheduler::new(4);
    let count = Arc::new(AtomicUsize::new(0));
    let ids: Vec<_> = (0..n)
        .map(|_| {
            let count = count.clone();
            pool.submit(
                task(move |_| {
                    count.fetch_add(1, Ordering::Relaxed);
                    Ok(())
                }),
                None,
            )
            .id
        })
        .collect();
    for id in ids {
        assert_eq!(wait(&pool, id), JobStatus::Succeeded);
    }
    assert_eq!(count.load(Ordering::Relaxed), n);
}

#[test]
fn random_submit_cancel_from_four_threads() {
    use std::sync::{Arc, Barrier};
    let pool = Arc::new(ThreadPoolScheduler::new(4));
    let barrier = Arc::new(Barrier::new(4));
    let (tx, rx) = mpsc::channel();
    let threads: Vec<_> = (1..=4)
        .map(|seed| {
            let pool = pool.clone();
            let barrier = barrier.clone();
            let tx = tx.clone();
            thread::spawn(move || {
                let mut rng = seed as u64;
                let mut ids = Vec::new();
                barrier.wait();
                for _ in 0..1_000 {
                    // Deterministic xorshift keeps failures reproducible without rand.
                    rng ^= rng << 13;
                    rng ^= rng >> 7;
                    rng ^= rng << 17;
                    let h = pool.submit(
                        Box::new(Task(
                            Priority::ALL[(rng % 6) as usize],
                            |ctx: &JobContext| {
                                for _ in 0..3 {
                                    ctx.check_cancelled()?;
                                    thread::yield_now();
                                }
                                Ok(())
                            },
                        )),
                        None,
                    );
                    ids.push(h.id);
                    match rng % 4 {
                        0 => h.cancel(),
                        1 => pool.cancel(JobTarget::Job(ids[(rng as usize) % ids.len()])),
                        2 => pool.reprioritize(JobTarget::Job(h.id), Priority::Ui),
                        _ => (),
                    }
                }
                tx.send(ids).unwrap();
            })
        })
        .collect();
    let mut ids = Vec::new();
    for _ in 0..4 {
        ids.extend(rx.recv_timeout(Duration::from_secs(30)).unwrap());
    }
    for producer in threads {
        producer.join().unwrap();
    }
    let mut unique = std::collections::HashSet::new();
    for id in ids {
        assert!(unique.insert(id));
        assert!(matches!(
            wait(&pool, id),
            JobStatus::Succeeded | JobStatus::Cancelled
        ));
    }
    assert_eq!(unique.len(), 4_000);
}

#[test]
fn dropping_pool_cancels_running_and_queued_work() {
    let pool = ThreadPoolScheduler::new(1);
    let (started, ready) = mpsc::channel();
    let (done, finished) = mpsc::channel();
    let running = pool.submit(
        task(move |ctx| {
            started.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            while !ctx.cancellation.is_cancelled() {
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
            done.send(()).unwrap();
            ctx.check_cancelled()
        }),
        None,
    );
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    let (started_queued, queued_result) = mpsc::channel();
    let queued = pool.submit(
        task(move |_| {
            started_queued.send(()).unwrap();
            Ok(())
        }),
        None,
    );
    drop(pool);
    finished.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(running.cancellation.is_cancelled());
    assert!(queued.cancellation.is_cancelled());
    assert!(matches!(
        queued_result.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
}

#[test]
#[ignore = "manual throughput benchmark: 100k no-op jobs"]
fn throughput_100k_noop_jobs() {
    let pool = ThreadPoolScheduler::default();
    let start = Instant::now();
    let ids: Vec<_> = (0..100_000)
        .map(|_| pool.submit(task(|_| Ok(())), None).id)
        .collect();
    for id in ids {
        assert_eq!(wait(&pool, id), JobStatus::Succeeded);
    }
    let elapsed = start.elapsed();
    eprintln!(
        "100000 no-op jobs on {} workers: {elapsed:?}, {:.0} jobs/s",
        pool.worker_count(),
        100_000.0 / elapsed.as_secs_f64()
    );
}

struct Task<F>(Priority, F);
impl<F: FnOnce(&JobContext) -> EngineResult<()> + Send> Job for Task<F> {
    fn label(&self) -> &str {
        "test"
    }
    fn priority(&self) -> Priority {
        self.0
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        (self.1)(ctx)
    }
}
fn task(f: impl FnOnce(&JobContext) -> EngineResult<()> + Send + 'static) -> Box<dyn Job> {
    Box::new(Task(Priority::Ui, f))
}
fn wait(pool: &ThreadPoolScheduler, id: JobId) -> JobStatus {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = pool.status(id);
        if !matches!(status, JobStatus::Queued | JobStatus::Running { .. }) {
            return status;
        }
        assert!(Instant::now() < deadline, "job {id} timed out: {status:?}");
        thread::sleep(Duration::from_millis(1));
    }
}
// Occupy the only worker before submitting the jobs whose order we assert.
fn occupy(pool: &ThreadPoolScheduler) -> mpsc::Sender<()> {
    let (started, ready) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    pool.submit(
        task(move |_| {
            started.send(()).unwrap();
            blocked.recv_timeout(Duration::from_secs(10)).unwrap();
            Ok(())
        }),
        None,
    );
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    release
}
#[test]
fn priority_then_fifo() {
    let pool = ThreadPoolScheduler::new(1);
    let release = occupy(&pool);
    let (tx, rx) = mpsc::channel();
    let mut ids = Vec::new();
    for (n, priority) in [
        Priority::Export,
        Priority::Ui,
        Priority::Viewport,
        Priority::Ui,
        Priority::Export,
    ]
    .into_iter()
    .enumerate()
    {
        let tx = tx.clone();
        ids.push(
            pool.submit(
                Box::new(Task(priority, move |_: &JobContext| {
                    tx.send(n).unwrap();
                    Ok(())
                })),
                None,
            )
            .id,
        );
    }
    release.send(()).unwrap();
    let order: Vec<_> = (0..ids.len())
        .map(|_| rx.recv_timeout(Duration::from_secs(10)).unwrap())
        .collect();
    assert_eq!(order, [1, 3, 2, 0, 4]);
    for id in ids {
        assert_eq!(wait(&pool, id), JobStatus::Succeeded);
    }
    assert_eq!(pool.status(JobId(u64::MAX)), JobStatus::Unknown);
}

#[test]
fn group_cancel_reaches_running_queued_and_future_jobs() {
    let pool = ThreadPoolScheduler::new(1);
    let group = JobGroup::new(JobGroupId(42));
    let (started, ready) = mpsc::channel();
    let running = group.submit(
        &pool,
        task(move |ctx| {
            started.send(()).unwrap();
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                ctx.check_cancelled()?;
                assert!(Instant::now() < deadline);
                thread::yield_now();
            }
        }),
    );
    ready.recv_timeout(Duration::from_secs(10)).unwrap();
    let queued = group.submit(&pool, task(|_| panic!("cancelled job started")));
    group.cancel();
    let future = group.submit(&pool, task(|_| panic!("cancelled group restarted")));
    for handle in [running, queued, future] {
        assert_eq!(wait(&pool, handle.id), JobStatus::Cancelled);
        assert!(handle.cancellation.is_cancelled());
    }
    assert_eq!(
        wait(&pool, pool.submit(task(|_| Ok(())), None).id),
        JobStatus::Succeeded
    );
}
