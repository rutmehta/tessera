use crate::{
    Console,
    console::{meta, record_empty},
    pixels::Source,
    unsupported,
};
use engine_api::{
    EngineError, EngineResult,
    recipe::{DevelopSettings, mask::LocalAdjustment},
    tools::*,
};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

impl Console {
    pub(crate) fn edit(&mut self, request: &ToolRequest) -> EngineResult<ToolOutput> {
        let image = match request.call {
            ToolCall::CreateMask { image, .. }
            | ToolCall::AdjustMask { image, .. }
            | ToolCall::Crop { image, .. }
            | ToolCall::ApplyStyle { image, .. } => image,
            _ => return Err(EngineError::internal("non-edit dispatched to edit")),
        };
        let (path, mut doc) = self.document(image)?;
        self.check(&doc, request)?;
        let mut next = doc.recipe.settings.clone();
        let mut created = None;
        match &request.call {
            ToolCall::CreateMask {
                name,
                components,
                params,
                ..
            } => {
                let mask = doc.recipe.allocate_mask_id();
                let group = LocalAdjustment {
                    id: mask,
                    name: name.clone().unwrap_or_else(|| "Agent mask".into()),
                    components: components.clone(),
                    params: params.clone(),
                    ..Default::default()
                };
                let coverage = mask_coverage(&path, &next, &group)?;
                next.locals.adjustments.push(group);
                created = Some((mask, coverage));
            }
            ToolCall::AdjustMask {
                mask,
                params,
                add_components,
                amount,
                invert,
                enabled,
                ..
            } => {
                let group = next
                    .locals
                    .adjustments
                    .iter_mut()
                    .find(|m| m.id == *mask)
                    .ok_or_else(|| EngineError::not_found("mask", mask))?;
                if let Some(params) = params {
                    group.params = params.clone();
                }
                group.components.extend(add_components.clone());
                if let Some(v) = amount {
                    group.amount = *v;
                }
                if let Some(v) = invert {
                    group.invert = *v;
                }
                if let Some(v) = enabled {
                    group.enabled = *v;
                }
                let group = group.clone();
                mask_coverage(&path, &next, &group)?;
            }
            ToolCall::Crop { rect, angle, .. } => {
                if !rect.is_valid() || !angle.is_finite() || !(-45. ..=45.).contains(angle) {
                    return Err(EngineError::invalid(
                        "crop",
                        "ordered normalized rectangle and angle -45..=45 required",
                    ));
                }
                next.geometry.crop.rect = *rect;
                next.geometry.crop.angle = *angle;
            }
            ToolCall::ApplyStyle { style, amount, .. } => {
                if !amount.is_finite() || !(0. ..=200.).contains(amount) {
                    return Err(EngineError::invalid("amount", "must be 0..=200"));
                }
                let root = self.app.join("styles");
                let map: BTreeMap<String, String> =
                    serde_json::from_slice(&std::fs::read(root.join("index.json"))?)?;
                let relative = map
                    .get(style.as_str())
                    .ok_or_else(|| EngineError::not_found("style", style))?;
                let root = root.canonicalize()?;
                let preset = root.join(relative).canonicalize()?;
                if !preset.starts_with(&root) {
                    return Err(EngineError::invalid(
                        "style",
                        "preset path must remain within styles directory",
                    ));
                }
                let mut patch: Value = serde_json::from_slice(&std::fs::read(preset)?)?;
                if let Some(settings) = patch.get("settings") {
                    patch = settings.clone();
                }
                if !patch.is_object() {
                    return Err(EngineError::invalid(
                        "style",
                        "preset must be a settings object or an object with settings",
                    ));
                }
                let mut value = serde_json::to_value(&next)?;
                merge_style(&mut value, &patch, *amount / 100., "")?;
                next = serde_json::from_value(value)?;
            }
            _ => unreachable!(),
        }
        // Validate by running the same reference engine that will render the edit.
        // Unsupported fields/masks cannot become successful but invisible edits.
        let source = Source::open(&path)?;
        pipeline_cpu::render_scaled(&next, &source.borrowed(), 16)?;
        let edit_meta = meta(request);
        let entry = doc
            .recipe
            .edit(edit_meta.clone(), |settings| *settings = next)?
            .unwrap_or_else(|| record_empty(&mut doc, edit_meta));
        self.save(&path, &mut doc)?;
        let recipe = doc.recipe.recipe_hash();
        Ok(match created {
            Some((mask, coverage)) => ToolOutput::MaskCreated {
                image,
                mask,
                coverage,
                entry,
                recipe,
            },
            None => ToolOutput::Edited {
                image,
                entry: Some(entry),
                recipe,
            },
        })
    }
    pub(crate) fn selection(&mut self, request: &ToolRequest) -> EngineResult<ToolOutput> {
        let ToolCall::SetSelection {
            images,
            decision,
            grade,
            mark,
        } = &request.call
        else {
            return Err(EngineError::internal(
                "non-selection dispatched to selection",
            ));
        };
        if images.is_empty() {
            return Err(EngineError::invalid("images", "must not be empty"));
        }
        let mut pending = Vec::new();
        for image in images.iter().copied().collect::<BTreeSet<_>>() {
            let (path, mut doc) = self.document(image)?;
            self.check(&doc, request)?;
            crate::catalog::writable(&path)?;
            if let Some(v) = decision {
                doc.recipe.selection.set_decision(*v);
            }
            if let Some(v) = grade {
                doc.recipe.selection.set_grade(match v {
                    FieldUpdate::Set(g) => Some(*g),
                    FieldUpdate::Clear => None,
                });
            }
            if let Some(v) = mark {
                doc.recipe.selection.mark = match v {
                    FieldUpdate::Set(m) => Some(m.clone()),
                    FieldUpdate::Clear => None,
                };
            }
            record_empty(&mut doc, meta(request));
            pending.push((image, path, doc));
        }
        let changed = pending.len() as u32;
        for (image, path, mut doc) in pending {
            self.save(&path, &mut doc)?;
            self.index.set_selection(image, &doc.recipe.selection)?;
        }
        Ok(ToolOutput::SelectionUpdated { changed })
    }
}
fn mask_coverage(
    path: &std::path::Path,
    settings: &DevelopSettings,
    group: &LocalAdjustment,
) -> EngineResult<f32> {
    if group.components.iter().any(|c| c.kind.is_ai()) {
        return Err(unsupported(
            "AI mask components require inference/cache inputs absent from ToolCall; procedural masks are supported",
        ));
    }
    if !group.amount.is_finite() || !(0. ..=200.).contains(&group.amount) {
        return Err(EngineError::invalid("amount", "must be 0..=200"));
    }
    let source = Source::open(path)?;
    let mut base = settings.clone();
    base.locals.adjustments.clear();
    base.geometry = Default::default();
    let linear = pipeline_cpu::render_linear_scaled(&base, &source.borrowed(), 8)?;
    let mask = pipeline_cpu::masks::rasterize(&linear, group, Default::default())?;
    Ok(mask.iter().sum::<f32>() / mask.len().max(1) as f32)
}
fn merge_style(target: &mut Value, patch: &Value, amount: f32, path: &str) -> EngineResult<()> {
    match (target, patch) {
        (Value::Object(target), Value::Object(patch)) => {
            for (key, value) in patch {
                let path = format!("{path}/{key}");
                let slot = target.get_mut(key).ok_or_else(|| {
                    EngineError::invalid("style", format!("unknown engine field {path}"))
                })?;
                merge_style(slot, value, amount, &path)?;
            }
        }
        (target, patch) if target.is_number() && patch.is_number() => {
            if amount == 0. {
                return Ok(());
            }
            if amount == 1. {
                *target = patch.clone();
            } else if target.as_i64().is_some() || target.as_u64().is_some() {
                return Err(unsupported(format!(
                    "fractional style amount for discrete field {path}"
                )));
            } else {
                let a = target
                    .as_f64()
                    .ok_or_else(|| EngineError::invalid("style", "invalid number"))?;
                let b = patch
                    .as_f64()
                    .ok_or_else(|| EngineError::invalid("style", "invalid number"))?;
                *target = serde_json::json!(a + (b - a) * f64::from(amount));
            }
        }
        (target, patch) => {
            if amount == 1. {
                *target = patch.clone();
            } else if amount != 0. && target != patch {
                return Err(unsupported(format!(
                    "fractional style amount for nonnumeric field {path}"
                )));
            }
        }
    }
    Ok(())
}
