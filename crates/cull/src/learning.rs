//! Library-local, explainable learning. Predictions are never Selection writes.
use crate::Decision;
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
mod pca;
mod review;
pub use review::{ReviewContext, ReviewEntry, ReviewMode, ReviewPlan, SuggestedDecision};

const NAMES: [&str; 11] = [
    "sharpness",
    "motion_blur",
    "exposure",
    "shadow_clipping",
    "highlight_clipping",
    "noise",
    "face_count",
    "face_focus",
    "eyes_closed",
    "largest_face_fraction",
    "burst_sharpness_rank",
];
const DIMENSIONS: usize = NAMES.len() + 32;

/// Normalized measurements. Missing signals are neutral, not defective.
#[derive(Debug, Clone, Default)]
pub struct Measurements {
    pub sharpness: Option<f64>,
    pub motion_blur: Option<f64>,
    pub exposure: Option<f64>,
    pub shadow_clipping: Option<f64>,
    pub highlight_clipping: Option<f64>,
    pub noise: Option<f64>,
    pub face_count: usize,
    pub min_face_focus: Option<f64>,
    /// Thresholded eye heuristic; unknown is not treated as closed.
    pub any_eyes_closed: bool,
    pub largest_face_fraction: Option<f64>,
    /// Percentile within burst, 1 is sharpest; ties share the same percentile.
    pub burst_sharpness_rank: Option<f64>,
}

/// Validated, bounded vector. Embedding components are appended by the learner.
#[derive(Debug, Clone)]
pub struct Features([f64; DIMENSIONS]);
impl Features {
    pub fn values(&self) -> &[f64] {
        &self.0
    }
}
impl Measurements {
    pub fn features(&self) -> EngineResult<Features> {
        let mut values = [0.; DIMENSIONS];
        let inputs = [
            self.sharpness,
            self.motion_blur,
            self.exposure,
            self.shadow_clipping,
            self.highlight_clipping,
            self.noise,
            Some((self.face_count as f64 / 10.).min(1.)),
            self.min_face_focus,
            Some(f64::from(self.any_eyes_closed)),
            self.largest_face_fraction,
            self.burst_sharpness_rank,
        ];
        for (i, input) in inputs.into_iter().enumerate() {
            if let Some(x) = input {
                if !(0. ..=1.).contains(&x) {
                    return Err(EngineError::invalid(
                        NAMES[i],
                        "expected finite [0,1] measurement",
                    ));
                }
                values[i] = if matches!(i, 0 | 1 | 2 | 7 | 10) {
                    2. * x - 1.
                } else {
                    x
                };
            }
        }
        Ok(Features(values))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Contribution {
    pub feature: String,
    /// Signed additive contribution to log-odds, not a causal attribution.
    pub contribution: f64,
}
#[derive(Debug, Clone, PartialEq)]
pub struct Prediction {
    pub p_keep: f64,
    /// Up to five nonzero terms, largest absolute contribution first.
    pub explanation: Vec<Contribution>,
}

/// Online logistic regression, one bounded SGD update per explicit label.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Learner {
    weights: Vec<f64>,
    bias: f64,
    labels: u64,
    pca: Option<pca::Pca>,
}
impl Default for Learner {
    fn default() -> Self {
        let mut weights = vec![0.; DIMENSIONS];
        weights[0] = 2.;
        weights[1] = -0.5;
        weights[3] = -1.;
        weights[4] = -1.;
        weights[5] = -0.5;
        weights[7] = 4.;
        weights[8] = -8.;
        Self {
            weights,
            bias: 0.,
            labels: 0,
            pca: None,
        }
    }
}
impl Learner {
    /// Fit once on this library's embeddings (e.g. ml-embed VectorIndex::rows).
    /// A changed basis requires a new learner, never reusing trained weights.
    pub fn with_embeddings(model: &str, samples: &[Vec<f32>]) -> EngineResult<Self> {
        Ok(Self {
            pca: Some(pca::Pca::fit(model, samples)?),
            ..Self::default()
        })
    }
    pub fn features(
        &self,
        measurements: &Measurements,
        embedding: Option<(&str, &[f32])>,
    ) -> EngineResult<Features> {
        let mut features = measurements.features()?;
        if let Some((model, vector)) = embedding {
            let pca = self
                .pca
                .as_ref()
                .ok_or_else(|| EngineError::invalid("embedding", "fit library PCA first"))?;
            features.0[NAMES.len()..].copy_from_slice(&pca.project(model, vector)?);
        }
        Ok(features)
    }
    /// Host supplies its Application Support root and a stable library ID.
    /// Single writer per library. Missing files cold-start; corrupt files fail closed.
    pub fn open(app_support: &Path, library: &str) -> EngineResult<Self> {
        let path = Self::storage_path(app_support, library)?;
        let Some(bytes) = crate::persistence::optional_bytes(&path)? else {
            return Ok(Self::default());
        };
        let stored: Stored = serde_json::from_slice(&bytes)?;
        let model = stored.learner;
        if stored.version != 1
            || stored.library != library
            || model.weights.len() != DIMENSIONS
            || model
                .weights
                .iter()
                .any(|w| !w.is_finite() || w.abs() > 32.)
            || !model.bias.is_finite()
            || model.bias.abs() > 32.
            || model.pca.as_ref().is_some_and(|pca| !pca.valid())
        {
            return Err(EngineError::invalid(
                "learner",
                "invalid or incompatible library model",
            ));
        }
        Ok(model)
    }
    /// Atomically save weights AND the frozen PCA basis together.
    pub fn save(&self, app_support: &Path, library: &str) -> EngineResult<()> {
        let path = Self::storage_path(app_support, library)?;
        crate::persistence::atomic_write(
            &path,
            &serde_json::to_vec(&Stored {
                version: 1,
                library: library.into(),
                learner: self.clone(),
            })?,
        )
    }
    pub fn storage_path(app_support: &Path, library: &str) -> EngineResult<PathBuf> {
        if library.is_empty() || library.len() > 100 {
            return Err(EngineError::invalid(
                "library",
                "requires stable ID of 1..100 bytes",
            ));
        }
        let key: String = library.bytes().map(|b| format!("{b:02x}")).collect();
        Ok(app_support
            .join("cull-learning")
            .join(format!("{key}.json")))
    }
    pub fn label_count(&self) -> u64 {
        self.labels
    }
    pub fn predict(&self, features: &Features) -> Prediction {
        let mut explanation: Vec<_> = features
            .0
            .iter()
            .zip(&self.weights)
            .enumerate()
            .map(|(i, (x, w))| Contribution {
                feature: NAMES
                    .get(i)
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| format!("embedding_pc_{}", i - NAMES.len() + 1)),
                contribution: x * w,
            })
            .filter(|c| c.contribution != 0.)
            .collect();
        let logit = self.bias + explanation.iter().map(|c| c.contribution).sum::<f64>();
        explanation.sort_by(|a, b| {
            b.contribution
                .abs()
                .total_cmp(&a.contribution.abs())
                .then(a.feature.cmp(&b.feature))
        });
        explanation.truncate(5);
        Prediction {
            p_keep: 1. / (1. + (-logit).exp()),
            explanation,
        }
    }
    /// Call only for user-confirmed Keep/Reject labels, never suggested states.
    /// Undecided is deliberately ignored.
    pub fn observe(&mut self, features: &Features, decision: Decision) {
        let target = match decision {
            Decision::Keep => 1.,
            Decision::Reject => 0.,
            Decision::Undecided => return,
        };
        let error = target - self.predict(features).p_keep;
        let rate = 0.8;
        for (w, x) in self.weights.iter_mut().zip(features.0) {
            *w = (*w + rate * (error * x - 0.001 * *w)).clamp(-32., 32.);
        }
        self.bias = (self.bias + rate * error).clamp(-32., 32.);
        self.labels = self.labels.saturating_add(1);
    }
}

#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    library: String,
    learner: Learner,
}
