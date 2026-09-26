//! Unlike other engine commands, tether calls belong on the process main thread.
//! The host calls tether_poll on a main-thread timer (e.g. every 100 ms).
//! Indexing/scoring runs on a worker, never in a native camera delegate callback.
use crate::{Engine, Result, failure};
use ::tether::{Session, TetherBackend, native::NativeBackend};
use std::{cell::RefCell, path::PathBuf, sync::Arc};

#[derive(Clone, Debug, uniffi::Record)]
pub struct TetherDevice {
    pub id: String,
    pub name: String,
    pub can_capture: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct TetherFrame {
    pub sequence: u64,
    pub path: String,
    pub image_id: Option<String>,
    pub preview: Option<String>,
    pub face_warning: Option<String>,
    pub error: Option<String>,
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
        }
    }
}
#[uniffi::export(with_foreign)]
pub trait TetherEventListener: Send + Sync {
    fn on_frame(&self, frame: TetherFrame);
}
struct Active {
    db: PathBuf,
    session: Session<NativeBackend>,
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
    pub fn tether_devices(&self) -> Result<Vec<TetherDevice>> {
        NativeBackend::new()
            .and_then(|mut b| b.devices())
            .map(|devices| {
                devices
                    .into_iter()
                    .map(|d| TetherDevice {
                        id: d.id,
                        name: d.name,
                        can_capture: d.can_capture,
                    })
                    .collect()
            })
            .map_err(failure)
    }
    pub fn tether_start(&self, session_folder: String, naming: String) -> Result<()> {
        ACTIVE.with(|slot| {
            let mut slot = slot
                .try_borrow_mut()
                .map_err(|_| failure("reentrant tether call"))?;
            if slot.is_some() {
                return Err(failure("a tether session is already active"));
            }
            let backend = NativeBackend::new().map_err(failure)?;
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
    pub fn tether_poll(&self) -> Result<Vec<TetherFrame>> {
        let (frames, listener, status) = with_active(self, |a| {
            let status = a.session.poll().map_err(failure);
            let frames: Vec<TetherFrame> = a.session.events().try_iter().map(Into::into).collect();
            Ok((frames, a.listener.clone(), status))
        })?;
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
        let frames: Vec<TetherFrame> = active.session.events().try_iter().map(Into::into).collect();
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
