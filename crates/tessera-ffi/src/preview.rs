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

    #[test]
    fn lr13_imported_proxy_thumbnail_renders_at_thumbnail_level() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("proxy.dng");
        std::fs::write(
            &path,
            include_bytes!("../../raw-decode/tests/fixtures/linear-gradient-jxl.dng"),
        )
        .unwrap();
        let recipe = core::Recipe::default();
        let id = engine_api::id::ImageId(13);
        let render = |max_px| {
            render_imported(&path, id, &recipe, dir.path(), max_px)
                .unwrap()
                .0
        };
        let full = render(u32::MAX);
        let (w, h) = full.dimensions();
        let long = w.max(h);
        assert!(long >= 8, "fixture must span several levels");
        // A grid request renders the coarsest level that still covers it.
        let thumb = render(long.div_ceil(4));
        assert_eq!(thumb.dimensions(), (w.div_ceil(4), h.div_ceil(4)));
        // A request is never upsampled or rendered coarser than it needs.
        assert_eq!(
            render(long.div_ceil(4) + 1).dimensions(),
            (w.div_ceil(2), h.div_ceil(2))
        );
        assert_eq!(render(long).dimensions(), (w, h));
        // Same picture: every thumbnail sample lies inside its (partial-edge)
        // source bin. The viewport reduces before the display transform, so
        // the exact average is neither the encoded nor the display-linear mean.
        for (x, y, pixel) in thumb.enumerate_pixels() {
            for c in 0..3 {
                let bin: Vec<u8> = (y * 4..(y * 4 + 4).min(h))
                    .flat_map(|sy| (x * 4..(x * 4 + 4).min(w)).map(move |sx| (sx, sy)))
                    .map(|(sx, sy)| full.get_pixel(sx, sy)[c])
                    .collect();
                let (low, high) = (*bin.iter().min().unwrap(), *bin.iter().max().unwrap());
                assert!(
                    (low.saturating_sub(2)..=high.saturating_add(2)).contains(&pixel[c]),
                    "thumbnail sample {} outside its Develop bin {low}..={high}",
                    pixel[c]
                );
            }
        }
        assert_ne!(
            thumb.get_pixel(0, 0),
            thumb.get_pixel(2, 0),
            "gradient kept"
        );
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
        let revision = previews::PreviewKey::for_source(
            &catalog::source_path(Path::new(&path)),
            max_px,
            0,
            [0; 32],
        )
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
        if catalog::lightroom_proxy(path).is_some() || catalog::catalog_orientation(path).is_some()
        {
            let (image, orientation) = render_imported(
                path,
                id,
                &recipe,
                engine.support_dir().map_err(|e| e.to_string())?,
                self.request.max_px,
            )
            .map_err(|e| e.to_string())?;
            let key = previews::PreviewKey::for_source(
                &catalog::source_path(path),
                self.request.max_px,
                orientation as u8,
                hash.0.0,
            )
            .map_err(|e| e.to_string())?;
            engine
                .previews
                .put_image_cancellable(&key, &image, self.request.max_px, &|| ctx.check_cancelled())
                .map_err(|e| e.to_string())?;
            return Ok(key);
        }

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
    /// Synchronous counterpart of the grid/loupe job for analysis and assist.
    /// Keep source resolution, catalog orientation and recipe dispatch identical.
    pub(crate) fn indexed_preview(
        &self,
        image_id: &str,
        path: &str,
        max_px: u32,
    ) -> Result<previews::PreviewKey> {
        let job = PreviewJob {
            engine: Weak::new(),
            request: RequestKey {
                image_id: image_id.into(),
                max_px,
                recipe_hash: String::new(),
                revision: [0; 32],
            },
            path: path.into(),
            completed: true,
        };
        job.render(
            self,
            &JobContext::new(
                engine_api::id::JobId(0),
                engine_api::jobs::CancellationToken::new(),
                None,
            ),
        )
        .map_err(failure)
    }

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

/// Long edge rendered for the 9x8 perceptual hash; the grid's thumbnail tier.
const CULL_HASH_PX: u32 = 256;

/// Culling uses the same source and recipe route as the app's grid and analysis.
pub(crate) fn cull_preview_hash(
    info: &index::ImageInfo,
    support: &Path,
) -> engine_api::EngineResult<Option<u64>> {
    if catalog::lightroom_proxy(&info.path).is_none()
        && catalog::catalog_orientation(&info.path).is_none()
    {
        return cull::preview_hash(info);
    }
    let recipe = catalog::document(&info.path, info.id)?.recipe;
    let (rgb, orientation) = render_imported(&info.path, info.id, &recipe, support, CULL_HASH_PX)
        .map_err(|e| engine_api::EngineError::Unsupported {
        what: e.to_string(),
    })?;
    Ok(Some(cull::dhash(&crate::assist::orient(
        rgb,
        orientation as u8,
    ))))
}

/// Coarsest engine level whose long edge still covers `max_px`: a request is
/// never upsampled and never rendered finer than it needs.
fn thumbnail_level(extent: engine_api::tile::Extent, max_px: u32) -> u8 {
    let long = extent.width.max(extent.height);
    let mut level = 0;
    while level < image_core::render::MAX_LEVEL && long.div_ceil(1 << (level + 1)) >= max_px.max(1)
    {
        level += 1;
    }
    level
}

/// Imported sources use Develop's source boundary, embedded DCP and renderable
/// settings. In particular no LinearRaw source reaches the LibRaw CFA decoder.
/// The frame is produced at the level a `max_px` request needs (the viewport's
/// coarse-level contract), so display encoding and stitching are thumbnail-sized.
fn render_imported(
    path: &Path,
    id: engine_api::id::ImageId,
    recipe: &core::Recipe,
    support: &Path,
    max_px: u32,
) -> Result<(image::RgbImage, u16)> {
    let image = catalog::open_image(id, path)?;
    let settings = crate::develop::session_renderable(&recipe.settings, true, false);
    let renderer = image_core::Renderer::new(Default::default()).for_recipe(recipe);
    let masks = crate::develop::masks::MaskShared::new(&image);
    masks.load_available_imported(support, &settings);
    renderer
        .mask_cache()
        .set_hooks(Some(Arc::new(crate::develop::masks::Hooks(masks))));
    let level = thumbnail_level(
        image_core::Renderer::output_extent(&image, &settings, 0)?,
        max_px,
    );
    let extent = image_core::Renderer::output_extent(&image, &settings, level)?;
    let tiles = renderer.render_region(
        &image,
        &settings,
        level,
        image_core::PixelRect::full(extent),
    )?;
    Ok((
        crate::lrcat_fidelity::stitch(extent, &tiles)?,
        image.metadata().orientation,
    ))
}
