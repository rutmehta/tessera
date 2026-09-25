//! Snapshot-based semantic edge policy for culling.
use crate::vector::VectorIndex;
use anyhow::Result;
use cull::{GroupingOptions, GroupingStrategy};
use engine_api::id::ImageId;
use index::ImageInfo;
use std::collections::HashMap;

/// Combines cosine similarity with capture proximity or cull's preview dHash.
/// With near-duplicates enabled, two embeddings must have cosine >= 0.92 and
/// either capture proximity or dHash distance <= 6. Missing embeddings require
/// both time and hash evidence. Disabling near-duplicates uses time alone.
/// The snapshot owns its vectors; no database access occurs during regrouping.
pub struct EmbeddingGrouping {
    vectors: HashMap<ImageId, Vec<f64>>,
}

impl EmbeddingGrouping {
    /// Snapshot the requested images from a single model's vector index.
    pub fn from_index(store: &dyn VectorIndex, ids: &[ImageId]) -> Result<Self> {
        let mut vectors = HashMap::new();
        let mut dimension = None;
        for &id in ids {
            if vectors.contains_key(&id) {
                continue;
            }
            if let Some(vector) = store.get(id)? {
                anyhow::ensure!(!vector.is_empty(), "empty embedding for {id:?}");
                anyhow::ensure!(
                    dimension.is_none_or(|n| n == vector.len()),
                    "embedding dimension mismatch for {id:?}"
                );
                dimension = Some(vector.len());
                let norm = vector
                    .iter()
                    .map(|&x| f64::from(x).powi(2))
                    .sum::<f64>()
                    .sqrt();
                anyhow::ensure!(
                    norm.is_finite() && norm > 0.0,
                    "embedding must be finite and nonzero for {id:?}"
                );
                vectors.insert(id, vector.iter().map(|&x| f64::from(x) / norm).collect());
            }
        }
        Ok(Self { vectors })
    }
}

impl GroupingStrategy for EmbeddingGrouping {
    fn related(
        &self,
        a: &ImageInfo,
        b: &ImageInfo,
        ha: Option<u64>,
        hb: Option<u64>,
        options: GroupingOptions,
    ) -> bool {
        let time = a
            .capture_seconds
            .zip(b.capture_seconds)
            .is_some_and(|(a, b)| (a - b).abs() <= options.burst_gap_seconds);
        if !options.near_duplicates {
            return time;
        }
        let hash = ha.zip(hb).is_some_and(|(a, b)| (a ^ b).count_ones() <= 6);
        match (self.vectors.get(&a.id), self.vectors.get(&b.id)) {
            (Some(a), Some(b)) => {
                a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>() >= 0.92 && (time || hash)
            }
            _ => time && hash,
        }
    }
}
