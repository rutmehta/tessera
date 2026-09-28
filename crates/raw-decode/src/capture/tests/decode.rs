//! Closed adapter contracts; synthetic protocol controls are not actual decoding.
use super::*;
use crate::{CfaLayout, CfaU16, RawMetadata, RawSource};
use std::{path::PathBuf, sync::mpsc, time::Duration};

fn owner(pool: &CapturePool, source: &Path) -> CapturedRaw {
    pool.capture(source, PinnedRawDecoderRoute::LibRawCfaV1, "ARW", None,
        &CancellationToken::new()).unwrap()
}
fn synthetic_output() -> (CfaU16, RawMetadata) {
    (CfaU16 { width: 2, height: 2, data: vec![1, 2, 3, 4], cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]) },
    RawMetadata {
        make: "synthetic".into(), model: "protocol-only".into(), lens: None,
        iso: 100.0, shutter_s: 0.01, aperture: 2.0, focal_mm: 50.0, capture_time: 0,
        orientation: 1, width: 2, height: 2, cfa_layout: CfaLayout::Bayer([[0, 1], [1, 2]]),
        black_levels: [0.0; 4], white_level: 100, as_shot_wb: [1.0; 4],
        camera_to_xyz: engine_api::color::ColorMatrix3([[1.0,0.0,0.0],[0.0,1.0,0.0],[0.0,0.0,1.0]]),
        cam_xyz: [[0.0;3];4], rgb_cam: [[0.0;4];3], default_crop: [0,0,2,2],
        has_gain_map: false, has_opcode_list: false, opcode_lists: [None,None,None],
    })
}
fn clean(pool: &CapturePool) {
    assert_eq!(pool.accounting_for_test(), (0,0));
    assert_eq!(fs::read_dir(pool.directory_for_test()).unwrap().count(), 0);
}

// Break caught: invalid bytes are accepted or a failed native decode leaks stage.
#[test]
fn actual_invalid_raw_returns_decode_and_releases_stage() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("invalid.raw");
    fs::write(&source, b"not a RAW").unwrap();
    let pool = CapturePool::create(root.path(), limits(64,1)).unwrap();
    assert!(matches!(owner(&pool,&source).decode_cfa(&CancellationToken::new()), Err(EngineError::Decode { .. })));
    clean(&pool);
}

// Break caught: recognized LinearRaw silently falls through to the CFA backend.
#[test]
fn linear_raw_classifier_refuses_cfa_route_before_decoder_entry() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("linear.bin");
    let mut bytes = b"II\x2a\x00\x08\x00\x00\x00\x01\x00".to_vec();
    bytes.extend_from_slice(&262u16.to_le_bytes()); bytes.extend_from_slice(&3u16.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes()); bytes.extend_from_slice(&34892u16.to_le_bytes());
    bytes.extend_from_slice(&[0;6]);
    fs::write(&source, bytes).unwrap();
    let pool = CapturePool::create(root.path(), limits(64,1)).unwrap();
    let called = AtomicUsize::new(0);
    let result = owner(&pool,&source).decode_cfa_with_for_test(&CancellationToken::new(), |_,_| {
        called.fetch_add(1,Ordering::SeqCst); Ok(synthetic_output())
    });
    assert!(matches!(result, Err(EngineError::Unsupported { .. })));
    assert_eq!(called.load(Ordering::SeqCst),0);
    clean(&pool);
}

// Break caught: malformed classification falls through to a different backend.
#[test]
fn malformed_classifier_is_decode_error_without_backend_fallback() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("bad.bin");
    fs::write(&source, b"II\x2a\x00\x08\x00\x00\x00\x01\x10").unwrap();
    let pool = CapturePool::create(root.path(), limits(64,1)).unwrap();
    let called=AtomicUsize::new(0);
    let result=owner(&pool,&source).decode_cfa_with_for_test(&CancellationToken::new(), |_,_| {
        called.fetch_add(1,Ordering::SeqCst); Ok(synthetic_output())
    });
    assert!(matches!(result,Err(EngineError::Decode { .. })));
    assert_eq!(called.load(Ordering::SeqCst),0); clean(&pool);
}

// Break caught: cancellation before or after closed decoding publishes output.
#[test]
fn cancelled_protocol_does_not_publish_and_precancel_does_not_enter_decoder() {
    for before in [true,false] {
        let root=tempfile::tempdir().unwrap(); let source=root.path().join("source.bin");
        fs::write(&source,b"synthetic bytes").unwrap();
        let pool=CapturePool::create(root.path(),limits(64,1)).unwrap();
        let captured=owner(&pool,&source); let token=CancellationToken::new();
        if before {token.cancel();}
        let called=AtomicUsize::new(0);
        let result=captured.decode_cfa_with_for_test(&token,|_,cancel| {
            called.fetch_add(1,Ordering::SeqCst); cancel.cancel(); Ok(synthetic_output())
        });
        assert!(matches!(result,Err(EngineError::Cancelled)));
        assert_eq!(called.load(Ordering::SeqCst),usize::from(!before)); clean(&pool);
    }
}

// Test native-like holder: protocol ordering only, not a fake pixel-fidelity claim.
struct DecoderLifetime { path: PathBuf, dropped: Arc<AtomicUsize> }
impl Drop for DecoderLifetime {
    fn drop(&mut self) { assert!(self.path.is_file()); self.dropped.store(1,Ordering::SeqCst); }
}

// Break caught: stage cleanup happens while a decoder is still reading/owned.
#[test]
fn closed_decoder_reads_held_a_and_drops_before_unlink_then_owned_output_survives() {
    let root=tempfile::tempdir().unwrap(); let source=root.path().join("source.bin");
    fs::write(&source,b"captured A").unwrap();
    let dropped=Arc::new(AtomicUsize::new(0)); let seen=dropped.clone();
    let ops=TestOperations { remove_file:Arc::new(move |path| {
        assert_eq!(seen.load(Ordering::SeqCst),1); fs::remove_file(path)
    }), ..TestOperations::default() };
    let pool=CapturePool::create_with_operations_for_test(root.path(),limits(64,1),ops).unwrap();
    let captured=owner(&pool,&source); let identity=captured.identity();
    let stage=captured.path_for_test().to_path_buf();
    fs::rename(&source,root.path().join("prior.bin")).unwrap(); fs::write(&source,b"replacement B").unwrap();
    let (entered_tx,entered_rx)=mpsc::channel(); let (release_tx,release_rx)=mpsc::channel();
    let worker=std::thread::spawn(move || captured.decode_cfa_with_for_test(&CancellationToken::new(),|path,_| {
        let _native=DecoderLifetime {path:path.to_path_buf(),dropped};
        entered_tx.send(()).unwrap(); release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(fs::read(path)?,b"captured A"); Ok(synthetic_output())
    }));
    let entered=entered_rx.recv_timeout(Duration::from_secs(5)); let held=pool.accounting_for_test(); let present=stage.is_file();
    let _=release_tx.send(()); let result=worker.join().unwrap(); entered.unwrap();
    assert_eq!(held,(64,1)); assert!(present);
    let result=result.unwrap(); assert!(!stage.exists()); clean(&pool); drop(pool);
    assert_eq!(result.image.data,[1,2,3,4]); assert_eq!(result.metadata.make,"synthetic"); assert_eq!(result.identity,identity);
    assert_eq!(result.route,PinnedRawDecoderRoute::LibRawCfaV1); assert_eq!(fs::read(source).unwrap(),b"replacement B");
}

// Break caught: secondary unlink failure hides native/cancel failure or allows a
// successful output to escape despite cleanup failure. Accounting remains real.
#[test]
fn decoder_primary_error_wins_and_success_requires_cleanup() {
    for fails in [false,true] {
        let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"protocol").unwrap();
        let count=Arc::new(AtomicUsize::new(0));let seen=count.clone();
        let ops=TestOperations {remove_file:Arc::new(move |path| {
            if seen.fetch_add(1,Ordering::SeqCst)==0 {Err(io::Error::other("decode unlink failure"))} else {fs::remove_file(path)}
        }),..TestOperations::default()};let diagnostics=ops.diagnostics.clone();
        let pool=CapturePool::create_with_operations_for_test(root.path(),limits(64,1),ops).unwrap();
        let result=owner(&pool,&source).decode_cfa_with_for_test(&CancellationToken::new(),|_,_| {
            if fails {Err(EngineError::Decode{format:"synthetic".into(),message:"primary decode failure".into()})}else{Ok(synthetic_output())}
        });
        if fails {assert!(matches!(result,Err(EngineError::Decode{ref message,..}) if message=="primary decode failure"));}
        else {assert!(matches!(result,Err(EngineError::Io{..})));}
        assert_eq!(pool.accounting_for_test(),(64,1));assert_eq!(count.load(Ordering::SeqCst),1);
        assert_eq!(diagnostics.lock().unwrap().iter().filter(|d| d.contains("decode unlink failure")).count(),usize::from(fails));
        drop(pool);assert_eq!(count.load(Ordering::SeqCst),2);
    }
}

// Break caught: panic bypasses retained-stage cleanup.
#[test]
fn decoder_panic_unwinds_stage_ownership() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"protocol").unwrap();
    let pool=CapturePool::create(root.path(),limits(64,1)).unwrap();let captured=owner(&pool,&source);
    let panic=std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| captured.decode_cfa_with_for_test(&CancellationToken::new(),|path,_| {
        assert!(path.is_file());panic!("decoder protocol panic")
    })));
    assert!(panic.is_err());clean(&pool);
}

fn assert_metadata(a:&RawMetadata,b:&RawMetadata) {
    assert_eq!((&a.make,&a.model,&a.lens,a.capture_time,a.orientation,a.width,a.height,a.cfa_layout,a.white_level,a.default_crop),
        (&b.make,&b.model,&b.lens,b.capture_time,b.orientation,b.width,b.height,b.cfa_layout,b.white_level,b.default_crop));
    assert_eq!((a.has_gain_map,a.has_opcode_list,&a.opcode_lists),(b.has_gain_map,b.has_opcode_list,&b.opcode_lists));
    assert_eq!([a.iso,a.shutter_s,a.aperture,a.focal_mm].map(f32::to_bits),[b.iso,b.shutter_s,b.aperture,b.focal_mm].map(f32::to_bits));
    assert_eq!(a.black_levels.map(f32::to_bits),b.black_levels.map(f32::to_bits));assert_eq!(a.as_shot_wb.map(f32::to_bits),b.as_shot_wb.map(f32::to_bits));
    assert_eq!(a.camera_to_xyz.0.map(|r|r.map(f64::to_bits)),b.camera_to_xyz.0.map(|r|r.map(f64::to_bits)));
    assert_eq!(a.cam_xyz.map(|r|r.map(f32::to_bits)),b.cam_xyz.map(|r|r.map(f32::to_bits)));
    assert_eq!(a.rgb_cam.map(|r|r.map(f32::to_bits)),b.rgb_cam.map(|r|r.map(f32::to_bits)));
}

// Actual decoder qualification: explicit opt-in; missing fixtures are not evidence
// of five-camera support. Run with --nocapture and inventory originals before/after.
#[test]
#[ignore = "requires explicit five-family TESSERA_CAPTURED_CFA_FIXTURES qualification"]
fn actual_cfa_fixtures_return_owned_samples_after_original_copy_replacement() {
    let directory=std::env::var_os("TESSERA_CAPTURED_CFA_FIXTURES")
        .expect("qualification requires explicit five-family fixture directory");
    for (name,suffix) in [("sony-arw.ARW","ARW"),("fuji-raf.RAF","RAF"),("nikon-nef.NEF","NEF"),("canon-cr3.CR3","CR3"),("sample.dng","DNG")] {
        let original=PathBuf::from(&directory).join(name);assert!(original.is_file(),"required fixture {name}");
        let root=tempfile::tempdir().unwrap();let copy=root.path().join("relocated.bin");fs::copy(&original,&copy).unwrap();
        let mut direct=RawSource::open(&copy).unwrap();let expected=direct.decode_cfa_u16().unwrap();let metadata=direct.metadata();drop(direct);
        let len=fs::metadata(&copy).unwrap().len();let pool=CapturePool::create(root.path(),CaptureLimits{max_asset_bytes:len,max_staged_bytes:len,max_live_captures:1}).unwrap();
        let captured=pool.capture(&copy,PinnedRawDecoderRoute::LibRawCfaV1,suffix,None,&CancellationToken::new()).unwrap();
        let identity=captured.identity();assert_eq!(captured.suffix(),suffix.to_ascii_lowercase());let stage=captured.path_for_test().to_path_buf();assert_eq!(stage.extension().unwrap().to_str().unwrap(),suffix.to_ascii_lowercase());let private_dir=pool.directory_for_test().to_path_buf();drop(pool);
        fs::rename(&copy,root.path().join("original-moved")).unwrap();fs::write(&copy,b"invalid replacement B").unwrap();
        let decoded=captured.decode_cfa(&CancellationToken::new()).unwrap();assert!(!stage.exists());assert!(!private_dir.exists());
        assert_eq!((decoded.image.width,decoded.image.height,decoded.image.cfa_layout),(expected.width,expected.height,expected.cfa_layout));
        assert_eq!(decoded.image.data,expected.data);assert_metadata(&decoded.metadata,&metadata);assert_eq!(decoded.identity,identity);
        assert_eq!(decoded.route,PinnedRawDecoderRoute::LibRawCfaV1);assert_eq!(fs::read(&copy).unwrap(),b"invalid replacement B");
        eprintln!("EXERCISED captured CFA {name}: {}x{}, {} owned samples",decoded.image.width,decoded.image.height,decoded.image.data.len());
    }
}


#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::capture) enum DecodePhase { Classified, Opened, Decoded }

// Break caught: native open starts after successful classification even though
// cancellation was requested. Invalid RAW would Decode-fail if open were attempted.
#[test]
fn cancellation_after_classification_prevents_native_open() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"not raw").unwrap();
    let pool=CapturePool::create(root.path(),limits(64,1)).unwrap();let captured=owner(&pool,&source);
    let token=CancellationToken::new();let mut phases=Vec::new();
    let result=captured.decode_cfa_with_phase_for_test(&token,|phase| {
        phases.push(phase);if phase==DecodePhase::Classified {token.cancel();}
    });
    assert!(matches!(result,Err(EngineError::Cancelled)));
    assert_eq!(phases,[DecodePhase::Classified]);clean(&pool);
}

// Break caught: a completed decoder error is overwritten by a concurrent cancel.
#[test]
fn completed_decoder_error_precedes_concurrent_cancellation() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"protocol").unwrap();
    let pool=CapturePool::create(root.path(),limits(64,1)).unwrap();let token=CancellationToken::new();
    let result=owner(&pool,&source).decode_cfa_with_for_test(&token,|_,cancel| {
        cancel.cancel();Err(EngineError::Decode{format:"synthetic".into(),message:"completed failure".into()})
    });
    assert!(matches!(result,Err(EngineError::Decode{ref message,..}) if message=="completed failure"));clean(&pool);
}

// Break caught: output pairs incompatible sensor plane and metadata dimensions/layout.
#[test]
fn closed_result_requires_matching_sensor_dimensions_and_layout() {
    for mismatch in ["size","layout"] {
        let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"protocol").unwrap();
        let pool=CapturePool::create(root.path(),limits(64,1)).unwrap();
        let result=owner(&pool,&source).decode_cfa_with_for_test(&CancellationToken::new(),|_,_| {
            let (image,mut metadata)=synthetic_output();
            if mismatch=="size" {metadata.width=3;}else{metadata.cfa_layout=CfaLayout::Unsupported;}
            Ok((image,metadata))
        });
        assert!(matches!(result,Err(EngineError::Decode{..})),"{mismatch}");clean(&pool);
    }
}

// Actual native boundaries, not fake drop instrumentation. Both named phases
// follow successful calls; no claim of cancellation inside LibRaw open/unpack.
#[test]
#[ignore = "requires explicit Sony CFA fixture for native cancellation boundaries"]
fn actual_native_success_boundaries_observe_cancellation_before_publication() {
    let directory=std::env::var_os("TESSERA_CAPTURED_CFA_FIXTURES").expect("fixture directory required");
    for stop in [DecodePhase::Opened,DecodePhase::Decoded] {
        let root=tempfile::tempdir().unwrap();let source=root.path().join("copy.arw");
        fs::copy(PathBuf::from(&directory).join("sony-arw.ARW"),&source).unwrap();
        let n=fs::metadata(&source).unwrap().len();
        let pool=CapturePool::create(root.path(),CaptureLimits{max_asset_bytes:n,max_staged_bytes:n,max_live_captures:1}).unwrap();
        let captured=owner(&pool,&source);let token=CancellationToken::new();let mut phases=Vec::new();
        let result=captured.decode_cfa_with_phase_for_test(&token,|phase|{phases.push(phase);if phase==stop{token.cancel();}});
        assert!(matches!(result,Err(EngineError::Cancelled)));assert_eq!(phases.last(),Some(&stop));clean(&pool);
    }
}


// Break caught: cleanup failure overrides cancellation after successful decoding.
#[test]
fn cancelled_decode_preserves_primary_error_when_unlink_fails() {
    let root=tempfile::tempdir().unwrap();let source=root.path().join("source.bin");fs::write(&source,b"protocol").unwrap();
    let calls=Arc::new(AtomicUsize::new(0));let count=calls.clone();
    let operations=TestOperations{remove_file:Arc::new(move |path|{
        if count.fetch_add(1,Ordering::SeqCst)==0{Err(io::Error::other("cancel cleanup denied"))}else{fs::remove_file(path)}
    }),..TestOperations::default()};let diagnostics=operations.diagnostics.clone();
    let pool=CapturePool::create_with_operations_for_test(root.path(),limits(64,1),operations).unwrap();
    let token=CancellationToken::new();let captured=owner(&pool,&source);let stage=captured.path_for_test().to_path_buf();
    let result=captured.decode_cfa_with_for_test(&token,|_,cancel|{cancel.cancel();Ok(synthetic_output())});
    assert!(matches!(result,Err(EngineError::Cancelled)));assert_eq!(pool.accounting_for_test(),(64,1));assert!(stage.exists());
    assert_eq!(diagnostics.lock().unwrap().iter().filter(|d|d.contains("cancel cleanup denied")).count(),1);
    assert_eq!(calls.load(Ordering::SeqCst),1);drop(pool);assert_eq!(calls.load(Ordering::SeqCst),2);assert!(!stage.exists());
}
