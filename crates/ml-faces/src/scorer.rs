use engine_api::{EngineResult, id::ImageId};
use std::collections::HashMap;

/// Snapshot of minimum per-image face focus. Missing faces score zero.
/// The geometry-only eyes proxy deliberately does not influence ranking.
#[derive(Default)]
pub struct FaceScorer(HashMap<ImageId, f64>);
impl FaceScorer {
    pub fn from_index(index: &index::Index, ids: &[ImageId]) -> EngineResult<Self> {
        let mut values = HashMap::new();
        for &id in ids {
            if let Some(score) = index
                .scores(id)?
                .into_iter()
                .find(|s| s.signal == "face_sharpness")
            {
                values.insert(id, score.value);
            }
        }
        Ok(Self(values))
    }
}
impl cull::Scorer for FaceScorer {
    fn score(&self, image: &index::ImageInfo) -> f64 {
        self.0.get(&image.id).copied().unwrap_or(0.)
    }
}
