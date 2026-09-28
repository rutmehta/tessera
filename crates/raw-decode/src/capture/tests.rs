//! Task 1 ownership contract tests.
//! Private allocate_stage tests exercise ownership before Task 2 byte copying.
//! Fault injection replaces filesystem operations, never accounting or owners.

use super::*;
use engine_api::EngineError;
use std::{
    fs, io,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

fn limits(bytes: u64, slots: usize) -> CaptureLimits {
    CaptureLimits {
        max_asset_bytes: 64,
        max_staged_bytes: bytes,
        max_live_captures: slots,
    }
}

// Break caught: permitting invalid policy before private storage admission.
#[test]
fn invalid_limits_do_not_create_storage() {
    let parent = tempfile::tempdir().unwrap();
    for policy in [
        CaptureLimits {
            max_asset_bytes: 0,
            ..limits(128, 2)
        },
        CaptureLimits {
            max_staged_bytes: 0,
            ..limits(128, 2)
        },
        limits(128, 0),
        limits(63, 2),
        CaptureLimits {
            max_asset_bytes: u64::MAX,
            max_staged_bytes: u64::MAX,
            max_live_captures: 1,
        },
    ] {
        assert!(matches!(
            CapturePool::create(parent.path(), policy),
            Err(EngineError::InvalidArgument { .. })
        ));
        assert_eq!(fs::read_dir(parent.path()).unwrap().count(), 0);
    }
}

// Break caught: ignoring full-limit byte reservations or slot reservations.
#[test]
fn held_stages_exhaust_each_bound_and_close_releases_once() {
    for policy in [limits(128, 8), limits(512, 2)] {
        let parent = tempfile::tempdir().unwrap();
        let pool = CapturePool::create(parent.path(), policy).unwrap();
        let token = CancellationToken::new();
        let a = pool.allocate_stage("arw", &token).unwrap();
        let b = pool.allocate_stage("arw", &token).unwrap();
        assert!(matches!(
            pool.allocate_stage("arw", &token),
            Err(EngineError::ResourceExhausted { .. })
        ));
        assert_eq!(pool.accounting_for_test(), (128, 2));
        a.close().unwrap();
        assert_eq!(pool.accounting_for_test(), (64, 1));
        let c = pool.allocate_stage("arw", &token).unwrap();
        drop((b, c));
        assert_eq!(pool.accounting_for_test(), (0, 0));
        assert_eq!(fs::read_dir(pool.directory_for_test()).unwrap().count(), 0);
    }
}

// Break caught: cancellation creates a file or leaks its reservation.
#[test]
fn cancelled_stage_admission_has_no_side_effect() {
    let parent = tempfile::tempdir().unwrap();
    let pool = CapturePool::create(parent.path(), limits(128, 2)).unwrap();
    let token = CancellationToken::new();
    token.cancel();
    assert!(matches!(
        pool.allocate_stage("arw", &token),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(pool.accounting_for_test(), (0, 0));
    assert_eq!(fs::read_dir(pool.directory_for_test()).unwrap().count(), 0);
}

// Break caught: a failed stage creation holds quota after the error.
#[test]
fn stage_creation_failure_releases_reservation() {
    let parent = tempfile::tempdir().unwrap();
    let pool = CapturePool::create(parent.path(), limits(64, 1)).unwrap();
    let directory = pool.directory_for_test().to_path_buf();
    fs::remove_dir(&directory).unwrap();
    assert!(matches!(
        pool.allocate_stage("arw", &CancellationToken::new()),
        Err(EngineError::Io { .. })
    ));
    assert_eq!(pool.accounting_for_test(), (0, 0));
    fs::create_dir(&directory).unwrap();
    pool.allocate_stage("arw", &CancellationToken::new())
        .unwrap()
        .close()
        .unwrap();
}

// Break caught: pool drop deletes storage while a held owner still needs it.
#[test]
fn held_stage_keeps_pool_directory_alive_until_close() {
    let parent = tempfile::tempdir().unwrap();
    let pool = CapturePool::create(parent.path(), limits(64, 1)).unwrap();
    let directory = pool.directory_for_test().to_path_buf();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    let path = stage.path_for_test().to_path_buf();
    drop(pool);
    assert!(path.is_file());
    stage.close().unwrap();
    assert!(!path.exists());
    assert!(!directory.exists());
}

// Break caught: already absent stage is quarantined instead of releasing quota.
#[test]
fn absent_stage_cleanup_releases_charge() {
    let parent = tempfile::tempdir().unwrap();
    let pool = CapturePool::create(parent.path(), limits(64, 1)).unwrap();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    fs::remove_file(stage.path_for_test()).unwrap();
    stage.close().unwrap();
    assert_eq!(pool.accounting_for_test(), (0, 0));
}

// Break caught: removal failure drops path, releases charge or retries silently.
// Only remove_file is injected; reservation, staging and cleanup ownership stay real.
#[test]
fn failed_removal_quarantines_until_one_pool_teardown_retry() {
    let parent = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let remove = Arc::new(move |path: &Path| -> io::Result<()> {
        if observed.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected unlink failure",
            ))
        } else {
            fs::remove_file(path)
        }
    });
    let pool =
        CapturePool::create_with_cleanup_for_test(parent.path(), limits(64, 1), remove).unwrap();
    let directory = pool.directory_for_test().to_path_buf();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    let path = stage.path_for_test().to_path_buf();
    assert!(matches!(stage.close(), Err(EngineError::Io { .. })));
    assert!(path.is_file());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(pool.accounting_for_test(), (64, 1));
    assert!(matches!(
        pool.allocate_stage("arw", &CancellationToken::new()),
        Err(EngineError::ResourceExhausted { .. })
    ));
    drop(pool);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!path.exists());
    assert!(!directory.exists());
}

// Break caught: stages or their parent can be read by other local users.
#[cfg(unix)]
#[test]
fn stages_and_directory_have_private_modes() {
    use std::os::unix::fs::PermissionsExt;
    let parent = tempfile::tempdir().unwrap();
    let pool = CapturePool::create(parent.path(), limits(64, 1)).unwrap();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    assert_eq!(
        fs::metadata(pool.directory_for_test())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(stage.path_for_test())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    stage.close().unwrap();
}

// Private test-only operation bundle, to be wired into real stage/accounting code
// after the compile-only observation. No public callbacks or synthetic quota state.
pub(super) type PathOperation = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;
pub(super) struct TestOperations {
    pub(super) setup: PathOperation,
    pub(super) remove_file: PathOperation,
    pub(super) remove_dir: PathOperation,
    pub(super) diagnostics: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Default for TestOperations {
    fn default() -> Self {
        Self {
            setup: Arc::new(|_| Ok(())), // post-permission-setup fault seam only
            remove_file: Arc::new(|path| fs::remove_file(path)),
            remove_dir: Arc::new(|path| fs::remove_dir(path)),
            diagnostics: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }
}

// Break caught: destructor ignores unlink failure, uncharges a retained stage,
// retries invisibly, or panics instead of recording its secondary error.
#[test]
fn failed_stage_drop_retains_charge_and_reports_before_bounded_retry() {
    let parent = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    let operations = TestOperations {
        remove_file: Arc::new(move |path| {
            if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "drop unlink denied",
                ))
            } else {
                fs::remove_file(path)
            }
        }),
        ..TestOperations::default()
    };
    let diagnostics = operations.diagnostics.clone();
    let pool =
        CapturePool::create_with_operations_for_test(parent.path(), limits(64, 1), operations)
            .unwrap();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    let path = stage.path_for_test().to_path_buf();
    drop(stage);
    assert!(path.is_file());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(pool.accounting_for_test(), (64, 1));
    assert!(
        diagnostics
            .lock()
            .unwrap()
            .iter()
            .any(|d| d.contains("drop unlink denied"))
    );
    assert!(matches!(
        pool.allocate_stage("arw", &CancellationToken::new()),
        Err(EngineError::ResourceExhausted { .. })
    ));
    drop(pool);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!path.exists());
}

// Break caught: teardown retries unboundedly or TempDir performs hidden recursive
// cleanup, removing retained captures or unrelated entries after reported failure.
#[test]
fn persistent_unlink_and_nonempty_directory_failures_leave_files_and_diagnostics() {
    let parent = tempfile::tempdir().unwrap();
    let unlinks = Arc::new(AtomicUsize::new(0));
    let removals = Arc::new(AtomicUsize::new(0));
    let observed_unlinks = unlinks.clone();
    let observed_removals = removals.clone();
    let operations = TestOperations {
        remove_file: Arc::new(move |_| {
            observed_unlinks.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "persistent unlink denied",
            ))
        }),
        remove_dir: Arc::new(move |path| {
            observed_removals.fetch_add(1, Ordering::SeqCst);
            fs::remove_dir(path).map_err(|e| io::Error::new(e.kind(), "nonempty pool retained"))
        }),
        ..TestOperations::default()
    };
    let diagnostics = operations.diagnostics.clone();
    let pool =
        CapturePool::create_with_operations_for_test(parent.path(), limits(64, 1), operations)
            .unwrap();
    let directory = pool.directory_for_test().to_path_buf();
    let sentinel = directory.join("unrelated-sentinel");
    fs::write(&sentinel, b"must survive nonrecursive teardown").unwrap();
    let stage = pool
        .allocate_stage("arw", &CancellationToken::new())
        .unwrap();
    let path = stage.path_for_test().to_path_buf();
    drop(stage);
    assert_eq!(pool.accounting_for_test(), (64, 1));
    assert_eq!(unlinks.load(Ordering::SeqCst), 1);
    drop(pool);
    assert_eq!(unlinks.load(Ordering::SeqCst), 2);
    assert_eq!(removals.load(Ordering::SeqCst), 1);
    assert!(path.is_file());
    assert_eq!(
        fs::read(&sentinel).unwrap(),
        b"must survive nonrecursive teardown"
    );
    let messages = diagnostics.lock().unwrap();
    assert_eq!(
        messages
            .iter()
            .filter(|d| d.contains("persistent unlink denied"))
            .count(),
        2
    );
    assert_eq!(
        messages
            .iter()
            .filter(|d| d.contains("nonempty pool retained"))
            .count(),
        1
    );
}

// Break caught: competing callers check capacity outside the accounting lock,
// allowing two owners through a single slot/64-byte reservation budget.
#[test]
fn concurrent_admission_holds_exactly_one_winner_until_both_results_observed() {
    use std::sync::{Barrier, mpsc};
    use std::time::Duration;
    let parent = tempfile::tempdir().unwrap();
    let pool = Arc::new(CapturePool::create(parent.path(), limits(64, 1)).unwrap());
    let start = Arc::new(Barrier::new(3));
    let (results_tx, results_rx) = mpsc::channel();
    let mut releases = Vec::new();
    let mut workers = Vec::new();
    for _ in 0..2 {
        let pool = pool.clone();
        let start = start.clone();
        let result = results_tx.clone();
        let (release_tx, release_rx) = mpsc::channel();
        releases.push(release_tx);
        workers.push(std::thread::spawn(move || {
            start.wait();
            let admitted = pool.allocate_stage("arw", &CancellationToken::new());
            result
                .send(match &admitted {
                    Ok(_) => Ok(true),
                    Err(EngineError::ResourceExhausted { .. }) => Ok(false),
                    Err(error) => Err(error.clone()),
                })
                .unwrap();
            // Retain both outcome and any winner through observation; no sleeps.
            let _ = release_rx.recv_timeout(Duration::from_secs(5));
            drop(admitted);
        }));
    }
    start.wait();
    let outcomes = (0..2)
        .map(|_| results_rx.recv_timeout(Duration::from_secs(5)))
        .collect::<Vec<_>>();
    let held = pool.accounting_for_test();
    for release in releases {
        let _ = release.send(());
    }
    for worker in workers {
        worker.join().unwrap();
    }
    let winners = outcomes
        .into_iter()
        .map(|r| r.unwrap().unwrap())
        .filter(|won| *won)
        .count();
    assert_eq!(winners, 1);
    assert_eq!(held, (64, 1));
    assert_eq!(pool.accounting_for_test(), (0, 0));
    assert_eq!(fs::read_dir(pool.directory_for_test()).unwrap().count(), 0);
}

// Break caught: permission/setup failure abandons an unowned directory, overwrites
// the primary error, or causes a hidden TempDir deletion after cleanup failure.
#[test]
fn initial_setup_failure_cleans_once_and_reports_secondary_failure() {
    for fail_cleanup in [false, true] {
        let parent = tempfile::tempdir().unwrap();
        let removals = Arc::new(AtomicUsize::new(0));
        let observed = removals.clone();
        let operations = TestOperations {
            setup: Arc::new(|_| {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "private permission setup denied",
                ))
            }),
            remove_dir: Arc::new(move |path| {
                observed.fetch_add(1, Ordering::SeqCst);
                if fail_cleanup {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "setup cleanup denied",
                    ))
                } else {
                    fs::remove_dir(path)
                }
            }),
            ..TestOperations::default()
        };
        let diagnostics = operations.diagnostics.clone();
        let result =
            CapturePool::create_with_operations_for_test(parent.path(), limits(64, 1), operations);
        assert!(matches!(result, Err(EngineError::Io { ref message, .. })
            if message.contains("private permission setup denied")));
        assert_eq!(removals.load(Ordering::SeqCst), 1);
        let entries = fs::read_dir(parent.path())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), usize::from(fail_cleanup));
        let messages = diagnostics.lock().unwrap();
        assert_eq!(
            messages
                .iter()
                .filter(|d| d.contains("setup cleanup denied"))
                .count(),
            usize::from(fail_cleanup)
        );
    }
}

#[cfg(unix)]
pub(super) mod stream;
