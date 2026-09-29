//! Positive downstream typechecking only; no decode/native call at runtime.
#[path = "owned_api/pass.rs"]
#[allow(dead_code)]
mod positive;
#[test]
fn documented_owned_api_typechecks_downstream() {
    let _ = positive::use_owner;
}

#[test]
#[ignore = "requires explicit five-family TESSERA_CAPTURED_CFA_FIXTURES; public API only"]
fn external_owned_api_five_families_survive_capture_cleanup() {
    use engine_api::{jobs::CancellationToken, pinned_raw::PinnedRawDecoderRoute};
    use raw_decode::capture::{CaptureLimits, CapturePool};
    let directory = std::path::PathBuf::from(
        std::env::var_os("TESSERA_CAPTURED_CFA_FIXTURES")
            .expect("explicit fixture inventory required"),
    );
    for (name, suffix) in [
        ("sony-arw.ARW", "arw"),
        ("fuji-raf.RAF", "raf"),
        ("nikon-nef.NEF", "nef"),
        ("canon-cr3.CR3", "cr3"),
        ("sample.dng", "dng"),
    ] {
        let original = directory.join(name);
        assert!(original.is_file(), "missing {name}");
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source.bin");
        std::fs::copy(&original, &source).unwrap();
        let bytes = std::fs::metadata(&source).unwrap().len();
        let pool = CapturePool::create(
            root.path(),
            CaptureLimits {
                max_asset_bytes: bytes,
                max_staged_bytes: bytes,
                max_live_captures: 1,
            },
        )
        .unwrap();
        let token = CancellationToken::new();
        let captured = pool
            .capture(
                &source,
                PinnedRawDecoderRoute::LibRawCfaV1,
                suffix,
                None,
                &token,
            )
            .unwrap();
        let identity = captured.identity();
        drop(pool);
        std::fs::write(&source, b"replacement B").unwrap();
        let decoded = captured.decode_owned_cfa(&token).unwrap();
        assert_eq!(decoded.identity(), identity);
        assert_eq!(decoded.route(), PinnedRawDecoderRoute::LibRawCfaV1);
        let facts = decoded.plane_facts();
        assert_eq!(
            (facts.width, facts.height, facts.layout),
            (
                decoded.metadata().width,
                decoded.metadata().height,
                decoded.metadata().cfa_layout
            )
        );
        assert_eq!(
            facts.sample_len as u64,
            u64::from(facts.width) * u64::from(facts.height)
        );
        assert!(facts.sample_capacity >= facts.sample_len);
        // Pool's directory must be removed after last capture's successful cleanup.
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(source).unwrap(), b"replacement B");
        eprintln!("EXERCISED external opaque API {name}");
    }
}
