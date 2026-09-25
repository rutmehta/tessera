use crate::Features;
use crate::{
    features::Pca,
    sliders::{overlay, sliders, values},
};
use engine_api::id::ImageId;
use engine_api::recipe::{history::Author, Recipe};
use engine_api::{
    recipe::{settings::WhiteBalanceMode, DevelopSettings},
    EngineError, EngineResult,
};
use serde::{Deserialize, Serialize};
use std::{
    io::Write,
    path::{Path, PathBuf},
};

/// Features must be measured from the unedited source, not the after render.
#[derive(Clone, Debug)]
pub struct ReferenceEdit {
    pub image: ImageId,
    pub features: Features,
    pub before: DevelopSettings,
    pub after: DevelopSettings,
}
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    profile: Profile,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Questionnaire {
    /// Signed preferences in [-1,1].
    pub brightness: f64,
    pub contrast: f64,
    pub warmth: f64,
    pub saturation: f64,
    /// Priority in [0,1].
    pub skin_tone_priority: f64,
}
impl Questionnaire {
    fn validate(&self) -> EngineResult<()> {
        if [self.brightness, self.contrast, self.warmth, self.saturation]
            .iter()
            .any(|x| !(-1. ..=1.).contains(x))
            || !(0. ..=1.).contains(&self.skin_tone_priority)
        {
            return Err(EngineError::invalid(
                "questionnaire",
                "preferences must be finite and in range",
            ));
        }
        Ok(())
    }
    fn prior(&self, f: &Features) -> DevelopSettings {
        let mut s = DevelopSettings::default();
        s.tone.exposure = self.brightness as f32;
        s.tone.contrast = (self.contrast * 35.) as f32;
        s.white_balance.temperature += (self.warmth * 1200.) as f32;
        if self.warmth != 0. {
            s.white_balance.mode = WhiteBalanceMode::Custom;
        }
        s.color.saturation = (self.saturation * 30.) as f32;
        if let Some(luma) = f.face_mean_luminance {
            let deficit = (0.18 / luma.max(0.001)).log2().clamp(0., 3.);
            s.tone.shadows = (deficit * 20. * self.skin_tone_priority) as f32;
            s.tone.exposure += (deficit * 0.2 * self.skin_tone_priority) as f32;
        }
        s
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SliderPrediction {
    /// JSON pointer into DevelopSettings.
    pub path: String,
    pub confidence: f64,
    pub rationale: String,
}
#[derive(Debug, Clone)]
pub struct Prediction {
    pub settings: DevelopSettings,
    pub sliders: Vec<SliderPrediction>,
}
#[derive(Debug, Clone, Serialize)]
pub struct Profile {
    pub(crate) library: String,
    questionnaire: Questionnaire,
    samples: Vec<Sample>,
    fitted: Option<Fitted>,
}
impl<'de> Deserialize<'de> for Profile {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Data {
            library: String,
            questionnaire: Questionnaire,
            samples: Vec<Sample>,
            fitted: Option<Fitted>,
        }
        let data = Data::deserialize(d)?;
        let profile = Self {
            library: data.library,
            questionnaire: data.questionnaire,
            samples: data.samples,
            fitted: data.fitted,
        };
        profile.validate().map_err(serde::de::Error::custom)?;
        Ok(profile)
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Sample {
    id: ImageId,
    features: Features,
    target: Vec<f64>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Fitted {
    pca: Pca,
    cameras: Vec<String>,
    lenses: Vec<String>,
    weights: Vec<Vec<f64>>,
    errors: Vec<f64>,
    rows: Vec<Vec<f64>>,
}
impl Fitted {
    fn row(&self, f: &Features) -> EngineResult<Vec<f64>> {
        let mut row = vec![1.];
        row.extend(f.numeric());
        // Retain linear scene mean in addition to logarithmic HDR descriptors.
        row.push(f.mean_luminance.min(100.));
        row.extend(self.pca.project(&f.embedding_model, &f.embedding)?);
        row.extend(self.cameras.iter().map(|c| f64::from(c == &f.camera)));
        row.extend(self.lenses.iter().map(|c| f64::from(c == &f.lens)));
        Ok(row)
    }
}
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}
// Positive-definite ridge system solved by Cholesky, with no numeric dependency.
fn solve(l: &[Vec<f64>], mut b: Vec<f64>) -> Vec<f64> {
    for i in 0..b.len() {
        b[i] = (b[i] - dot(&l[i][..i], &b[..i])) / l[i][i];
    }
    for i in (0..b.len()).rev() {
        b[i] = (b[i] - (i + 1..b.len()).map(|j| l[j][i] * b[j]).sum::<f64>()) / l[i][i];
    }
    b
}
impl Profile {
    /// Ingest the last user-confirmed state on the active lineage, excluding
    /// abandoned redo branches and subsequent unaccepted agent changes.
    pub fn collect(
        &mut self,
        images: impl IntoIterator<Item = (ImageId, Features, Recipe)>,
    ) -> EngineResult<usize> {
        let mut next = self.clone();
        let mut count = 0;
        for (id, features, recipe) in images {
            recipe.validate()?;
            if recipe.image_id.is_some_and(|stored| stored != id) {
                return Err(EngineError::invalid("image", "recipe identity mismatch"));
            }
            if let Some(entry) = recipe
                .history
                .lineage(recipe.history.head)?
                .into_iter()
                .rev()
                .find(|e| matches!(e.meta.author, Author::User))
            {
                features.validate()?;
                let target = values(&recipe.history.state_at(Some(entry.id))?)?;
                next.samples.retain(|s| s.id != id);
                next.samples.push(Sample {
                    id,
                    features,
                    target,
                });
                count += 1;
            }
        }
        next.fit()?;
        *self = next;
        Ok(count)
    }
    /// Before validates the reference pair; the supervised target is the final
    /// after state relative to engine defaults, not the after-minus-before edit.
    pub fn seed_references(&mut self, references: &[ReferenceEdit]) -> EngineResult<()> {
        let mut next = self.clone();
        for r in references {
            values(&r.before)?;
            next.record_feedback(r.image, &r.features, &r.after)?;
        }
        *self = next;
        Ok(())
    }
    pub fn storage_path(root: &Path, library: &str) -> EngineResult<PathBuf> {
        Self::new(library, Questionnaire::default())?;
        let key: String = library.bytes().map(|b| format!("{b:02x}")).collect();
        Ok(root.join("style-profile").join(format!("{key}.json")))
    }
    /// Atomic single-file JSON includes version, identity, labels, PCA and weights.
    /// One writer per library, coordinated by the host.
    pub fn save(&self, root: &Path) -> EngineResult<()> {
        self.validate()?;
        let path = Self::storage_path(root, &self.library)?;
        std::fs::create_dir_all(path.parent().expect("profile parent"))?;
        let mut temp = tempfile::NamedTempFile::new_in(path.parent().expect("profile parent"))?;
        temp.write_all(&serde_json::to_vec(&Stored {
            version: 1,
            profile: self.clone(),
        })?)?;
        temp.as_file().sync_all()?;
        temp.persist(&path)
            .map_err(|e| EngineError::io_at(&path, &e.error))?;
        Ok(())
    }
    pub fn open(root: &Path, library: &str) -> EngineResult<Option<Self>> {
        let path = Self::storage_path(root, library)?;
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let stored: Stored = serde_json::from_slice(&bytes)?;
        if stored.version != 1 || stored.profile.library != library {
            return Err(EngineError::invalid(
                "profile",
                "incompatible version or library",
            ));
        }
        stored.profile.validate()?;
        Ok(Some(stored.profile))
    }
    fn validate(&self) -> EngineResult<()> {
        Self::new(&self.library, self.questionnaire.clone())?;
        let bad = || EngineError::invalid("profile", "invalid model dimensions or values");
        let specs = sliders();
        let mut ids = std::collections::HashSet::new();
        for s in &self.samples {
            s.features.validate()?;
            if !ids.insert(s.id)
                || s.target.len() != specs.len()
                || s.target.iter().zip(&specs).any(|(v, p)| {
                    !v.is_finite()
                        || !(p.min - 1e-6..=p.max + 1e-6).contains(&(p.default + v * p.range()))
                })
            {
                return Err(bad());
            }
        }
        match &self.fitted {
            None if self.samples.is_empty() => (),
            Some(m) if !self.samples.is_empty() => {
                if !m.pca.valid() {
                    return Err(bad());
                }
                let n = m.row(&self.samples[0].features)?.len();
                if m.weights.len() != specs.len()
                    || m.errors.len() != specs.len()
                    || m.rows.len() != self.samples.len()
                    || m.weights
                        .iter()
                        .chain(&m.rows)
                        .any(|r| r.len() != n || r.iter().any(|x| !x.is_finite()))
                    || m.errors.iter().any(|e| !e.is_finite() || *e < 0.)
                {
                    return Err(bad());
                }
                for s in &self.samples {
                    m.row(&s.features)?;
                }
            }
            _ => return Err(bad()),
        }
        Ok(())
    }
    pub fn new(library: &str, questionnaire: Questionnaire) -> EngineResult<Self> {
        questionnaire.validate()?;
        if library.is_empty() || library.len() > 100 {
            return Err(EngineError::invalid("library", "requires 1..100 bytes"));
        }
        Ok(Self {
            library: library.into(),
            questionnaire,
            samples: vec![],
            fitted: None,
        })
    }
    pub fn predict(&self, f: &Features) -> EngineResult<Prediction> {
        f.validate()?;
        let prior = self.questionnaire.prior(f);
        let specs = sliders();
        let (deltas, confidence) = if let Some(model) = &self.fitted {
            let row = model.row(f)?;
            let nearest = model
                .rows
                .iter()
                .map(|r| {
                    r.iter()
                        .zip(&row)
                        .map(|(a, b)| (a - b).powi(2))
                        .sum::<f64>()
                })
                .fold(f64::INFINITY, f64::min);
            let support = self.samples.len() as f64 / (self.samples.len() as f64 + 8.);
            (
                model
                    .weights
                    .iter()
                    .map(|w| dot(w, &row))
                    .collect::<Vec<_>>(),
                model
                    .errors
                    .iter()
                    .map(|e| (support / (1. + 10. * e + nearest.sqrt())).clamp(0., 1.))
                    .collect::<Vec<_>>(),
            )
        } else {
            (values(&prior)?, vec![0.1; specs.len()])
        };
        let settings = overlay(&prior, &deltas)?;
        let actual = values(&settings)?;
        let sliders = specs
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let source = if self.fitted.is_some() {
                    format!(
                        "ridge fit from {} confirmed edits; scene mean {:.3}",
                        self.samples.len(),
                        f.mean_luminance
                    )
                } else {
                    "questionnaire prior (cold start)".into()
                };
                let face = if s.path == "/tone/shadows" {
                    f.face_mean_luminance
                        .map(|l| {
                            format!(
                                "; faces measured {:.2} EV below mid-grey",
                                (0.18 / l.max(0.001)).log2()
                            )
                        })
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                SliderPrediction {
                    path: s.path.clone(),
                    confidence: confidence[i],
                    rationale: format!(
                        "{} delta {:+.2}: {}{} (association, not causal attribution)",
                        s.path,
                        actual[i] * s.range(),
                        source,
                        face
                    ),
                }
            })
            .collect();
        Ok(Prediction { settings, sliders })
    }
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }
    /// Explicit user acceptance or correction only, never predictions. Replaces an
    /// image's previous label. Refitting PCA and weights together avoids basis drift.
    pub fn record_feedback(
        &mut self,
        id: ImageId,
        f: &Features,
        final_settings: &DevelopSettings,
    ) -> EngineResult<()> {
        f.validate()?;
        let target = values(final_settings)?;
        let mut next = self.clone();
        let sample = Sample {
            id,
            features: f.clone(),
            target,
        };
        if let Some(old) = next.samples.iter_mut().find(|s| s.id == id) {
            *old = sample;
        } else {
            next.samples.push(sample);
        }
        next.fit()?;
        *self = next;
        Ok(())
    }
    fn fit(&mut self) -> EngineResult<()> {
        let Some(first) = self.samples.first() else {
            self.fitted = None;
            return Ok(());
        };
        if self
            .samples
            .iter()
            .any(|s| s.features.embedding_model != first.features.embedding_model)
        {
            return Err(EngineError::invalid("embedding", "mixed model versions"));
        }
        let embeddings = self
            .samples
            .iter()
            .map(|s| s.features.embedding.clone())
            .collect::<Vec<_>>();
        let categories = |camera: bool| {
            let mut keys = self
                .samples
                .iter()
                .map(|s| {
                    if camera {
                        s.features.camera.clone()
                    } else {
                        s.features.lens.clone()
                    }
                })
                .collect::<Vec<_>>();
            keys.sort();
            keys.dedup();
            keys
        };
        let mut model = Fitted {
            pca: Pca::fit(&first.features.embedding_model, &embeddings)?,
            cameras: categories(true),
            lenses: categories(false),
            weights: vec![],
            errors: vec![],
            rows: vec![],
        };
        model.rows = self
            .samples
            .iter()
            .map(|s| model.row(&s.features))
            .collect::<EngineResult<_>>()?;
        let n = model.rows[0].len();
        let mut l = vec![vec![0.; n]; n];
        for i in 0..n {
            for j in 0..=i {
                let a = model.rows.iter().map(|r| r[i] * r[j]).sum::<f64>()
                    + if i == j { 0.01 } else { 0. };
                let residual = a - dot(&l[i][..j], &l[j][..j]);
                l[i][j] = if i == j {
                    residual.sqrt()
                } else {
                    residual / l[j][j]
                };
            }
        }
        for k in 0..sliders().len() {
            let b = (0..n)
                .map(|j| {
                    model
                        .rows
                        .iter()
                        .zip(&self.samples)
                        .map(|(r, s)| r[j] * s.target[k])
                        .sum()
                })
                .collect();
            let w = solve(&l, b);
            if w.iter().any(|x| !x.is_finite()) {
                return Err(EngineError::invalid("model", "ill-conditioned features"));
            }
            let rmse = (model
                .rows
                .iter()
                .zip(&self.samples)
                .map(|(r, s)| (dot(&w, r) - s.target[k]).powi(2))
                .sum::<f64>()
                / self.samples.len() as f64)
                .sqrt();
            model.errors.push(rmse);
            model.weights.push(w);
        }
        self.fitted = Some(model);
        Ok(())
    }
}
