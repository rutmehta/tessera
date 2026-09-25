use super::*;
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
    length: u64,
    modified: Option<std::time::SystemTime>,
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
        let metadata = std::fs::metadata(&path)?;
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
            length: metadata.len(),
            modified: metadata.modified().ok(),
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
}
impl PreviewJob {
    /// Default recipes use the embedded-JPEG fast path. Edited recipes use the
    /// preview the develop session stored under the recipe hash, or render
    /// the renderable subset of the settings (bilinear demosaic, as for the
    /// default RAW fallback).
    fn render(&self, engine: &Engine) -> std::result::Result<previews::PreviewKey, String> {
        let path = Path::new(&self.path);
        let id = parse_id(&self.request.image_id).map_err(|e| e.to_string())?;
        let recipe = catalog::document(path, id)
            .map(|d| d.recipe)
            .unwrap_or_default();
        let hash = recipe.recipe_hash();
        if hash == core::Recipe::default().recipe_hash() {
            return engine
                .previews
                .from_raw(path, self.request.max_px)
                .map(|(key, _)| key)
                .map_err(|e| e.to_string());
        }
        let mut settings = crate::develop::renderable(&recipe.settings);
        settings.demosaic.method = core::settings::DemosaicMethod::Bilinear;
        engine
            .previews
            .from_raw_settings(path, self.request.max_px, &settings, hash.0.0)
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
impl Job for PreviewJob {
    fn label(&self) -> &str {
        "RAW preview"
    }
    fn priority(&self) -> Priority {
        Priority::Preview
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        ctx.check_cancelled()?;
        let Some(engine) = self.engine.upgrade() else {
            return Ok(());
        };
        let state = match self.render(&engine) {
            Ok(key) => State::Ready(key),
            Err(error) => State::Failed(error),
        };
        engine
            .preview_states
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(self.request.clone(), state);
        // Terminal notification also wakes failed requests so callers receive the error.
        // State is published first and no engine/store locks are held during callbacks.
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.request.image_id,
            max_px: self.request.max_px,
        });
        Ok(())
    }
}
