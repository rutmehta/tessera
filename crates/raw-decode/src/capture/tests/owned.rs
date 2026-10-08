//! New-owner protocol tests reuse existing decoder fixture builders. No synthetic native claim.
use super::*;
#[test]
fn owned_success_binds_allocation_metadata_identity_after_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.bin");
    fs::write(&source, b"captured A").unwrap();
    let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
    let captured = owner(&pool, &source);
    let identity = captured.identity();
    let stage = captured.path_for_test().to_path_buf();
    fs::write(&source, b"replacement B").unwrap();
    let (mut image, mut metadata) = synthetic_output();
    image.data.reserve_exact(19);
    metadata.opcode_lists[0] = Some(vec![1, 3, 5]);
    let ptr = image.data.as_ptr();
    let capacity = image.data.capacity();
    let expected = metadata.clone();
    let calls = Cell::new(0);
    let result = captured
        .decode_owned_cfa_with_for_test(&CancellationToken::new(), |path, _| {
            calls.set(calls.get() + 1);
            assert_eq!(fs::read(path)?, b"captured A");
            Ok((image, metadata))
        })
        .unwrap();
    assert_eq!(calls.get(), 1);
    assert!(!stage.exists());
    clean(&pool);
    drop(pool);
    assert_eq!(result.identity(), identity);
    assert_eq!(result.route(), PinnedRawDecoderRoute::LibRawCfaV1);
    assert_eq!(result.image_for_test().data.as_ptr(), ptr);
    assert_eq!(result.image_for_test().data, [1, 2, 3, 4]);
    assert_eq!(
        result.plane_facts(),
        CfaPlaneFacts {
            width: 2,
            height: 2,
            layout: expected.cfa_layout,
            sample_len: 4,
            sample_capacity: capacity
        }
    );
    assert_metadata(result.metadata(), &expected);
    let mut cloned = result.metadata().clone();
    cloned.opcode_lists[0].as_mut().unwrap()[0] = 99;
    cloned.make.clear();
    assert_metadata(result.metadata(), &expected);
    // Private compatibility projection must move, not clone, retained ownership.
    let public = result.into_public_for_test().unwrap();
    assert_eq!(public.image.data.as_ptr(), ptr);
    assert_eq!(public.image.data.capacity(), capacity);
    assert_metadata(&public.metadata, &expected);
    assert_eq!(public.identity, identity);
    assert_eq!(fs::read(source).unwrap(), b"replacement B");
}
use std::cell::Cell;
#[test]
fn owned_precancel_and_invalid_native_bytes_release_stage() {
    for cancelled in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("invalid.bin");
        fs::write(&source, b"not raw").unwrap();
        let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
        let token = CancellationToken::new();
        if cancelled {
            token.cancel();
        }
        let result = owner(&pool, &source).decode_owned_cfa(&token);
        if cancelled {
            assert!(matches!(result, Err(EngineError::Cancelled)));
        } else {
            assert!(matches!(result, Err(EngineError::Decode { .. })));
        }
        clean(&pool);
    }
}
#[test]
fn owned_mismatch_and_decode_errors_use_common_cleanup() {
    for mode in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        fs::write(&source, b"protocol").unwrap();
        let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
        let calls = Cell::new(0);
        let result = owner(&pool, &source).decode_owned_cfa_with_for_test(
            &CancellationToken::new(),
            |_, _| {
                calls.set(calls.get() + 1);
                if mode == 0 {
                    return Err(EngineError::Decode {
                        format: "synthetic".into(),
                        message: "primary".into(),
                    });
                }
                let (mut image, meta) = synthetic_output();
                if mode == 1 {
                    image.width = 3;
                } else {
                    image.cfa_layout = CfaLayout::XTrans([[1; 6]; 6]);
                }
                Ok((image, meta))
            },
        );
        assert!(matches!(result, Err(EngineError::Decode { .. })));
        assert_eq!(calls.get(), 1);
        clean(&pool);
    }
}
#[test]
fn owned_cleanup_failure_never_publishes_and_preserves_primary() {
    for mode in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        fs::write(&source, b"protocol").unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let ops = TestOperations {
            remove_file: Arc::new(move |p| {
                if seen.fetch_add(1, Ordering::SeqCst) == 0 {
                    Err(io::Error::other("owned unlink failure"))
                } else {
                    fs::remove_file(p)
                }
            }),
            ..TestOperations::default()
        };
        let diagnostics = ops.diagnostics.clone();
        let pool =
            CapturePool::create_with_operations_for_test(root.path(), limits(64, 1), ops).unwrap();
        let result = owner(&pool, &source).decode_owned_cfa_with_for_test(
            &CancellationToken::new(),
            |_, _| match mode {
                0 => Ok(synthetic_output()),
                1 => Err(EngineError::Decode {
                    format: "synthetic".into(),
                    message: "primary".into(),
                }),
                _ => Err(EngineError::Cancelled),
            },
        );
        match mode {
            0 => assert!(matches!(result, Err(EngineError::Io { .. }))),
            1 => assert!(
                matches!(result,Err(EngineError::Decode{ref message,..}) if message=="primary")
            ),
            _ => assert!(matches!(result, Err(EngineError::Cancelled))),
        }
        assert_eq!(pool.accounting_for_test(), (64, 1));
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(
            diagnostics
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.contains("owned unlink failure"))
                .count(),
            usize::from(mode != 0)
        );
        drop(pool);
        assert_eq!(count.load(Ordering::SeqCst), 2);
    }
}
#[test]
fn owned_classifier_refusal_prevents_decoder_and_releases_capture() {
    for linear in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        let mut bytes = b"II\x2a\x00\x08\x00\x00\x00\x01\x00".to_vec();
        bytes.extend_from_slice(&262u16.to_le_bytes());
        bytes.extend_from_slice(&3u16.to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.extend_from_slice(&34892u16.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        if !linear {
            bytes = b"II\x2a\x00\x08\x00\x00\x00\x01\x10".to_vec();
        }
        fs::write(&source, bytes).unwrap();
        let pool = CapturePool::create(root.path(), limits(64, 1)).unwrap();
        let calls = Cell::new(0);
        let result = owner(&pool, &source).decode_owned_cfa_with_for_test(
            &CancellationToken::new(),
            |_, _| {
                calls.set(calls.get() + 1);
                Ok(synthetic_output())
            },
        );
        if linear {
            assert!(matches!(result, Err(EngineError::Unsupported { .. })));
        } else {
            assert!(matches!(result, Err(EngineError::Decode { .. })));
        }
        assert_eq!(calls.get(), 0);
        clean(&pool);
    }
}

#[test]
fn actual_opaque_owner_five_families_after_original_copy_replacement() {
    let Some(directory) = captured_cfa_fixtures(&FIVE_FAMILIES.map(|(name, _)| name)) else {
        return;
    };
    for (name, suffix) in FIVE_FAMILIES {
        let original = directory.join(name);
        let root = tempfile::tempdir().unwrap();
        let copy = root.path().join("relocated.bin");
        fs::copy(&original, &copy).unwrap();
        let mut direct = RawSource::open(&copy).unwrap();
        let expected = direct.decode_cfa_u16().unwrap();
        let metadata = direct.metadata();
        drop(direct);
        let len = fs::metadata(&copy).unwrap().len();
        let pool = CapturePool::create(
            root.path(),
            CaptureLimits {
                max_asset_bytes: len,
                max_staged_bytes: len,
                max_live_captures: 1,
            },
        )
        .unwrap();
        let captured = pool
            .capture(
                &copy,
                PinnedRawDecoderRoute::LibRawCfaV1,
                suffix,
                None,
                &CancellationToken::new(),
            )
            .unwrap();
        let identity = captured.identity();
        assert_eq!(captured.suffix(), suffix.to_ascii_lowercase());
        let stage = captured.path_for_test().to_path_buf();
        assert_eq!(
            stage.extension().unwrap().to_str().unwrap(),
            suffix.to_ascii_lowercase()
        );
        let private_dir = pool.directory_for_test().to_path_buf();
        drop(pool);
        fs::rename(&copy, root.path().join("original-moved")).unwrap();
        fs::write(&copy, b"invalid replacement B").unwrap();
        let decoded = captured
            .decode_owned_cfa(&CancellationToken::new())
            .unwrap();
        assert!(!stage.exists());
        assert!(!private_dir.exists());
        assert_eq!(
            (
                decoded.image_for_test().width,
                decoded.image_for_test().height,
                decoded.image_for_test().cfa_layout
            ),
            (expected.width, expected.height, expected.cfa_layout)
        );
        assert_eq!(decoded.image_for_test().data, expected.data);
        assert_metadata(decoded.metadata(), &metadata);
        assert_eq!(decoded.identity(), identity);
        assert_eq!(decoded.route(), PinnedRawDecoderRoute::LibRawCfaV1);
        assert_eq!(fs::read(&copy).unwrap(), b"invalid replacement B");
        eprintln!(
            "EXERCISED captured CFA {name}: {}x{}, {} owned samples",
            decoded.image_for_test().width,
            decoded.image_for_test().height,
            decoded.image_for_test().data.len()
        );
    }
}
