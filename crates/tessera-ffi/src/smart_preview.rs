//! Local camera-linear assets and offline recipe authority. No proxy becomes a catalog photo.
use crate::{
    Engine, Result, catalog, failure, image_edit_admission, parse_id,
    smart_preview_store::{JournalSnapshot, SmartPreviewJournal},
};
use engine_api::{id::ImageId, recipe::Recipe};
use image_core::RawImage;
use pipeline_cpu::{CameraLinearProxy, SmartPreviewTier};
use std::{
    fs,
    io::{Read, Seek, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum SmartPreviewState {
    Missing,
    Ready,
    OriginalOffline,
    Dirty,
    Stale,
    Conflict,
    Failed,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct SmartPreviewInfo {
    pub image_id: String,
    pub state: SmartPreviewState,
    pub original_available: bool,
    pub dirty: bool,
    pub width: u32,
    pub height: u32,
    pub message: String,
}
const MAX_ORIGINAL_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MAX_PROXY_BYTES: u64 = 256 * 1024 * 1024;
pub(crate) fn optional_bytes(path: impl AsRef<Path>) -> Result<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(v) => Ok(Some(v)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn bounded_bytes(path: &Path, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(max + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > max {
        return Err(failure("Smart Preview size limit exceeded"));
    }
    Ok(bytes)
}
fn hash_reader(reader: impl Read) -> Result<([u8; 32], u64)> {
    let mut reader = reader.take(MAX_ORIGINAL_BYTES + 1);
    let mut hash = blake3::Hasher::new();
    let mut count = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        count += n as u64;
    }
    if count > MAX_ORIGINAL_BYTES {
        return Err(failure("original exceeds Smart Preview build limit"));
    }
    Ok((*hash.finalize().as_bytes(), count))
}
fn fingerprint(meta: &fs::Metadata) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            meta.dev(),
            meta.ino(),
            meta.len(),
            meta.mtime(),
            meta.mtime_nsec(),
            meta.ctime(),
            meta.ctime_nsec()
        )
    }
    #[cfg(not(unix))]
    {
        format!("{}:{:?}:{:?}", meta.len(), meta.modified(), meta.created())
    }
}
/// Decode only a pinned local snapshot. Verify both the opened inode and the path
/// again against a second streaming digest; any observed mutation fails the build.
fn pinned_original(
    path: &Path,
    directory: &Path,
) -> Result<(tempfile::NamedTempFile, [u8; 32], u64)> {
    let mut source =
        fs::File::open(path).map_err(|e| failure(format!("original unavailable: {e}")))?;
    let before = fingerprint(&source.metadata()?);
    if source.metadata()?.len() > MAX_ORIGINAL_BYTES {
        return Err(failure("original exceeds Smart Preview build limit"));
    }
    let suffix = format!(
        ".{}",
        path.extension().and_then(|v| v.to_str()).unwrap_or("raw")
    );
    let mut copy = tempfile::Builder::new()
        .prefix("source-")
        .suffix(&suffix)
        .tempfile_in(directory)?;
    let mut hash = blake3::Hasher::new();
    let mut len = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = source.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        len += n as u64;
        if len > MAX_ORIGINAL_BYTES {
            return Err(failure("original exceeds Smart Preview build limit"));
        }
        copy.write_all(&buffer[..n])?;
        hash.update(&buffer[..n]);
    }
    copy.as_file().sync_all()?;
    let digest = *hash.finalize().as_bytes();
    source.rewind()?;
    if hash_reader(&mut source)? != (digest, len)
        || before != fingerprint(&source.metadata()?)
        || before != fingerprint(&fs::metadata(path)?)
    {
        return Err(failure("original changed while building Smart Preview"));
    }
    Ok((copy, digest, len))
}
fn atomic_local(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_local_with_sync(path, bytes, |parent| {
        fs::File::open(parent)?.sync_all().map_err(Into::into)
    })
}
fn atomic_local_with_sync(
    path: &Path,
    bytes: &[u8],
    sync: impl FnOnce(&Path) -> Result<()>,
) -> Result<()> {
    sidecar::Sidecar::ensure_writable_destination(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| failure("missing local parent"))?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(failure)?;
    sync(parent)?;
    Ok(())
}

impl Engine {
    fn smart_dir(&self, id: ImageId) -> Result<PathBuf> {
        Ok(self
            .support_dir()?
            .join("smart-previews")
            .join(id.to_string()))
    }
    pub(crate) fn local_smart_preview(
        &self,
        id: ImageId,
    ) -> Result<Option<(SmartPreviewJournal, JournalSnapshot)>> {
        if !self.smart_dir(id)?.join("journal.json").try_exists()? {
            return Ok(None);
        }
        SmartPreviewJournal::open(self.support_dir()?, id)
            .map(Some)
            .map_err(failure)
    }
    pub(crate) fn require_smart_preview_synced(&self, id: ImageId) -> Result<()> {
        if let Some((_, snapshot)) = self.local_smart_preview(id)?
            && snapshot.dirty
        {
            return Err(failure(
                "Smart Preview needs sync: local edits must be synchronized with the original first",
            ));
        }
        Ok(())
    }
    fn original_path(&self, id: ImageId) -> Result<PathBuf> {
        let catalog = self.lock()?;
        Ok(PathBuf::from(Self::path(&catalog, &id.to_string())?))
    }
    pub(crate) fn load_smart_preview(
        &self,
        id: ImageId,
    ) -> Result<(SmartPreviewJournal, JournalSnapshot, RawImage, PathBuf)> {
        let (journal, snapshot) = self
            .local_smart_preview(id)?
            .ok_or_else(|| failure("Smart Preview missing"))?;
        let decoded = CameraLinearProxy::decode_persistent(&bounded_bytes(
            &self.smart_dir(id)?.join("pixels.tsp"),
            MAX_PROXY_BYTES,
        )?)?;
        if decoded.original_byte_length != snapshot.source_len
            || decoded.proxy.original_content_digest() != snapshot.source_digest
        {
            return Err(failure("Smart Preview source identity mismatch"));
        }
        let path = self.original_path(id)?;
        if path.try_exists()?
            && hash_reader(fs::File::open(&path)?)? != (snapshot.source_digest, snapshot.source_len)
        {
            return Err(failure("Smart Preview stale: original content changed"));
        }
        let mut identity = [0; 16];
        identity.copy_from_slice(&decoded.container_digest[..16]);
        let mut render_id = ImageId(u128::from_le_bytes(identity));
        if render_id == id {
            render_id.0 ^= 1;
        }
        let image = RawImage::from_camera_linear_proxy(id, render_id, Arc::new(decoded.proxy))?;
        Ok((journal, snapshot, image, path))
    }
}

#[uniffi::export]
impl Engine {
    pub fn build_smart_preview(&self, image_id: String) -> Result<SmartPreviewInfo> {
        let id = parse_id(&image_id)?;
        let gate = image_edit_admission::gate_for(id)?;
        let write = gate.begin_write()?;
        let destination = self.smart_dir(id)?;
        if destination.try_exists()? {
            return Err(failure(
                "Smart Preview already exists; synchronize edits and discard before rebuilding",
            ));
        }
        let path = self.original_path(id)?;
        let path_gate = crate::recipe_write::gate_for(&path)?;
        let path_read = path_gate.begin_read()?;
        let doc = catalog::document(&path, id)?;
        let baseline_recipe = optional_bytes(sidecar::Sidecar::paths(&path).recipe)?;
        let baseline_xmp = optional_bytes(catalog::xmp_path(&path))?;
        let initial_recipe = baseline_recipe
            .clone()
            .unwrap_or(serde_json::to_vec(&doc).map_err(failure)?);
        validate_local_document(&initial_recipe)?;
        fs::create_dir_all(destination.parent().unwrap())?;
        fs::File::open(self.support_dir()?)?.sync_all()?;
        let staging = tempfile::tempdir_in(destination.parent().unwrap())?;
        let (copy, digest, len) = pinned_original(&path, staging.path())?;
        let image = RawImage::open(id, copy.path())?;
        if image.source_kind() != "raw" {
            return Err(failure("Smart Preview requires a mosaic RAW original"));
        }
        let proxy = CameraLinearProxy::generate_with_tier(
            image.cfa(),
            image.metadata(),
            &doc.recipe.settings,
            doc.recipe.process_version,
            digest,
            &Default::default(),
            SmartPreviewTier::Compact2048,
        )?;
        let bytes = proxy.encode_persistent(len)?;
        CameraLinearProxy::decode_persistent(&bytes)?;
        if optional_bytes(sidecar::Sidecar::paths(&path).recipe)? != baseline_recipe
            || optional_bytes(catalog::xmp_path(&path))? != baseline_xmp
            || hash_reader(fs::File::open(&path)?)? != (digest, len)
        {
            return Err(failure(
                "original or sidecars changed during Smart Preview build",
            ));
        }
        let journal = SmartPreviewJournal::create(
            staging.path(),
            id,
            digest,
            len,
            initial_recipe,
            baseline_recipe,
            baseline_xmp,
        )
        .map_err(failure)?;
        drop(journal);
        let staged = staging.path().join("smart-previews").join(id.to_string());
        atomic_local(&staged.join("pixels.tsp"), &bytes)?;
        // One directory rename advertises both durable parts; no partial journal is visible.
        fs::rename(&staged, &destination)?;
        fs::File::open(destination.parent().unwrap())?.sync_all()?;
        drop(path_read);
        drop(write);
        self.smart_preview_info(image_id)
    }
    pub fn smart_preview_info(&self, image_id: String) -> Result<SmartPreviewInfo> {
        let id = parse_id(&image_id)?;
        let gate = image_edit_admission::gate_for(id)?;
        let _read = gate.begin_read()?;
        let path = self.original_path(id)?;
        let mut info = SmartPreviewInfo {
            image_id,
            state: SmartPreviewState::Missing,
            original_available: path.is_file(),
            dirty: false,
            width: 0,
            height: 0,
            message: String::new(),
        };
        let Some((_, snapshot)) = self.local_smart_preview(id)? else {
            return Ok(info);
        };
        info.dirty = snapshot.dirty;
        match self.load_smart_preview(id) {
            Ok((_, _, image, _)) => {
                info.width = image.camera_linear_proxy().unwrap().pixels().width();
                info.height = image.camera_linear_proxy().unwrap().pixels().height();
                info.state = if info.dirty {
                    SmartPreviewState::Dirty
                } else if info.original_available {
                    SmartPreviewState::Ready
                } else {
                    SmartPreviewState::OriginalOffline
                };
            }
            Err(e) => {
                info.message = e.to_string();
                info.state = if info.message.contains("stale") {
                    SmartPreviewState::Stale
                } else {
                    SmartPreviewState::Failed
                };
            }
        }
        if info.original_available
            && (optional_bytes(sidecar::Sidecar::paths(&path).recipe)? != snapshot.baseline_recipe
                || optional_bytes(catalog::xmp_path(&path))? != snapshot.baseline_xmp)
        {
            info.state = SmartPreviewState::Conflict;
            info.message =
                "Original sidecars differ from the offline baseline; both copies retained".into();
        }
        Ok(info)
    }
    pub fn discard_smart_preview(&self, image_id: String) -> Result<()> {
        let id = parse_id(&image_id)?;
        let gate = image_edit_admission::gate_for(id)?;
        let _write = gate.begin_write()?;
        if let Some((journal, _)) = self.local_smart_preview(id)? {
            journal.discard_clean().map_err(failure)?;
        }
        let dir = self.smart_dir(id)?;
        match fs::remove_file(dir.join("pixels.tsp")) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        match fs::remove_file(dir.join("sync-pending.json")) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        match fs::remove_dir(&dir) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.into()),
        }
        if dir.parent().unwrap().exists() {
            fs::File::open(dir.parent().unwrap())?.sync_all()?;
        }
        Ok(())
    }
    pub fn synchronize_smart_preview(&self, image_id: String) -> Result<SmartPreviewInfo> {
        let id = parse_id(&image_id)?;
        let gate = image_edit_admission::gate_for(id)?;
        let write = gate.begin_write()?;
        let (mut journal, snapshot, _, path) = self.load_smart_preview(id)?;
        reconcile_acknowledged_intent(&journal)?;
        if !path.is_file() {
            return Err(failure(
                "original unavailable: reconnect before synchronizing",
            ));
        }
        let path_gate = crate::recipe_write::gate_for(&path)?;
        let path_write = path_gate.begin_write()?;
        if snapshot.dirty {
            let recipe_path = sidecar::Sidecar::paths(&path).recipe;
            let xmp_path = catalog::xmp_path(&path);
            let current_recipe = optional_bytes(&recipe_path)?;
            let current_xmp = optional_bytes(&xmp_path)?;
            let pending_path = self.smart_dir(id)?.join("sync-pending.json");
            let pending: PendingSync = if let Some(bytes) = optional_bytes(&pending_path)? {
                let pending: PendingSync = serde_json::from_slice(&bytes).map_err(failure)?;
                if pending.generation != snapshot.generation {
                    return Err(failure(
                        "conflict: failed sync has a newer local edit; both copies retained for recovery",
                    ));
                }
                pending
            } else {
                if current_recipe != snapshot.baseline_recipe
                    || current_xmp != snapshot.baseline_xmp
                {
                    return Err(failure(
                        "conflict: original sidecars changed; local edits retained",
                    ));
                }
                let mut doc: sidecar::RecipeDocument =
                    serde_json::from_slice(&snapshot.recipe).map_err(failure)?;
                doc.record_write("tessera-mac", crate::now_ms())?;
                let packet = catalog::selection_packet(&path, &doc)?.with_recipe(&doc.recipe)?;
                let mut envelope: serde_json::Value =
                    serde_json::from_slice(&snapshot.recipe).map_err(failure)?;
                let stamped = serde_json::to_value(&doc).map_err(failure)?;
                envelope["vector_clock"] = stamped["vector_clock"].clone();
                envelope["last_writer"] = stamped["last_writer"].clone();
                let pending = PendingSync {
                    generation: snapshot.generation,
                    recipe: serde_json::to_vec_pretty(&envelope).map_err(failure)?,
                    xmp: packet.xml.into_bytes(),
                };
                atomic_local(
                    &pending_path,
                    &serde_json::to_vec(&pending).map_err(failure)?,
                )?;
                pending
            };
            if (current_recipe != snapshot.baseline_recipe
                && current_recipe.as_deref() != Some(&pending.recipe[..]))
                || (current_xmp != snapshot.baseline_xmp
                    && current_xmp.as_deref() != Some(&pending.xmp[..]))
            {
                return Err(failure(
                    "conflict: original changed during sync recovery; both copies retained",
                ));
            }
            if hash_reader(fs::File::open(&path)?)? != (snapshot.source_digest, snapshot.source_len)
            {
                return Err(failure("conflict: original content changed"));
            }
            publish_sync_sidecars(&pending, &recipe_path, &xmp_path, atomic_local)?;
            self.lock()?.index.scan(
                path.parent()
                    .ok_or_else(|| failure("original has no folder"))?,
                &catalog::Sidecars,
                &catalog::EmbeddedMetadata,
            )?;
            journal
                .mark_synced(
                    pending.recipe.clone(),
                    Some(pending.recipe),
                    Some(pending.xmp),
                )
                .map_err(failure)?;
            fs::remove_file(&pending_path)?;
            fs::File::open(pending_path.parent().unwrap())?.sync_all()?;
        }
        drop(path_write);
        drop(write);
        self.notify_changes();
        self.smart_preview_info(image_id)
    }
}
#[derive(serde::Serialize, serde::Deserialize)]
struct PendingSync {
    generation: u64,
    recipe: Vec<u8>,
    xmp: Vec<u8>,
}

pub(crate) fn save_local_recipe(journal: &mut SmartPreviewJournal, recipe: &Recipe) -> Result<()> {
    reconcile_acknowledged_intent(journal)?;
    let snapshot = journal.snapshot().map_err(failure)?;
    validate_local_document(&snapshot.recipe)?;
    let mut value: serde_json::Value = serde_json::from_slice(&snapshot.recipe).map_err(failure)?;
    let updated = serde_json::to_value(recipe).map_err(failure)?;
    // Top-level envelope and recipe unknown members remain byte-value exact.
    // Unknown nested Develop controls cannot be safely reinterpreted: reject their loss.
    for key in [
        "process_version",
        "source_kind",
        "settings",
        "history",
        "ids",
    ] {
        value["recipe"][key] = updated[key].clone();
    }
    journal
        .save_recipe(serde_json::to_vec(&value).map_err(failure)?)
        .map_err(failure)?;
    Ok(())
}

/// Reject only fields the typed old document cannot represent. Comparing to
/// the old projection permits ordinary array removal/reordering in new edits.
pub(crate) fn validate_local_document(bytes: &[u8]) -> Result<sidecar::RecipeDocument> {
    let document: sidecar::RecipeDocument = serde_json::from_slice(bytes).map_err(failure)?;
    let raw: serde_json::Value = serde_json::from_slice(bytes).map_err(failure)?;
    let normalized = serde_json::to_value(&document.recipe).map_err(failure)?;
    for key in [
        "process_version",
        "source_kind",
        "settings",
        "history",
        "ids",
    ] {
        reject_unrepresented(&raw["recipe"][key], &normalized[key], key)?;
    }
    Ok(document)
}
fn reject_unrepresented(
    raw: &serde_json::Value,
    normalized: &serde_json::Value,
    field: &str,
) -> Result<()> {
    match raw {
        serde_json::Value::Object(old) => {
            let current = normalized.as_object().ok_or_else(|| {
                failure(format!(
                    "original required: unsupported nested {field} object"
                ))
            })?;
            for (key, value) in old {
                let other = current.get(key).ok_or_else(|| {
                    failure(format!(
                        "original required: unsupported nested {field} member {key}"
                    ))
                })?;
                reject_unrepresented(value, other, field)?;
            }
        }
        serde_json::Value::Array(old) => {
            let current = normalized
                .as_array()
                .filter(|values| values.len() == old.len())
                .ok_or_else(|| {
                    failure(format!(
                        "original required: unsupported nested {field} array"
                    ))
                })?;
            for (before, after) in old.iter().zip(current) {
                reject_unrepresented(before, after, field)?;
            }
        }
        _ => (),
    }
    Ok(())
}

fn publish_sync_sidecars(
    pending: &PendingSync,
    recipe_path: &Path,
    xmp_path: &Path,
    mut publish: impl FnMut(&Path, &[u8]) -> Result<()>,
) -> Result<()> {
    sidecar::Sidecar::ensure_writable_destination(recipe_path)?;
    sidecar::Sidecar::ensure_writable_destination(xmp_path)?;
    let doc = validate_local_document(&pending.recipe)?;
    doc.recipe.validate()?;
    doc.recipe.to_json()?;
    sidecar::XmpPacket::parse(String::from_utf8(pending.xmp.clone()).map_err(failure)?)?;
    if let Some(parent) = recipe_path.parent() {
        fs::create_dir_all(parent)?;
        if let Some(grandparent) = parent.parent() {
            fs::File::open(grandparent)?.sync_all()?;
        }
    }
    publish(recipe_path, &pending.recipe)?;
    // Both file AND directory publication must be durable before clean ack.
    publish(xmp_path, &pending.xmp)?;
    Ok(())
}

/// A successful acknowledgement may survive while intent cleanup does not.
/// The durable journal baseline proves the exact intended bytes were already
/// acknowledged; this recovery is safe while the original volume is offline.
pub(crate) fn reconcile_acknowledged_intent(journal: &SmartPreviewJournal) -> Result<()> {
    let path = journal.directory().join("sync-pending.json");
    let Some(bytes) = optional_bytes(&path)? else {
        return Ok(());
    };
    let pending: PendingSync = serde_json::from_slice(&bytes).map_err(failure)?;
    let snapshot = journal.snapshot().map_err(failure)?;
    if snapshot.generation > pending.generation
        && snapshot.baseline_recipe.as_deref() == Some(&pending.recipe[..])
        && snapshot.baseline_xmp.as_deref() == Some(&pending.xmp[..])
    {
        fs::remove_file(&path)?;
        fs::File::open(journal.directory())?.sync_all()?;
    } else if !snapshot.dirty {
        return Err(failure(
            "conflict: unacknowledged sync intent does not match clean journal baseline; preserve both copies for recovery",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn recipe(id: ImageId) -> Vec<u8> {
        let mut value = serde_json::to_value(sidecar::RecipeDocument {
            recipe: Recipe::new(id),
            ..Default::default()
        })
        .unwrap();
        value["future_envelope"] = serde_json::json!({"opaque":[1,2,3]});
        value["recipe"]["future_control"] = serde_json::json!({"kept":true});
        serde_json::to_vec(&value).unwrap()
    }
    #[test]
    fn offline_edit_save_reopen_keeps_original_and_unknown_members() {
        let local = tempfile::tempdir().unwrap();
        let id = ImageId(9101);
        let original = local.path().join("unmounted-original.arw");
        fs::write(&original, b"untouched").unwrap();
        let mut journal =
            SmartPreviewJournal::create(local.path(), id, [1; 32], 9, recipe(id), None, None)
                .unwrap();
        let mut doc: sidecar::RecipeDocument = serde_json::from_slice(&recipe(id)).unwrap();
        doc.recipe.settings.tone.exposure = 1.25;
        doc.recipe
            .history
            .record(
                &doc.recipe.history.base.clone(),
                &doc.recipe.settings,
                engine_api::recipe::EditMeta::user("test edit", 1),
            )
            .unwrap();
        save_local_recipe(&mut journal, &doc.recipe).unwrap();
        drop(journal);
        let (journal, snapshot) = SmartPreviewJournal::open(local.path(), id).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&snapshot.recipe).unwrap();
        assert_eq!(
            value["future_envelope"]["opaque"],
            serde_json::json!([1, 2, 3])
        );
        assert!(value["recipe"]["future_control"]["kept"].as_bool().unwrap());
        assert!(snapshot.dirty);
        assert!(journal.discard_clean().is_err());
        assert_eq!(fs::read(original).unwrap(), b"untouched");
        let doc: sidecar::RecipeDocument = serde_json::from_slice(&snapshot.recipe).unwrap();
        assert_eq!(doc.recipe.settings.tone.exposure, 1.25);
    }
    #[test]
    fn dirty_local_recipe_blocks_full_quality_export_and_direct_original_write() {
        let local = tempfile::tempdir().unwrap();
        let engine = Engine::open(local.path().to_string_lossy().into()).unwrap();
        let id = ImageId(9102);
        let mut journal =
            SmartPreviewJournal::create(local.path(), id, [1; 32], 9, recipe(id), None, None)
                .unwrap();
        assert!(engine.require_smart_preview_synced(id).is_ok());
        journal.save_recipe(recipe(id)).unwrap();
        assert!(
            engine
                .require_smart_preview_synced(id)
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        assert!(
            engine
                .get_recipe(id.to_string())
                .unwrap()
                .contains("future_control")
        );
    }
    #[test]
    fn absent_journal_read_does_not_create_per_photo_directories() {
        let local = tempfile::tempdir().unwrap();
        let engine = Engine::open(local.path().to_string_lossy().into()).unwrap();
        let id = ImageId(9103);
        assert!(engine.local_smart_preview(id).unwrap().is_none());
        assert!(!engine.smart_dir(id).unwrap().exists());
    }
    #[test]
    fn pinned_snapshot_has_original_suffix_and_identical_verified_bytes() {
        let local = tempfile::tempdir().unwrap();
        let original = local.path().join("photo.ARW");
        let bytes = b"stable camera bytes";
        fs::write(&original, bytes).unwrap();
        let (snapshot, digest, len) = pinned_original(&original, local.path()).unwrap();
        assert_eq!(snapshot.path().extension().unwrap(), "ARW");
        assert_eq!(fs::read(snapshot.path()).unwrap(), bytes);
        assert_eq!(digest, *blake3::hash(bytes).as_bytes());
        assert_eq!(len, bytes.len() as u64);
        assert_eq!(fs::read(original).unwrap(), bytes);
    }
    #[test]
    fn sync_acknowledgement_cannot_erase_newer_or_recreated_dirty_work() {
        let local = tempfile::tempdir().unwrap();
        let id = ImageId(9104);
        let mut stale =
            SmartPreviewJournal::create(local.path(), id, [1; 32], 9, recipe(id), None, None)
                .unwrap();
        let (mut latest, _) = SmartPreviewJournal::open(local.path(), id).unwrap();
        latest.save_recipe(recipe(id)).unwrap();
        assert!(stale.mark_synced(recipe(id), None, None).is_err());
        assert!(latest.snapshot().unwrap().dirty);
    }
}

/// The renderer and writer share the component's exact admitted prefix.
pub(crate) fn validate_proxy_recipe(image: &RawImage, recipe: &Recipe) -> Result<()> {
    if recipe.process_version.family != engine_api::recipe::ProcessFamily::Native
        || recipe.process_version.revision != 2
    {
        return Err(failure(
            "original required: Smart Preview supports Native revision 2",
        ));
    }
    image
        .camera_linear_proxy()
        .ok_or_else(|| failure("missing camera-linear source"))?
        .validate_prefix(&recipe.settings)?;
    Ok(())
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    fn document(id: ImageId) -> Vec<u8> {
        serde_json::to_vec(&sidecar::RecipeDocument {
            recipe: Recipe::new(id),
            ..Default::default()
        })
        .unwrap()
    }
    fn pending(id: ImageId, generation: u64) -> PendingSync {
        let recipe = Recipe::new(id);
        PendingSync {
            generation,
            recipe: document(id),
            xmp: sidecar::XmpPacket::from_selection(&recipe.selection, &Default::default())
                .xml
                .into_bytes(),
        }
    }
    #[test]
    fn unknown_array_member_is_rejected_without_advancing_or_erasing_journal() {
        let root = tempfile::tempdir().unwrap();
        let id = ImageId(9301);
        let mut value: serde_json::Value = serde_json::from_slice(&document(id)).unwrap();
        value["recipe"]["settings"]["color"]["point_colors"] = serde_json::json!([{"source_lch":[0.5,0.1,30.0],"hue_shift":0.0,"saturation_shift":0.0,"luminance_shift":0.0,"range":50.0,"future_member":{"kept":true}}]);
        value["recipe"]["history"]["base"] = value["recipe"]["settings"].clone();
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut journal =
            SmartPreviewJournal::create(root.path(), id, [1; 32], 1, bytes.clone(), None, None)
                .unwrap();
        let doc: sidecar::RecipeDocument = serde_json::from_slice(&bytes).unwrap();
        assert!(
            save_local_recipe(&mut journal, &doc.recipe)
                .unwrap_err()
                .to_string()
                .contains("future_member")
        );
        assert_eq!(journal.snapshot().unwrap().recipe, bytes);
        assert_eq!(journal.snapshot().unwrap().generation, 1);
        assert!(
            reject_unrepresented(
                &serde_json::json!({"future":1}),
                &serde_json::json!(null),
                "settings"
            )
            .is_err()
        );
    }
    #[test]
    fn ordinary_array_deletion_is_allowed_when_old_document_is_fully_represented() {
        let root = tempfile::tempdir().unwrap();
        let id = ImageId(9302);
        let mut value: serde_json::Value = serde_json::from_slice(&document(id)).unwrap();
        value["recipe"]["settings"]["color"]["point_colors"] = serde_json::json!([{"source_lch":[0.5,0.1,30.0],"hue_shift":0.0,"saturation_shift":0.0,"luminance_shift":0.0,"range":50.0}]);
        value["recipe"]["history"]["base"] = value["recipe"]["settings"].clone();
        let bytes = serde_json::to_vec(&value).unwrap();
        let mut journal =
            SmartPreviewJournal::create(root.path(), id, [1; 32], 1, bytes.clone(), None, None)
                .unwrap();
        let mut doc: sidecar::RecipeDocument = serde_json::from_slice(&bytes).unwrap();
        doc.recipe.settings.color.point_colors.clear();
        doc.recipe
            .history
            .record(
                &doc.recipe.history.base.clone(),
                &doc.recipe.settings,
                engine_api::recipe::EditMeta::user("test edit", 1),
            )
            .unwrap();
        save_local_recipe(&mut journal, &doc.recipe).unwrap();
        assert!(journal.snapshot().unwrap().dirty);
    }
    #[test]
    fn xmp_directory_sync_failure_keeps_dirty_journal_and_replay_intent() {
        let root = tempfile::tempdir().unwrap();
        let id = ImageId(9303);
        let mut journal =
            SmartPreviewJournal::create(root.path(), id, [1; 32], 1, document(id), None, None)
                .unwrap();
        journal.save_recipe(document(id)).unwrap();
        let intent = pending(id, journal.snapshot().unwrap().generation);
        let intent_path = journal.directory().join("sync-pending.json");
        atomic_local(&intent_path, &serde_json::to_vec(&intent).unwrap()).unwrap();
        let photo = root.path().join("photo.raw");
        let paths = sidecar::Sidecar::paths(&photo);
        let failure = publish_sync_sidecars(&intent, &paths.recipe, &paths.xmp, |path, bytes| {
            atomic_local_with_sync(path, bytes, |parent| {
                if path == paths.xmp {
                    return Err(crate::failure("injected XMP directory sync failure"));
                }
                fs::File::open(parent)?.sync_all()?;
                Ok(())
            })
        });
        assert!(failure.unwrap_err().to_string().contains("injected"));
        let (_, reopened) = SmartPreviewJournal::open(root.path(), id).unwrap();
        assert!(reopened.dirty);
        assert!(intent_path.exists());
        assert_eq!(fs::read(&paths.recipe).unwrap(), intent.recipe);
        publish_sync_sidecars(&intent, &paths.recipe, &paths.xmp, atomic_local).unwrap();
        journal
            .mark_synced(
                intent.recipe.clone(),
                Some(intent.recipe.clone()),
                Some(intent.xmp.clone()),
            )
            .unwrap();
        reconcile_acknowledged_intent(&journal).unwrap();
        assert!(!intent_path.exists());
        assert!(!journal.snapshot().unwrap().dirty);
    }
    #[test]
    fn restart_after_ack_before_cleanup_recovers_offline_and_accepts_new_edit() {
        let root = tempfile::tempdir().unwrap();
        let id = ImageId(9304);
        let mut journal =
            SmartPreviewJournal::create(root.path(), id, [1; 32], 1, document(id), None, None)
                .unwrap();
        journal.save_recipe(document(id)).unwrap();
        let intent = pending(id, journal.snapshot().unwrap().generation);
        let intent_path = journal.directory().join("sync-pending.json");
        atomic_local(&intent_path, &serde_json::to_vec(&intent).unwrap()).unwrap();
        journal
            .mark_synced(
                intent.recipe.clone(),
                Some(intent.recipe.clone()),
                Some(intent.xmp.clone()),
            )
            .unwrap();
        drop(journal);
        let (mut journal, _) = SmartPreviewJournal::open(root.path(), id).unwrap();
        let mut edited: sidecar::RecipeDocument = serde_json::from_slice(&document(id)).unwrap();
        edited.recipe.settings.tone.exposure = 1.0;
        edited
            .recipe
            .history
            .record(
                &edited.recipe.history.base.clone(),
                &edited.recipe.settings,
                engine_api::recipe::EditMeta::user("test edit", 1),
            )
            .unwrap();
        save_local_recipe(&mut journal, &edited.recipe).unwrap();
        assert!(!intent_path.exists());
        assert!(journal.snapshot().unwrap().dirty);
        // Also recover an acknowledged leftover produced by a previous app that
        // already allowed a later dirty generation to be written.
        atomic_local(&intent_path, &serde_json::to_vec(&intent).unwrap()).unwrap();
        reconcile_acknowledged_intent(&journal).unwrap();
        assert!(!intent_path.exists());
        assert!(journal.snapshot().unwrap().dirty);
    }
}

#[cfg(test)]
mod lightroom_safety_tests {
    #[test]
    fn raw_sync_writer_preserves_lightroom_owned_files() {
        let root = tempfile::tempdir().unwrap();
        let folder = root.path().join("X.lrdata");
        std::fs::create_dir(&folder).unwrap();
        let path = folder.join("photo.xmp");
        std::fs::write(&path, b"original").unwrap();
        assert!(super::atomic_local(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(folder).unwrap().count(), 1);
    }
}
