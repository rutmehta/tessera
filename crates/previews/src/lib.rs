//! Content-addressed JPEG preview pyramids.
mod disk;
pub mod masks;
mod raw;
mod revision;
use image::{RgbImage, imageops::FilterType};
pub use raw::PreviewSource;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum Level {
    Eighth,
    Quarter,
    Half,
    Full,
}
impl Level {
    pub const ALL: [Level; 4] = [Self::Eighth, Self::Quarter, Self::Half, Self::Full];
    fn divisor(self) -> u32 {
        match self {
            Self::Eighth => 8,
            Self::Quarter => 4,
            Self::Half => 2,
            Self::Full => 1,
        }
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct PreviewKey {
    pub file_hash: [u8; 32],
    pub orientation: u8,
    pub recipe_hash: [u8; 32],
}
/// Bumped when the renderer's output for an unchanged recipe changes, so disk
/// previews rendered by an older engine are never served. Epoch 1 used
/// unprefixed directories. Epoch 2 (ENG-7/7b): the default lens mode no
/// longer applies image-estimated distortion, built-in DNG opcode corrections
/// apply in every profile mode, and automatic CA is off by default. Epoch 3
/// (ENG-8): maker-note built-in corrections (Fujifilm) apply in every profile
/// mode. Stale directories are left to LRU eviction.
pub const RENDER_EPOCH: u32 = 3;

impl PreviewKey {
    pub fn new(bytes: &[u8], orientation: u8, recipe_hash: [u8; 32]) -> Self {
        Self {
            file_hash: *blake3::hash(bytes).as_bytes(),
            orientation,
            recipe_hash,
        }
    }
    /// On-disk location. The render epoch is part of the path only, so key
    /// equality and every key constructor stay unchanged.
    fn directory(&self) -> String {
        format!(
            "e{RENDER_EPOCH}-{}-{}-{}",
            hex(&self.file_hash),
            self.orientation,
            hex(&self.recipe_hash)
        )
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, thiserror::Error)]
pub enum PreviewError {
    #[error("preview render error: {0}")]
    Render(#[from] engine_api::EngineError),
    #[error("image codec error: {0}")]
    Codec(#[from] image::ImageError),
    #[error("preview cache I/O error: {0}")]
    Io(#[from] std::io::Error),
}
pub type Result<T> = std::result::Result<T, PreviewError>;
pub type Bytes = Vec<u8>;

pub trait Codec: Send + Sync {
    fn encode(&self, image: &RgbImage) -> Result<Vec<u8>>;
    fn decode(&self, bytes: &[u8]) -> Result<RgbImage>;
}
pub struct Jpeg;
impl Codec for Jpeg {
    fn encode(&self, image: &RgbImage) -> Result<Vec<u8>> {
        let mut data = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, 90)
            .encode_image(&image::DynamicImage::ImageRgb8(image.clone()))?;
        Ok(data)
    }
    fn decode(&self, bytes: &[u8]) -> Result<RgbImage> {
        Ok(image::load_from_memory_with_format(bytes, image::ImageFormat::Jpeg)?.to_rgb8())
    }
}

pub struct PreviewStore {
    retouch: Option<std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>>,
    disk: std::sync::Arc<disk::Disk>,
    renders: std::sync::atomic::AtomicU64,
    source_work: std::sync::atomic::AtomicU64,
    root: PathBuf,
    cap: u64,
}
impl PreviewStore {
    pub fn new(root: impl AsRef<Path>, cap_bytes: u64) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            retouch: None,
            root: fs::canonicalize(root.as_ref())?,
            disk: disk::Disk::shared(root.as_ref(), cap_bytes)?,
            renders: std::sync::atomic::AtomicU64::new(0),
            source_work: std::sync::atomic::AtomicU64::new(0),
            cap: cap_bytes,
        })
    }
    /// Supply the engine-owned retouch implementation for edited source previews.
    pub fn with_retouch_renderer(
        mut self,
        renderer: std::sync::Arc<dyn pipeline_cpu::RetouchRenderer>,
    ) -> Self {
        self.retouch = Some(renderer);
        self
    }
    fn render_context(&self) -> pipeline_cpu::LensContext<'static> {
        pipeline_cpu::LensContext {
            retouch: self.retouch.clone(),
            ..Default::default()
        }
    }
    fn path(&self, key: &PreviewKey, level: Level) -> PathBuf {
        self.root
            .join(key.directory())
            .join(format!("{}.jpg", level.divisor()))
    }
    pub fn get(&self, key: &PreviewKey, level: Level) -> Option<Bytes> {
        self.disk.get(&self.path(key, level))
    }
    /// Bounded local-cache lookup for explicit offline consumers. Reject links and
    /// oversized cache files without falling back to source I/O. Ordinary get is unchanged.
    pub fn get_bounded(&self, key: &PreviewKey, level: Level, max_bytes: usize) -> Option<Bytes> {
        use std::io::Read;
        let path = self.path(key, level);
        for directory in [self.root.as_path(), path.parent()?] {
            if !fs::symlink_metadata(directory).ok()?.file_type().is_dir() {
                return None;
            }
        }
        if !fs::symlink_metadata(&path).ok()?.file_type().is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(u64::try_from(max_bytes).ok()?.checked_add(1)?)
            .read_to_end(&mut bytes)
            .ok()?;
        (!bytes.is_empty() && bytes.len() <= max_bytes).then_some(bytes)
    }
    /// Restricted cache publication for explicit local-only consumers. A linked
    /// directory/file or pre-existing temporary leaf is an error, never followed.
    pub fn put_image_local_cancellable(
        &self,
        key: &PreviewKey,
        image: &RgbImage,
        max_px: u32,
        check: &dyn Fn() -> engine_api::EngineResult<()>,
    ) -> Result<()> {
        let guarded = || -> engine_api::EngineResult<()> {
            check()?;
            let reject = || std::io::Error::other("unsafe local preview cache path");
            if !fs::symlink_metadata(&self.root)?.file_type().is_dir() {
                return Err(reject().into());
            }
            let directory = self.root.join(key.directory());
            match fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(reject().into()),
            }
            for level in Level::ALL {
                let path = self.path(key, level);
                match fs::symlink_metadata(&path) {
                    Ok(metadata) if metadata.file_type().is_file() => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return Err(reject().into()),
                }
                match fs::symlink_metadata(path.with_extension("tmp")) {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    _ => return Err(reject().into()),
                }
            }
            Ok(())
        };
        self.put_image_cancellable(key, image, max_px, &guarded)
    }
    pub fn put(&self, key: &PreviewKey, level: Level, bytes: &[u8]) -> Result<()> {
        self.disk.put(&self.path(key, level), bytes, self.cap)
    }
    pub fn ensure(
        &self,
        key: &PreviewKey,
        level: Level,
        source: &dyn Fn(Level) -> RgbImage,
    ) -> Result<()> {
        let target = level.divisor();
        let largest_cached = Level::ALL
            .into_iter()
            .filter(|candidate| candidate.divisor() <= target)
            .find_map(|candidate| self.get(key, candidate).map(|bytes| (candidate, bytes)));
        let (base_level, base) = match largest_cached {
            Some((cached_level, bytes)) => (cached_level, Jpeg.decode(&bytes)?),
            None => (Level::Full, source(level)),
        };
        for candidate in Level::ALL.into_iter().filter(|candidate| {
            candidate.divisor() <= target
                && (candidate.divisor() > 1 || level == Level::Full)
                && candidate.divisor() > base_level.divisor()
        }) {
            let scale = candidate.divisor() / base_level.divisor();
            let width = (base.width() / scale).max(1);
            let height = (base.height() / scale).max(1);
            let scaled = image::imageops::resize(&base, width, height, FilterType::Lanczos3);
            self.put(key, candidate, &Jpeg.encode(&scaled)?)?;
        }
        if self.get(key, level).is_none() {
            self.put(key, level, &Jpeg.encode(&base)?)?;
        }
        Ok(())
    }
    pub fn from_embedded_jpeg(
        &self,
        bytes: &[u8],
        orientation: u8,
        recipe_hash: [u8; 32],
    ) -> Result<PreviewKey> {
        let key = PreviewKey::new(bytes, orientation, recipe_hash);
        let mut img = Jpeg.decode(bytes)?;
        img = orient(img, orientation);
        let full = img.clone();
        for level in Level::ALL {
            let d = level.divisor();
            let scaled = image::imageops::resize(
                &full,
                (full.width() / d).max(1),
                (full.height() / d).max(1),
                FilterType::Lanczos3,
            );
            self.put(&key, level, &Jpeg.encode(&scaled)?)?;
        }
        Ok(key)
    }
}
#[cfg(test)]
fn tick() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}
fn orient(img: RgbImage, orientation: u8) -> RgbImage {
    match orientation {
        2 => image::imageops::flip_horizontal(&img),
        3 => image::imageops::rotate180(&img),
        4 => image::imageops::flip_vertical(&img),
        5 => image::imageops::rotate270(&image::imageops::flip_horizontal(&img)),
        6 => image::imageops::rotate90(&img),
        7 => image::imageops::rotate90(&image::imageops::flip_horizontal(&img)),
        8 => image::imageops::rotate270(&img),
        _ => img,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    /// ENG-7b: renders cached before the default lens mode stopped applying
    /// image-estimated distortion (render epoch 1: unprefixed directories)
    /// must not be served. ENG-8: neither may renders from before maker-note
    /// built-in corrections (epoch 2). Only the on-disk location changes; key
    /// equality, `recipe_hash` and every key constructor are untouched.
    #[test]
    fn previews_cached_under_an_earlier_render_epoch_miss() {
        let dir = tempfile::tempdir().unwrap();
        let k = PreviewKey::new(b"epoch", 1, [3; 32]);
        let name = format!(
            "{}-{}-{}",
            hex(&k.file_hash),
            k.orientation,
            hex(&k.recipe_hash)
        );
        let earlier = [name.clone(), format!("e2-{name}")];
        for legacy in &earlier {
            for level in Level::ALL {
                let d = dir.path().join(legacy);
                fs::create_dir_all(&d).unwrap();
                fs::write(d.join(format!("{}.jpg", level.divisor())), [7; 16]).unwrap();
            }
        }
        let s = PreviewStore::new(dir.path(), u64::MAX).unwrap();
        assert!(!earlier.contains(&k.directory()));
        assert!(k.directory().starts_with(&format!("e{RENDER_EPOCH}-")));
        const { assert!(RENDER_EPOCH >= 3) };
        for level in Level::ALL {
            assert!(
                s.get(&k, level).is_none(),
                "{level:?} served a stale render"
            );
        }
        s.put(&k, Level::Full, &[1; 16]).unwrap();
        assert_eq!(s.get(&k, Level::Full).unwrap(), [1; 16]);
        // Revision aliases use a new domain tag as well.
        let photo = dir.path().join("photo.jpg");
        fs::write(&photo, [0; 8]).unwrap();
        let revision = PreviewKey::for_source(&photo, 64, 1, [0; 32]).unwrap();
        assert!(
            revision
                .directory()
                .starts_with(&format!("e{RENDER_EPOCH}-"))
        );
        assert_eq!(revision::REVISION_DOMAIN, b"tessera-preview-revision-v3\0");
    }
    #[test]
    fn restart_recovers_cap_and_abandoned_writes() {
        let (p, s) = store(u64::MAX);
        let k = PreviewKey::new(b"restart", 1, [0; 32]);
        s.put(&k, Level::Full, &[1; 40]).unwrap();
        fs::write(s.path(&k, Level::Full).with_extension("tmp"), [0; 100]).unwrap();
        drop(s);
        let s = PreviewStore::new(&p, 20).unwrap();
        assert!(s.get(&k, Level::Full).is_none());
        assert!(!s.path(&k, Level::Full).with_extension("tmp").exists());
        fs::remove_dir_all(p).unwrap();
    }

    #[test]
    #[ignore = "M2-55 repeatable measurement"]
    fn cache_80k_measurement() {
        let (p, s) = store(u64::MAX);
        drop(s);
        for i in 0_u64..20_000 {
            let k = PreviewKey::new(&i.to_le_bytes(), 1, [0; 32]);
            let dir = p.join(k.directory());
            fs::create_dir(&dir).unwrap();
            for level in Level::ALL {
                fs::write(dir.join(format!("{}.jpg", level.divisor())), [0; 32]).unwrap();
            }
        }
        let start = std::time::Instant::now();
        let s = PreviewStore::new(&p, u64::MAX).unwrap();
        assert_eq!(s.disk.accounted_bytes(), 80_000 * 32);
        println!(
            "80k startup_ms={:.3}",
            start.elapsed().as_secs_f64() * 1000.
        );
        let k = PreviewKey::new(b"measurement", 1, [0; 32]);
        let mut times = Vec::new();
        for _ in 0..20 {
            let start = std::time::Instant::now();
            s.put(&k, Level::Full, &[1; 32]).unwrap();
            assert_eq!(s.get(&k, Level::Full).unwrap(), [1; 32]);
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.sort_by(f64::total_cmp);
        println!("80k put_get_p95_ms={:.3}", times[18]);
        assert_eq!(s.disk.accounted_bytes(), 80_001 * 32);
        // Force real bounded eviction at the seeded occupancy. Reader lock
        // independence is exercised separately with the writer mutex held.
        let start = std::time::Instant::now();
        s.disk
            .put(&s.path(&k, Level::Full), &[2; 2048], 80_000 * 32)
            .unwrap();
        println!(
            "80k maintenance_ms={:.3} reader_mutex_hold_ms=0 (no reader mutex)",
            start.elapsed().as_secs_f64() * 1000.
        );
        assert!(s.disk.accounted_bytes() <= 80_000 * 32);
        fs::remove_dir_all(p).unwrap();
    }

    #[test]
    #[ignore = "M2-55 baseline JPEG measurement"]
    fn jpeg_baseline_measurement() {
        let (p, s) = store(u64::MAX);
        let path = p.join("source.jpg");
        let img = RgbImage::from_fn(1536, 1024, |x, y| {
            image::Rgb([x as u8, y as u8, (x + y) as u8])
        });
        fs::write(&path, Jpeg.encode(&img).unwrap()).unwrap();
        let mut times = Vec::new();
        for _ in 0..101 {
            let start = std::time::Instant::now();
            let decoded = Jpeg.decode(&fs::read(&path).unwrap()).unwrap();
            let scaled = image::DynamicImage::ImageRgb8(decoded)
                .thumbnail(384, 384)
                .to_rgb8();
            let jpeg = Jpeg.encode(&scaled).unwrap();
            let k = PreviewKey::new(&jpeg, 1, [0; 32]);
            if s.get(&k, Level::Full).is_none() {
                s.from_embedded_jpeg(&jpeg, 1, [0; 32]).unwrap();
            }
            times.push(start.elapsed().as_secs_f64() * 1000.);
        }
        times.remove(0);
        times.sort_by(f64::total_cmp);
        println!("JPEG baseline warm_p95_ms={:.3}", times[94]);
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    fn all_exif_orientations() {
        let im = RgbImage::from_fn(2, 3, |x, y| image::Rgb([(y * 2 + x + 1) as u8; 3]));
        for (orientation, expected) in [
            (1, vec![1, 2, 3, 4, 5, 6]),
            (2, vec![2, 1, 4, 3, 6, 5]),
            (3, vec![6, 5, 4, 3, 2, 1]),
            (4, vec![5, 6, 3, 4, 1, 2]),
            (5, vec![1, 3, 5, 2, 4, 6]),
            (6, vec![5, 3, 1, 6, 4, 2]),
            (7, vec![6, 4, 2, 5, 3, 1]),
            (8, vec![2, 4, 6, 1, 3, 5]),
        ] {
            let out = orient(im.clone(), orientation);
            assert_eq!(
                out.pixels().map(|p| p[0]).collect::<Vec<_>>(),
                expected,
                "orientation {orientation}"
            );
        }
    }

    #[test]
    fn embedded_raw_does_not_render() {
        let (p, s) = store(u64::MAX);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW");
        let (key, source) = s.from_raw(&path, 384).unwrap();
        assert_eq!(source, PreviewSource::Embedded);
        assert_eq!(s.render_count(), 0);
        assert!(s.get(&key, Level::Full).is_some());
        fs::remove_dir_all(p).unwrap();
    }
    #[test]
    #[cfg_attr(
        debug_assertions,
        ignore = "release-only latency bound: skipped in debug builds"
    )]
    fn raw_without_jpeg_is_rendered() {
        let (p, s) = store(u64::MAX);
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sample.dng");
        let start = std::time::Instant::now();
        let (key, source) = s.from_raw(&path, 384).unwrap();
        assert_eq!(source, PreviewSource::Rendered);
        let im = Jpeg.decode(&s.get(&key, Level::Full).unwrap()).unwrap();
        let ys: Vec<f64> = im
            .pixels()
            .map(|p| {
                (0.2126 * f64::from(p[0]) + 0.7152 * f64::from(p[1]) + 0.0722 * f64::from(p[2]))
                    / 255.0
            })
            .collect();
        let mean = ys.iter().sum::<f64>() / ys.len() as f64;
        let stddev = (ys.iter().map(|y| (y - mean).powi(2)).sum::<f64>() / ys.len() as f64).sqrt();
        println!(
            "preview {:?}: mean={mean}, stddev={stddev}",
            start.elapsed()
        );
        assert!(mean > 0.02 && stddev > 0.01);
        // Timing budget is a local performance check; CI runners are far slower and shared.
        if !cfg!(debug_assertions) && std::env::var_os("CI").is_none() {
            assert!(start.elapsed().as_secs_f64() < 3.0);
        }
        fs::remove_dir_all(p).unwrap();
    }
    fn store(cap: u64) -> (PathBuf, PreviewStore) {
        let p = std::env::temp_dir().join(format!("previews-{}", tick()));
        let s = PreviewStore::new(&p, cap).unwrap();
        (p, s)
    }
    #[test]
    fn pyramid_dimensions() {
        let (p, s) = store(1_000_000);
        let k = PreviewKey::new(b"raw", 1, [0; 32]);
        s.ensure(&k, Level::Eighth, &|_| RgbImage::new(800, 400))
            .unwrap();
        for (l, w, h) in [
            (Level::Eighth, 100, 50),
            (Level::Quarter, 200, 100),
            (Level::Half, 400, 200),
        ] {
            let im = Jpeg.decode(&s.get(&k, l).unwrap()).unwrap();
            assert_eq!((im.width(), im.height()), (w, h));
        }
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn eviction_respects_cap() {
        let (p, s) = store(20);
        let k = PreviewKey::new(b"raw", 1, [0; 32]);
        s.put(&k, Level::Full, &[1; 40]).unwrap();
        let size = fs::metadata(s.path(&k, Level::Full))
            .map(|m| m.len())
            .unwrap_or(0);
        assert!(size <= 20);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn orientation_six_rotates() {
        let mut im = RgbImage::new(2, 3);
        for (x, y, c) in [
            (0, 0, [255, 0, 0]),
            (1, 0, [0, 255, 0]),
            (0, 1, [0, 0, 255]),
            (1, 1, [255, 255, 0]),
            (0, 2, [255, 0, 255]),
            (1, 2, [0, 255, 255]),
        ] {
            im.put_pixel(x, y, image::Rgb(c));
        }
        let out = orient(im, 6);
        assert_eq!((out.width(), out.height()), (3, 2));
        assert_eq!(out.get_pixel(2, 0).0, [255, 0, 0]);
        assert_eq!(out.get_pixel(0, 1).0, [0, 255, 255]);
    }
    #[test]
    #[ignore = "release performance check"]
    fn synthetic_45mp_under_400ms() {
        let (p, s) = store(u64::MAX);
        let k = PreviewKey::new(b"45mp", 1, [0; 32]);
        let image = RgbImage::new(8000, 5625);
        let start = std::time::Instant::now();
        s.ensure(&k, Level::Eighth, &|_| image.clone()).unwrap();
        let elapsed = start.elapsed();
        println!("45 MP pyramid: {elapsed:?}");
        assert!(elapsed.as_millis() < 400);
        let _ = fs::remove_dir_all(p);
    }
    #[test]
    fn bounded_lookup_rejects_oversized_empty_and_linked_cache_files() {
        let (root, store) = store(u64::MAX);
        let key = PreviewKey::new(b"bounded-offline", 1, [0; 32]);
        store.put(&key, Level::Full, b"12345").unwrap();
        assert_eq!(
            store.get_bounded(&key, Level::Full, 5),
            Some(b"12345".to_vec())
        );
        assert!(store.get_bounded(&key, Level::Full, 4).is_none());
        let path = store.path(&key, Level::Full);
        fs::write(&path, []).unwrap();
        assert!(store.get_bounded(&key, Level::Full, 5).is_none());
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let target = root.join("untouched-source");
            fs::write(&target, b"secret").unwrap();
            fs::remove_file(&path).unwrap();
            symlink(&target, &path).unwrap();
            assert!(store.get_bounded(&key, Level::Full, 100).is_none());
            assert_eq!(fs::read(&target).unwrap(), b"secret");
            fs::remove_file(&path).unwrap();
            let directory = path.parent().unwrap();
            let moved = root.join("moved-key");
            fs::rename(directory, &moved).unwrap();
            fs::write(moved.join("1.jpg"), b"linked").unwrap();
            symlink(&moved, directory).unwrap();
            assert!(store.get_bounded(&key, Level::Full, 100).is_none());
        }
        let _ = fs::remove_dir_all(root);
    }
}
