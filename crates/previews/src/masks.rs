//! Lossless f32 AI mask rasters, separate from lossy JPEG previews.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

#[derive(Clone, Debug, PartialEq)]
pub struct MaskRaster {
    width: u32,
    height: u32,
    data: Vec<f32>,
}
impl MaskRaster {
    pub fn new(width: u32, height: u32, data: Vec<f32>) -> io::Result<Self> {
        if width == 0
            || height == 0
            || (width as usize).checked_mul(height as usize) != Some(data.len())
            || data
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid alpha raster",
            ));
        }
        Ok(Self {
            width,
            height,
            data,
        })
    }
    pub fn width(&self) -> u32 {
        self.width
    }
    pub fn height(&self) -> u32 {
        self.height
    }
    pub fn data(&self) -> &[f32] {
        &self.data
    }
    pub fn inverted(&self) -> Self {
        Self {
            width: self.width,
            height: self.height,
            data: self.data.iter().map(|v| 1. - v).collect(),
        }
    }
}

/// Content-addressed files with atomic replacement and an independent disk cap.
/// Payload checksum catches truncated or corrupt entries, which are cache misses.
/// Store instances sharing a directory may race eviction, but never expose partial files.
pub struct MaskStore {
    root: PathBuf,
    cap: u64,
    io: Mutex<()>,
}
impl MaskStore {
    pub fn new(root: impl AsRef<Path>, cap: u64) -> io::Result<Self> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: root.as_ref().into(),
            cap,
            io: Mutex::new(()),
        })
    }
    fn path(&self, key: &[u8; 32]) -> PathBuf {
        self.root
            .join(format!("{}.mask", blake3::Hash::from_bytes(*key).to_hex()))
    }
    pub fn get(&self, key: &[u8; 32]) -> Option<MaskRaster> {
        let _lock = self.io.lock().ok()?;
        let path = self.path(key);
        if fs::metadata(&path).ok()?.len() > self.cap {
            return None;
        }
        let bytes = fs::read(path).ok()?;
        if bytes.len() < 48 || &bytes[..8] != b"TSMASK01" {
            return None;
        }
        let (payload, checksum) = bytes.split_at(bytes.len() - 32);
        if blake3::hash(payload).as_bytes() != checksum {
            return None;
        }
        let width = u32::from_le_bytes(bytes[8..12].try_into().ok()?);
        let height = u32::from_le_bytes(bytes[12..16].try_into().ok()?);
        if (payload.len() - 16) % 4 != 0 {
            return None;
        }
        let data = payload[16..]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|v| f32::from_le_bytes(*v))
            .collect();
        MaskRaster::new(width, height, data).ok()
    }
    pub fn put(&self, key: &[u8; 32], raster: &MaskRaster) -> io::Result<()> {
        let _lock = self.io.lock().unwrap_or_else(|e| e.into_inner());
        if raster.data.len() as u64 * 4 + 48 > self.cap {
            return Ok(());
        }
        let mut bytes = b"TSMASK01".to_vec();
        bytes.extend(raster.width.to_le_bytes());
        bytes.extend(raster.height.to_le_bytes());
        for value in &raster.data {
            bytes.extend(value.to_le_bytes());
        }
        bytes.extend(*blake3::hash(&bytes).as_bytes());
        let mut temp = tempfile::NamedTempFile::new_in(&self.root)?;
        temp.write_all(&bytes)?;
        temp.as_file().sync_all()?;
        temp.persist(self.path(key)).map_err(|e| e.error)?;
        let mut entries = Vec::new();
        let mut total = 0;
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.path().extension().is_some_and(|s| s == "mask") {
                let Ok(meta) = entry.metadata() else { continue };
                total += meta.len();
                entries.push((meta.modified()?, entry.path(), meta.len()));
            }
        }
        entries.sort();
        for (_, path, len) in entries {
            if total <= self.cap {
                break;
            }
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
            total -= len;
        }
        Ok(())
    }
}
