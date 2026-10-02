#[test]
fn ordinary_dng_retains_default_baseline_exposure() {
    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
    let source = raw_decode::RawSource::open(path).unwrap();
    assert_eq!(source.metadata().baseline_exposure, -0.5);
}
