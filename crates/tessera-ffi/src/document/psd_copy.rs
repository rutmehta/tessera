//! One host-owned rasterized copy. Cancellation is cooperative; PSD conversion
//! and encoding currently check only at their boundaries (A-owned follow-up).
use super::{DocumentSession, Shared, filtering, find, io};
use crate::{Result, failure};
use compositor::{Document, Layer, LayerId, LayerKind};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

/// Cancellation must originate at a typed checkpoint, never from a later flag
/// observed while handling an unrelated evaluator, validation or IO failure.
#[derive(Debug)]
pub(super) enum CopyError {
    Cancelled,
    Failed(crate::BridgeError),
}
pub(super) type CopyResult<T> = std::result::Result<T, CopyError>;
impl From<crate::BridgeError> for CopyError {
    fn from(error: crate::BridgeError) -> Self {
        Self::Failed(error)
    }
}
impl From<engine_api::EngineError> for CopyError {
    fn from(error: engine_api::EngineError) -> Self {
        match error {
            engine_api::EngineError::Cancelled => Self::Cancelled,
            other => Self::Failed(other.into()),
        }
    }
}
impl From<std::io::Error> for CopyError {
    fn from(error: std::io::Error) -> Self {
        Self::Failed(error.into())
    }
}
fn outcome(result: CopyResult<()>) -> Result<RasterizedPsdCopyOutcome> {
    match result {
        Ok(()) => Ok(RasterizedPsdCopyOutcome::Saved),
        Err(CopyError::Cancelled) => Ok(RasterizedPsdCopyOutcome::Cancelled),
        Err(CopyError::Failed(error)) => Err(error),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum RasterizedPsdCopyOutcome {
    Saved,
    Cancelled,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Prepared,
    Running,
    Committing,
    Finished,
}
struct Lifecycle {
    phase: Phase,
    claimed: bool,
    cancelled: bool,
}
struct CopyState {
    life: Mutex<Lifecycle>,
    cancel: Arc<filtering::RequestCancellation>,
}
impl CopyState {
    fn new() -> Self {
        Self {
            life: Mutex::new(Lifecycle {
                phase: Phase::Prepared,
                claimed: false,
                cancelled: false,
            }),
            cancel: Arc::default(),
        }
    }
    fn life(&self) -> MutexGuard<'_, Lifecycle> {
        self.life.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn cancel(&self) -> bool {
        let mut life = self.life();
        if life.cancelled {
            return true;
        }
        if matches!(life.phase, Phase::Committing | Phase::Finished) {
            return false;
        }
        life.cancelled = true;
        self.cancel.cancel();
        true
    }
    fn start(&self) -> Result<bool> {
        let mut life = self.life();
        if life.claimed {
            return Err(failure("rasterized copy operation is single-use"));
        }
        life.claimed = true;
        if life.cancelled {
            life.phase = Phase::Finished;
            return Ok(false);
        }
        life.phase = Phase::Running;
        Ok(true)
    }
    fn check(&self) -> CopyResult<()> {
        if self.cancel.is_cancelled() {
            Err(CopyError::Cancelled)
        } else {
            Ok(())
        }
    }
    fn finish_result(&self, result: CopyResult<()>) -> Result<RasterizedPsdCopyOutcome> {
        // Drop the lifecycle lock before mapping/returning the owned error.
        self.life().phase = Phase::Finished;
        outcome(result)
    }
    fn holds_admission(&self) -> bool {
        let life = self.life();
        // A running cancellation retains admission until its RAII guard drains.
        life.phase != Phase::Finished && !(life.phase == Phase::Prepared && life.cancelled)
    }
}
#[derive(Default)]
struct RegistryState {
    closed: bool,
    active: Weak<CopyState>,
}
#[derive(Default)]
pub(super) struct CopyRegistry {
    inner: Mutex<RegistryState>,
}
impl CopyRegistry {
    fn lock(&self) -> MutexGuard<'_, RegistryState> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn prepare(&self) -> Result<Arc<CopyState>> {
        let mut registry = self.lock();
        if registry.closed {
            return Err(failure("document is closed"));
        }
        if registry
            .active
            .upgrade()
            .is_some_and(|s| s.holds_admission())
        {
            return Err(failure("a rasterized copy is still running or draining"));
        }
        let state = Arc::new(CopyState::new());
        registry.active = Arc::downgrade(&state);
        Ok(state)
    }
    pub(super) fn close(&self) {
        let mut registry = self.lock();
        registry.closed = true;
        if let Some(state) = registry.active.upgrade() {
            state.cancel();
        }
    }
    fn admit_commit(&self, state: &Arc<CopyState>) -> CopyResult<()> {
        let registry = self.lock();
        let mut life = state.life();
        if registry.closed || life.cancelled || life.phase != Phase::Running {
            return Err(CopyError::Cancelled);
        }
        if !registry.active.ptr_eq(&Arc::downgrade(state)) {
            return Err(failure("rasterized copy lost ownership").into());
        }
        life.phase = Phase::Committing;
        // Neither gate is held during persist, evaluation, copying or callbacks.
        Ok(())
    }
    fn finish(&self, state: &Arc<CopyState>) {
        state.life().phase = Phase::Finished;
        let mut registry = self.lock();
        if registry.active.ptr_eq(&Arc::downgrade(state)) {
            registry.active = Weak::new();
        }
    }
}
struct Drain<'a> {
    registry: &'a CopyRegistry,
    state: &'a Arc<CopyState>,
}
impl Drop for Drain<'_> {
    fn drop(&mut self) {
        self.registry.finish(self.state);
    }
}

#[derive(uniffi::Object)]
pub struct RasterizedPsdCopyOperation {
    shared: Arc<Shared>,
    state: Arc<CopyState>,
}
#[uniffi::export]
impl DocumentSession {
    /// Cheap handle preparation: no snapshot, evaluation or IO. One live copy
    /// per document, including cancelled work that has not finished unwinding.
    pub fn prepare_rasterized_psd_copy(&self) -> Result<Arc<RasterizedPsdCopyOperation>> {
        let state = self.shared.copies.prepare()?;
        Ok(Arc::new(RasterizedPsdCopyOperation {
            shared: self.shared.clone(),
            state,
        }))
    }
}
#[uniffi::export]
impl RasterizedPsdCopyOperation {
    /// False once output commit was admitted. Never waits on backend state/IO.
    pub fn cancel(&self) -> bool {
        self.state.cancel()
    }

    pub fn run(&self, path: String) -> Result<RasterizedPsdCopyOutcome> {
        let started = self.state.start()?;
        let _drain = Drain {
            registry: &self.shared.copies,
            state: &self.state,
        };
        if !started {
            return Ok(RasterizedPsdCopyOutcome::Cancelled);
        }
        let result = self.run_inner(std::path::Path::new(&path));
        // Finalize admission under the lifecycle gate, but do not reclassify
        // a genuine failure using cancellation that arrived after that failure.
        self.state.finish_result(result)
    }
}
impl RasterizedPsdCopyOperation {
    fn run_inner(&self, path: &std::path::Path) -> CopyResult<()> {
        self.state.check()?;
        let kind = io::save_kind(path)?;
        if kind == io::SaveKind::Native {
            return Err(failure("the rasterized copy is a .psd or .psb file").into());
        }
        let snapshot = {
            let st = self.shared.lock()?;
            self.state.check()?;
            st.open()?;
            st.doc.state().clone()
        };
        self.state.check()?;
        if kind == io::SaveKind::Psd
            && (snapshot.canvas.width > 30_000 || snapshot.canvas.height > 30_000)
        {
            return Err(failure("PSD is limited to 30000 pixels: save as .psb").into());
        }
        let mut ids = Vec::new();
        collect_layers(&snapshot.root, &mut ids, &|| self.state.check())?;
        let copy = rasterize_layers(&snapshot, &ids, &self.state, |layer| {
            filtering::rasterize_smart_stack_with_cancel(&snapshot, layer, &self.state.cancel)
        })?;
        io::save_psd_copy_checked(
            &Document::new(copy),
            path,
            self.state.cancel.native_token(),
            &|| self.state.check(),
            &|| self.shared.copies.admit_commit(&self.state),
        )
    }
}
fn rasterize_layers(
    snapshot: &compositor::DocState,
    ids: &[u64],
    state: &CopyState,
    mut rasterize: impl FnMut(&Layer) -> CopyResult<compositor::Raster>,
) -> CopyResult<compositor::DocState> {
    state.check()?;
    let mut copy = snapshot.clone();
    for &id in ids {
        state.check()?;
        let raster = rasterize(find(snapshot, id)?)?;
        state.check()?;
        copy.layer_mut(LayerId(id), |l| l.kind = LayerKind::Pixel(raster));
    }
    Ok(copy)
}
fn collect_layers(
    layers: &[Arc<Layer>],
    out: &mut Vec<u64>,
    check: &impl Fn() -> CopyResult<()>,
) -> CopyResult<()> {
    for layer in layers {
        check()?;
        match &layer.kind {
            LayerKind::SmartObject(so) if so.filters.iter().any(|f| f.enabled) => {
                out.push(layer.id.0)
            }
            LayerKind::Group { children, .. } => collect_layers(children, out, check)?,
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn session_close_signals_running_copy_before_waiting_for_backend_state() {
        let dir = tempfile::tempdir().unwrap();
        let engine =
            crate::Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        let extent = engine_api::tile::Extent::new(2, 2);
        let session = engine.adopt_document(
            Document::new(compositor::DocState::new(extent, compositor::Depth::U8)),
            "tiny copy".into(),
        );
        let operation = session.prepare_rasterized_psd_copy().unwrap();
        let destination = dir.path().join("copy.psd");
        std::fs::write(&destination, b"sentinel").unwrap();

        let state_guard = session.shared.state.lock().unwrap();
        let worker_operation = operation.clone();
        let worker_path = destination.to_string_lossy().into_owned();
        let worker = std::thread::spawn(move || worker_operation.run(worker_path));
        fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            loop {
                if condition() {
                    return true;
                }
                if std::time::Instant::now() >= deadline {
                    return false;
                }
                std::thread::yield_now();
            }
        }
        let running = wait_until(|| operation.state.life().phase == Phase::Running);
        let close_session = session.clone();
        let closer = std::thread::spawn(move || close_session.close());
        let signalled = wait_until(|| operation.state.cancel.is_cancelled());
        // A broken shutdown ordering must be able to unwind before asserting.
        drop(state_guard);
        closer.join().unwrap();
        let result = worker.join().unwrap().unwrap();
        assert!(running, "copy worker never reached the backend lock");
        assert!(
            signalled,
            "close waited for backend state before cancelling"
        );
        assert_eq!(result, RasterizedPsdCopyOutcome::Cancelled);
        assert_eq!(std::fs::read(&destination).unwrap(), b"sentinel");
    }

    #[test]
    fn close_cancels_blocked_worker_and_holds_admission_until_unwind() {
        let registry = Arc::new(CopyRegistry::default());
        let state = registry.prepare().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let worker_registry = registry.clone();
        let worker_state = state.clone();
        let worker = std::thread::spawn(move || {
            worker_state.start().unwrap();
            let _drain = Drain {
                registry: &worker_registry,
                state: &worker_state,
            };
            started_tx.send(()).unwrap();
            release_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            worker_state.finish_result(worker_state.check())
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        registry.close();
        assert!(state.cancel.is_cancelled());
        assert!(state.holds_admission(), "running worker has not unwound");
        assert!(
            registry.prepare().is_err(),
            "closed document cannot admit work"
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            worker.join().unwrap().unwrap(),
            RasterizedPsdCopyOutcome::Cancelled
        );
        assert!(!state.holds_admission());
    }
    #[test]
    fn genuine_failure_wins_cancel_between_work_and_finalization() {
        for message in [
            "evaluator failed",
            "invalid PSD destination",
            "write failed",
        ] {
            let registry = Arc::new(CopyRegistry::default());
            let state = registry.prepare().unwrap();
            let (failed_tx, failed_rx) = std::sync::mpsc::channel();
            let (finish_tx, finish_rx) = std::sync::mpsc::channel();
            let r = registry.clone();
            let worker_state = state.clone();
            let worker = std::thread::spawn(move || {
                worker_state.start().unwrap();
                let _drain = Drain {
                    registry: &r,
                    state: &worker_state,
                };
                let result = Err(CopyError::from(failure(message)));
                failed_tx.send(()).unwrap();
                finish_rx
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .unwrap();
                worker_state.finish_result(result)
            });
            failed_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert!(state.cancel());
            assert!(registry.prepare().is_err(), "failure/cancel still draining");
            finish_tx.send(()).unwrap();
            let error = worker.join().unwrap().unwrap_err();
            assert_eq!(error.to_string(), message);
            assert!(registry.prepare().is_ok());
        }
    }

    #[test]
    fn only_typed_cancellation_maps_to_cancelled() {
        let state = CopyState::new();
        state.start().unwrap();
        state.cancel();
        assert_eq!(
            state.finish_result(state.check()).unwrap(),
            RasterizedPsdCopyOutcome::Cancelled
        );
        assert_eq!(
            outcome(Err(engine_api::EngineError::Cancelled.into())).unwrap(),
            RasterizedPsdCopyOutcome::Cancelled
        );
        // Even a legacy failure with this text stays an error; no string matching.
        assert!(outcome(Err(failure("cancelled").into())).is_err());
        assert!(
            outcome(Err(
                engine_api::EngineError::invalid("test", "bad raster").into()
            ))
            .is_err()
        );
        assert!(outcome(Err(std::io::Error::other("disk failure").into())).is_err());
    }
    #[test]
    fn cancelled_first_layer_prevents_next_and_preserves_source() {
        let extent = engine_api::tile::Extent::new(2, 2);
        let mut snapshot = compositor::DocState::new(extent, compositor::Depth::U8);
        for id in [1, 2] {
            let mut layer = Layer::new(
                "tiny",
                LayerKind::Pixel(compositor::Raster::new(
                    extent,
                    4,
                    compositor::Depth::U8,
                    0.0,
                )),
            );
            layer.id = LayerId(id);
            snapshot.root.push(Arc::new(layer));
        }
        let original = snapshot.root.clone();
        let state = CopyState::new();
        state.start().unwrap();
        let mut calls = 0;
        let result = rasterize_layers(&snapshot, &[1, 2], &state, |_| {
            calls += 1;
            state.cancel();
            Ok(compositor::Raster::new(
                extent,
                4,
                compositor::Depth::U8,
                0.0,
            ))
        });
        assert!(result.is_err());
        assert_eq!(calls, 1);
        assert!(
            snapshot
                .root
                .iter()
                .zip(&original)
                .all(|(a, b)| Arc::ptr_eq(a, b))
        );
        let fresh = CopyState::new();
        let copied = rasterize_layers(&snapshot, &[1, 2], &fresh, |_| {
            Ok(compositor::Raster::new(
                extent,
                4,
                compositor::Depth::U8,
                0.0,
            ))
        })
        .unwrap();
        assert_eq!(copied.root.len(), 2);
    }
    #[test]
    fn prepared_cancel_is_single_use_and_late_cleanup_preserves_replacement() {
        let registry = CopyRegistry::default();
        let old = registry.prepare().unwrap();
        assert!(old.cancel());
        let fresh = registry.prepare().unwrap();
        assert!(!old.start().unwrap());
        registry.finish(&old);
        assert!(old.start().is_err());
        assert!(registry.prepare().is_err());
        assert!(fresh.start().unwrap());
        registry.admit_commit(&fresh).unwrap();
        assert!(!fresh.cancel());
        registry.finish(&fresh);
        assert!(registry.prepare().is_ok());
    }
    #[test]
    fn running_cancel_holds_admission_until_gated_unwind() {
        let registry = Arc::new(CopyRegistry::default());
        let state = registry.prepare().unwrap();
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (exit_tx, exit_rx) = std::sync::mpsc::channel();
        let r = registry.clone();
        let s = state.clone();
        let worker = std::thread::spawn(move || {
            assert!(s.start().unwrap());
            let _drain = Drain {
                registry: &r,
                state: &s,
            };
            started_tx.send(()).unwrap();
            exit_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap();
            assert!(s.check().is_err());
        });
        started_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert!(state.cancel());
        assert!(registry.prepare().is_err());
        assert!(registry.admit_commit(&state).is_err());
        exit_tx.send(()).unwrap();
        worker.join().unwrap();
        assert!(registry.prepare().is_ok());
    }
    #[test]
    fn close_and_commit_have_explicit_ordering() {
        let registry = CopyRegistry::default();
        let state = registry.prepare().unwrap();
        state.start().unwrap();
        registry.close();
        assert!(state.check().is_err());
        assert!(registry.admit_commit(&state).is_err());
        assert!(registry.prepare().is_err());
        let registry = CopyRegistry::default();
        let state = registry.prepare().unwrap();
        state.start().unwrap();
        registry.admit_commit(&state).unwrap();
        registry.close();
        assert!(!state.cancel());
        assert!(state.check().is_ok());
    }
}
