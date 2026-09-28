//! Task3 tests model a controlled consumer only; no RAW decoder is invoked.
use super::*;
use std::{path::PathBuf, sync::mpsc, time::Duration};

fn capture(pool: &CapturePool, source: &Path) -> CapturedRaw {
    pool.capture(
        source,
        PinnedRawDecoderRoute::LibRawCfaV1,
        "ARW",
        None,
        &CancellationToken::new(),
    )
    .unwrap()
}

// Break caught: consumer reads the original locator rather than the held stage,
// or route/suffix are rediscovered from the replaced original basename.
#[test]
fn replacement_original_does_not_redirect_any_consumer_read() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("original.bin");
    fs::write(&source, b"captured A").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let owner = capture(&pool, &source);
    let stage = owner.path_for_test().to_path_buf();
    let identity = owner.identity();
    fs::rename(&source, root.path().join("prior.bin")).unwrap();
    fs::write(&source, b"replacement B").unwrap();
    let value = owner
        .consume_for_test(&CancellationToken::new(), |path, route| {
            assert_ne!(path, source);
            assert_eq!(path.extension().unwrap(), "arw");
            assert_eq!(route, PinnedRawDecoderRoute::LibRawCfaV1);
            assert_eq!(fs::read(path)?, b"captured A");
            let bytes = fs::read(path)?;
            assert_eq!(Digest::derive(ASSET_DOMAIN, &bytes), identity.digest);
            Ok(bytes)
        })
        .unwrap();
    assert_eq!(value, b"captured A");
    assert!(!stage.exists());
    assert_eq!(pool.accounting_for_test(), (0, 0));
    assert_eq!(fs::read(&source).unwrap(), b"replacement B");
}

// Break caught: the owner releases its path/quota before delayed reads finish.
#[test]
fn delayed_consumer_retains_stage_and_quota_through_last_read() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"delayed read").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let owner = capture(&pool, &source);
    let stage = owner.path_for_test().to_path_buf();
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        owner.consume_for_test(&CancellationToken::new(), |path, _| {
            assert_eq!(fs::read(path)?, b"delayed read");
            entered_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            fs::read(path).map_err(EngineError::from)
        })
    });
    let entered = entered_rx.recv_timeout(Duration::from_secs(5));
    let held = pool.accounting_for_test();
    let present = stage.is_file();
    let occupied = pool.capture(
        &source,
        PinnedRawDecoderRoute::LibRawCfaV1,
        "arw",
        None,
        &CancellationToken::new(),
    );
    let _ = release_tx.send(());
    let result = worker.join().unwrap();
    entered.unwrap();
    assert_eq!(held, (64, 1));
    assert!(present);
    assert!(matches!(
        occupied,
        Err(EngineError::ResourceExhausted { .. })
    ));
    assert_eq!(result.unwrap(), b"delayed read");
    assert!(!stage.exists());
    assert_eq!(pool.accounting_for_test(), (0, 0));
}

// Break caught: panic bypasses RAII ownership or leaks the charged stage.
#[test]
fn panicking_consumer_unwinds_and_cleans_held_stage() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"panic control").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let owner = capture(&pool, &source);
    let stage = owner.path_for_test().to_path_buf();
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owner.consume_for_test::<()>(&CancellationToken::new(), |path, _| {
            assert_eq!(fs::read(path)?, b"panic control");
            panic!("injected consumer panic");
        })
    }));
    assert!(panic.is_err());
    assert!(!stage.exists());
    assert_eq!(pool.accounting_for_test(), (0, 0));
}

// Break caught: entering the consumer despite cancellation after capture.
#[test]
fn cancelled_consumer_is_not_called_and_owner_is_cleaned() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"cancel control").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let owner = capture(&pool, &source);
    let stage = owner.path_for_test().to_path_buf();
    let token = CancellationToken::new();
    token.cancel();
    let called = AtomicUsize::new(0);
    let result = owner.consume_for_test(&token, |_, _| {
        called.fetch_add(1, Ordering::SeqCst);
        Ok(())
    });
    assert!(matches!(result, Err(EngineError::Cancelled)));
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert!(!stage.exists());
    assert_eq!(pool.accounting_for_test(), (0, 0));
}

// Composition control only: mismatch produces no owner to hand to this private
// consumer. This does not claim a production decoder/resolver admission API.
#[test]
fn expected_mismatch_cannot_supply_a_consumer_owner() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"BBBB").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let called = AtomicUsize::new(0);
    let token = CancellationToken::new();
    let result = pool
        .capture(
            &source,
            PinnedRawDecoderRoute::LibRawCfaV1,
            "arw",
            Some(CapturedAssetIdentity {
                digest: Digest::derive(ASSET_DOMAIN, b"AAAA"),
                byte_len: 4,
            }),
            &token,
        )
        .and_then(|owner| {
            owner.consume_for_test(&token, |_, _| {
                called.fetch_add(1, Ordering::SeqCst);
                Ok(())
            })
        });
    assert!(matches!(result, Err(EngineError::Conflict { .. })));
    assert_eq!(called.load(Ordering::SeqCst), 0);
    assert_eq!(pool.accounting_for_test(), (0, 0));
}

// Break caught: cleanup replaces a primary consumer error, or cleanup error is
// silently discarded after successful consumption. Keep real stage/accounting.
#[test]
fn cleanup_error_preserves_consumer_error_but_fails_successful_consumption() {
    for consumer_fails in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        fs::write(&source, b"error control").unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let count = calls.clone();
        let operations = TestOperations {
            remove_file: Arc::new(move |path| {
                if count.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "consumer cleanup denied",
                    ))
                } else {
                    fs::remove_file(path)
                }
            }),
            ..TestOperations::default()
        };
        let diagnostics = operations.diagnostics.clone();
        let pool =
            CapturePool::create_with_operations_for_test(root.path(), limits(64, 1), operations)
                .unwrap();
        let owner = capture(&pool, &source);
        let stage: PathBuf = owner.path_for_test().to_path_buf();
        let primary = EngineError::Decode {
            format: "synthetic consumer".into(),
            message: "primary consumer failure".into(),
        };
        let result = owner.consume_for_test(&CancellationToken::new(), |path, _| {
            assert_eq!(fs::read(path)?, b"error control");
            if consumer_fails {
                Err(primary.clone())
            } else {
                Ok(())
            }
        });
        if consumer_fails {
            assert_eq!(result, Err(primary));
        } else {
            assert!(
                matches!(result, Err(EngineError::Io { ref message, .. }) if message.contains("consumer cleanup denied"))
            );
        }
        assert_eq!(
            diagnostics
                .lock()
                .unwrap()
                .iter()
                .filter(|d| d.contains("consumer cleanup denied"))
                .count(),
            usize::from(consumer_fails)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(pool.accounting_for_test(), (64, 1));
        assert!(stage.is_file());
        drop(pool);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!stage.exists());
    }
}

// Break caught: ordinary consumer failure bypasses cleanup or changes its error.
#[test]
fn consumer_error_is_returned_after_successful_stage_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"ordinary error").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let owner = capture(&pool, &source);
    let stage = owner.path_for_test().to_path_buf();
    let primary = EngineError::Decode {
        format: "synthetic".into(),
        message: "consumer failed".into(),
    };
    let result = owner.consume_for_test::<()>(&CancellationToken::new(), |path, _| {
        assert_eq!(fs::read(path)?, b"ordinary error");
        Err(primary.clone())
    });
    assert_eq!(result, Err(primary));
    assert!(!stage.exists());
    assert_eq!(pool.accounting_for_test(), (0, 0));
}
