use crate::*;
use std::{collections::HashSet, path::PathBuf};
#[derive(Debug, Clone)]
pub struct BatchInput {
    pub path: PathBuf,
    /// Same scene/sequence key used by style-profile's burst consensus.
    pub burst: Option<String>,
    pub people: Vec<String>,
}
impl Agent {
    /// Pin grouped tone/WB to style-profile consensus, including across revisions.
    /// Disagreement is surfaced in rationales instead of silently drifting a shoot.
    pub fn edit_batch(
        &mut self,
        inputs: &[BatchInput],
        mut planner: Option<&mut dyn Planner>,
        redo: Option<&str>,
        dry_run: bool,
    ) -> Result<Vec<Report>> {
        let mut images = Vec::new();
        let mut recipes = Vec::new();
        let mut seen = HashSet::new();
        for input in inputs {
            let packet = self.perceive(&input.path)?;
            ensure!(seen.insert(packet.image), "duplicate batch image");
            images.push(style_profile::BatchImage {
                image: packet.image,
                features: packet.features,
                burst: input.burst.clone(),
                people: input.people.clone(),
            });
            // Consensus is an absolute shared look; independent existing edits remain
            // untouched outside the explicitly pinned tone/WB controls.
            recipes.push(engine_api::recipe::Recipe::default());
        }
        let review = style_profile::apply_batch(&self.profile, &images, &mut recipes, 1., now())?;
        let mut reports = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            let shared_scene = input.burst.as_ref().is_some_and(|key| {
                inputs
                    .iter()
                    .filter(|item| item.burst.as_ref() == Some(key))
                    .count()
                    > 1
            });
            let shared_person = input.people.iter().any(|key| {
                inputs
                    .iter()
                    .filter(|item| item.people.contains(key))
                    .count()
                    > 1
            });
            let mut wrapper = ConsistentPlanner {
                inner: match planner.as_mut() {
                    Some(p) => Some(&mut **p),
                    None => None,
                },
                target: recipes[i].settings.clone(),
                scene: shared_scene,
                person: shared_person,
                redo: redo.map(str::to_owned),
            };
            let mut report = self.edit(&input.path, Some(&mut wrapper), redo, dry_run)?;
            if let Some(entry) = review.iter().find(|r| r.image == report.image) {
                report.confidence = report.confidence.min(entry.confidence);
            }
            reports.push(report);
        }
        reports.sort_by(|a, b| {
            a.confidence
                .total_cmp(&b.confidence)
                .then(a.image.cmp(&b.image))
        });
        Ok(reports)
    }
}
struct ConsistentPlanner<'a> {
    inner: Option<&'a mut dyn Planner>,
    target: DevelopSettings,
    scene: bool,
    person: bool,
    redo: Option<String>,
}
impl Planner for ConsistentPlanner<'_> {
    fn plan(&mut self, context: &Value, budget: Duration) -> Result<Vec<ToolRequest>> {
        let mut context = context.clone();
        context["batch_consistency"] =
            json!({"scene_locked":self.scene,"person_locked":self.person,"target":self.target});
        let mut plan = if let Some(p) = self.inner.as_deref_mut() {
            p.plan(&context, budget)?
        } else {
            let packet: PerceptionPacket = serde_json::from_value(context["perception"].clone())?;
            let current = serde_json::from_value(context["current_settings"].clone())?;
            style_plan(&packet, &current, self.redo.as_deref())?
        };
        if self.redo.is_some() {
            return Ok(plan);
        }
        if self.scene || self.person {
            // A preset or local skin operator could bypass the consensus. Only
            // global typed controls are permitted for grouped batch base edits.
            ensure!(
                plan.iter()
                    .all(|r| matches!(r.call, ToolCall::SetTone { .. })),
                "batch consensus permits only set_tone"
            );
            if plan.is_empty() {
                plan.push(request(
                    ToolCall::SetTone {
                        image: serde_json::from_value(context["perception"]["image"].clone())?,
                        update: ToneUpdate::default(),
                    },
                    "Apply shared style-profile consensus",
                ));
            }
            for step in &mut plan {
                if let ToolCall::SetTone { update, .. } = &mut step.call {
                    let s = &self.target;
                    if self.scene {
                        update.exposure = Some(s.tone.exposure);
                        update.contrast = Some(s.tone.contrast);
                        update.highlights = Some(s.tone.highlights);
                        update.shadows = Some(s.tone.shadows);
                        update.whites = Some(s.tone.whites);
                        update.blacks = Some(s.tone.blacks);
                        update.texture = Some(s.tone.texture);
                        update.clarity = Some(s.tone.clarity);
                        update.dehaze = Some(s.tone.dehaze);
                        update.temperature = Some(s.white_balance.temperature);
                        update.tint = Some(s.white_balance.tint);
                    }
                    if self.person {
                        update.exposure = Some(s.tone.exposure);
                        update.texture = Some(s.tone.texture);
                    }
                    step.rationale = Some(format!(
                        "{}; constrained to style-profile scene/person consensus",
                        step.rationale.as_deref().unwrap_or("Base edit")
                    ));
                }
            }
        }
        Ok(plan)
    }
    fn critique(&mut self, context: &Value, budget: Duration) -> Result<Option<Value>> {
        match self.inner.as_deref_mut() {
            Some(p) => p.critique(context, budget),
            None => Ok(None),
        }
    }
}
