//! Local SigLIP embeddings and versioned vector retrieval.
mod model;
pub mod vector;
pub use model::{DIMENSION, MODEL_VERSION, REVISION, Siglip, TOKENIZER_SHA256};

/// SigLIP resizes (without cropping) to 224², then maps RGB bytes to [-1, 1].
pub fn preprocess(images: &[image::RgbImage]) -> anyhow::Result<Vec<f32>> {
    anyhow::ensure!(!images.is_empty(), "empty image batch");
    let mut data = Vec::new();
    for image in images {
        anyhow::ensure!(image.width() > 0 && image.height() > 0, "empty preview");
        let resized =
            image::imageops::resize(image, 224, 224, image::imageops::FilterType::CatmullRom);
        for channel in 0..3 {
            data.extend(
                resized
                    .pixels()
                    .map(|p| f32::from(p[channel]) / 127.5 - 1.0),
            );
        }
    }
    Ok(data)
}

mod grouping;
pub use grouping::EmbeddingGrouping;

mod job;
pub use job::{EmbedFolderJob, ImageEmbedder};
mod search;
pub use search::SemanticIndex;
