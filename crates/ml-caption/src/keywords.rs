use anyhow::{Result, ensure};
use image::RgbImage;
use ml_embed::{DIMENSION, Siglip};
use serde::{Deserialize, Serialize};

/// Independent sigmoid scores, not a softmax over mutually exclusive labels.
/// Defaults are a heuristic starting point; tune temperature/midpoint on your
/// validation set. They are not a claim of measured probability calibration.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Calibration {
    pub temperature: f32,
    pub midpoint: f32,
}
impl Default for Calibration {
    fn default() -> Self {
        Self {
            temperature: 0.05,
            midpoint: 0.2,
        }
    }
}
pub fn rank_keywords(
    labels: &[String],
    similarities: &[f32],
    calibration: Calibration,
) -> Result<Vec<(String, f32)>> {
    ensure!(
        calibration.temperature.is_finite()
            && calibration.temperature > 0.
            && calibration.midpoint.is_finite(),
        "invalid calibration"
    );
    ensure!(
        labels.len() == similarities.len(),
        "label/score length mismatch"
    );
    ensure!(
        similarities
            .iter()
            .all(|x| x.is_finite() && (-1.001..=1.001).contains(x)),
        "invalid cosine similarity"
    );
    let mut ranked: Vec<_> = labels
        .iter()
        .zip(similarities)
        .map(|(label, similarity)| {
            (
                label.clone(),
                1. / (1. + (-(*similarity - calibration.midpoint) / calibration.temperature).exp()),
            )
        })
        .collect();
    ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    Ok(ranked)
}

pub struct KeywordModel {
    model: Siglip,
    labels: Vec<String>,
    vectors: Vec<[f32; DIMENSION]>,
    calibration: Calibration,
}
impl KeywordModel {
    pub fn model_version(&self) -> String {
        use sha2::{Digest, Sha256};
        let mut hash = Sha256::new();
        for label in &self.labels {
            hash.update(label.as_bytes());
            hash.update([0]);
        }
        format!(
            "{}/photo-prompt-v1/{:x}/t{}/m{}",
            ml_embed::MODEL_VERSION,
            hash.finalize(),
            self.calibration.temperature,
            self.calibration.midpoint
        )
    }
    /// Encode the vocabulary once and reuse it for every bounded image batch.
    pub fn new(mut model: Siglip, labels: Vec<String>, calibration: Calibration) -> Result<Self> {
        ensure!(
            !labels.is_empty() && labels.len() <= 10_000,
            "invalid vocabulary size"
        );
        let mut unique = std::collections::HashSet::new();
        for label in &labels {
            ensure!(
                !label.trim().is_empty() && unique.insert(label.trim().to_lowercase()),
                "empty or duplicate label"
            );
        }
        rank_keywords(&[], &[], calibration)?;
        let vectors = labels
            .iter()
            .map(|label| model.embed_text(&format!("a photo of {label}.")))
            .collect::<Result<_>>()?;
        Ok(Self {
            model,
            labels,
            vectors,
            calibration,
        })
    }
    pub fn suggest_keywords(&mut self, image: &RgbImage) -> Result<Vec<(String, f32)>> {
        Ok(self.suggest_batch(std::slice::from_ref(image))?.remove(0))
    }
    pub fn suggest_batch(&mut self, images: &[RgbImage]) -> Result<Vec<Vec<(String, f32)>>> {
        self.model
            .embed_images(images)?
            .iter()
            .map(|image| {
                let scores: Vec<_> = self
                    .vectors
                    .iter()
                    .map(|text| image.iter().zip(text).map(|(a, b)| a * b).sum())
                    .collect();
                rank_keywords(&self.labels, &scores, self.calibration)
            })
            .collect()
    }
    pub fn partition_reports(
        &mut self,
    ) -> Result<(ml_runtime::PartitionReport, ml_runtime::PartitionReport)> {
        self.model.partition_reports()
    }
}

/// Original Apache-2.0 photographic vocabulary, one concept per line.
pub fn vocabulary() -> Vec<String> {
    include_str!("../data/vocabulary.txt")
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .map(str::to_owned)
        .collect()
}
