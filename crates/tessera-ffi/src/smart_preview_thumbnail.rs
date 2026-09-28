//! Explicit thumbnails rendered from validated local camera-linear pixels.
//! No original source/sidecar I/O and no changes to ordinary preview admission.
use crate::smart_preview_store::{JournalSnapshot, SmartPreviewJournal};
use crate::{Engine, EngineEvent, PreviewResponse, Result, failure, parse_id};
use engine_api::{
    id::ImageId,
    jobs::{Job, JobContext, Priority, Scheduler},
    tile::{TILE_SIZE, TileCoord},
};
use image_core::{RawImage, RenderOutput, Renderer, RendererConfig};
use pipeline_cpu::CameraLinearProxy;
use std::{
    collections::HashMap,
    fs,
    io::Read,
    path::PathBuf,
    sync::{Arc, Weak},
};

const MAX_ASSET_BYTES: u64 = 256 * 1024 * 1024;
const MAX_JPEG_BYTES: usize = 32 * 1024 * 1024;
const MAX_PENDING: usize = 8;
const MAX_STATES: usize = 128;
type Slot = (ImageId, u32);

#[derive(Clone, Debug, PartialEq, Eq)]
struct Identity {
    incarnation: [u8; 32],
    generation: u64,
    asset: [u8; 32],
    recipe: [u8; 32],
}
struct Local {
    identity: Identity,
    snapshot: JournalSnapshot,
    path: PathBuf,
}
enum State {
    Pending,
    Interrupted,
    Ready(previews::PreviewKey),
    Failed(String),
}
struct Entry {
    identity: Identity,
    state: State,
}
#[derive(Default)]
pub(crate) struct States {
    entries: HashMap<Slot, Entry>,
}

fn asset_hash(path: &std::path::Path) -> Result<[u8; 32]> {
    let file = fs::File::open(path)?;
    if !file.metadata()?.is_file() {
        return Err(failure("Smart Preview asset is not a regular file"));
    }
    let mut reader = file.take(MAX_ASSET_BYTES + 1);
    let mut hash = blake3::Hasher::new();
    let mut size = 0;
    let mut buffer = [0; 65536];
    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_ASSET_BYTES {
            return Err(failure("Smart Preview asset exceeds limit"));
        }
        hash.update(&buffer[..n]);
    }
    if size == 0 {
        return Err(failure("Smart Preview asset is empty"));
    }
    Ok(*hash.finalize().as_bytes())
}
impl Engine {
    fn thumbnail_local(&self, id: ImageId) -> Result<Local> {
        // Catalog ownership only. Self::path is a SQL lookup, not a filesystem probe.
        {
            let catalog = self.lock()?;
            Self::path(&catalog, &id.to_string())?;
        }
        let root = self.support_dir()?;
        let (incarnation, snapshot) =
            SmartPreviewJournal::read_local_snapshot(root, id).map_err(failure)?;
        let path = crate::smart_preview_store::local_regular_file(root, id, "pixels.tsp")
            .map_err(failure)?;
        let asset = asset_hash(&path)?;
        Ok(Local {
            identity: Identity {
                incarnation,
                generation: snapshot.generation,
                asset,
                recipe: snapshot.recipe_digest,
            },
            snapshot,
            path,
        })
    }
}

#[uniffi::export]
impl Engine {
    /// Explicit Smart Preview pixels, including saved offline edits. Never falls back
    /// to originals. Dispatch this bounded local read off the UI thread, as for previews.
    /// A cold request returns pending; PreviewReady also signals terminal failures.
    pub fn smart_preview_thumbnail(
        self: Arc<Self>,
        image_id: String,
        max_px: u32,
    ) -> Result<PreviewResponse> {
        if !(1..=2560).contains(&max_px) {
            return Err(failure("max_px must be 1...2560"));
        }
        let id = parse_id(&image_id)?;
        let gate = crate::image_edit_admission::gate_for(id)?;
        let read = gate.begin_read()?;
        let local = self.thumbnail_local(id)?;
        let slot = (id, max_px);
        let mut states = self.smart_thumbnail_states.lock().map_err(failure)?;
        if let Some(entry) = states.entries.get(&slot) {
            // Keep at most one queued/running job per owner+tier, including across edits.
            // Its terminal event causes the next request to admit the latest identity.
            if matches!(entry.state, State::Pending) {
                return Ok(PreviewResponse {
                    bytes: None,
                    pending: true,
                });
            }
            if entry.identity == local.identity {
                match &entry.state {
                    State::Ready(key) => {
                        if let Some(bytes) =
                            self.previews
                                .get_bounded(key, previews::Level::Full, MAX_JPEG_BYTES)
                        {
                            drop(states);
                            if self.thumbnail_local(id)?.identity != local.identity {
                                return Err(failure("Smart Preview thumbnail changed; retry"));
                            }
                            return Ok(PreviewResponse {
                                bytes: Some(bytes),
                                pending: false,
                            });
                        }
                    }
                    State::Failed(message) => {
                        // Terminal errors are observed once, not a permanent negative cache.
                        // A later request may retry unchanged pixels after transient I/O recovers.
                        let error = failure(message);
                        states.entries.remove(&slot);
                        return Err(error);
                    }
                    State::Interrupted => {}
                    State::Pending => unreachable!(),
                }
            }
        }
        if states
            .entries
            .values()
            .filter(|entry| matches!(entry.state, State::Pending))
            .count()
            >= MAX_PENDING
        {
            return Err(failure(
                "Smart Preview thumbnail queue is full; retry later",
            ));
        }
        if states.entries.len() >= MAX_STATES && !states.entries.contains_key(&slot) {
            let evict = states
                .entries
                .iter()
                .find(|(_, entry)| !matches!(entry.state, State::Pending))
                .map(|(slot, _)| *slot);
            if let Some(evict) = evict {
                states.entries.remove(&evict);
            }
        }
        states.entries.insert(
            slot,
            Entry {
                identity: local.identity.clone(),
                state: State::Pending,
            },
        );
        drop(states);
        drop(read);
        self.jobs.submit(
            Box::new(ThumbnailJob {
                engine: Arc::downgrade(&self),
                slot,
                identity: local.identity,
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
struct ThumbnailJob {
    engine: Weak<Engine>,
    slot: Slot,
    identity: Identity,
    completed: bool,
}
impl ThumbnailJob {
    fn render(&self, engine: &Engine, ctx: &JobContext) -> Result<previews::PreviewKey> {
        ctx.check_cancelled()?;
        let local = {
            let gate = crate::image_edit_admission::gate_for(self.slot.0)?;
            let _read = gate.begin_read()?;
            engine.thumbnail_local(self.slot.0)?
        };
        if local.identity != self.identity {
            return Err(failure("Smart Preview thumbnail changed; retry"));
        }
        let mut bytes = Vec::new();
        let path = crate::smart_preview_store::local_regular_file(
            engine.support_dir()?,
            self.slot.0,
            "pixels.tsp",
        )
        .map_err(failure)?;
        if path != local.path {
            return Err(failure("Smart Preview asset path changed"));
        }
        fs::File::open(path)?
            .take(MAX_ASSET_BYTES + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() as u64 > MAX_ASSET_BYTES
            || *blake3::hash(&bytes).as_bytes() != self.identity.asset
        {
            return Err(failure("Smart Preview asset changed during read"));
        }
        let decoded = CameraLinearProxy::decode_persistent(&bytes)?;
        drop(bytes);
        if decoded.original_byte_length != local.snapshot.source_len
            || decoded.proxy.original_content_digest() != local.snapshot.source_digest
        {
            return Err(failure("Smart Preview source identity mismatch"));
        }
        let document = crate::smart_preview::validate_local_document(&local.snapshot.recipe)?;
        let mut render_identity = [0; 16];
        render_identity.copy_from_slice(&decoded.container_digest[..16]);
        let mut render_id = ImageId(u128::from_le_bytes(render_identity));
        if render_id == self.slot.0 {
            render_id.0 ^= 1;
        }
        let source =
            RawImage::from_camera_linear_proxy(self.slot.0, render_id, Arc::new(decoded.proxy))?;
        crate::smart_preview::validate_proxy_recipe(&source, &document.recipe)?;
        // The renderer enforces the same unsupported dependency rules as Develop.
        let renderer = Renderer::new(RendererConfig {
            threads: 1,
            cache_budget_bytes: 0,
            process_version: document.recipe.process_version,
            ..Default::default()
        });
        let key = cache_key(
            self.slot,
            &self.identity,
            source.metadata().orientation as u8,
        );
        // Empty tile admission runs the complete dependency/settings validator.
        renderer.render_tiles(
            &source,
            &document.recipe.settings,
            &[],
            RenderOutput::Display,
            &ctx.cancellation,
            &mut |_| {},
        )?;
        if engine
            .previews
            .get_bounded(&key, previews::Level::Full, MAX_JPEG_BYTES)
            .is_some()
        {
            return Ok(key);
        }
        let extent = Renderer::output_extent(&source, &document.recipe.settings, 0)?;
        let grid = extent.tile_grid(TILE_SIZE);
        let coords: Vec<_> = (0..grid.1)
            .flat_map(|y| (0..grid.0).map(move |x| TileCoord::new(0, x, y)))
            .collect();
        let mut image = image::RgbImage::new(extent.width, extent.height);
        let mut tile_error = None;
        renderer.render_tiles(
            &source,
            &document.recipe.settings,
            &coords,
            RenderOutput::Display,
            &ctx.cancellation,
            &mut |tile| {
                let data = match tile.samples::<u8>() {
                    Ok(data) => data,
                    Err(error) => {
                        tile_error = Some(error);
                        return;
                    }
                };
                let layout = tile.layout();
                let n = layout.plane_len();
                let (ox, oy) = tile.coord().pixel_origin(TILE_SIZE);
                for y in 0..layout.extent.height {
                    for x in 0..layout.extent.width {
                        let i = (y * layout.extent.width + x) as usize;
                        image.put_pixel(
                            ox + x,
                            oy + y,
                            image::Rgb([data[i], data[n + i], data[2 * n + i]]),
                        );
                    }
                }
            },
        )?;
        if let Some(error) = tile_error {
            return Err(error.into());
        }
        ctx.check_cancelled()?;
        // Old immutable cache entries may exist, but cannot be delivered for a new identity.
        engine
            .previews
            .put_image_local_cancellable(&key, &image, self.slot.1, &|| ctx.check_cancelled())
            .map_err(failure)?;
        Ok(key)
    }
    fn finish(&mut self, engine: &Engine, result: Result<previews::PreviewKey>, interrupted: bool) {
        // Serialize the last freshness check and state publication against participating
        // edit/discard/rebuild operations, while allowing an active proxy editor.
        let gate = crate::image_edit_admission::gate_for(self.slot.0);
        let read = gate.as_ref().ok().and_then(|gate| gate.begin_read().ok());
        let current = if read.is_some() {
            engine
                .thumbnail_local(self.slot.0)
                .map(|local| local.identity)
        } else {
            Err(failure("Smart Preview read admission failed"))
        };
        let state = match (current, result) {
            (Ok(identity), Ok(key)) if identity == self.identity => State::Ready(key),
            (Ok(identity), Err(_)) if identity == self.identity && interrupted => {
                State::Interrupted
            }
            (Ok(identity), Err(error)) if identity == self.identity => {
                State::Failed(error.to_string())
            }
            _ => State::Failed("Smart Preview thumbnail changed; retry".into()),
        };
        let mut states = engine
            .smart_thumbnail_states
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(entry) = states.entries.get_mut(&self.slot)
            && entry.identity == self.identity
            && matches!(entry.state, State::Pending)
        {
            entry.state = state;
        }
        drop(states);
        drop(read);
        self.completed = true;
        engine.emit(EngineEvent::PreviewReady {
            image_id: self.slot.0.to_string(),
            max_px: self.slot.1,
        });
    }
}
fn cache_key(slot: Slot, identity: &Identity, orientation: u8) -> previews::PreviewKey {
    let mut hash = blake3::Hasher::new();
    hash.update(b"tessera-smart-preview-thumbnail-v1\0");
    hash.update(&slot.0.0.to_le_bytes());
    hash.update(&slot.1.to_le_bytes());
    hash.update(&identity.incarnation);
    hash.update(&identity.generation.to_le_bytes());
    hash.update(&identity.asset);
    previews::PreviewKey {
        file_hash: *hash.finalize().as_bytes(),
        orientation,
        recipe_hash: identity.recipe,
    }
}
impl Drop for ThumbnailJob {
    fn drop(&mut self) {
        if !self.completed
            && let Some(engine) = self.engine.upgrade()
        {
            self.finish(
                &engine,
                Err(failure("Smart Preview thumbnail cancelled or interrupted")),
                true,
            );
        }
    }
}
impl Job for ThumbnailJob {
    fn label(&self) -> &str {
        "Smart Preview thumbnail"
    }
    fn priority(&self) -> Priority {
        Priority::Preview
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> engine_api::EngineResult<()> {
        ctx.check_cancelled()?;
        if let Some(engine) = self.engine.upgrade() {
            let result = self.render(&engine, ctx);
            ctx.check_cancelled()?;
            self.finish(&engine, result, false);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
