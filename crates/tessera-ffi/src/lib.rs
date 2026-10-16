//! Narrow, synchronous commands. Swift dispatches blocking work off its main actor.
mod agent_runs;
pub mod alloc;
mod assist;
mod backend;
mod catalog;
mod changes;
mod collections;
mod develop;
mod document;
mod drive;
mod enhance;
mod export;
#[allow(
    dead_code,
    reason = "journal storage awaits Smart Preview caller integration"
)]
mod image_edit_admission;
mod lock;
mod lrcat;
mod lrcat_fidelity;
mod lrcat_masks;
mod mem_trace;
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
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64},
    },
};
pub use understanding::*;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BridgeError {
    #[error("{message}")]
    Failure { message: String },
    /// A UI-safe call found the engine busy for longer than its budget (see
    /// `lock::UI_BUDGET`); nothing was done. Retry shortly, off the main thread.
    #[error("{message}")]
    Busy { message: String },
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
    /// Catalog file name of an imported Smart Preview (user data: show it,
    /// never put it in accessibility identifiers). None for ordinary files.
    #[uniffi(default = None)]
    pub display_name: Option<String>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct FolderHandle {
    pub path: String,
    pub updated: u64,
}
/// An indexed root (CS-1). Roots on external volumes are remembered by volume
/// UUID and relative path; `path` is where the root is now, or where it was
/// last seen while its volume is not attached.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct RootInfo {
    pub path: String,
    /// The folder's own name.
    pub name: String,
    /// None for boot-volume roots (plain paths).
    pub volume_uuid: Option<String>,
    pub relative_path: String,
    /// The drive's name as mounted (its last mount point's name).
    pub volume_name: Option<String>,
    pub attached: bool,
    pub images: u64,
}

struct Catalog {
    index: index::Index,
    /// The read-only connection, shared with `Engine::reader`: a lookup that
    /// needs no `Index` API takes it alone and never waits on the writer (WAL
    /// readers see committed rows and are never blocked). It is inner to the
    /// catalog lock: a catalog holder may take it, never the other way round.
    reader: Arc<lock::Traced<Connection>>,
}
impl Catalog {
    #[track_caller]
    fn reader(&self) -> Result<lock::Guard<'_, Connection>> {
        self.reader.lock()
    }
}
/// The engine's resolver, handed to the index as well.
#[derive(Debug)]
struct SharedVolumes(Arc<dyn index::VolumeResolver>);
impl index::VolumeResolver for SharedVolumes {
    fn volume_of(&self, path: &Path) -> engine_api::EngineResult<Option<index::Volume>> {
        self.0.volume_of(path)
    }
    fn mounted(&self) -> engine_api::EngineResult<Vec<index::Volume>> {
        self.0.mounted()
    }
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
    /// The catalog (index writer). Never hold it across file I/O, rendering,
    /// ML or hashing: snapshot under the lock, work outside, write back
    /// briefly. `lock` reports long holds and waits (see `lock`).
    catalog: lock::Traced<Catalog>,
    /// The catalog's read-only connection (see `Catalog::reader`), for
    /// lookups that must not queue behind a scan or a save.
    reader: Arc<lock::Traced<Connection>>,
    previews: previews::PreviewStore,
    /// Shared scheduler: RAW previews (`Priority::Preview`) and develop
    /// viewports (`Priority::Viewport`). Jobs are not pre-empted, so it keeps
    /// a worker free for the viewport while previews run.
    jobs: jobs::ThreadPoolScheduler,
    /// Develop renderer and memo cache shared by every session (lazy: the
    /// Metal context is created on the first develop session).
    renderer: std::sync::OnceLock<backend::Backend>,
    /// Open develop sessions, for the memory trace (`mem_trace`) and for
    /// release-on-close decisions. Weak: a session's lifetime is the host's.
    sessions: Mutex<Vec<std::sync::Weak<develop::Shared>>>,
    /// AI mask segmentation (loaded on first use; see `develop::masks`).
    segmenter: develop::SegmenterSlot,
    /// Face detector + recognizer (downloaded and loaded on first face analysis).
    faces: Mutex<Option<ml_faces::FaceModels>>,
    smart_thumbnail_states: Mutex<smart_preview_thumbnail::States>,
    preview_states: Mutex<std::collections::HashMap<preview::RequestKey, preview::State>>,
    /// Owner-recipe facts for thumbnail requests, keyed by path and recipe hash.
    preview_sources: Mutex<preview::PreviewSources>,
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
    /// Debounced `.tessera/` writer (CS-1, see `drive`).
    drive: drive::DriveWriter,
    /// The index's volume resolver, shared so `roots` can list mounted
    /// volumes before taking the catalog lock.
    volumes: Arc<dyn index::VolumeResolver>,
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
    #[track_caller]
    fn lock(&self) -> Result<lock::Guard<'_, Catalog>> {
        self.catalog.lock()
    }
    /// The catalog for a UI-safe method: waits at most `lock::UI_BUDGET`,
    /// then returns `BridgeError::Busy` for the host to retry.
    #[track_caller]
    fn lock_ui(&self) -> Result<lock::Guard<'_, Catalog>> {
        self.catalog.lock_for(lock::UI_BUDGET)
    }
    fn path(c: &Catalog, id: &str) -> Result<String> {
        Self::path_on(&*c.reader()?, id)
    }
    fn path_on(reader: &Connection, id: &str) -> Result<String> {
        parse_id(id)?;
        Ok(reader.query_row(
            "SELECT f.path FROM image i JOIN file f ON f.id=i.file_id WHERE i.id=?",
            [id],
            |r| r.get(0),
        )?)
    }
    /// The reader for a UI-safe lookup: never queues behind the writer, and
    /// waits at most `lock::UI_BUDGET` for another reader.
    #[track_caller]
    fn reader_ui(&self) -> Result<lock::Guard<'_, Connection>> {
        self.reader.lock_for(lock::UI_BUDGET)
    }
    /// The image still resolves to `path`: a relocated volume or a prune
    /// since the caller looked it up fails the write instead of landing
    /// sidecars beside the wrong file.
    fn check_path(&self, image_id: &str, path: &Path, before: &str) -> Result<()> {
        let current = Self::path(&*self.lock()?, image_id)?;
        if Path::new(&current) != path {
            return Err(failure(format!("image path changed before {before}")));
        }
        Ok(())
    }
    /// Writes the recipe and XMP sidecars, then refreshes the image's index
    /// row. The caller holds the destination's recipe-write gate (which
    /// serializes writers and readers of the sidecars); the catalog lock is
    /// taken only for the index refresh, never across the writes (ST-1).
    fn persist(&self, path: &Path, doc: &sidecar::RecipeDocument) -> Result<()> {
        let packet = catalog::selection_packet(path, doc)?;
        self.persist_with_packet(path, doc, &packet)
    }
    fn persist_with_packet(
        &self,
        path: &Path,
        doc: &sidecar::RecipeDocument,
        packet: &sidecar::XmpPacket,
    ) -> Result<()> {
        // Recipe is the authoritative commit. A later scan repairs the rebuildable index.
        let paths = catalog::write_paths(path)?;
        Sidecar::write_recipe(paths.recipe, doc)?;
        Sidecar::write_xmp(paths.xmp, packet)?;
        self.lock()?
            .index
            .scan_file(path, &catalog::Sidecars, &catalog::IndexedMetadata)?;
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

impl Engine {
    /// Re-keys album membership in a folder's library document after image
    /// ids changed (CS-1 migration); a folder without a document is skipped.
    fn rekey_library(folder: &Path, pairs: &[(ImageId, ImageId)]) -> Result<()> {
        let path = library::Library::resolve_in(folder);
        if pairs.is_empty() || !path.exists() {
            return Ok(());
        }
        let mut library = library::Library::read(&path)?;
        if library.rekey_images(&pairs.iter().copied().collect()) {
            library.write(&path)?;
        }
        Ok(())
    }

    /// One-time id migration (docs/05 §2.3): a catalog with path-derived ids
    /// re-keys every file on an attached external volume in place, then the
    /// roots' album documents. Files on absent volumes re-key on their next scan.
    fn migrate_image_ids(index: &index::Index) -> Result<()> {
        if index.id_scheme()? != "path" {
            return Ok(());
        }
        let legacy = index.legacy_images()?;
        let pairs: Vec<_> = legacy.iter().map(|l| (l.old, l.new)).collect();
        index.rekey_images(&pairs)?;
        for root in index.roots()? {
            Self::rekey_library(&root.path, &pairs)?;
        }
        index.set_id_scheme("volume")?;
        if !pairs.is_empty() {
            eprintln!(
                "catalog: migrated {} image ids to volume-relative ids",
                pairs.len()
            );
        }
        Ok(())
    }

    /// `open` with the volume resolver the index should use (tests).
    pub(crate) fn open_with_volumes(
        app_support_dir: String,
        volumes: Option<Box<dyn index::VolumeResolver>>,
    ) -> Result<Arc<Self>> {
        std::fs::create_dir_all(&app_support_dir)?;
        let db = Path::new(&app_support_dir).join("index.sqlite");
        let mut index = index::Index::open(&db)?;
        let volumes: Arc<dyn index::VolumeResolver> = match volumes {
            Some(volumes) => Arc::from(volumes),
            None => Arc::new(index::SystemVolumes),
        };
        index.set_volume_resolver(Box::new(SharedVolumes(volumes.clone())));
        index.set_machine(&drive::machine_id(Path::new(&app_support_dir))?)?;
        Self::migrate_image_ids(&index)?;
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
        let reader = Arc::new(lock::Traced::new("catalog reader", reader));
        let engine = Arc::new_cyclic(|this| Self {
            db,
            catalog: lock::Traced::new(
                "catalog",
                Catalog {
                    index,
                    reader: reader.clone(),
                },
            ),
            reader,
            previews,
            jobs: jobs::ThreadPoolScheduler::with_interactive_reservation(3),
            renderer: std::sync::OnceLock::new(),
            sessions: Mutex::new(Vec::new()),
            segmenter: Default::default(),
            faces: Mutex::new(None),
            smart_thumbnail_states: Mutex::new(Default::default()),
            preview_states: Mutex::new(std::collections::HashMap::new()),
            preview_sources: Mutex::new(Default::default()),
            listener: Mutex::new(None),
            understanding: Default::default(),
            gpu: std::sync::OnceLock::new(),
            documents: Default::default(),
            notified,
            heads,
            watching: AtomicBool::new(false),
            drive: Default::default(),
            volumes,
            this: this.clone(),
            #[cfg(test)]
            develop_writer_constructions: std::sync::atomic::AtomicUsize::new(0),
        });
        engine.start_drive_writer();
        Ok(engine)
    }
}

#[uniffi::export]
impl Engine {
    #[uniffi::constructor]
    pub fn open(app_support_dir: String) -> Result<Arc<Self>> {
        Self::open_with_volumes(app_support_dir, None)
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
        let (updated, rekeyed) = {
            let mut c = self.lock()?;
            // What the drive carries (people, scores, albums) is there before
            // the scan; the scan fills the gaps (CS-1).
            let volume = c.index.volumes().volume_of(&path)?;
            let imported = c.index.import_drive_state(&path, volume.as_ref())?;
            if imported != index::ImportCounts::default() {
                eprintln!(
                    "drive state: {} analysis records, {} people, {} assignments from {}",
                    imported.analysis,
                    imported.persons,
                    imported.assignments,
                    path.display()
                );
            }
            let updated = c
                .index
                .scan(&path, &catalog::Sidecars, &catalog::IndexedMetadata)?;
            // A root the drive has not seen yet gets its existing analysis and
            // people written out (a catalog older than CS-1).
            let tessera = index::TesseraDir::of_root(&path);
            if !tessera.analysis_dir().exists() {
                c.index.mark_root_dirty(&path)?;
            }
            (updated, c.index.take_rekeyed())
        };
        // Legacy ids found on a drive were re-keyed in place: follow in the albums.
        Self::rekey_library(&path, &rekeyed)?;
        library::Library::resolve_in(&path);
        self.emit(EngineEvent::ScanProgress {
            path: path.to_string_lossy().into_owned(),
            updated: updated as u64,
            finished: true,
        });
        self.notify_changes();
        self.trace_memory("after scan");
        Ok(FolderHandle {
            path: path.to_string_lossy().into_owned(),
            updated: updated as u64,
        })
    }
    /// Every indexed root. A root whose volume is mounted elsewhere than last
    /// time is relocated in the catalog and reported at its current path; a
    /// root whose volume is absent is reported not attached, with its counts.
    /// UI-safe (see `lock`): `Busy` when a worker holds the catalog.
    pub fn roots(&self) -> Result<Vec<RootInfo>> {
        // Listing mounted volumes stats each one; a slow or hung volume must
        // not stall the catalog.
        let mounted = self.volumes.mounted()?;
        let c = self.lock_ui()?;
        let mut out = Vec::new();
        for root in c.index.roots()? {
            let located = c.index.locate_root_among(&root, &mounted)?;
            let path = located.clone().unwrap_or_else(|| root.path.clone());
            let volume_name = root.location.volume_uuid.as_ref().map(|_| {
                // `/Volumes/<name>/<relative>`: the component before the relative path.
                let depth = Path::new(&root.location.relative_path).components().count();
                let mut mount = path.clone();
                for _ in 0..depth {
                    mount.pop();
                }
                mount
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            });
            out.push(RootInfo {
                name: path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.to_string_lossy().into_owned()),
                path: path.to_string_lossy().into_owned(),
                volume_uuid: root.location.volume_uuid,
                relative_path: root.location.relative_path,
                volume_name,
                attached: located.is_some_and(|p| p.is_dir()),
                images: root.images,
            });
        }
        Ok(out)
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
            let (path, capture_time, orientation, recipe_hash, listed, original) = c.reader()?.query_row(
                "SELECT f.path,i.capture_time,COALESCE((SELECT value FROM metadata WHERE image_id=i.id AND key='orientation'),'1'),COALESCE(h.hash,''),(SELECT value FROM metadata WHERE image_id=i.id AND key=?2),(SELECT value FROM metadata WHERE image_id=i.id AND key=?3) FROM image i JOIN file f ON f.id=i.file_id LEFT JOIN recipe_hash h ON h.image_id=i.id WHERE i.id=?1",
                [id_string.as_str(), catalog::LISTING_RECIPE_KEY, catalog::LISTING_ORIGINAL_KEY], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<String>>(4)?, r.get::<_, Option<String>>(5)?)))?;
            let row = catalog::project(
                Path::new(&path),
                Some(&catalog::ScanFacts {
                    recipe: listed,
                    original,
                    current: recipe_hash.clone(),
                }),
            );
            result.push(ImageSummary {
                lightroom_smart_preview: row.offline,
                display_name: row.display_name,
                id: id_string,
                path: row.source,
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
        self.check_path(&image_id, Path::new(&path), "recipe write")?;
        let mut doc = catalog::document(Path::new(&path), parse_id(&image_id)?)?;
        doc.recipe.selection = selection;
        doc.record_write("tessera-mac", now_ms())?;
        self.persist(Path::new(&path), &doc)?;
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
                let path = Self::path_on(&*self.reader.lock()?, &image_id)?;
                // Indexed ownership is sufficient offline; never canonicalize the
                // absent original folder or synthesize a default recipe here.
                !Path::new(&path).is_file()
            })
        {
            let doc: sidecar::RecipeDocument =
                serde_json::from_slice(&snapshot.recipe).map_err(failure)?;
            return String::from_utf8(doc.recipe.to_json()?).map_err(failure);
        }
        let path = Self::path_on(&*self.reader.lock()?, &image_id)?;
        // The sidecars are read under their destination gate, not the catalog.
        let gate = recipe_write::gate_for(Path::new(&path))?;
        let _read = gate.begin_read()?;
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
        self.check_path(&image_id, Path::new(&path), "recipe write")?;
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
        self.persist_with_packet(Path::new(&path), &doc, &packet)?;
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
            let orientation: String = c.reader()?.query_row("SELECT COALESCE((SELECT value FROM metadata WHERE image_id=? AND key='orientation'),'1')", [&image_id], |r| r.get(0))?;
            let recipe_hash: String = c.reader()?.query_row(
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

    /// ST-1: a UI-safe method answers `Busy` within its budget while a
    /// worker holds the catalog or the queue, and works again once released.
    #[test]
    fn ui_safe_methods_report_busy_instead_of_waiting() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        image::RgbImage::new(32, 16)
            .save(photos.join("one.jpg"))
            .unwrap();
        let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let folder = engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap()
            .path;
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        let session = engine.open_cull_session(folder).unwrap();
        let busy = |r: Result<()>| matches!(r, Err(BridgeError::Busy { .. }));
        std::thread::scope(|s| {
            let held = engine.lock().unwrap();
            s.spawn(|| {
                let started = std::time::Instant::now();
                assert!(busy(engine.roots().map(drop)));
                // A path lookup uses the reader alone: the writer never delays it.
                assert!(engine.agent_provenance(id.clone()).is_ok());
                assert!(started.elapsed() < std::time::Duration::from_millis(500));
                // The queue has its own lock: still answering.
                assert_eq!(session.images().unwrap().len(), 1);
            });
            // Longer than every budget above together.
            std::thread::sleep(std::time::Duration::from_millis(400));
            drop(held);
        });
        std::thread::scope(|s| {
            let held = session.lock().unwrap();
            s.spawn(|| {
                assert!(busy(session.images().map(drop)));
                assert!(busy(session.groups().map(drop)));
                assert!(busy(session.people(false).map(drop)));
                assert!(busy(session.face_strip(id.clone()).map(drop)));
                assert!(busy(session.assist_status().map(drop)));
                assert!(busy(session.review().map(drop)));
                assert_eq!(engine.roots().unwrap().len(), 1);
            });
            // Longer than every budget above together.
            std::thread::sleep(std::time::Duration::from_millis(400));
            drop(held);
        });
        assert_eq!(engine.roots().unwrap().len(), 1);
        assert_eq!(session.images().unwrap().len(), 1);
        assert!(session.review().is_ok());
    }

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

    /// CS-1: a catalog with path-derived ids re-keys a drive's files on the
    /// next open; decisions, albums and legacy-stamped sidecars survive.
    #[test]
    fn legacy_ids_migrate_to_volume_ids_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let mount = dir.path().canonicalize().unwrap().join("Drive");
        let photos = mount.join("shoot");
        std::fs::create_dir_all(&photos).unwrap();
        image::RgbImage::new(32, 16)
            .save(photos.join("one.jpg"))
            .unwrap();
        let support = dir.path().join("support").to_string_lossy().into_owned();
        let boot = || Some(Box::new(index::FixedVolumes(vec![])) as Box<dyn index::VolumeResolver>);
        let drive = || {
            Some(Box::new(index::FixedVolumes(vec![index::Volume {
                uuid: "DRV-1".into(),
                mount_point: mount.clone(),
            }])) as Box<dyn index::VolumeResolver>)
        };
        let engine = Engine::open_with_volumes(support.clone(), boot()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap();
        let old = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        assert_eq!(
            old,
            ImageId::for_path(&photos.join("one.jpg").to_string_lossy()).to_string()
        );
        engine
            .set_selection(
                old.clone(),
                Selection {
                    decision: Decision::Keep,
                    grade: Some(1),
                    mark: None,
                },
            )
            .unwrap();
        let store = engine
            .clone()
            .open_library(photos.join("library.json").to_string_lossy().into())
            .unwrap();
        let album = store.create_album("Picks".into(), None).unwrap();
        store.add_to_album(album, vec![old.clone()]).unwrap();
        // What a pre-1.8 catalog records.
        engine.lock().unwrap().index.set_id_scheme("path").unwrap();
        drop(store);
        drop(engine);

        let engine = Engine::open_with_volumes(support, drive()).unwrap();
        let new = ImageId::for_volume_path("DRV-1", "shoot/one.jpg").to_string();
        let rows = engine.list_images(ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, new);
        assert_eq!(rows[0].selection.grade, Some(1));
        assert_eq!(engine.lock().unwrap().index.id_scheme().unwrap(), "volume");
        // The document moved into `.tessera/` on open (CS-1) and was re-keyed there.
        assert!(!photos.join("library.json").exists());
        let library = library::Library::read(library::Library::path_in(&photos)).unwrap();
        assert_eq!(
            library.albums["Picks"].images,
            vec![parse_id(&new).unwrap()]
        );
        // The sidecar still carries the legacy id and reads as this image.
        let recipe = engine.get_recipe(new.clone()).unwrap();
        assert!(recipe.contains(&new));
        assert!(engine.get_recipe(old).is_err());
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
