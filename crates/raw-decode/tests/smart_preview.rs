//! Opt-in prerequisite check; no private fixture or metadata is retained.

#[test]
fn lightroom_smart_preview_libraw_decode() {
    let Some(path) = std::env::var_os("TESSERA_SMART_PREVIEW_SAMPLE") else {
        eprintln!("smart preview sample not supplied; opt-in decode check skipped");
        return;
    };
    let dimensions = libraw_ffi::RawFile::open(&path).ok().map(|raw| {
        let info = raw.sensor_info();
        (info.width, info.height)
    });
    let decoded = std::fs::File::open(path)
        .ok()
        .and_then(|mut file| raw_decode::lossy_dng::read(&mut file).ok().flatten());
    let Some(decoded) = decoded else {
        if let Some((width, height)) = dimensions {
            panic!("decodes: no, dimensions: {width}x{height}");
        }
        panic!("decodes: no, dimensions: unavailable");
    };
    assert!(!decoded.pixels.is_empty());
    eprintln!("decodes: yes, {} × {}", decoded.width, decoded.height);
}
