//! Opt-in prerequisite check; no private fixture or metadata is retained.

#[test]
fn lightroom_smart_preview_libraw_decode() {
    let Some(path) = std::env::var_os("TESSERA_SMART_PREVIEW_SAMPLE") else {
        eprintln!("smart preview sample not supplied; opt-in decode check skipped");
        return;
    };
    let decoded = std::fs::File::open(path).ok()
        .and_then(|mut file| raw_decode::lossy_dng::read(&mut file).ok().flatten())
        .expect("decodes: no, dimensions: unavailable");
    assert!(!decoded.pixels.is_empty());
    eprintln!("decodes: yes, dimensions: {}x{}", decoded.width, decoded.height);
}
