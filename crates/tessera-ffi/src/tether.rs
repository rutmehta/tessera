//! Unlike other engine commands, tether calls belong on the process main thread.
//! The host calls tether_poll on a main-thread timer (e.g. every 100 ms).
//! Indexing/scoring runs on a worker, never in a native camera delegate callback.
//!
//! `tether_use_fake` (the app's hidden `--fake-tether <folder>` aid) swaps the
//! ImageCaptureCore backend for `tether::fake::FolderDropBackend` on this thread,
//! so the whole path (download, naming, index, preview, scores) runs without a camera.
use crate::{Engine, Result, failure};
use ::tether::{Device, Session, TetherBackend, fake::FolderDropBackend, native::NativeBackend};
use std::{cell::RefCell, path::PathBuf, sync::Arc, time::Duration};

#[derive(Clone, Debug, uniffi::Record)]
pub struct TetherDevice {
    pub id: String,
    pub name: String,
    pub can_capture: bool,
    /// Battery charge when the camera reports it.
    pub battery_percent: Option<u8>,
    /// Frames left on the card when the backend knows it.
    pub shots_remaining: Option<u32>,
}
impl From<Device> for TetherDevice {
    fn from(d: Device) -> Self {
        Self {
            id: d.id,
            name: d.name,
            can_capture: d.can_capture,
            battery_percent: d.battery_percent,
            shots_remaining: d.shots_remaining,
        }
    }
}
/// A downloaded, indexed and scored frame. Scores are read back from the catalog
/// when the frame is published, so the host can badge it without another call.
#[derive(Clone, Debug, uniffi::Record)]
pub struct TetherFrame {
    pub sequence: u64,
    pub path: String,
    pub image_id: Option<String>,
    pub preview: Option<String>,
    pub face_warning: Option<String>,
    pub error: Option<String>,
    /// Whole-frame classical sharpness in [0, 1] (ml-quality).
    pub sharpness: Option<f64>,
    /// Faces found; `None` when face analysis did not run (see `face_warning`).
    pub faces: Option<u32>,
    /// Sharpness of the largest face (the likely subject), in [0, 1].
    pub face_focus: Option<f64>,
    /// Lowest eyes-open proxy across faces (anyone blinking), in [0, 1].
    pub eyes_open: Option<f64>,
}
impl From<::tether::Frame> for TetherFrame {
    fn from(f: ::tether::Frame) -> Self {
        Self {
            sequence: f.sequence,
            path: f.path.to_string_lossy().into_owned(),
            image_id: f.image_id.map(|id| id.to_string()),
            preview: f.preview.map(|p| p.to_string_lossy().into_owned()),
            face_warning: f.face_warning,
            error: f.error,
            sharpness: None,
            faces: None,
            face_focus: None,
            eyes_open: None,
        }
    }
}

/// Reads the frame's stored scores. Missing scores stay `None` (never zero).
fn with_scores(index: Option<&index::Index>, frame: ::tether::Frame) -> TetherFrame {
    let id = frame.image_id;
    let analysed_faces = frame.face_warning.is_none();
    let mut out = TetherFrame::from(frame);
    let (Some(index), Some(id)) = (index, id) else {
        return out;
    };
    out.sharpness = index.scores(id).ok().and_then(|scores| {
        scores
            .into_iter()
            .find(|s| s.signal == "sharpness")
            .map(|s| s.value)
    });
    if analysed_faces && let Ok(faces) = index.faces(id) {
        out.faces = Some(faces.len() as u32);
        out.face_focus = faces
            .iter()
            .max_by(|a, b| (a.bbox[2] * a.bbox[3]).total_cmp(&(b.bbox[2] * b.bbox[3])))
            .map(|f| f.sharpness);
        out.eyes_open = faces
            .iter()
            .filter_map(|f| f.eyes_open)
            .min_by(f64::total_cmp);
    }
    out
}

fn frames(db: &std::path::Path, raw: Vec<::tether::Frame>) -> Vec<TetherFrame> {
    if raw.is_empty() {
        return Vec::new();
    }
    let index = index::Index::open(db).ok();
    raw.into_iter()
        .map(|f| with_scores(index.as_ref(), f))
        .collect()
}

type Backend = Box<dyn TetherBackend>;
thread_local! {
    /// Test camera: (source folder, drop interval). See `tether_use_fake`.
    static FAKE: RefCell<Option<(PathBuf, Duration)>> = const { RefCell::new(None) };
}
fn backend() -> Result<Backend> {
    match FAKE.with(|f| f.borrow().clone()) {
        Some((folder, interval)) => Ok(Box::new(
            FolderDropBackend::new(&folder, interval).map_err(failure)?,
        )),
        None => Ok(Box::new(NativeBackend::new().map_err(failure)?)),
    }
}
#[uniffi::export(with_foreign)]
pub trait TetherEventListener: Send + Sync {
    fn on_frame(&self, frame: TetherFrame);
}
struct Active {
    db: PathBuf,
    session: Session<Backend>,
    listener: Option<Arc<dyn TetherEventListener>>,
}
thread_local! { static ACTIVE: RefCell<Option<Active>> = const { RefCell::new(None) }; }

fn with_active<T>(engine: &Engine, f: impl FnOnce(&mut Active) -> Result<T>) -> Result<T> {
    ACTIVE.with(|slot| {
        let mut slot = slot
            .try_borrow_mut()
            .map_err(|_| failure("reentrant tether call"))?;
        let active = slot.as_mut().ok_or_else(|| {
            failure("no tether session on this thread; use the process main thread")
        })?;
        if active.db != engine.db {
            return Err(failure("tether session belongs to another catalog"));
        }
        f(active)
    })
}

#[uniffi::export]
impl Engine {
    /// Connected cameras. During a session this asks the session's own backend
    /// (fresh battery / remaining-frame readouts) instead of opening another.
    pub fn tether_devices(&self) -> Result<Vec<TetherDevice>> {
        let active = ACTIVE.with(|slot| {
            let mut slot = slot
                .try_borrow_mut()
                .map_err(|_| failure("reentrant tether call"))?;
            match slot.as_mut() {
                Some(a) if a.db == self.db => a.session.devices().map(Some).map_err(failure),
                _ => Ok(None),
            }
        })?;
        let devices = match active {
            Some(devices) => devices,
            None => backend()?.devices().map_err(failure)?,
        };
        Ok(devices.into_iter().map(Into::into).collect())
    }
    /// Hidden test aid: `Some(folder)` makes the next sessions on this thread use a
    /// test camera that shoots that folder's images (on `tether_capture`, and every
    /// `interval_ms` when non-zero); `None` restores ImageCaptureCore.
    pub fn tether_use_fake(&self, source_folder: Option<String>, interval_ms: u64) -> Result<()> {
        if let Some(folder) = &source_folder
            && !std::path::Path::new(folder).is_dir()
        {
            return Err(failure(format!(
                "test camera folder {folder} does not exist"
            )));
        }
        FAKE.with(|f| {
            *f.borrow_mut() = source_folder
                .map(|folder| (PathBuf::from(folder), Duration::from_millis(interval_ms)))
        });
        Ok(())
    }
    /// Whether this catalog has a tether session on this thread.
    pub fn tether_active(&self) -> bool {
        ACTIVE.with(|slot| {
            slot.try_borrow()
                .map(|s| s.as_ref().is_some_and(|a| a.db == self.db))
                .unwrap_or(true)
        })
    }
    pub fn tether_start(&self, session_folder: String, naming: String) -> Result<()> {
        ACTIVE.with(|slot| {
            let mut slot = slot
                .try_borrow_mut()
                .map_err(|_| failure("reentrant tether call"))?;
            if slot.is_some() {
                return Err(failure("a tether session is already active"));
            }
            let backend = backend()?;
            let session = Session::start(
                backend,
                std::path::Path::new(&session_folder),
                &naming,
                &self.db,
                self.db.parent().map(PathBuf::from),
            )
            .map_err(failure)?;
            *slot = Some(Active {
                db: self.db.clone(),
                session,
                listener: None,
            });
            Ok(())
        })
    }
    pub fn tether_capture(&self) -> Result<()> {
        with_active(self, |a| a.session.capture().map_err(failure))
    }
    pub fn tether_set_listener(
        &self,
        listener: Option<Arc<dyn TetherEventListener>>,
    ) -> Result<()> {
        with_active(self, |a| {
            a.listener = listener;
            Ok(())
        })
    }
    /// Returns frames as well as notifying the optional listener. Callbacks run
    /// outside the session borrow, so UI code may safely reenter tether commands.
    /// The ingest worker writes the catalog on its own connection; each poll
    /// announces those writes as `EngineEvent::LibraryChanged` (frames join the
    /// library through the change feed, not a reload).
    pub fn tether_poll(&self) -> Result<Vec<TetherFrame>> {
        let (frames, listener, status) = with_active(self, |a| {
            let status = a.session.poll().map_err(failure);
            let raw: Vec<::tether::Frame> = a.session.events().try_iter().collect();
            Ok((frames(&a.db, raw), a.listener.clone(), status))
        })?;
        self.notify_changes();
        if let Some(listener) = listener {
            for frame in &frames {
                listener.on_frame(frame.clone());
            }
        }
        status?;
        Ok(frames)
    }
    pub fn tether_stop(&self) -> Result<Vec<TetherFrame>> {
        with_active(self, |_| Ok(()))?;
        let mut active = ACTIVE
            .with(|slot| slot.borrow_mut().take())
            .ok_or_else(|| failure("no active session"))?;
        let status = active.session.stop().map_err(failure);
        let raw: Vec<::tether::Frame> = active.session.events().try_iter().collect();
        let frames = frames(&active.db, raw);
        self.notify_changes();
        if let Some(listener) = active.listener {
            for frame in &frames {
                listener.on_frame(frame.clone());
            }
        }
        status?;
        Ok(frames)
    }
    pub fn tether_live_view(&self) -> Result<Vec<u8>> {
        with_active(self, |a| a.session.live_view().map_err(failure))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn commands_without_session_fail_without_camera_access() {
        let dir = tempfile::tempdir().unwrap();
        let engine = crate::Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        assert!(engine.tether_capture().is_err());
        assert!(engine.tether_poll().is_err());
        assert!(engine.tether_stop().is_err());
    }
}
