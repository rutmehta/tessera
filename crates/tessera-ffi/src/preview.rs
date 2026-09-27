use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::{id::JobId, jobs::CancellationToken};

    struct Events(Mutex<Vec<EngineEvent>>);
    impl EngineEventListener for Events {
        fn on_event(&self, event: EngineEvent) {
            self.0.lock().unwrap().push(event);
        }
    }

    fn pending_job(engine: &Arc<Engine>) -> Box<PreviewJob> {
        let request = RequestKey {
            image_id: "1".into(),
            max_px: 64,
            recipe_hash: String::new(),
            revision: [0; 32],
        };
        engine
            .preview_states
            .lock()
            .unwrap()
            .insert(request.clone(), State::Pending);
        Box::new(PreviewJob {
            engine: Arc::downgrade(engine),
            request,
            path: "missing.raw".into(),
            completed: false,
        })
    }

    #[test]
    fn cancelled_preview_publishes_terminal_state_and_completion() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let events = Arc::new(Events(Mutex::new(Vec::new())));
        engine.set_event_listener(Some(events.clone()));
        let job = pending_job(&engine);
        let key = job.request.clone();
        let token = CancellationToken::new();
        token.cancel();
        assert!(matches!(
            job.run(&JobContext::new(JobId(0), token, None)),
            Err(engine_api::EngineError::Cancelled)
        ));
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&key),
            Some(State::Failed(_))
        ));
        assert_eq!(events.0.lock().unwrap().len(), 1);
    }

    #[test]
    fn completed_job_drop_does_not_fail_a_replacement_request() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        struct Replace {
            engine: Weak<Engine>,
            key: RequestKey,
            events: AtomicUsize,
        }
        impl EngineEventListener for Replace {
            fn on_event(&self, _: EngineEvent) {
                let engine = self.engine.upgrade().unwrap();
                // A callback can reenter after an evicted Ready preview and
                // admit a replacement under the same request key.
                if self.events.fetch_add(1, Ordering::SeqCst) == 0 {
                    engine
                        .preview_states
                        .lock()
                        .unwrap()
                        .insert(self.key.clone(), State::Pending);
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let job = pending_job(&engine);
        let listener = Arc::new(Replace {
            engine: Arc::downgrade(&engine),
            key: job.request.clone(),
            events: AtomicUsize::new(0),
        });
        engine.set_event_listener(Some(listener.clone()));
        job.run(&JobContext::new(JobId(0), CancellationToken::new(), None))
            .unwrap();
        assert_eq!(listener.events.load(Ordering::SeqCst), 1);
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&listener.key),
            Some(State::Pending)
        ));
    }

    #[test]
    fn discarded_preview_publishes_terminal_state_and_completion() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().to_string_lossy().into_owned()).unwrap();
        let events = Arc::new(Events(Mutex::new(Vec::new())));
        engine.set_event_listener(Some(events.clone()));
        let job = pending_job(&engine);
        let key = job.request.clone();
        drop(job); // Scheduler may discard cancelled queued work without run().
        assert!(matches!(
            engine.preview_states.lock().unwrap().get(&key),
            Some(State::Failed(_))
        ));
        assert_eq!(events.0.lock().unwrap().len(), 1);
    }
}

use engine_api::jobs::{Job, JobContext, Priority, Scheduler};
use std::sync::Weak;

#[derive(Clone, Debug, uniffi::Record)]
pub struct PreviewResponse {
    pub bytes: Option<Vec<u8>>,
    pub pending: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct RequestKey {
    image_id: String,
    max_px: u32,
    recipe_hash: String,
    revision: [u8; 32],
}
pub(super) enum State {
    Pending,
    Ready(previews::PreviewKey),
    Failed(String),
}
impl Engine {
    pub(super) fn request_raw(
        self: &Arc<Self>,
        image_id: String,
        path: String,
        max_px: u32,
        recipe_hash: String,
    ) -> Result<PreviewResponse> {
        let revision = previews::PreviewKey::for_source(Path::new(&path), max_px, 0, [0; 32])
            .map_err(failure)?
            .file_hash;
        let default_hash = core::Recipe::default().recipe_hash().to_string();
        let request = RequestKey {
            image_id,
            max_px,
            // Edited recipes key their own previews (written by the develop
            // session or rendered here); unedited images share the default.
            recipe_hash: if recipe_hash.is_empty() {
                default_hash
            } else {
                recipe_hash
            },
            revision,
        };
        let mut states = self.preview_states.lock().map_err(failure)?;
        match states.get(&request) {
            Some(State::Pending) => {
                return Ok(PreviewResponse {
                    bytes: None,
                    pending: true,
                });
            }
            Some(State::Failed(message)) => return Err(failure(message)),
            Some(State::Ready(key)) => {
                if let Some(bytes) = self.previews.get(key, previews::Level::Full) {
                    return Ok(PreviewResponse {
                        bytes: Some(bytes),
                        pending: false,
                    });
                }
            }
            None => {}
        }
        states.insert(request.clone(), State::Pending);
        drop(states);
        self.jobs.submit(
            Box::new(PreviewJob {
                engine: Arc::downgrade(self),
                request,
                path,
                completed: false,
            }),
            None,
        );
        Ok(PreviewResponse {
            bytes: None,
            pending: true,
        })
    }
}
struct PreviewJob {
    engine: Weak<Engine>,
    request: RequestKey,
    path: String,
    completed: bool,
}
impl PreviewJob {
    /// Default recipes use the embedded-JPEG fast path. Edited recipes use the
    /// preview the develop session stored under the recipe hash, or render
    /// the renderable subset of the settings (bilinear demosaic, as for the
    /// default RAW fallback).
    fn render(
        &self,
        engine: &Engine,
        ctx: &JobContext,
    ) -> std::result::Result<previews::PreviewKey, String> {
        ctx.check_cancelled().map_err(|e| e.to_string())?;
        let path = Path::new(&self.path);
        let id = parse_id(&self.request.image_id).map_err(|e| e.to_string())?;
        let recipe = catalog::document(path, id)
            .map(|d| d.recipe)
            .unwrap_or_default();
        ctx.check_cancelled().map_err(|e| e.to_string())?;
        let hash = recipe.recipe_hash();
        if hash == core::Recipe::default().recipe_hash() {
            return engine
                .previews
                .from_raw_cancellable(path, self.request.max_px, &|| ctx.check_cancelled())
                .map(|(key, _)| key)
                .map_err(|e| e.to_string());
        }
        let mut settings = crate::develop::renderable(&recipe.settings);
        settings.demosaic.method = core::settings::DemosaicMethod::Bilinear;
        engine
            .previews
            .from_raw_settings_cancellable(path, self.request.max_px, &settings, hash.0.0, &|| {
                ctx.check_cancelled()
            })
            .map_err(|e| e.to_string())
    }
}
impl Engine {
    /// Preview sizes requested so far for an image (the app's tiers).
    pub(super) fn requested_preview_sizes(&self, image_id: &str) -> Vec<u32> {
        let states = self
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let mut sizes: Vec<u32> = states
            .keys()
            .filter(|k| k.image_id == image_id)
            .map(|k| k.max_px)
            .collect();
        sizes.sort_unstable();
        sizes.dedup();
        sizes
    }
}
// Queued jobs can be discarded without run(); also covers cancellation and
// unwinding before normal publication. Never leave an admitted request Pending.
// The scheduler must drop discarded jobs outside its lock (callbacks may reenter).
impl Drop for PreviewJob {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        let mut states = engine
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if !matches!(states.get(&self.request), Some(State::Pending)) {
            return;
        }
        states.insert(
            self.request.clone(),
            State::Failed("RAW preview cancelled or interrupted".into()),
        );
        drop(states);
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.request.image_id.clone(),
            max_px: self.request.max_px,
        });
    }
}

impl Job for PreviewJob {
    fn label(&self) -> &str {
        "RAW preview"
    }
    fn priority(&self) -> Priority {
        Priority::Preview
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        ctx.check_cancelled()?;
        let Some(engine) = self.engine.upgrade() else {
            return Ok(());
        };
        let result = self.render(&engine, ctx);
        ctx.check_cancelled()?;
        let state = match result {
            Ok(key) => State::Ready(key),
            Err(error) => State::Failed(error),
        };
        engine
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.request.clone(), state);
        self.completed = true;
        // Terminal notification also wakes failed requests so callers receive the error.
        // State is published first and no engine/store locks are held during callbacks.
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.request.image_id.clone(),
            max_px: self.request.max_px,
        });
        Ok(())
    }
}
