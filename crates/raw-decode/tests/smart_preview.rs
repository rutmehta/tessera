//! Opt-in prerequisite check; no private fixture or metadata is retained.

#[test]
fn lightroom_smart_preview_libraw_decode() {
    let Some(path) = std::env::var_os("TESSERA_SMART_PREVIEW_SAMPLE") else {
        eprintln!("smart preview sample not supplied; opt-in decode check skipped");
        return;
    };
    // Exercise LibRaw directly: a linear RGB DNG is not a CFA mosaic, so the
    // CFA-only RawSource decode API is not an appropriate format probe.
    let Ok(mut raw) = libraw_ffi::RawFile::open(path) else {
        panic!("decodes: no, dimensions: unavailable");
    };
    let info = raw.sensor_info();
    assert!(
        raw.unpack().is_ok(),
        "decodes: no, dimensions: {}x{}",
        info.width,
        info.height
    );
    assert!(info.width > 0 && info.height > 0);
    eprintln!("decodes: yes, dimensions: {}x{}", info.width, info.height);
}
