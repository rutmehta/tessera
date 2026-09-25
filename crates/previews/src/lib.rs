//! Content-addressed JPEG preview pyramids.
mod raw;
use image::{RgbImage, imageops::FilterType};
pub use raw::PreviewSource;
use std::{
    collections::HashMap,
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
impl PreviewKey {
    pub fn new(bytes: &[u8], orientation: u8, recipe_hash: [u8; 32]) -> Self {
        Self {
            file_hash: *blake3::hash(bytes).as_bytes(),
            orientation,
            recipe_hash,
        }
    }
    fn directory(&self) -> String {
        format!(
            "{}-{}-{}",
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
    io: Mutex<()>,
    renders: std::sync::atomic::AtomicU64,
    root: PathBuf,
    cap: u64,
    access: Mutex<HashMap<PathBuf, u64>>,
}
impl PreviewStore {
    pub fn new(root: impl AsRef<Path>, cap_bytes: u64) -> Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: root.as_ref().to_owned(),
            io: Mutex::new(()),
            renders: std::sync::atomic::AtomicU64::new(0),
            cap: cap_bytes,
            access: Mutex::new(HashMap::new()),
        })
    }
    fn path(&self, key: &PreviewKey, level: Level) -> PathBuf {
        self.root
            .join(key.directory())
            .join(format!("{}.jpg", level.divisor()))
    }
    pub fn get(&self, key: &PreviewKey, level: Level) -> Option<Bytes> {
        let _io = self.io.lock().ok()?;
        let p = self.path(key, level);
        let bytes = fs::read(&p).ok()?;
        self.access.lock().ok()?.insert(p, tick());
        Some(bytes)
    }
    pub fn put(&self, key: &PreviewKey, level: Level, bytes: &[u8]) -> Result<()> {
        let _io = self.io.lock().unwrap_or_else(|e| e.into_inner());
        let p = self.path(key, level);
        fs::create_dir_all(p.parent().unwrap())?;
        fs::write(&p, bytes)?;
        self.access.lock().unwrap().insert(p, tick());
        self.evict()?;
        Ok(())
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
    fn evict(&self) -> Result<()> {
        let mut files = Vec::new();
        let mut total = 0u64;
        for dir in fs::read_dir(&self.root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for f in fs::read_dir(dir.path())? {
                let f = f?;
                let n = f.metadata()?.len();
                total += n;
                let stamp = self
                    .access
                    .lock()
                    .unwrap()
                    .get(&f.path())
                    .copied()
                    .unwrap_or(0);
                files.push((stamp, f.path(), n));
            }
        }
        files.sort_by_key(|x| x.0);
        for (_, path, size) in files {
            if total <= self.cap {
                break;
            }
            fs::remove_file(&path)?;
            total -= size;
        }
        Ok(())
    }
}
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
        if !cfg!(debug_assertions) {
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
}
