use previews::{Codec, Jpeg, Level, PreviewKey, PreviewStore};
use std::{fs, path::Path, time::Instant};

#[test]
fn jpeg_disk_hit_is_byte_identical_and_does_no_source_work() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source.jpg");
    let image = image::RgbImage::from_fn(1536, 1024, |x, y| {
        image::Rgb([x as u8, y as u8, (x + y) as u8])
    });
    fs::write(&path, Jpeg.encode(&image).unwrap()).unwrap();
    let cache = dir.path().join("cache");
    let store = PreviewStore::new(&cache, u64::MAX).unwrap();
    let bytes = store.jpeg_preview(&path, 384, 6, [0; 32]).unwrap();
    // Preserve the previous resize/encode/decode/orient/encode boundary exactly.
    let scaled = image::DynamicImage::ImageRgb8(Jpeg.decode(&fs::read(&path).unwrap()).unwrap())
        .thumbnail(384, 384)
        .to_rgb8();
    let legacy = store
        .from_embedded_jpeg(&Jpeg.encode(&scaled).unwrap(), 6, [0; 32])
        .unwrap();
    assert_eq!(bytes, store.get(&legacy, Level::Full).unwrap());
    drop(store);
    let store = PreviewStore::new(&cache, u64::MAX).unwrap();
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        assert_eq!(store.jpeg_preview(&path, 384, 6, [0; 32]).unwrap(), bytes);
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "JPEG revision warm_p95_ms={:.3} source_work={}",
        times[94],
        store.source_work_count()
    );
    assert_eq!(store.source_work_count(), 0);
    if !cfg!(debug_assertions) {
        assert!(times[94] < 20.);
    }
    assert_ne!(store.jpeg_preview(&path, 384, 1, [0; 32]).unwrap(), bytes);
    assert_ne!(store.jpeg_preview(&path, 192, 6, [0; 32]).unwrap(), bytes);
    store.jpeg_preview(&path, 384, 6, [1; 32]).unwrap();
    assert_eq!(store.source_work_count(), 3);
}

#[test]
fn revision_detects_replacement_and_same_size_rewrite_with_preserved_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("source");
    fs::write(&path, b"one").unwrap();
    let key = PreviewKey::for_source(&path, 384, 1, [0; 32]).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, b"two").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    assert_ne!(key, PreviewKey::for_source(&path, 384, 1, [0; 32]).unwrap());
    let other = dir.path().join("other");
    fs::write(&other, b"one").unwrap();
    fs::File::options()
        .write(true)
        .open(&other)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    fs::rename(other, &path).unwrap();
    assert_ne!(key, PreviewKey::for_source(&path, 384, 1, [0; 32]).unwrap());
}

#[test]
fn raw_reopen_hits_before_any_source_open_or_decode() {
    let dir = tempfile::tempdir().unwrap();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW");
    let cache = dir.path().join("cache");
    let store = PreviewStore::new(&cache, u64::MAX).unwrap();
    let (key, source) = store.from_raw(&path, 384).unwrap();
    let bytes = store.get(&key, Level::Full).unwrap();
    drop(store);
    let store = PreviewStore::new(&cache, u64::MAX).unwrap();
    let mut times = Vec::new();
    for _ in 0..100 {
        let start = Instant::now();
        assert_eq!(store.from_raw(&path, 384).unwrap(), (key.clone(), source));
        assert_eq!(store.get(&key, Level::Full).unwrap(), bytes);
        times.push(start.elapsed().as_secs_f64() * 1000.);
    }
    times.sort_by(f64::total_cmp);
    println!(
        "RAW revision warm_p95_ms={:.3} source_work={}",
        times[94],
        store.source_work_count()
    );
    assert_eq!(store.source_work_count(), 0);
    if !cfg!(debug_assertions) {
        assert!(times[94] < 20.);
    }
}
