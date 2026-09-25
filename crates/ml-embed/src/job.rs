use crate::{
    Siglip,
    model::{DIMENSION, MODEL_VERSION},
    vector::{SqliteVectorIndex, VectorIndex},
};
use engine_api::{
    error::{EngineError, EngineResult},
    jobs::{Job, JobContext, Priority},
};
use image::RgbImage;
use std::path::PathBuf;

/// Injectable inference boundary; production callers supply an already loaded Siglip.
pub trait ImageEmbedder: Send {
    fn model_version(&self) -> &str;
    fn dimension(&self) -> usize;
    fn embed_images(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>>;
}
impl ImageEmbedder for Siglip {
    fn model_version(&self) -> &str {
        MODEL_VERSION
    }
    fn dimension(&self) -> usize {
        DIMENSION
    }
    fn embed_images(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(Siglip::embed_images(self, images)?
            .into_iter()
            .map(|v| v.to_vec())
            .collect())
    }
}

/// Embeds catalogued images in a folder using bounded batches of eight.
/// Construction performs no I/O and never downloads a model.
/// Corrupt/missing previews or inference errors stop the job; already written
/// rows remain durable. The current undecoded batch is not persisted. Retrying
/// skips rows for this model version, so repaired files resume without rework.
/// Cancellation is cooperative: an in-flight decode/inference cannot be
/// interrupted, but its result is checked before any embedding write.
pub struct EmbedFolderJob {
    catalog_path: PathBuf,
    index_dir: PathBuf,
    folder: PathBuf,
    embedder: Box<dyn ImageEmbedder>,
}
impl EmbedFolderJob {
    pub fn new(
        catalog_path: PathBuf,
        index_dir: PathBuf,
        folder: PathBuf,
        embedder: Box<dyn ImageEmbedder>,
    ) -> Self {
        Self {
            catalog_path,
            index_dir,
            folder,
            embedder,
        }
    }
}
impl Job for EmbedFolderJob {
    fn label(&self) -> &str {
        "embed folder"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        ctx.check_cancelled()?;
        // Catalog scans canonicalize paths. Match aliases/relative folders to
        // that representation, while allowing already-embedded offline folders.
        let folder = self
            .folder
            .canonicalize()
            .or_else(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    std::path::absolute(&self.folder)
                } else {
                    Err(error)
                }
            })
            .map_err(|e| EngineError::io_at(&self.folder, &e))?;
        let catalog = index::Index::open(&self.catalog_path)?;
        let model = self.embedder.model_version().to_owned();
        let error = |e: anyhow::Error| EngineError::Model {
            model: model.clone(),
            message: format!("{e:#}"),
        };
        ctx.check_cancelled()?;
        let mut store = SqliteVectorIndex::open(&self.index_dir, &model, self.embedder.dimension())
            .map_err(error)?;
        ctx.check_cancelled()?;
        let mut ids = Vec::new();
        for id in catalog.search(&index::Query {
            limit: i64::MAX as usize,
            ..Default::default()
        })? {
            ctx.check_cancelled()?;
            if catalog.image_info(id)?.path.starts_with(&folder)
                && store.get(id).map_err(error)?.is_none()
            {
                ids.push(id);
            }
        }
        let mut completed = 0;
        for chunk in ids.chunks(8) {
            let mut images = Vec::new();
            for &id in chunk {
                ctx.check_cancelled()?;
                let info = catalog.image_info(id)?;
                if !info
                    .path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| {
                        s.eq_ignore_ascii_case("jpg") || s.eq_ignore_ascii_case("jpeg")
                    })
                {
                    ctx.check_cancelled()?;
                    let previews = previews::PreviewStore::new(
                        self.index_dir.join("previews"),
                        512 * 1024 * 1024,
                    )
                    .map_err(|e| error(e.into()))?;
                    ctx.check_cancelled()?;
                    let (key, _) = previews
                        .from_raw(&info.path, 384)
                        .map_err(|e| error(e.into()))?;
                    ctx.check_cancelled()?;
                    let bytes = previews.get(&key, previews::Level::Full).ok_or_else(|| {
                        error(anyhow::anyhow!(
                            "missing RAW preview: {}",
                            info.path.display()
                        ))
                    })?;
                    images.push(
                        image::load_from_memory(&bytes)
                            .map_err(|e| error(e.into()))?
                            .to_rgb8(),
                    );
                    continue;
                }
                use image::ImageDecoder;
                ctx.check_cancelled()?;
                let mut decoder = image::ImageReader::open(&info.path)
                    .map_err(|e| error(e.into()))?
                    .into_decoder()
                    .map_err(|e| error(e.into()))?;
                let orientation = decoder.orientation().map_err(|e| error(e.into()))?;
                ctx.check_cancelled()?;
                let mut image =
                    image::DynamicImage::from_decoder(decoder).map_err(|e| error(e.into()))?;
                image.apply_orientation(orientation);
                images.push(image.to_rgb8());
            }
            ctx.check_cancelled()?;
            let result = self.embedder.embed_images(&images);
            ctx.check_cancelled()?;
            let vectors = result.map_err(error)?;
            if vectors.len() != chunk.len() {
                return Err(error(anyhow::anyhow!(
                    "embedding batch cardinality mismatch: expected {}, got {}",
                    chunk.len(),
                    vectors.len()
                )));
            }
            for (&id, vector) in chunk.iter().zip(vectors) {
                ctx.check_cancelled()?;
                store.insert(id, &vector).map_err(error)?;
                completed += 1;
                ctx.report_progress(completed as f32 / ids.len() as f32, None);
            }
        }
        ctx.check_cancelled()?;
        ctx.report_progress(1.0, None);
        ctx.check_cancelled()
    }
}
