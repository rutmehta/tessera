//! Task 2 contracts. Synthetic files only; not yet run against an implementation.
use super::*;
use std::{fs::File, io::{Read, Write}, os::fd::AsRawFd, path::PathBuf};

// These replace individual I/O operations, not accounting, sealing or metadata.
// Production capture must call the same core with real I/O, not a second path.
type ReadHook = Box<dyn FnMut(&mut File, &mut [u8]) -> io::Result<usize>>;
type WriteHook = Box<dyn FnMut(&mut File, &[u8]) -> io::Result<usize>>;
type SyncHook = Box<dyn FnMut(&File) -> io::Result<()>>;
type ReopenHook = Box<dyn FnMut(&Path) -> io::Result<File>>;
type AfterCopyHook = Box<dyn FnMut(&Path, &Path) -> io::Result<()>>;
#[derive(Default)]
pub(in crate::capture) struct StreamHooks {
    pub(in crate::capture) source_read: Option<ReadHook>,
    pub(in crate::capture) stage_write: Option<WriteHook>,
    pub(in crate::capture) stage_sync: Option<SyncHook>,
    pub(in crate::capture) hash_read: Option<ReadHook>,
    pub(in crate::capture) after_copy: Option<AfterCopyHook>,
    pub(in crate::capture) readonly_reopen: Option<ReopenHook>,
}

struct Fixture {
    _root: tempfile::TempDir,
    source: PathBuf,
    pool: CapturePool,
}
impl Fixture {
    fn new(bytes: &[u8], limit: u64) -> Self {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("original.bin");
        fs::write(&source, bytes).unwrap();
        let pool = CapturePool::create(root.path(), CaptureLimits {
            max_asset_bytes: limit, max_staged_bytes: limit, max_live_captures: 1,
        }).unwrap();
        Self { _root: root, source, pool }
    }
    fn capture(&self) -> EngineResult<CapturedRaw> {
        self.pool.capture(&self.source, PinnedRawDecoderRoute::LibRawCfaV1,
            "ARW", None, &CancellationToken::new())
    }
    fn injected(&self, hooks: StreamHooks, cancel: &CancellationToken) -> EngineResult<CapturedRaw> {
        self.pool.capture_with_hooks_for_test(&self.source,
            PinnedRawDecoderRoute::LibRawCfaV1, "ARW", None, cancel, hooks)
    }
    fn assert_clean(&self) {
        assert_eq!(self.pool.accounting_for_test(), (0, 0));
        assert_eq!(fs::read_dir(self.pool.directory_for_test()).unwrap().count(), 0);
    }
}

// Break caught: hashing a declaration/prefix, wrong domain, or truncating chunks.
#[test]
fn completed_stage_identity_matches_exact_bytes_across_chunk_boundaries() {
    for len in [1, CHUNK_BYTES - 1, CHUNK_BYTES, CHUNK_BYTES + 1] {
        let bytes: Vec<_> = (0..len).map(|i| (i % 251) as u8).collect();
        let fixture = Fixture::new(&bytes, (CHUNK_BYTES * 2) as u64);
        let capture = fixture.capture().unwrap();
        assert_eq!(capture.identity(), CapturedAssetIdentity {
            digest: Digest::derive("tessera pinned RAW asset v1", &bytes), byte_len: len as u64,
        });
        assert_eq!(fs::read(capture.path_for_test()).unwrap(), bytes);
        assert_eq!(capture.suffix(), "arw");
        assert_eq!(capture.route(), PinnedRawDecoderRoute::LibRawCfaV1);
        assert_eq!(capture.path_for_test().extension().unwrap(), "arw");
        capture.close().unwrap();
        fixture.assert_clean();
    }
}

// Break caught: returning a stage with writable descriptor ownership after sealing.
#[test]
fn returned_stage_descriptor_is_read_only_and_retains_its_charge() {
    let fixture = Fixture::new(b"sealed bytes", 64);
    let capture = fixture.capture().unwrap();
    // SAFETY: F_GETFL only inspects the valid descriptor borrowed from the owner.
    let flags = unsafe { libc::fcntl(capture.readonly_file_for_test().as_raw_fd(), libc::F_GETFL) };
    assert_ne!(flags, -1);
    assert_eq!(flags & libc::O_ACCMODE, libc::O_RDONLY);
    assert_eq!(fixture.pool.accounting_for_test(), (64, 1));
    capture.close().unwrap();
    fixture.assert_clean();
}

// Break caught: admission accepts empty/oversized streams or rejects exact limit.
#[test]
fn empty_and_limit_plus_one_reject_but_exact_limit_succeeds() {
    for (bytes, expected) in [(Vec::new(), "empty"), (vec![7; 64], "ok"), (vec![7; 65], "large")] {
        let fixture = Fixture::new(&bytes, 64);
        match (expected, fixture.capture()) {
            ("empty", Err(EngineError::InvalidArgument { .. })) => (),
            ("large", Err(EngineError::ResourceExhausted { .. })) => (),
            ("ok", Ok(capture)) => { assert_eq!(capture.identity().byte_len, 64); capture.close().unwrap(); }
            _ => panic!("unexpected capture outcome for {expected}"),
        }
        fixture.assert_clean();
    }
}

// Break caught: suffix escaping creates a stage before validating the route hint.
#[test]
fn invalid_suffix_is_rejected_before_source_lookup_or_stage_creation() {
    let fixture = Fixture::new(b"x", 64);
    fs::remove_file(&fixture.source).unwrap();
    for suffix in ["", ".arw", "../arw", "a/b", "a\\b", "é", "abcdefghijklmnopq"] {
        assert!(matches!(fixture.pool.capture(&fixture.source,
            PinnedRawDecoderRoute::LibRawCfaV1, suffix, None, &CancellationToken::new()),
            Err(EngineError::InvalidArgument { .. })));
        fixture.assert_clean();
    }
}

// Break caught: trusting metadata length allows an unbounded growing source read.
#[test]
fn growing_source_reads_at_most_limit_plus_one_and_never_writes_excess() {
    let fixture = Fixture::new(b"a", 64);
    let source = fixture.source.clone();
    let consumed = Arc::new(AtomicUsize::new(0));
    let written = Arc::new(AtomicUsize::new(0));
    let read_count = consumed.clone();
    let write_count = written.clone();
    let mut grew = false;
    let hooks = StreamHooks {
        source_read: Some(Box::new(move |file, buffer| {
            assert!(buffer.len() <= 65);
            if !grew {
                File::options().append(true).open(&source)?.write_all(&[3; 256])?;
                grew = true;
            }
            let n = file.read(buffer)?;
            read_count.fetch_add(n, Ordering::SeqCst);
            Ok(n)
        })),
        stage_write: Some(Box::new(move |file, bytes| {
            let n = file.write(bytes)?;
            write_count.fetch_add(n, Ordering::SeqCst);
            Ok(n)
        })),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()),
        Err(EngineError::ResourceExhausted { .. })));
    assert_eq!(consumed.load(Ordering::SeqCst), 65);
    assert!(written.load(Ordering::SeqCst) <= 64);
    fixture.assert_clean();
}

// Break caught: short reads/writes truncate a stage or Interrupted aborts capture.
#[test]
fn short_reads_and_writes_and_interrupted_reads_preserve_complete_stream() {
    let bytes = b"complete synthetic capture";
    let fixture = Fixture::new(bytes, 64);
    let mut interrupted = false;
    let hooks = StreamHooks {
        source_read: Some(Box::new(move |file, buffer| {
            if !interrupted { interrupted = true; return Err(io::ErrorKind::Interrupted.into()); }
            let n = buffer.len().min(3);
            file.read(&mut buffer[..n])
        })),
        stage_write: Some(Box::new(|file, bytes| file.write(&bytes[..bytes.len().min(2)]))),
        ..StreamHooks::default()
    };
    let capture = fixture.injected(hooks, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity().digest, Digest::derive(ASSET_DOMAIN, bytes));
    assert_eq!(fs::read(capture.path_for_test()).unwrap(), bytes);
    capture.close().unwrap();
    fixture.assert_clean();
}

// Break caught: swallowing an I/O error publishes incomplete/unverified bytes.
#[test]
fn copy_write_sync_and_hash_read_errors_release_every_stage() {
    for phase in ["read", "write", "sync", "reopen", "hash"] {
        let fixture = Fixture::new(b"not a photo", 64);
        let mut hooks = StreamHooks::default();
        match phase {
            "read" => hooks.source_read = Some(Box::new(|_, _| Err(io::Error::other("source failure")))),
            "write" => hooks.stage_write = Some(Box::new(|_, _| Err(io::Error::other("write failure")))),
            "sync" => hooks.stage_sync = Some(Box::new(|_| Err(io::Error::other("sync failure")))),
            "reopen" => hooks.readonly_reopen = Some(Box::new(|_| Err(io::Error::other("reopen failure")))),
            "hash" => hooks.hash_read = Some(Box::new(|_, _| Err(io::Error::other("hash failure")))),
            _ => unreachable!(),
        }
        assert!(matches!(fixture.injected(hooks, &CancellationToken::new()), Err(EngineError::Io { .. })), "{phase}");
        fixture.assert_clean();
    }
}

// Break caught: hashing the copy input instead of the completed staged bytes.
// Stage mutation is test injection before sealing, not an external safety claim.
#[test]
fn digest_is_derived_from_completed_stage_not_source_reader() {
    let fixture = Fixture::new(b"AAAA", 64);
    let hooks = StreamHooks {
        after_copy: Some(Box::new(|_, stage| fs::write(stage, b"BBBB"))),
        ..StreamHooks::default()
    };
    let capture = fixture.injected(hooks, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity().digest, Digest::derive(ASSET_DOMAIN, b"BBBB"));
    assert_eq!(fs::read(capture.path_for_test()).unwrap(), b"BBBB");
    capture.close().unwrap();
    fixture.assert_clean();
}

// Break caught: stage length mutation is accepted despite copied byte count.
#[test]
fn completed_stage_length_must_match_copied_count() {
    let fixture = Fixture::new(b"AAAA", 64);
    let hooks = StreamHooks {
        after_copy: Some(Box::new(|_, stage| fs::write(stage, b"B"))),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()), Err(EngineError::Conflict { .. })));
    fixture.assert_clean();
}

// Break caught: cancellation after an I/O operation still publishes an owner.
#[test]
fn cancellation_after_copy_and_hash_reads_never_returns_capture() {
    for phase in ["copy", "hash"] {
        let fixture = Fixture::new(b"cancel at a deterministic read boundary", 64);
        let token = CancellationToken::new();
        let cancel = token.clone();
        let hook: ReadHook = Box::new(move |file, buffer| {
            let n = file.read(buffer)?;
            cancel.cancel();
            Ok(n)
        });
        let mut hooks = StreamHooks::default();
        if phase == "copy" { hooks.source_read = Some(hook); } else { hooks.hash_read = Some(hook); }
        assert!(matches!(fixture.injected(hooks, &token), Err(EngineError::Cancelled)), "{phase}");
        fixture.assert_clean();
    }
}

// Break caught: accepting matching length but wrong digest (or converse).
#[test]
fn expected_identity_requires_both_digest_and_length_and_does_not_mutate_input() {
    let fixture = Fixture::new(b"frozen", 64);
    let identity = CapturedAssetIdentity { digest: Digest::derive(ASSET_DOMAIN, b"frozen"), byte_len: 6 };
    for expected in [CapturedAssetIdentity { digest: Digest::derive(ASSET_DOMAIN, b"other!"), ..identity },
        CapturedAssetIdentity { byte_len: 5, ..identity }, identity] {
        let original = expected;
        let result = fixture.pool.capture(&fixture.source, PinnedRawDecoderRoute::LibRawCfaV1,
            "arw", Some(expected), &CancellationToken::new());
        if expected == identity { result.unwrap().close().unwrap(); }
        else { assert!(matches!(result, Err(EngineError::Conflict { .. }))); }
        assert_eq!(expected, original);
        assert_eq!(fs::read(&fixture.source).unwrap(), b"frozen");
        fixture.assert_clean();
    }
}

// Break caught: ignoring observed replacement/unlink/growth after source opening.
#[test]
fn observed_source_changes_reject_before_returning_owner() {
    for mutation in ["replace", "remove", "grow"] {
        let fixture = Fixture::new(b"AAAA", 64);
        let hooks = StreamHooks {
            after_copy: Some(Box::new(move |source, _| match mutation {
                "replace" => { fs::rename(source, source.with_extension("old"))?; fs::write(source, b"BBBB") },
                "remove" => fs::remove_file(source),
                "grow" => File::options().append(true).open(source)?.write_all(b"B"),
                _ => unreachable!(),
            })),
            ..StreamHooks::default()
        };
        assert!(matches!(fixture.injected(hooks, &CancellationToken::new()), Err(EngineError::Conflict { .. })), "{mutation}");
        fixture.assert_clean();
    }
}

// Break caught: falsely promising that metadata checks reject every mixed stream.
#[test]
fn unchanged_metadata_can_capture_a_mixed_stream_with_its_honest_identity() {
    let fixture = Fixture::new(b"AAAA", 64);
    let mut reader = io::Cursor::new(b"AABB");
    let hooks = StreamHooks { source_read: Some(Box::new(move |_, buffer| reader.read(buffer))),
        ..StreamHooks::default() };
    let capture = fixture.injected(hooks, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity(), CapturedAssetIdentity { digest: Digest::derive(ASSET_DOMAIN, b"AABB"), byte_len: 4 });
    assert_eq!(fs::read(capture.path_for_test()).unwrap(), b"AABB");
    assert_eq!(fs::read(&fixture.source).unwrap(), b"AAAA");
    capture.close().unwrap();
    fixture.assert_clean();
}

// Break caught: banning legitimate source symlinks or admitting directories.
#[test]
fn source_symlink_is_a_locator_but_directory_is_not_a_stream() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new(b"locator bytes", 64);
    let alias = fixture._root.path().join("alias.arw");
    symlink(&fixture.source, &alias).unwrap();
    let capture = fixture.pool.capture(&alias, PinnedRawDecoderRoute::LibRawCfaV1,
        "arw", None, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity().digest, Digest::derive(ASSET_DOMAIN, b"locator bytes"));
    capture.close().unwrap();
    assert!(matches!(fixture.pool.capture(fixture._root.path(), PinnedRawDecoderRoute::LibRawCfaV1,
        "arw", None, &CancellationToken::new()), Err(EngineError::InvalidArgument { .. })));
    fixture.assert_clean();
}

// Break caught: ordinary blocking open hangs forever on FIFO admission.
#[test]
fn fifo_without_writer_is_rejected_without_waiting_for_a_producer() {
    use std::{ffi::CString, os::unix::{ffi::OsStrExt, fs::OpenOptionsExt}, sync::mpsc, time::Duration};
    let fixture = Fixture::new(b"unused", 64);
    let fifo = fixture._root.path().join("source.fifo");
    let cpath = CString::new(fifo.as_os_str().as_bytes()).unwrap();
    // SAFETY: cpath is a live NUL-terminated path, mode is a valid permission mask.
    assert_eq!(unsafe { libc::mkfifo(cpath.as_ptr(), 0o600) }, 0);
    let pool = fixture.pool.clone();
    let source = fifo.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let result = pool.capture(&source, PinnedRawDecoderRoute::LibRawCfaV1,
            "arw", None, &CancellationToken::new());
        tx.send(matches!(result, Err(EngineError::InvalidArgument { .. }))).unwrap();
    });
    let outcome = rx.recv_timeout(Duration::from_secs(2));
    if outcome.is_err() {
        // Rescue a regressed blocking reader, so failure does not strand the test.
        let _ = File::options().write(true).custom_flags(libc::O_NONBLOCK).open(&fifo);
    }
    worker.join().unwrap();
    assert!(outcome.unwrap());
    fixture.assert_clean();
}


// Break caught: losing the disabled TempPath/reservation while transitioning from
// writer to read-only ownership leaks the sealed stage on reopen failure.
#[test]
fn readonly_reopen_failure_retains_cleanup_authority_even_when_unlink_fails() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"sealing transition").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let operations = TestOperations {
        remove_file: Arc::new(move |path| {
            if count.fetch_add(1, Ordering::SeqCst) == 0 {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "sealed unlink denied"))
            } else { fs::remove_file(path) }
        }),
        ..TestOperations::default()
    };
    let diagnostics = operations.diagnostics.clone();
    let pool = CapturePool::create_with_operations_for_test(root.path(), limits(64, 1), operations).unwrap();
    let hooks = StreamHooks {
        readonly_reopen: Some(Box::new(|_| Err(io::Error::other("primary reopen failure")))),
        ..StreamHooks::default()
    };
    let result = pool.capture_with_hooks_for_test(&source, PinnedRawDecoderRoute::LibRawCfaV1,
        "arw", None, &CancellationToken::new(), hooks);
    assert!(matches!(result, Err(EngineError::Io { ref message, .. }) if message.contains("primary reopen failure")));
    assert_eq!(pool.accounting_for_test(), (64, 1));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let entries = fs::read_dir(pool.directory_for_test()).unwrap().collect::<Result<Vec<_>, _>>().unwrap();
    assert_eq!(entries.len(), 1);
    let retained = entries[0].path();
    assert_eq!(fs::read(&retained).unwrap(), b"sealing transition");
    assert!(diagnostics.lock().unwrap().iter().any(|d| d.contains("sealed unlink denied")));
    drop(pool);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(!retained.exists());
    assert_eq!(fs::read(&source).unwrap(), b"sealing transition");
}

// Break caught: re-reading the stage allocates/reads an unbounded modified tail.
#[test]
fn stage_hash_read_is_bounded_independently_of_copied_source() {
    let fixture = Fixture::new(b"A", 64);
    let consumed = Arc::new(AtomicUsize::new(0));
    let count = consumed.clone();
    let hooks = StreamHooks {
        after_copy: Some(Box::new(|_, stage| fs::write(stage, [2; 256]))),
        hash_read: Some(Box::new(move |file, buffer| {
            assert!(buffer.len() <= 65);
            let n = file.read(buffer)?;
            count.fetch_add(n, Ordering::SeqCst);
            Ok(n)
        })),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()),
        Err(EngineError::ResourceExhausted { .. }) | Err(EngineError::Conflict { .. })));
    // Early readonly-stage length rejection is also valid; never consume >limit+1.
    assert!(consumed.load(Ordering::SeqCst) <= 65);
    fixture.assert_clean();
}

// Break caught: the Interrupted retry path fails to observe cancellation.
#[test]
fn interrupted_read_with_cancellation_terminates_without_write() {
    let fixture = Fixture::new(b"interrupt", 64);
    let token = CancellationToken::new();
    let cancel = token.clone();
    let writes = Arc::new(AtomicUsize::new(0));
    let count = writes.clone();
    let hooks = StreamHooks {
        source_read: Some(Box::new(move |_, _| { cancel.cancel(); Err(io::ErrorKind::Interrupted.into()) })),
        stage_write: Some(Box::new(move |file, bytes| { count.fetch_add(1, Ordering::SeqCst); file.write(bytes) })),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &token), Err(EngineError::Cancelled)));
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    fixture.assert_clean();
}

// Break caught: the stage length check passes, then the hash loop reads a grown
// tail without its own limit+1 bound. Growth happens inside the first hash read.
#[test]
fn stage_growth_after_readonly_admission_is_still_stream_bounded() {
    let fixture = Fixture::new(b"A", 64);
    let path = Arc::new(std::sync::Mutex::new(None::<PathBuf>));
    let recorded = path.clone();
    let hash_path = path.clone();
    let consumed = Arc::new(AtomicUsize::new(0));
    let count = consumed.clone();
    let mut grew = false;
    let hooks = StreamHooks {
        after_copy: Some(Box::new(move |_, stage| {
            *recorded.lock().unwrap() = Some(stage.to_path_buf());
            Ok(())
        })),
        hash_read: Some(Box::new(move |file, buffer| {
            assert!(buffer.len() <= 65);
            if !grew {
                let stage = hash_path.lock().unwrap().clone().unwrap();
                File::options().append(true).open(stage)?.write_all(&[4; 256])?;
                grew = true;
            }
            let n = file.read(buffer)?;
            count.fetch_add(n, Ordering::SeqCst);
            Ok(n)
        })),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()),
        Err(EngineError::ResourceExhausted { .. })));
    assert_eq!(consumed.load(Ordering::SeqCst), 65);
    fixture.assert_clean();
}

// Break caught: metadata comparison uses only inode and length, ignoring times.
// Fixed modification times avoid sleeps and filesystem clock-resolution races.
#[test]
fn same_inode_same_length_overwrite_with_changed_timestamp_is_rejected() {
    use std::{fs::FileTimes, os::unix::fs::MetadataExt, time::{Duration, UNIX_EPOCH}};
    let fixture = Fixture::new(b"AAAA", 64);
    File::options().write(true).open(&fixture.source).unwrap().set_times(
        FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(100))).unwrap();
    let initial = fs::metadata(&fixture.source).unwrap();
    let hooks = StreamHooks {
        after_copy: Some(Box::new(move |source, _| {
            let mut writer = File::options().write(true).open(source)?;
            writer.write_all(b"BBBB")?;
            writer.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(200)))?;
            let changed = writer.metadata()?;
            assert_eq!(changed.ino(), initial.ino());
            assert_eq!(changed.len(), initial.len());
            assert_ne!((changed.mtime(), changed.mtime_nsec()), (initial.mtime(), initial.mtime_nsec()));
            Ok(())
        })),
        ..StreamHooks::default()
    };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()), Err(EngineError::Conflict { .. })));
    assert_eq!(fs::read(&fixture.source).unwrap(), b"BBBB");
    fixture.assert_clean();
}

// Break caught: using the entire remaining asset limit as an I/O buffer rather
// than bounding each source and hash read independently by CHUNK_BYTES.
#[test]
fn source_and_hash_requests_stay_chunk_bounded_with_large_asset_budget() {
    let bytes = vec![9; CHUNK_BYTES * 2 + 7];
    let fixture = Fixture::new(&bytes, (CHUNK_BYTES * 4) as u64);
    let source_calls = Arc::new(AtomicUsize::new(0));
    let hash_calls = Arc::new(AtomicUsize::new(0));
    let source_count = source_calls.clone();
    let hash_count = hash_calls.clone();
    let hooks = StreamHooks {
        source_read: Some(Box::new(move |file, buffer| {
            assert!(!buffer.is_empty() && buffer.len() <= CHUNK_BYTES);
            source_count.fetch_add(1, Ordering::SeqCst);
            file.read(buffer)
        })),
        hash_read: Some(Box::new(move |file, buffer| {
            assert!(!buffer.is_empty() && buffer.len() <= CHUNK_BYTES);
            hash_count.fetch_add(1, Ordering::SeqCst);
            file.read(buffer)
        })),
        ..StreamHooks::default()
    };
    let capture = fixture.injected(hooks, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity(), CapturedAssetIdentity {
        digest: Digest::derive(ASSET_DOMAIN, &bytes), byte_len: bytes.len() as u64,
    });
    assert!(source_calls.load(Ordering::SeqCst) >= 3);
    assert!(hash_calls.load(Ordering::SeqCst) >= 3);
    capture.close().unwrap();
    fixture.assert_clean();
}

// Break caught: a write returning zero is treated as completed progress instead
// of WriteZero, or spins without releasing the stage.
#[test]
fn zero_write_is_an_io_failure_with_no_published_capture() {
    let fixture = Fixture::new(b"write-zero", 64);
    let hooks = StreamHooks { stage_write: Some(Box::new(|_, _| Ok(0))),
        ..StreamHooks::default() };
    assert!(matches!(fixture.injected(hooks, &CancellationToken::new()), Err(EngineError::Io { .. })));
    fixture.assert_clean();
}

// Break caught: retryable Interrupted from a writer or the staged reread loses
// bytes, aborts valid capture, or incorrectly advances its byte count.
#[test]
fn interrupted_write_and_hash_read_retry_without_losing_bytes() {
    let bytes = b"retry complete bytes";
    let fixture = Fixture::new(bytes, 64);
    let mut write_interrupted = false;
    let mut hash_interrupted = false;
    let hooks = StreamHooks {
        stage_write: Some(Box::new(move |file, bytes| {
            if !write_interrupted {
                write_interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            file.write(bytes)
        })),
        hash_read: Some(Box::new(move |file, buffer| {
            if !hash_interrupted {
                hash_interrupted = true;
                return Err(io::ErrorKind::Interrupted.into());
            }
            file.read(buffer)
        })),
        ..StreamHooks::default()
    };
    let capture = fixture.injected(hooks, &CancellationToken::new()).unwrap();
    assert_eq!(capture.identity(), CapturedAssetIdentity {
        digest: Digest::derive(ASSET_DOMAIN, bytes), byte_len: bytes.len() as u64,
    });
    assert_eq!(fs::read(capture.path_for_test()).unwrap(), bytes);
    capture.close().unwrap();
    fixture.assert_clean();
}
