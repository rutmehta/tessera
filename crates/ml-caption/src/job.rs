use crate::{FLORENCE_VERSION, Florence, KeywordModel};
use anyhow::{Context, Result, ensure};
use engine_api::{
    error::{EngineError, EngineResult},
    id::ImageId,
    jobs::{Job, JobContext, Priority},
};
use image::{ImageDecoder, RgbImage};
use std::path::{Path, PathBuf};

pub trait ImageUnderstanding: Send {
    fn model_version(&self) -> String;
    fn analyze_batch(&mut self, images: &[RgbImage]) -> Result<Vec<index::Understanding>>;
}
pub struct UnderstandingModel {
    pub keywords: KeywordModel,
    pub florence: Florence,
}
impl ImageUnderstanding for UnderstandingModel {
    fn model_version(&self) -> String {
        format!("{}+{}", self.keywords.model_version(), FLORENCE_VERSION)
    }
    fn analyze_batch(&mut self, images: &[RgbImage]) -> Result<Vec<index::Understanding>> {
        let keywords = self.keywords.suggest_batch(images)?;
        images
            .iter()
            .zip(keywords)
            .map(|(image, keywords)| {
                let caption = self.florence.caption(image)?;
                let ocr = self.florence.ocr(image)?;
                Ok(index::Understanding {
                    model_version: self.model_version(),
                    keywords: keywords
                        .into_iter()
                        .take(20)
                        .map(|(keyword, confidence)| index::KeywordSuggestion {
                            keyword,
                            confidence,
                        })
                        .collect(),
                    caption: caption.caption,
                    alt_text: caption.alt_text,
                    ocr,
                })
            })
            .collect()
    }
}
/// Display-oriented bounded preview decoding shared by background and CLI paths.
pub fn decode_image(path: &Path, preview_cache: &Path) -> Result<RgbImage> {
    let extension = path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_lowercase();
    if matches!(extension.as_str(), "jpg" | "jpeg" | "png") {
        let mut decoder = image::ImageReader::open(path)?.into_decoder()?;
        let orientation = decoder.orientation()?;
        let mut decoded = image::DynamicImage::from_decoder(decoder)?;
        decoded.apply_orientation(orientation);
        return Ok(decoded.thumbnail(1536, 1536).to_rgb8());
    }
    let store = previews::PreviewStore::new(preview_cache, 512 * 1024 * 1024)?;
    let (key, _) = store.from_raw(path, 1536)?;
    let bytes = store
        .get(&key, previews::Level::Full)
        .context("RAW preview unavailable")?;
    Ok(image::load_from_memory(&bytes)?.to_rgb8())
}
/// Priority::Score job, batches four decoded previews, durable between batches.
/// Does not download, accept keywords, modify originals or write XMP. Submit
/// through jobs::ThreadPoolScheduler with an already-loaded UnderstandingModel.
pub struct UnderstandingJob {
    catalog: PathBuf,
    previews: PathBuf,
    images: Vec<ImageId>,
    model: Box<dyn ImageUnderstanding>,
}
impl UnderstandingJob {
    pub fn new(
        catalog: PathBuf,
        previews: PathBuf,
        images: Vec<ImageId>,
        model: Box<dyn ImageUnderstanding>,
    ) -> Self {
        Self {
            catalog,
            previews,
            images,
            model,
        }
    }
}
impl Job for UnderstandingJob {
    fn label(&self) -> &str {
        "Keywords, captions and OCR"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        ctx.check_cancelled()?;
        let index = index::Index::open(&self.catalog)?;
        let version = self.model.model_version();
        let error = |e: anyhow::Error| EngineError::Model {
            model: version.clone(),
            message: format!("{e:#}"),
        };
        let mut remaining = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for id in self.images {
            ctx.check_cancelled()?;
            if seen.insert(id)
                && index
                    .understanding(id)
                    .map_err(error)?
                    .is_none_or(|v| v.model_version != version)
            {
                remaining.push(id);
            }
        }
        let total = remaining.len();
        let mut complete = 0;
        for chunk in remaining.chunks(4) {
            jobs::yield_to_interactive(
                &ctx.cancellation,
                std::time::Duration::from_millis(16),
                std::time::Duration::from_millis(250),
            )?;
            let mut images = Vec::new();
            for &id in chunk {
                ctx.check_cancelled()?;
                images.push(
                    decode_image(&index.image_info(id)?.path, &self.previews).map_err(error)?,
                );
            }
            ctx.check_cancelled()?;
            let result = self.model.analyze_batch(&images);
            ctx.check_cancelled()?;
            let values = result.map_err(error)?;
            (|| -> Result<()> {
                ensure!(
                    values.len() == chunk.len(),
                    "understanding batch cardinality mismatch"
                );
                ensure!(
                    values.iter().all(|v| v.model_version == version),
                    "understanding model version mismatch"
                );
                Ok(())
            })()
            .map_err(error)?;
            for (&id, value) in chunk.iter().zip(values) {
                ctx.check_cancelled()?;
                index.set_understanding(id, &value).map_err(error)?;
                complete += 1;
                ctx.report_progress(complete as f32 / total as f32, None);
            }
        }
        ctx.report_progress(1., None);
        ctx.check_cancelled()
    }
}
