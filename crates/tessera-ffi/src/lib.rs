//! Narrow, synchronous commands. Swift dispatches blocking work off its main actor.
mod backend;
mod catalog;
mod collections;
mod develop;
mod metadata;
mod preview;
mod session;
#[doc(hidden)]
pub mod surface;
pub use collections::*;
pub use develop::*;
use engine_api::{id::ImageId, recipe as core};
pub use metadata::*;
pub use preview::PreviewResponse;
use rusqlite::{Connection, OpenFlags};
pub use session::*;
use sidecar::Sidecar;
use std::{
    path::Path,
    sync::{Arc, Mutex, MutexGuard},
};

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
    preview_states: Mutex<std::collections::HashMap<preview::RequestKey, preview::State>>,
    listener: Mutex<Option<Arc<dyn EngineEventListener>>>,
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
    ) -> (Arc<image_core::Renderer>, String) {
        let backend = self.renderer.get_or_init(|| backend::select(image));
        (Arc::new(backend.renderer()), backend.name.clone())
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
        // Recipe is the authoritative commit. A later scan repairs the rebuildable index.
        Sidecar::write_recipe(Sidecar::paths(path).recipe, doc)?;
        Sidecar::write_xmp(catalog::xmp_path(path), &packet)?;
        c.index.scan(
            path.parent()
                .ok_or_else(|| failure("image has no folder"))?,
            &catalog::Sidecars,
            &catalog::EmbeddedMetadata,
        )?;
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
        Ok(Arc::new(Self {
            db,
            catalog: Mutex::new(Catalog { index, reader }),
            previews: previews::PreviewStore::new(
                Path::new(&app_support_dir).join("previews"),
                512 << 20,
            )
            .map_err(failure)?,
            jobs: jobs::ThreadPoolScheduler::new(3),
            renderer: std::sync::OnceLock::new(),
            segmenter: Default::default(),
            preview_states: Mutex::new(std::collections::HashMap::new()),
            listener: Mutex::new(None),
        }))
    }
    pub fn set_event_listener(&self, listener: Option<Arc<dyn EngineEventListener>>) {
        *self.listener.lock().unwrap_or_else(|e| e.into_inner()) = listener;
    }
    pub fn index_folder(&self, path: String) -> Result<FolderHandle> {
        let path = Path::new(&path).canonicalize()?;
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

            result.push(ImageSummary {
                id: id_string,
                path,
                capture_time,
                orientation: orientation.parse().unwrap_or(1),
                selection: c.index.selection(id)?.unwrap_or_default().into(),
                recipe_hash,
            });
        }
        Ok(result)
    }
    pub fn set_selection(&self, image_id: String, selection: Selection) -> Result<()> {
        let selection = selection.into_core()?;
        let mut c = self.lock()?;
        let path = Self::path(&c, &image_id)?;
        let mut doc = catalog::document(Path::new(&path), parse_id(&image_id)?)?;
        doc.recipe.selection = selection;
        doc.record_write("tessera-mac", now_ms())?;
        Self::persist(&mut c, Path::new(&path), &doc)
    }
    pub fn get_recipe(&self, image_id: String) -> Result<String> {
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
        let mut recipe: core::Recipe = serde_json::from_str(&json).map_err(failure)?;
        if recipe.image_id != Some(parse_id(&image_id)?) {
            return Err(failure("recipe image_id mismatch"));
        }
        recipe.validate()?;
        recipe.to_json()?; // Reject forward-version documents before any write.
        let mut c = self.lock()?;
        let path = Self::path(&c, &image_id)?;
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
        Self::persist(&mut c, Path::new(&path), &doc)
    }
    /// RAW cache misses return pending immediately; PreviewReady signals completion.
    pub fn embedded_preview(
        self: Arc<Self>,
        image_id: String,
        max_px: u32,
    ) -> Result<PreviewResponse> {
        use previews::Codec;
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
        let jpeg = std::fs::read(&path)?;
        // Bound the pyramid work to the requested tier, not the full camera JPEG.
        let decoded = previews::Jpeg.decode(&jpeg).map_err(failure)?;
        let scaled = image::DynamicImage::ImageRgb8(decoded)
            .thumbnail(max_px, max_px)
            .to_rgb8();
        let jpeg = previews::Jpeg.encode(&scaled).map_err(failure)?;
        let bytes = {
            let store = &self.previews;
            let recipe_hash = core::Recipe::default().recipe_hash().0.0;
            let key = previews::PreviewKey::new(&jpeg, orientation, recipe_hash);
            if let Some(bytes) = store.get(&key, previews::Level::Full) {
                bytes
            } else {
                let key = store
                    .from_embedded_jpeg(&jpeg, orientation, recipe_hash)
                    .map_err(failure)?;
                store
                    .get(&key, previews::Level::Full)
                    .ok_or_else(|| failure("preview was evicted"))?
            }
        };
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
