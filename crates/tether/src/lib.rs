//! Tethered capture. Backends deliver completed downloads, never partial files.
//! A single scoring worker preserves arrival order; queue events follow persisted
//! indexing, preview extraction and scoring. Decisions are never changed.
pub mod fake;
mod ingest;
mod naming;
pub mod native;

use engine_api::{id::ImageId, jobs::Scheduler};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported operation")]
    Unsupported,
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Engine(#[from] engine_api::EngineError),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Debug, serde::Serialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub can_capture: bool,
    /// Battery charge when the camera reports it.
    pub battery_percent: Option<u8>,
    /// Frames left on the card when the backend knows it.
    pub shots_remaining: Option<u32>,
}

/// Backends own their threading requirements. A dynamic libgphoto2 adapter can
/// implement this without exposing vendor types to callers.
pub trait TetherBackend {
    fn devices(&mut self) -> Result<Vec<Device>>;
    fn start(&mut self, folder: &Path) -> Result<()>;
    fn capture(&mut self) -> Result<()>;
    fn poll(&mut self) -> Result<Vec<PathBuf>>;
    fn stop(&mut self) -> Result<()>;
    fn live_view(&mut self) -> Result<Vec<u8>> {
        Err(Error::Unsupported)
    }
}

/// Lets callers choose a backend at run time (the app's `--fake-tether` aid).
impl<T: TetherBackend + ?Sized> TetherBackend for Box<T> {
    fn devices(&mut self) -> Result<Vec<Device>> {
        (**self).devices()
    }
    fn start(&mut self, folder: &Path) -> Result<()> {
        (**self).start(folder)
    }
    fn capture(&mut self) -> Result<()> {
        (**self).capture()
    }
    fn poll(&mut self) -> Result<Vec<PathBuf>> {
        (**self).poll()
    }
    fn stop(&mut self) -> Result<()> {
        (**self).stop()
    }
    fn live_view(&mut self) -> Result<Vec<u8>> {
        (**self).live_view()
    }
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Frame {
    pub sequence: u64,
    pub path: PathBuf,
    pub image_id: Option<ImageId>,
    /// JPEG bytes persisted outside the scanned image namespace (.preview).
    pub preview: Option<PathBuf>,
    /// Missing models/inference failure is not equivalent to detecting no faces.
    pub face_warning: Option<String>,
    pub error: Option<String>,
}

pub struct Session<B: TetherBackend> {
    backend: B,
    staging: tempfile::TempDir,
    folder: PathBuf,
    naming: String,
    db: PathBuf,
    sequence: u64,
    seen: HashSet<PathBuf>,
    pool: Option<jobs::ThreadPoolScheduler>,
    faces: Arc<Mutex<ingest::Faces>>,
    tx: mpsc::Sender<Frame>,
    rx: mpsc::Receiver<Frame>,
    stopped: bool,
}
impl<B: TetherBackend> Session<B> {
    pub fn start(
        mut backend: B,
        folder: &Path,
        naming: &str,
        db: &Path,
        model_dir: Option<PathBuf>,
    ) -> Result<Self> {
        naming::render(naming, Path::new("test.jpg"), 1)?;
        std::fs::create_dir_all(folder)?;
        let folder = folder.canonicalize()?;
        index::Index::open(db)?;
        let staging = tempfile::tempdir()?;
        backend.start(staging.path())?;
        let (tx, rx) = mpsc::channel();
        Ok(Self {
            backend,
            staging,
            folder,
            naming: naming.into(),
            db: db.into(),
            sequence: 0,
            seen: HashSet::new(),
            pool: Some(jobs::ThreadPoolScheduler::new(1)),
            faces: Arc::new(Mutex::new(ingest::Faces::Pending(model_dir))),
            tx,
            rx,
            stopped: false,
        })
    }
    pub fn capture(&mut self) -> Result<()> {
        if self.stopped {
            return Err(Error::Message("session stopped".into()));
        }
        self.backend.capture()
    }
    pub fn live_view(&mut self) -> Result<Vec<u8>> {
        self.backend.live_view()
    }
    /// The session's camera(s), e.g. to refresh battery or remaining-frame readouts.
    pub fn devices(&mut self) -> Result<Vec<Device>> {
        self.backend.devices()
    }
    pub fn folder(&self) -> &Path {
        &self.folder
    }
    pub fn events(&self) -> &mpsc::Receiver<Frame> {
        &self.rx
    }
    /// Pump native callbacks on the owning thread, then enqueue score work.
    pub fn poll(&mut self) -> Result<()> {
        if self.stopped {
            return Err(Error::Message("session stopped".into()));
        }
        for path in self.backend.poll()? {
            if !self.seen.insert(path.clone()) {
                continue;
            }
            self.sequence += 1;
            self.pool.as_ref().expect("active pool").submit(
                Box::new(ingest::Ingest {
                    source: path,
                    staging: self.staging.path().to_owned(),
                    folder: self.folder.clone(),
                    naming: self.naming.clone(),
                    db: self.db.clone(),
                    sequence: self.sequence,
                    faces: self.faces.clone(),
                    tx: self.tx.clone(),
                }),
                None,
            );
        }
        Ok(())
    }
    /// Close camera, then finish accepted score jobs before returning. No new
    /// downloads are accepted afterwards. Callers may drain final events.
    pub fn stop(&mut self) -> Result<()> {
        if self.stopped {
            return Ok(());
        }
        let pending = self.poll();
        let result = self.backend.stop();
        let final_downloads = self.poll();
        self.stopped = true;
        // A FIFO barrier lets accepted jobs finish before pool Drop cancels.
        if let Some(pool) = self.pool.take() {
            let (tx, rx) = mpsc::channel();
            pool.submit(Box::new(ingest::Barrier(tx)), None);
            let _ = rx.recv();
            drop(pool);
        }
        pending.and(result).and(final_downloads)
    }
}
impl<B: TetherBackend> Drop for Session<B> {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}
