//! Metadata-only revision lookup; content keys remain compatible with Develop.
use super::*;
use std::sync::atomic::Ordering;

/// Domain tag of source-revision keys; v2 since render epoch 2 (ENG-7b), v3
/// since render epoch 3 (ENG-8).
pub(crate) const REVISION_DOMAIN: &[u8] = b"tessera-preview-revision-v3\0";

impl PreviewKey {
    /// No source open/read. On Unix, inode/device + nanosecond mtime/ctime +
    /// length catch replacement and in-place edits, including restored mtime.
    /// Orientation/profile embedded in RAW are covered by this source revision.
    /// Explicit orientation and recipe changes remain separate key components.
    pub fn for_source(
        path: &Path,
        max_px: u32,
        orientation: u8,
        recipe_hash: [u8; 32],
    ) -> Result<Self> {
        let metadata = fs::metadata(path)?;
        let mut hash = blake3::Hasher::new();
        hash.update(REVISION_DOMAIN);
        hash.update(path.as_os_str().as_encoded_bytes());
        hash.update(&max_px.to_le_bytes());
        hash.update(&metadata.len().to_le_bytes());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            hash.update(&metadata.dev().to_le_bytes());
            hash.update(&metadata.ino().to_le_bytes());
            hash.update(&metadata.mtime().to_le_bytes());
            hash.update(&metadata.mtime_nsec().to_le_bytes());
            hash.update(&metadata.ctime().to_le_bytes());
            hash.update(&metadata.ctime_nsec().to_le_bytes());
        }
        #[cfg(not(unix))]
        {
            // Conservative fallback: platforms without Unix revision metadata
            // use a content digest rather than risk stale same-size replacements.
            hash.update(&fs::read(path)?);
        }
        Ok(Self {
            file_hash: *hash.finalize().as_bytes(),
            orientation,
            recipe_hash,
        })
    }
}
impl PreviewStore {
    /// Cold source-work boundaries entered (includes RAW header opens). A warm
    /// disk hit must leave this unchanged, excluding even source codec setup.
    pub fn source_work_count(&self) -> u64 {
        self.source_work.load(Ordering::Relaxed)
    }
    pub fn jpeg_preview(
        &self,
        path: &Path,
        max_px: u32,
        orientation: u8,
        recipe_hash: [u8; 32],
    ) -> Result<Bytes> {
        if max_px == 0 {
            return Err(engine_api::EngineError::invalid("max_px", "must be positive").into());
        }
        let key = PreviewKey::for_source(path, max_px, orientation, recipe_hash)?;
        if let Some(bytes) = self.get(&key, Level::Full) {
            return Ok(bytes);
        }
        self.source_work.fetch_add(1, Ordering::Relaxed);
        let decoded = Jpeg.decode(&fs::read(path)?)?;
        let scaled = image::DynamicImage::ImageRgb8(decoded)
            .thumbnail(max_px, max_px)
            .to_rgb8();
        // Keep the old two JPEG generations, color and orientation behavior.
        let full = orient(Jpeg.decode(&Jpeg.encode(&scaled)?)?, orientation);
        let bytes = Jpeg.encode(&image::imageops::resize(
            &full,
            full.width(),
            full.height(),
            FilterType::Lanczos3,
        ))?;
        if key == PreviewKey::for_source(path, max_px, orientation, recipe_hash)? {
            self.put(&key, Level::Full, &bytes)?;
        }
        Ok(bytes)
    }
    pub(super) fn raw_alias(&self, revision: &PreviewKey) -> Option<(PreviewKey, PreviewSource)> {
        let bytes = self.get(revision, Level::Full)?;
        if bytes.len() != 35 || bytes[0] != 1 || !(1..=8).contains(&bytes[2]) {
            return None;
        }
        let source = match bytes[1] {
            0 => PreviewSource::Embedded,
            1 => PreviewSource::Rendered,
            _ => return None,
        };
        let key = PreviewKey {
            file_hash: bytes[3..].try_into().ok()?,
            orientation: bytes[2],
            recipe_hash: revision.recipe_hash,
        };
        self.get(&key, Level::Full)?;
        Some((key, source))
    }
    pub(super) fn put_raw_alias(
        &self,
        revision: &PreviewKey,
        key: &PreviewKey,
        source: PreviewSource,
    ) -> Result<()> {
        let mut bytes = vec![
            1,
            if source == PreviewSource::Embedded {
                0
            } else {
                1
            },
            key.orientation,
        ];
        bytes.extend_from_slice(&key.file_hash);
        self.disk
            .put_if_room(&self.path(revision, Level::Full), &bytes, self.cap)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alias_admission_never_evicts_the_preview_it_names() {
        let dir = tempfile::tempdir().unwrap();
        let store = PreviewStore::new(dir.path(), 100).unwrap();
        let key = PreviewKey::new(b"image", 1, [0; 32]);
        let revision = PreviewKey::new(b"revision", 0, [0; 32]);
        store.put(&key, Level::Full, &[1; 100]).unwrap();
        store
            .put_raw_alias(&revision, &key, PreviewSource::Embedded)
            .unwrap();
        assert_eq!(store.get(&key, Level::Full).unwrap(), [1; 100]);
        assert!(store.raw_alias(&revision).is_none());
    }
}
