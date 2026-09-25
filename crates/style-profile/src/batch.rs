use crate::{
    sliders::{overlay, sliders, values},
    Features, Profile, SliderPrediction,
};
use engine_api::{
    id::{HistoryEntryId, HistoryGroupId, ImageId},
    jobs::{Job, JobContext, Priority},
    recipe::{
        history::{Author, EditMeta, HistoryEntry},
        DevelopSettings, Recipe,
    },
    EngineError, EngineResult,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashSet},
    sync::mpsc::{self, Receiver, Sender},
};

#[derive(Clone, Debug)]
pub struct BatchImage {
    pub image: ImageId,
    pub features: Features,
    pub burst: Option<String>,
    pub people: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewEntry {
    pub image: ImageId,
    pub group: HistoryGroupId,
    pub confidence: f64,
    pub sliders: Vec<SliderPrediction>,
}
/// Versioned extension keyed by HistoryGroupId, until engine-api grows a typed amount.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupAmount {
    pub version: u32,
    pub amount: f64,
    pub before: DevelopSettings,
    pub full: DevelopSettings,
    pub sliders: Vec<SliderPrediction>,
}
impl GroupAmount {
    /// Absolute blend from the original pre-group state, never cumulative. The UI
    /// must replay later edits on top, not overwrite them with this materialization.
    pub fn settings_at(&self, amount: f64) -> EngineResult<DevelopSettings> {
        check_amount(amount)?;
        if self.version != 1 {
            return Err(EngineError::invalid("group", "unknown amount version"));
        }
        if amount == 0. {
            return Ok(self.before.clone());
        }
        if amount == 1. {
            return Ok(self.full.clone());
        }
        let a = values(&self.before)?;
        let b = values(&self.full)?;
        let delta = a
            .iter()
            .zip(b)
            .map(|(a, b)| a + (b - a) * amount)
            .collect::<Vec<_>>();
        let mut result = overlay(&self.before, &delta)?;
        result.white_balance.mode = self.full.white_balance.mode;
        Ok(result)
    }
}
const EXTENSION: &str = "tessera_style_profile_groups_v1";
pub fn group_amount(recipe: &Recipe, group: HistoryGroupId) -> EngineResult<GroupAmount> {
    let value = recipe
        .unknown
        .get(EXTENSION)
        .and_then(|v| v.get(group.0.to_string()))
        .ok_or_else(|| EngineError::not_found("style group", group))?;
    Ok(serde_json::from_value(value.clone())?)
}
fn check_amount(amount: f64) -> EngineResult<()> {
    if !(0. ..=1.).contains(&amount) {
        Err(EngineError::invalid("amount", "expected finite [0,1]"))
    } else {
        Ok(())
    }
}
// Components are computed per slider, so person exposure linking never breaks an
// existing burst tone equality. This also handles multiple identities in one frame.
fn components(images: &[BatchImage], burst: bool, person: bool) -> Vec<Vec<usize>> {
    let mut parent = (0..images.len()).collect::<Vec<_>>();
    fn root(p: &[usize], mut i: usize) -> usize {
        while p[i] != i {
            i = p[i];
        }
        i
    }
    let mut keys = BTreeMap::new();
    for (i, image) in images.iter().enumerate() {
        let mut labels = Vec::new();
        if burst {
            if let Some(b) = &image.burst {
                labels.push(format!("b:{b}"));
            }
        }
        if person {
            labels.extend(image.people.iter().map(|p| format!("p:{p}")));
        }
        for key in labels {
            if let Some(&j) = keys.get(&key) {
                let a = root(&parent, i);
                let b = root(&parent, j);
                parent[a] = b;
            } else {
                keys.insert(key, i);
            }
        }
    }
    let mut groups: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for i in 0..images.len() {
        groups.entry(root(&parent, i)).or_default().push(i);
    }
    groups.into_values().collect()
}
/// All-or-nothing in-memory operation. Persistent jobs commit one image at a time.
pub fn apply_batch(
    profile: &Profile,
    images: &[BatchImage],
    recipes: &mut [Recipe],
    amount: f64,
    timestamp_ms: i64,
) -> EngineResult<Vec<ReviewEntry>> {
    check_amount(amount)?;
    if images.len() != recipes.len() {
        return Err(EngineError::invalid("batch", "length mismatch"));
    }
    let mut ids = HashSet::new();
    for (image, recipe) in images.iter().zip(recipes.iter()) {
        recipe.validate()?;
        if !ids.insert(image.image) || recipe.image_id.is_some_and(|id| id != image.image) {
            return Err(EngineError::invalid(
                "image",
                "duplicate or mismatched identity",
            ));
        }
    }
    let mut predictions = images
        .iter()
        .map(|i| profile.predict(&i.features))
        .collect::<EngineResult<Vec<_>>>()?;
    let mut deltas = predictions
        .iter()
        .map(|p| values(&p.settings))
        .collect::<EngineResult<Vec<_>>>()?;
    for (k, spec) in sliders().iter().enumerate() {
        let burst = spec.path.starts_with("/white_balance/") || spec.path.starts_with("/tone/");
        let person = matches!(spec.path.as_str(), "/tone/exposure" | "/tone/texture")
            || (spec.path.starts_with("/color/hsl/")
                && (spec.path.ends_with("/orange") || spec.path.ends_with("/red")));
        if !burst && !person {
            continue;
        }
        for group in components(images, burst, person) {
            if group.len() < 2 {
                continue;
            }
            let mean = group.iter().map(|&i| deltas[i][k]).sum::<f64>() / group.len() as f64;
            let spread = group
                .iter()
                .map(|&i| (deltas[i][k] - mean).abs())
                .fold(0., f64::max);
            let confidence = group
                .iter()
                .map(|&i| predictions[i].sliders[k].confidence)
                .fold(1., f64::min)
                / (1. + 10. * spread);
            for i in group {
                deltas[i][k] = mean;
                predictions[i].sliders[k].confidence = confidence;
                predictions[i].sliders[k].rationale.push_str(&format!(
                    "; consistency consensus delta {:+.2}",
                    mean * spec.range()
                ));
            }
        }
    }
    let mut next = recipes.to_vec();
    let mut queue = Vec::new();
    for (i, recipe) in next.iter_mut().enumerate() {
        let full = overlay(&recipe.settings, &deltas[i])?;
        let meta = GroupAmount {
            version: 1,
            amount,
            before: recipe.settings.clone(),
            full,
            sliders: predictions[i].sliders.clone(),
        };
        let target = meta.settings_at(amount)?;
        let group = recipe.history.add_group("Agent base edit");
        let edit = EditMeta {
            label: "Agent base edit".into(),
            author: Author::Agent {
                name: "style-profile/v1".into(),
            },
            timestamp_ms,
            group: Some(group),
            rationale: Some(
                meta.sliders
                    .iter()
                    .map(|s| s.rationale.as_str())
                    .collect::<Vec<_>>()
                    .join("; "),
            ),
        };
        if recipe.edit(edit.clone(), |s| *s = target)?.is_none() {
            // Preserve provenance even for neutral/zero-amount predictions, as Console does.
            let id = HistoryEntryId(recipe.history.entries.len() as u64 + 1);
            recipe.history.entries.push(HistoryEntry {
                id,
                parent: recipe.history.head,
                meta: edit,
                changes: vec![],
            });
            recipe.history.head = Some(id);
        }
        let extension = recipe
            .unknown
            .entry(EXTENSION.into())
            .or_insert_with(|| serde_json::json!({}));
        let map = extension
            .as_object_mut()
            .ok_or_else(|| EngineError::invalid("group", "invalid amount extension"))?;
        map.insert(group.0.to_string(), serde_json::to_value(&meta)?);
        recipe.image_id = Some(images[i].image);
        recipe.validate()?;
        queue.push(ReviewEntry {
            image: images[i].image,
            group,
            confidence: meta.sliders.iter().map(|s| s.confidence).fold(1., f64::min),
            sliders: meta.sliders,
        });
    }
    queue.sort_by(|a, b| {
        a.confidence
            .total_cmp(&b.confidence)
            .then(a.image.cmp(&b.image))
    });
    recipes.clone_from_slice(&next);
    Ok(queue)
}
/// Host implementations must serialize writes with interactive edits and invalidate
/// their recipe-hash/render caches after commit. Expected includes history, not only pixels.
pub trait RecipeStore: Send {
    fn load(&mut self, image: ImageId) -> EngineResult<Recipe>;
    fn commit(
        &mut self,
        image: ImageId,
        expected: &Recipe,
        next: &Recipe,
        timestamp_ms: i64,
    ) -> EngineResult<()>;
}
pub struct BatchJob<S: RecipeStore> {
    profile: Profile,
    images: Vec<BatchImage>,
    store: S,
    amount: f64,
    timestamp_ms: i64,
    output: Sender<Vec<ReviewEntry>>,
}
impl<S: RecipeStore> BatchJob<S> {
    pub fn new(
        profile: Profile,
        images: Vec<BatchImage>,
        store: S,
        amount: f64,
        timestamp_ms: i64,
    ) -> (Self, Receiver<Vec<ReviewEntry>>) {
        let (output, rx) = mpsc::channel();
        (
            Self {
                profile,
                images,
                store,
                amount,
                timestamp_ms,
                output,
            },
            rx,
        )
    }
}
impl<S: RecipeStore> Job for BatchJob<S> {
    fn label(&self) -> &str {
        "Agent base edit"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        ctx.check_cancelled()?;
        let mut recipes = Vec::new();
        for image in &self.images {
            ctx.check_cancelled()?;
            recipes.push(self.store.load(image.image)?);
        }
        let before = recipes.clone();
        let queue = apply_batch(
            &self.profile,
            &self.images,
            &mut recipes,
            self.amount,
            self.timestamp_ms,
        )?;
        let mut committed = HashSet::new();
        for (i, image) in self.images.iter().enumerate() {
            let result = ctx.check_cancelled().and_then(|()| {
                self.store
                    .commit(image.image, &before[i], &recipes[i], self.timestamp_ms)
            });
            if let Err(e) = result {
                let _ = self.output.send(
                    queue
                        .into_iter()
                        .filter(|r| committed.contains(&r.image))
                        .collect(),
                );
                return Err(e);
            }
            committed.insert(image.image);
            ctx.report_progress(
                (i + 1) as f32 / self.images.len() as f32,
                Some("Agent base edit"),
            );
        }
        let _ = self.output.send(queue);
        Ok(())
    }
}
