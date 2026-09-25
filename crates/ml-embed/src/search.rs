use crate::{
    DIMENSION, MODEL_VERSION, Siglip,
    vector::{AutoVectorIndex, VectorIndex},
};
use anyhow::Result;
use engine_api::{EngineError, EngineResult, id::ImageId};
use std::path::Path;

/// Version-pinned image/text retrieval. Reopen after a background embedding job
/// finishes so an HNSW snapshot incorporates its new rows.
pub struct SemanticIndex {
    model: Siglip,
    vectors: AutoVectorIndex,
}
impl SemanticIndex {
    pub fn open(model: Siglip, index_dir: &Path) -> Result<Self> {
        Ok(Self {
            model,
            vectors: AutoVectorIndex::open(index_dir, MODEL_VERSION, DIMENSION)?,
        })
    }
    pub fn insert(&mut self, id: ImageId, vector: &[f32; DIMENSION]) -> Result<()> {
        self.vectors.insert(id, vector)
    }
    pub fn search_text(&mut self, query: &str, k: usize) -> Result<Vec<(ImageId, f32)>> {
        let query = self.model.embed_text(query)?;
        self.vectors.search(&query, k)
    }
}
impl index::SemanticSearch for SemanticIndex {
    fn search_text(&mut self, query: &str, k: usize) -> EngineResult<Vec<(ImageId, f32)>> {
        SemanticIndex::search_text(self, query, k).map_err(|e| EngineError::Model {
            model: MODEL_VERSION.into(),
            message: format!("{e:#}"),
        })
    }
}
