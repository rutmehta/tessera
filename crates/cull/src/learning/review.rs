use super::{Features, Learner, Measurements, Prediction};
use crate::{CullSession, Decision, Group, ImageId, Selection};
use engine_api::{EngineError, EngineResult};
use index::Index;
use std::{
    collections::{HashMap, HashSet},
    ops::Deref,
    path::PathBuf,
};

/// Extra inputs owned by other feature producers. Dimensions must use the same
/// coordinate space as the catalog's face boxes (usually the analyzed preview).
#[derive(Debug, Clone)]
pub struct ReviewContext {
    pub dimensions: HashMap<ImageId, (u32, u32)>,
    /// Feed from ml-embed VectorIndex::get/rows; no runtime dependency in cull.
    pub embeddings: HashMap<ImageId, Vec<f32>>,
    pub embedding_model: String,
    /// Threshold for the existing weak eyes-open proxy, NOT measured blinking.
    pub eyes_closed_below: f64,
}
impl Default for ReviewContext {
    fn default() -> Self {
        Self {
            dimensions: HashMap::new(),
            embeddings: HashMap::new(),
            embedding_model: String::new(),
            eyes_closed_below: 0.3,
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub enum ReviewMode {
    Assisted,
    Automated { reject_below: f64, keep_above: f64 },
}
/// Intentionally not a Selection/Decision, and never persisted to sidecars.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestedDecision {
    Keep,
    Reject,
}
#[derive(Debug, Clone)]
pub struct ReviewEntry {
    pub image: ImageId,
    pub prediction: Prediction,
    pub suggested: Option<SuggestedDecision>,
    pub technical_score: f64,
    pub sharpness: Option<f64>,
    features: Features,
    before: Selection,
    path: PathBuf,
}
/// Immutable review snapshot. Regenerate after signals, decisions or model change.
#[derive(Debug, Clone)]
pub struct ReviewPlan {
    entries: Vec<ReviewEntry>,
    rejects: Vec<ImageId>,
    basis: Option<super::pca::Pca>,
}
impl ReviewPlan {
    /// Argmax of 70% learned keep probability and 30% technical quality.
    /// Exact ties use sharpness, then original group order. Read-only suggestion.
    pub fn best_in_group(&self, group: &Group) -> EngineResult<ImageId> {
        let mut best: Option<(&ReviewEntry, f64)> = None;
        for id in &group.images {
            let entry = self
                .entries
                .iter()
                .find(|e| e.image == *id)
                .ok_or_else(|| invalid("group image absent from review"))?;
            let blend = 0.7 * entry.prediction.p_keep + 0.3 * entry.technical_score;
            if best.is_none_or(|(previous, score)| {
                blend > score
                    || (blend == score
                        && entry.sharpness.unwrap_or(-1.) > previous.sharpness.unwrap_or(-1.))
            }) {
                best = Some((entry, blend));
            }
        }
        best.map(|(e, _)| e.image)
            .ok_or_else(|| invalid("empty group"))
    }
    pub fn entries(&self) -> &[ReviewEntry] {
        &self.entries
    }
    /// Undecided frames with P(keep) < .5, contiguous at the end of the queue.
    pub fn likely_rejects(&self) -> &[ImageId] {
        &self.rejects
    }
}
fn invalid(message: &str) -> EngineError {
    EngineError::invalid("assistance", message)
}
fn get(scores: &[index::Score], signal: &str) -> Option<f64> {
    scores.iter().find(|s| s.signal == signal).map(|s| s.value)
}
impl Measurements {
    /// Read persisted quality/face signals. An absent measurement stays neutral.
    pub fn from_index(
        index: &Index,
        id: ImageId,
        group: &Group,
        context: &ReviewContext,
    ) -> EngineResult<Self> {
        index.image_info(id)?;
        if !(0. ..=1.).contains(&context.eyes_closed_below) {
            return Err(invalid("invalid eye threshold"));
        }
        let scores = index.scores(id)?;
        let faces = index.faces(id)?;
        let mean_channels = |name: &str| -> Option<f64> {
            let values = ["r", "g", "b"].map(|c| get(&scores, &format!("{name}_{c}")));
            Some(
                values
                    .into_iter()
                    .collect::<Option<Vec<_>>>()?
                    .iter()
                    .sum::<f64>()
                    / 3.,
            )
        };
        let largest_face_fraction = if let Some(&(width, height)) = context.dimensions.get(&id) {
            if width == 0 || height == 0 {
                return Err(invalid("empty face coordinate space"));
            }
            let area = f64::from(width) * f64::from(height);
            Some(
                faces
                    .iter()
                    .map(|face| {
                        let [x, y, w, h] = face.bbox.map(f64::from);
                        let right = (x + w).min(f64::from(width));
                        let bottom = (y + h).min(f64::from(height));
                        (right - x).max(0.) * (bottom - y).max(0.) / area
                    })
                    .fold(0., f64::max),
            )
        } else {
            None
        };
        let sharpness = get(&scores, "sharpness");
        let mut peers = Vec::new();
        for peer in &group.images {
            if let Some(value) = get(&index.scores(*peer)?, "sharpness") {
                if !(0. ..=1.).contains(&value) {
                    return Err(invalid("invalid sharpness"));
                }
                peers.push(value);
            }
        }
        let rank = sharpness.and_then(|s| {
            if peers.len() < 2 {
                None
            } else {
                let below = peers.iter().filter(|v| **v < s).count() as f64;
                let tied_others =
                    peers.iter().filter(|v| **v == s).count().saturating_sub(1) as f64;
                Some((below + 0.5 * tied_others) / (peers.len() - 1) as f64)
            }
        });
        let result = Self {
            sharpness,
            motion_blur: get(&scores, "motion_blur"),
            exposure: mean_channels("exposure"),
            shadow_clipping: mean_channels("shadow_clipping"),
            highlight_clipping: mean_channels("highlight_clipping"),
            noise: get(&scores, "noise"),
            face_count: faces.len(),
            min_face_focus: faces.iter().map(|f| f.sharpness).reduce(f64::min),
            any_eyes_closed: faces
                .iter()
                .any(|f| f.eyes_open.is_some_and(|v| v < context.eyes_closed_below)),
            largest_face_fraction,
            burst_sharpness_rank: rank,
        };
        result.features()?;
        Ok(result)
    }
}
impl<I: Deref<Target = Index>> CullSession<I> {
    /// Manual decision plus one learning update after a successful, non-noop write.
    /// Undo restores Selection, not historical training events. Corrections provide
    /// a new label; callers wanting exact retraining can rebuild from final selections.
    pub fn decide_with_learning(
        &mut self,
        decision: Decision,
        learner: &mut Learner,
        context: &ReviewContext,
    ) -> EngineResult<()> {
        let id = self.require_current()?;
        if !self.index.image_info(id)?.path.is_file() {
            return Err(invalid("image is no longer available"));
        }
        let before = self.selection(id)?.decision;
        let group = &self.groups[self.group_of(id).ok_or_else(|| invalid("missing group"))?];
        let measurements = Measurements::from_index(&self.index, id, group, context)?;
        let embedding = context
            .embeddings
            .get(&id)
            .map(|v| (context.embedding_model.as_str(), v.as_slice()));
        let features = learner.features(&measurements, embedding)?;
        self.decide(decision)?;
        if before != decision {
            learner.observe(&features, decision);
        }
        Ok(())
    }
    /// Read-only: predicts and orders keepers first, uncertain next, rejects last.
    /// Existing decisions never receive suggestions or enter the bulk reject list.
    pub fn review(
        &self,
        learner: &Learner,
        context: &ReviewContext,
        mode: ReviewMode,
    ) -> EngineResult<ReviewPlan> {
        if let ReviewMode::Automated {
            reject_below,
            keep_above,
        } = mode
            && (!(0. ..0.5).contains(&reject_below)
                || !(0.5..=1.).contains(&keep_above)
                || keep_above == 0.5)
        {
            return Err(invalid(
                "thresholds must satisfy 0 <= reject < .5 < keep <= 1",
            ));
        }
        let mut entries = Vec::new();
        for &image in &self.images {
            let group = &self.groups[self
                .group_of(image)
                .ok_or_else(|| invalid("missing group"))?];
            let measurements = Measurements::from_index(&self.index, image, group, context)?;
            let embedding = context
                .embeddings
                .get(&image)
                .map(|v| (context.embedding_model.as_str(), v.as_slice()));
            let features = learner.features(&measurements, embedding)?;
            let prediction = learner.predict(&features);
            let before = self.selection(image)?;
            let suggested = if before.decision != Decision::Undecided {
                None
            } else {
                match mode {
                    ReviewMode::Automated { keep_above, .. } if prediction.p_keep > keep_above => {
                        Some(SuggestedDecision::Keep)
                    }
                    ReviewMode::Automated { reject_below, .. }
                        if prediction.p_keep < reject_below =>
                    {
                        Some(SuggestedDecision::Reject)
                    }
                    _ => None,
                }
            };
            let quality = get(&self.index.scores(image)?, "quality").unwrap_or_else(|| {
                measurements.sharpness.unwrap_or(0.5)
                    * (1. - 0.25 * measurements.motion_blur.unwrap_or(0.))
                    * (1. - 0.5 * measurements.noise.unwrap_or(0.))
                    * (1. - measurements.highlight_clipping.unwrap_or(0.))
                    * (1. - measurements.shadow_clipping.unwrap_or(0.))
            });
            if !(0. ..=1.).contains(&quality) {
                return Err(invalid("invalid technical score"));
            }
            let technical_score =
                quality * measurements.min_face_focus.map_or(1., |f| 0.5 + 0.5 * f);
            entries.push(ReviewEntry {
                image,
                prediction,
                suggested,
                technical_score,
                sharpness: measurements.sharpness,
                features,
                before,
                path: self.index.image_info(image)?.path,
            });
        }
        let likely_reject =
            |e: &ReviewEntry| e.before.decision == Decision::Undecided && e.prediction.p_keep < 0.5;
        entries.sort_by(|a, b| {
            likely_reject(a)
                .cmp(&likely_reject(b))
                .then_with(|| b.prediction.p_keep.total_cmp(&a.prediction.p_keep))
        });
        let rejects = entries
            .iter()
            .filter(|e| likely_reject(e))
            .map(|e| e.image)
            .collect();
        Ok(ReviewPlan {
            entries,
            rejects,
            basis: learner.pca.clone(),
        })
    }
    /// Changes only navigation order, preserving cursor image and undo/redo cursors.
    pub fn reorder_review(&mut self, plan: &ReviewPlan) -> EngineResult<()> {
        let positions: HashMap<_, _> = plan
            .entries
            .iter()
            .enumerate()
            .map(|(n, e)| (e.image, n))
            .collect();
        if positions.len() != self.images.len()
            || self.images.iter().any(|id| !positions.contains_key(id))
        {
            return Err(invalid("review belongs to a different queue"));
        }
        for action in self.undo.iter_mut().chain(&mut self.redo) {
            action.before_position = positions[&self.images[action.before_position]];
            action.after_position = positions[&self.images[action.after_position]];
        }
        if let Some(id) = self.current() {
            self.position = positions[&id];
        }
        self.images = plan.entries.iter().map(|e| e.image).collect();
        for group in &mut self.groups {
            group.images.sort_by_key(|id| positions[id]);
        }
        self.groups.sort_by_key(|g| positions[&g.images[0]]);
        Ok(())
    }
    /// Explicit user confirmation only, one undoable batch. Rejects stale selections,
    /// foreign images, duplicate IDs and unsuggested images before any write/learning.
    /// Caller saves the learner separately; a model-save failure never undoes a decision.
    pub fn confirm_suggestions(
        &mut self,
        plan: &ReviewPlan,
        ids: &[ImageId],
        learner: &mut Learner,
    ) -> EngineResult<()> {
        if plan.basis != learner.pca {
            return Err(invalid("learner basis changed; regenerate the review"));
        }
        let mut seen = HashSet::new();
        let mut confirmed = Vec::new();
        for id in ids {
            let entry = plan
                .entries
                .iter()
                .find(|e| e.image == *id)
                .ok_or_else(|| invalid("image not in review"))?;
            if !entry.path.is_file()
                || !seen.insert(*id)
                || self.selection(*id)? != entry.before
                || self.index.image_info(*id)?.path != entry.path
            {
                return Err(invalid("duplicate or stale suggestion"));
            }
            let decision = match entry.suggested {
                Some(SuggestedDecision::Keep) => Decision::Keep,
                Some(SuggestedDecision::Reject) => Decision::Reject,
                None => return Err(invalid("image has no suggested decision")),
            };
            confirmed.push((entry, decision));
        }
        self.decide_each(
            &confirmed
                .iter()
                .map(|(e, d)| (e.image, *d))
                .collect::<Vec<_>>(),
        )?;
        for (entry, decision) in confirmed {
            learner.observe(&entry.features, decision);
        }
        Ok(())
    }
}
