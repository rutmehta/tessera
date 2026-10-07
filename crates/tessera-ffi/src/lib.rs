//! Narrow, synchronous commands. Swift dispatches blocking work off its main actor.
mod agent_runs;
mod assist;
mod backend;
mod catalog;
mod changes;
mod collections;
mod develop;
mod document;
mod enhance;
mod export;
#[allow(
    dead_code,
    reason = "journal storage awaits Smart Preview caller integration"
)]
mod image_edit_admission;
mod lrcat;
mod lrcat_fidelity;
mod lrcat_masks;
mod merge;
mod metadata;
mod models;
mod original_write;
mod preview;
mod proof;
mod recipe_write;
#[cfg(test)]
mod recipe_write_tests;
mod session;
mod smart_preview;
#[allow(
    dead_code,
    reason = "journal storage awaits Smart Preview caller integration"
)]
mod smart_preview_store;
mod smart_preview_thumbnail;
pub use smart_preview::{SmartPreviewInfo, SmartPreviewState};
#[doc(hidden)]
pub mod surface;
mod understanding;
pub use agent_runs::*;
pub use assist::*;
pub use changes::*;
pub mod tether;
pub use collections::*;
pub use develop::*;
pub use document::*;
use engine_api::{id::ImageId, recipe as core};
pub use enhance::*;
pub use export::*;
pub use lrcat::*;
pub use lrcat_fidelity::*;
pub use merge::*;
pub use metadata::*;
pub use preview::PreviewResponse;
pub use proof::*;
use rusqlite::{Connection, OpenFlags};
pub use session::*;
use sidecar::Sidecar;
use std::{
    path::Path,
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, AtomicU64},
    },
};
pub use understanding::*;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{message}")]
    Failure { message: String },
}
type Result<T> = std::result::Result<T, BridgeError>;
fn failure(e: impl std::fmt::Display) -> BridgeError {
    BridgeError::Failure {
        message: e.to_string(),
    }
}
impl From<engine_api::EngineError> for BridgeError {
    fn from(e: engine_api::EngineError) -> Self {
        failure(e)
    }
}
impl From<rusqlite::Error> for BridgeError {
    fn from(e: rusqlite::Error) -> Self {
        failure(e)
    }
}
impl From<std::io::Error> for BridgeError {
    fn from(e: std::io::Error) -> Self {
        failure(e)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum Decision {
    Undecided,
    Reject,
    Keep,
}
impl From<Decision> for core::Decision {
    fn from(d: Decision) -> Self {
        match d {
            Decision::Undecided => Self::Undecided,
            Decision::Reject => Self::Reject,
            Decision::Keep => Self::Keep,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct Selection {
    pub decision: Decision,
    pub grade: Option<u8>,
    pub mark: Option<String>,
}
impl Selection {
    fn into_core(self) -> Result<core::Selection> {
        let grade = match self.grade {
            None => None,
            Some(1) => Some(core::Grade::One),
            Some(2) => Some(core::Grade::Two),
            Some(3) => Some(core::Grade::Three),
            _ => return Err(failure("grade must be 1, 2, or 3")),
        };
        Ok(core::Selection {
            decision: self.decision.into(),
            grade,
            mark: self.mark.map(core::Mark::new),
        }
        .normalized())
    }
}
impl From<core::Selection> for Selection {
    fn from(s: core::Selection) -> Self {
        Self {
            decision: match s.decision {
                core::Decision::Undecided => Decision::Undecided,
                core::Decision::Reject => Decision::Reject,
                core::Decision::Keep => Decision::Keep,
            },
            grade: s.grade.map(|g| g as u8),
            mark: s.mark.map(|m| m.0),
        }
    }
}
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct ImageQuery {
    pub folder: Option<String>,
    pub text: Option<String>,
    pub decision: Option<Decision>,
    /// Zero means all matching rows. Offset is applied before returning them.
    pub limit: u32,
    pub offset: u32,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct ImageSummary {
    pub lightroom_smart_preview: bool,
    pub id: String,
    pub path: String,
    /// RAW: Unix seconds; EXIF JPEG/TIFF: ISO local date-time without a timezone.
    pub capture_time: Option<String>,
    pub orientation: u16,
    pub selection: Selection,
    pub recipe_hash: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct FolderHandle {
    pub path: String,
    pub updated: u64,
}

struct Catalog {
    index: index::Index,
    reader: Connection,
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum EngineEvent {
    ScanProgress {
        path: String,
        updated: u64,
        finished: bool,
    },
    PreviewReady {
        image_id: String,
        max_px: u32,
    },
    /// The catalog changed up to `sequence`: pull `changes_since` (or
    /// `CullSession::sync_changes`) from the last sequence you applied.
    LibraryChanged {
        sequence: u64,
    },
}

/// Called on the command's worker thread, never while holding a catalog lock.
#[uniffi::export(with_foreign)]
pub trait EngineEventListener: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}
#[derive(uniffi::Object)]
pub struct Engine {
    db: std::path::PathBuf,
    catalog: Mutex<Catalog>,
    previews: previews::PreviewStore,
    /// Shared scheduler: RAW previews (`Priority::Preview`) and develop
    /// viewports (`Priority::Viewport`). Jobs are not pre-empted, so it keeps
    /// a worker free for the viewport while previews run.
    jobs: jobs::ThreadPoolScheduler,
    /// Develop renderer and memo cache shared by every session (lazy: the
    /// Metal context is created on the first develop session).
    renderer: std::sync::OnceLock<backend::Backend>,
    /// AI mask segmentation (loaded on first use; see `develop::masks`).
    segmenter: develop::SegmenterSlot,
    /// Face detector + recognizer (downloaded and loaded on first face analysis).
    faces: Mutex<Option<ml_faces::FaceModels>>,
    smart_thumbnail_states: Mutex<smart_preview_thumbnail::States>,
    preview_states: Mutex<std::collections::HashMap<preview::RequestKey, preview::State>>,
    listener: Mutex<Option<Arc<dyn EngineEventListener>>>,
    /// Keyword suggestions, captions and OCR: models and background jobs.
    understanding: understanding::UnderstandingState,
    /// The process's one Metal device (`gpu-core`), created on first use by
    /// a develop or document session and shared by both.
    gpu: std::sync::OnceLock<Option<gpu_core::GpuDevice>>,
    /// Layered-document sessions (`document.rs`) and the compositor's GPU
    /// pipelines on the shared device.
    documents: document::Registry,
    /// Highest change sequence announced with `LibraryChanged`.
    notified: AtomicU64,
    /// Reads the change head without the catalog lock (tether polls must never wait on a scan).
    heads: Mutex<Connection>,
    /// The change watcher thread was started (see `changes`).
    watching: AtomicBool,
    /// This engine, for background work that must not keep it alive.
    this: std::sync::Weak<Engine>,
    /// Test-only observation of writer creation, used to prove read-only Engine APIs
    /// do not construct a temporary Develop session.
    #[cfg(test)]
    develop_writer_constructions: std::sync::atomic::AtomicUsize,
}
impl Engine {
    fn emit(&self, event: EngineEvent) {
        let listener = self
            .listener
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if let Some(listener) = listener {
            listener.on_event(event);
        }
    }
    /// Calibrates against the first opened RAW; explicit CPU/GPU overrides
    /// skip calibration. The selected renderer and its caches are shared.
    fn develop_renderer(
        &self,
        image: &image_core::RawImage,
        settings: &core::DevelopSettings,
    ) -> (Arc<image_core::Renderer>, String) {
        if image.camera_linear_proxy().is_some() {
            // Explicit proxy selection calibrates independently of Original.
            // Unsupported tails/maps retain scalar CPU fallback on every edit.
            let backend = backend::select_proxy(image, settings, || self.shared_gpu());
            // Selection/capability label, not per-frame telemetry. Proxy maps
            // (captured distortion, crop, transform, upright) use scalar CPU.
            let name = if backend.name.starts_with("Metal (") {
                format!("{} Smart Preview (GPU with CPU fallback)", backend.name)
            } else {
                format!("{} Smart Preview", backend.name)
            };
            return (Arc::new(backend.renderer()), name);
        }
        let backend = self
            .renderer
            .get_or_init(|| backend::select(image, || self.shared_gpu()));
        (Arc::new(backend.renderer()), backend.name.clone())
    }
    /// The shared Metal device, or `None` when Metal is unavailable.
    fn shared_gpu(&self) -> Option<gpu_core::GpuDevice> {
        self.gpu
            .get_or_init(|| match gpu_core::GpuDevice::new() {
                Ok(device) => Some(device),
                Err(e) => {
                    eprintln!("engine: Metal unavailable: {e}");
                    None
                }
            })
            .clone()
    }
    fn lock(&self) -> Result<MutexGuard<'_, Catalog>> {
        self.catalog.lock().map_err(failure)
    }
    fn path(c: &Catalog, id: &str) -> Result<String> {
        parse_id(id)?;
        Ok(c.reader.query_row(
            "SELECT f.path FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
            [id],
            |r| r.get(0),
        )?)
    }
    fn persist(c: &mut Catalog, path: &Path, doc: &sidecar::RecipeDocument) -> Result<()> {
        let packet = catalog::selection_packet(path, doc)?;
        Self::persist_with_packet(c, path, doc, &packet)
    }
    fn persist_with_packet(
        c: &mut Catalog,
        path: &Path,
        doc: &sidecar::RecipeDocument,
        packet: &sidecar::XmpPacket,
    ) -> Result<()> {
        // Recipe is the authoritative commit. A later scan repairs the rebuildable index.
        let paths = catalog::write_paths(path)?;
        Sidecar::write_recipe(paths.recipe, doc)?;
        Sidecar::write_xmp(paths.xmp, packet)?;
        c.index
            .scan_file(path, &catalog::Sidecars, &catalog::EmbeddedMetadata)?;
        Ok(())
    }
}
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn parse_id(id: &str) -> Result<ImageId> {
    if id.len() != 32 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(failure("image_id must contain 32 hexadecimal characters"));
    }
    u128::from_str_radix(id, 16).map(ImageId).map_err(failure)
}

#[uniffi::export]
impl Engine {
    #[uniffi::constructor]
    pub fn open(app_support_dir: String) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&app_support_dir)?;
        let db = Path::new(&app_support_dir).join("index.sqlite");
        let index = index::Index::open(&db)?;
        let reader = Connection::open_with_flags(&db, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        // Restore source-to-store routing before any reopened catalog reads.
        {
            let mut statement = reader.prepare("SELECT path FROM file")?;
            let mut folders = std::collections::BTreeSet::new();
            for path in statement.query_map([], |row| row.get::<_, String>(0))? {
                let path = std::path::PathBuf::from(path?);
                if let Some(parent) = path.parent() {
                    folders.insert(parent.to_path_buf());
                }
            }
            for folder in folders {
                Sidecar::register_store(&folder, Path::new(&app_support_dir));
            }
        }
        let notified = AtomicU64::new(index.change_head()?);
        let heads = Mutex::new(Connection::open_with_flags(
            &db,
            OpenFlags::SQLITE_OPEN_READ_ONLY,
        )?);
        let previews =
            previews::PreviewStore::new(Path::new(&app_support_dir).join("previews"), 512 << 20)
                .map_err(failure)?
                .with_retouch_renderer(Arc::new(brush::render_retouch));
        Ok(Arc::new_cyclic(|this| Self {
            db,
            catalog: Mutex::new(Catalog { index, reader }),
            previews,
            jobs: jobs::ThreadPoolScheduler::with_interactive_reservation(3),
            renderer: std::sync::OnceLock::new(),
            segmenter: Default::default(),
            faces: Mutex::new(None),
            smart_thumbnail_states: Mutex::new(Default::default()),
            preview_states: Mutex::new(std::collections::HashMap::new()),
            listener: Mutex::new(None),
            understanding: Default::default(),
            gpu: std::sync::OnceLock::new(),
            documents: Default::default(),
            notified,
            heads,
            watching: AtomicBool::new(false),
            this: this.clone(),
            #[cfg(test)]
            develop_writer_constructions: std::sync::atomic::AtomicUsize::new(0),
        }))
    }
    /// Setting a listener also starts watching the catalog for writes made
    /// elsewhere (reported as `LibraryChanged`).
    pub fn set_event_listener(&self, listener: Option<Arc<dyn EngineEventListener>>) {
        let watch = listener.is_some();
        *self.listener.lock().unwrap_or_else(|e| e.into_inner()) = listener;
        if watch {
            self.watch_changes();
        }
    }
    pub fn index_folder(&self, path: String) -> Result<FolderHandle> {
        let path = Path::new(&path).canonicalize()?;
        Sidecar::register_store(&path, self.support_dir()?);
        self.emit(EngineEvent::ScanProgress {
            path: path.to_string_lossy().into_owned(),
            updated: 0,
            finished: false,
        });
        let updated =
            self.lock()?
                .index
                .scan(&path, &catalog::Sidecars, &catalog::EmbeddedMetadata)?;
        self.emit(EngineEvent::ScanProgress {
            path: path.to_string_lossy().into_owned(),
            updated: updated as u64,
            finished: true,
        });
        self.notify_changes();
        Ok(FolderHandle {
            path: path.to_string_lossy().into_owned(),
            updated: updated as u64,
        })
    }
    pub fn list_images(&self, query: ImageQuery) -> Result<Vec<ImageSummary>> {
        let c = self.lock()?;
        let ids = c.index.search(&index::Query {
            folder: query.folder,
            text: query.text,
            decision: query.decision.map(Into::into),
            limit: if query.limit == 0 {
                i64::MAX as usize
            } else {
                query.limit as usize
            },
            offset: query.offset as usize,
            ..Default::default()
        })?;
        let mut result = Vec::new();
        for id in ids {
            let id_string = id.to_string();
            let (path, capture_time, orientation, recipe_hash) = c.reader.query_row(
                "SELECT f.path,i.capture_time,COALESCE((SELECT value FROM metadata WHERE image_id=i.id AND key='orientation'),'1'),COALESCE(h.hash,'') FROM image i JOIN file f ON f.id=i.file_id LEFT JOIN recipe_hash h ON h.image_id=i.id WHERE i.id=?",
                [&id_string], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?)))?;

            let (source, lightroom_smart_preview) = catalog::source_projection(Path::new(&path));
            result.push(ImageSummary {
                lightroom_smart_preview,
                id: id_string,
                path: source.to_string_lossy().into_owned(),
                capture_time,
                orientation: orientation.parse().unwrap_or(1),
                selection: c.index.selection(id)?.unwrap_or_default().into(),
                recipe_hash,
            });
        }
        Ok(result)
    }
    pub fn set_selection(&self, image_id: String, selection: Selection) -> Result<()> {
        let id_gate = image_edit_admission::gate_for(parse_id(&image_id)?)?;
        let _id_write = id_gate.begin_selection_write()?;
        self.require_smart_preview_synced(parse_id(&image_id)?)?;
        let selection = selection.into_core()?;
        let path = {
            let c = self.lock()?;
            Self::path(&c, &image_id)?
        };
        let gate = recipe_write::gate_for(Path::new(&path))?;
        let write = gate.begin_selection_write()?;
        let mut c = self.lock()?;
        if Self::path(&c, &image_id)? != path {
            return Err(failure("image path changed before recipe write"));
        }
        let mut doc = catalog::document(Path::new(&path), parse_id(&image_id)?)?;
        doc.recipe.selection = selection;
        doc.record_write("tessera-mac", now_ms())?;
        Self::persist(&mut c, Path::new(&path), &doc)?;
        drop(c);
        drop(write);
        drop(_id_write);
        self.notify_changes();
        Ok(())
    }
    pub fn get_recipe(&self, image_id: String) -> Result<String> {
        let id = parse_id(&image_id)?;
        let gate = image_edit_admission::gate_for(id)?;
        let _read = gate.begin_read()?;
        if let Some((_, snapshot)) = self.local_smart_preview(id)?
            && (snapshot.dirty || {
                let catalog = self.lock()?;
                // Indexed ownership is sufficient offline; never canonicalize the
                // absent original folder or synthesize a default recipe here.
                !Path::new(&Self::path(&catalog, &image_id)?).is_file()
            })
        {
            let doc: sidecar::RecipeDocument =
                serde_json::from_slice(&snapshot.recipe).map_err(failure)?;
            return String::from_utf8(doc.recipe.to_json()?).map_err(failure);
        }
        let c = self.lock()?;
        let path = Self::path(&c, &image_id)?;
        String::from_utf8(
            catalog::document(Path::new(&path), parse_id(&image_id)?)?
                .recipe
                .to_json()?,
        )
        .map_err(failure)
    }
    pub fn set_recipe_json(&self, image_id: String, json: String) -> Result<()> {
        let id_gate = image_edit_admission::gate_for(parse_id(&image_id)?)?;
        let _id_write = id_gate.begin_write()?;
        self.require_smart_preview_synced(parse_id(&image_id)?)?;
        let mut recipe: core::Recipe = serde_json::from_str(&json).map_err(failure)?;
        if recipe.image_id != Some(parse_id(&image_id)?) {
            return Err(failure("recipe image_id mismatch"));
        }
        recipe.validate()?;
        recipe.to_json()?; // Reject forward-version documents before any write.
        let path = {
            let c = self.lock()?;
            Self::path(&c, &image_id)?
        };
        let gate = recipe_write::gate_for(Path::new(&path))?;
        let write = gate.begin_write()?;
        let mut c = self.lock()?;
        if Self::path(&c, &image_id)? != path {
            return Err(failure("image path changed before recipe write"));
        }
        let mut doc = catalog::document(Path::new(&path), parse_id(&image_id)?)?;
        // Existing history and unknown fields cannot be discarded by a stale client.
        if recipe.history.base != doc.recipe.history.base
            || !recipe
                .history
                .entries
                .starts_with(&doc.recipe.history.entries)
        {
            return Err(failure("history must be append-only"));
        }
        for (key, value) in &doc.recipe.unknown {
            recipe
                .unknown
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        recipe.ids.next_mask = recipe.ids.next_mask.max(doc.recipe.ids.next_mask);
        recipe.ids.next_retouch = recipe.ids.next_retouch.max(doc.recipe.ids.next_retouch);
        doc.recipe = recipe;
        doc.record_write("tessera-mac", now_ms())?;
        let packet = catalog::selection_packet(Path::new(&path), &doc)?.with_recipe(&doc.recipe)?;
        Self::persist_with_packet(&mut c, Path::new(&path), &doc, &packet)?;
        drop(c);
        drop(write);
        drop(_id_write);
        self.notify_changes();
        Ok(())
    }
    /// RAW cache misses return pending immediately; PreviewReady signals completion.
    pub fn embedded_preview(
        self: Arc<Self>,
        image_id: String,
        max_px: u32,
    ) -> Result<PreviewResponse> {
        if max_px == 0 || max_px > 8192 {
            return Err(failure("max_px must be 1...8192"));
        }
        let (path, orientation, recipe_hash) = {
            let c = self.lock()?;
            let path = Self::path(&c, &image_id)?;
            let orientation: String = c.reader.query_row("SELECT COALESCE((SELECT value FROM metadata WHERE image_id=? AND key='orientation'),'1')", [&image_id], |r| r.get(0))?;
            let recipe_hash: String = c.reader.query_row(
                "SELECT COALESCE((SELECT hash FROM recipe_hash WHERE image_id=?),'')",
                [&image_id],
                |r| r.get(0),
            )?;
            (path, orientation.parse::<u8>().unwrap_or(1), recipe_hash)
        };
        let ext = Path::new(&path)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !matches!(ext.as_str(), "jpg" | "jpeg") {
            return self.request_raw(image_id, path, max_px, recipe_hash);
        }
        let bytes = self
            .previews
            .jpeg_preview(
                Path::new(&path),
                max_px,
                orientation,
                *blake3::hash(recipe_hash.as_bytes()).as_bytes(),
            )
            .map_err(failure)?;
        self.emit(EngineEvent::PreviewReady { image_id, max_px });
        Ok(PreviewResponse {
            bytes: Some(bytes),
            pending: false,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_preview_reopen_uses_revision_cache_without_source_work() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let path = photos.join("one.jpg");
        image::RgbImage::new(768, 512).save(&path).unwrap();
        let support = dir.path().join("support").to_string_lossy().into_owned();
        let engine = Engine::open(support.clone()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        let first = engine.clone().embedded_preview(id.clone(), 384).unwrap();
        assert!(!first.pending);
        assert_eq!(engine.previews.source_work_count(), 1);
        drop(engine);
        let engine = Engine::open(support).unwrap();
        let hit = engine.clone().embedded_preview(id.clone(), 384).unwrap();
        assert!(!hit.pending);
        assert_eq!(first.bytes, hit.bytes);
        assert_eq!(engine.previews.source_work_count(), 0);
        // Same size replacement, with mtime restored, must not return the hit.
        let metadata = std::fs::metadata(&path).unwrap();
        let replacement = photos.join("replacement");
        std::fs::write(&replacement, vec![0; metadata.len() as usize]).unwrap();
        std::fs::File::options()
            .write(true)
            .open(&replacement)
            .unwrap()
            .set_modified(metadata.modified().unwrap())
            .unwrap();
        std::fs::rename(replacement, path).unwrap();
        assert!(engine.clone().embedded_preview(id, 384).is_err());
        assert_eq!(engine.previews.source_work_count(), 1);
    }

    #[test]
    fn selection_survives_reopen_and_incremental_scan() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        image::RgbImage::new(32, 16)
            .save(photos.join("one.jpg"))
            .unwrap();
        let support = dir.path().join("support").to_string_lossy().into_owned();
        let engine = Engine::open(support.clone()).unwrap();
        let folder = engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        assert_eq!(folder.updated, 1);
        let rows = engine.list_images(ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        engine
            .set_selection(
                rows[0].id.clone(),
                Selection {
                    decision: Decision::Keep,
                    grade: Some(2),
                    mark: Some("Blue".into()),
                },
            )
            .unwrap();
        drop(engine);
        let reopened = Engine::open(support).unwrap();
        let rows = reopened.list_images(ImageQuery::default()).unwrap();
        assert_eq!(rows[0].selection.decision, Decision::Keep);
        assert_eq!(rows[0].selection.grade, Some(2));
        assert_eq!(reopened.index_folder(folder.path).unwrap().updated, 0);
        assert!(
            sidecar::Sidecar::paths(photos.join("one.jpg"))
                .recipe
                .exists()
        );
    }
}
