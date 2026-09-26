//! Bounded, non-generative editing through the same Console as MCP.
mod batch;
pub mod metrics;
pub mod providers;
use anyhow::{Context, Result, bail, ensure};
use base64::Engine;
pub use batch::BatchInput;
use engine_api::{
    id::{HistoryGroupId, ImageId},
    recipe::DevelopSettings,
    tools::*,
};
use providers::Planner;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sidecar::{RecipeDocument, Sidecar};
use std::{
    path::Path,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use style_profile::{Features, Profile, SliderPrediction};
use tessera_mcp::Console;

#[derive(Clone, Debug)]
pub struct Config {
    pub max_iterations: usize,
    pub time_budget: Duration,
    pub target_luminance: f64,
    pub luminance_tolerance: f64,
    pub max_noise: f64,
    pub skin_band: Option<metrics::SkinBand>,
    pub allow_masks: bool,
    pub allow_crop: bool,
    /// Retouch remains unavailable until Console implements a reversible operator.
    pub allow_skin_retouch: bool,
    pub visual_critic: bool,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            max_iterations: 3,
            time_budget: Duration::from_secs(60),
            target_luminance: 0.18,
            luminance_tolerance: 0.25,
            max_noise: 0.8,
            skin_band: None,
            allow_masks: true,
            allow_crop: true,
            allow_skin_retouch: false,
            visual_critic: false,
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PerceptionPacket {
    pub image: ImageId,
    pub scene_labels: Vec<String>,
    pub scene_source: String,
    pub faces: Vec<FaceScore>,
    pub quality: Scores,
    /// None means unavailable, not a negative inference.
    pub depth_available: Option<bool>,
    pub histogram: Histogram,
    pub metrics: metrics::Metrics,
    pub exif: Value,
    pub features: Features,
    pub style_settings: DevelopSettings,
    pub style_rationale: Vec<SliderPrediction>,
    pub preview_jpeg_base64: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Critique {
    pub metrics: metrics::Metrics,
    pub accepted: bool,
    pub confidence: f64,
    pub reasons: Vec<String>,
    pub visual: Option<Value>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Report {
    pub image: ImageId,
    pub plans: Vec<Vec<ToolRequest>>,
    pub critiques: Vec<Critique>,
    pub accepted: bool,
    pub confidence: f64,
    pub dry_run: bool,
    pub stop_reason: String,
}
/// Optional cached perception owned by the host, never inferred identities.
#[derive(Clone, Debug, Default)]
pub struct PerceptionHints {
    pub nearest_caption: Option<String>,
    /// Normalized source-coordinate boxes, with persistent identities if known.
    pub faces: Option<Vec<FaceScore>>,
    pub depth_available: Option<bool>,
    pub embedding: Option<(String, Vec<f32>)>,
}
pub struct Agent {
    console: Console,
    profile: Profile,
    pub config: Config,
    pub perception_hints: std::collections::BTreeMap<ImageId, PerceptionHints>,
}
impl Agent {
    pub fn open(app: impl AsRef<Path>, profile: Profile, config: Config) -> Result<Self> {
        ensure!(
            config.max_iterations > 0 && config.max_iterations <= 20,
            "iterations must be 1..20"
        );
        ensure!(
            !config.time_budget.is_zero(),
            "time budget must be positive"
        );
        ensure!(
            (0. ..=1.).contains(&config.target_luminance)
                && (0. ..=1.).contains(&config.luminance_tolerance)
                && (0. ..=1.).contains(&config.max_noise),
            "invalid critic thresholds"
        );
        Ok(Self {
            console: Console::open(app)?,
            profile,
            config,
            perception_hints: Default::default(),
        })
    }
    pub fn perceive(&mut self, path: &Path) -> Result<PerceptionPacket> {
        let image = self.console.open_image(path)?;
        let mut description = self.console.describe_image(image)?;
        if let Ok(exif) = exif::Reader::new()
            .read_from_container(&mut std::io::BufReader::new(std::fs::File::open(path)?))
        {
            let mut fields = serde_json::Map::new();
            for field in exif.fields() {
                fields.insert(
                    field.tag.to_string(),
                    json!(field.display_value().with_unit(&exif).to_string()),
                );
            }
            description["embedded"] = Value::Object(fields);
        }
        let mut quality: Scores = serde_json::from_value(description["quality"].clone())?;
        let hints = self
            .perception_hints
            .get(&image)
            .cloned()
            .unwrap_or_default();
        if let Some(faces) = &hints.faces {
            ensure!(
                faces.iter().all(|f| f.region.is_valid()),
                "invalid cached face box"
            );
            quality.faces = faces.clone();
        }
        let rgb = self.console.render_preview(image, 512)?;
        let measured = metrics::measure(&rgb, &quality.faces, self.config.skin_band.as_ref())?;
        let histogram = match self.console.execute(request(
            ToolCall::GetHistogram {
                image,
                space: HistogramSpace::Display,
                bins: 64,
            },
            "Measure current preview",
        )) {
            ToolResponse::Ok(ToolOutput::Histogram { histogram, .. }) => histogram,
            ToolResponse::Error(e) => return Err(e.into()),
            _ => bail!("unexpected histogram response"),
        };
        // Style features are measured from the source, never the edited preview.
        let source = self.console.source_preview(image)?;
        let mut features = crate::source::features(&source, &quality.faces, &description)?;
        if let Some((model, embedding)) = hints.embedding {
            features.embedding_model = model;
            features.embedding = embedding;
            features.validate()?;
        }
        let prediction = self.profile.predict(&features)?;
        let mut scene_labels = vec![
            if measured.mean_luminance < 0.1 {
                "low-key"
            } else if measured.mean_luminance > 0.6 {
                "high-key"
            } else {
                "midtone scene"
            }
            .into(),
        ];
        if measured.percentiles[2] - measured.percentiles[0] > 0.5 {
            scene_labels.push("wide tonal range".into());
        }
        let scene_source =
            if let Some(caption) = hints.nearest_caption.filter(|s| !s.trim().is_empty()) {
                scene_labels = vec![caption];
                "cached embedding nearest-caption"
            } else {
                "histogram placeholder; no nearest-caption cache available"
            };
        Ok(PerceptionPacket {
            image,
            scene_labels,
            scene_source: scene_source.into(),
            faces: quality.faces.clone(),
            quality,
            depth_available: hints.depth_available,
            histogram,
            metrics: measured,
            exif: description,
            features,
            style_settings: prediction.settings,
            style_rationale: prediction.sliders,
            preview_jpeg_base64: jpeg(&rgb)?,
        })
    }
    pub fn edit(
        &mut self,
        path: &Path,
        mut planner: Option<&mut dyn Planner>,
        redo: Option<&str>,
        dry_run: bool,
    ) -> Result<Report> {
        let start = Instant::now();
        let mut packet = self.perceive(path)?;
        let mut doc = document(path)?;
        ensure!(
            doc.recipe.image_id.is_none_or(|id| id == packet.image),
            "sidecar image identity mismatch"
        );
        let group = HistoryGroupId(
            doc.recipe
                .history
                .groups
                .iter()
                .map(|g| g.id.0)
                .max()
                .unwrap_or(0)
                + 1,
        );
        let mut report = Report {
            image: packet.image,
            plans: vec![],
            critiques: vec![],
            accepted: false,
            confidence: 0.,
            dry_run,
            stop_reason: "iteration limit".into(),
        };
        let scope = redo.map(redo_scope).transpose()?;
        for iteration in 0..self.config.max_iterations {
            let remaining = self.config.time_budget.saturating_sub(start.elapsed());
            if remaining.is_zero() {
                report.stop_reason = "time budget".into();
                break;
            }
            let context = json!({"perception":packet,"current_settings":doc.recipe.settings,"iteration":iteration,"critic":report.critiques.last(),"redo":redo,"allowed_tone_fields":scope,"constraints":{"no_generative_pixels":true,"no_body_reshaping":true,"retouch":self.config.allow_skin_retouch,"max_highlight_clipping":0.05}});
            let mut plan = if let Some(p) = planner.as_deref_mut() {
                p.plan(&context, remaining)?
            } else {
                style_plan(&packet, &doc.recipe.settings, redo)?
            };
            ensure!(plan.len() <= 32, "plan exceeds 32 steps");
            for step in &mut plan {
                self.guard(step, packet.image, scope.as_deref())?;
                step.group = Some(group);
            }
            if start.elapsed() >= self.config.time_budget {
                report.stop_reason = "time budget".into();
                break;
            }
            report.plans.push(plan.clone());
            if dry_run {
                report.stop_reason = "dry run; not executed or accepted".into();
                break;
            }
            for mut step in plan {
                if start.elapsed() >= self.config.time_budget {
                    report.stop_reason = "time budget".into();
                    break;
                }
                step.expect_recipe = Some(doc.recipe.recipe_hash());
                match self.console.execute(step) {
                    ToolResponse::Error(e) => return Err(e.into()),
                    ToolResponse::Ok(_) => {}
                }
                doc = document(path)?;
                // Persist provenance before rendering or another network call can fail.
                doc.recipe.unknown.insert("tessera_agent_v1".into(), json!({"version":1,"provenance":"AI-assisted, non-generative edits","group":group,"status":"in_progress","report":report}));
                doc.record_write("agent", now())?;
                Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc)?;
                let rgb = self.console.render_preview(packet.image, 512)?;
                packet.metrics =
                    metrics::measure(&rgb, &packet.faces, self.config.skin_band.as_ref())?;
                packet.preview_jpeg_base64 = jpeg(&rgb)?;
            }
            let mut critique = self.critique(&packet.metrics);
            if self.config.visual_critic
                && start.elapsed() < self.config.time_budget
                && let Some(p) = planner.as_deref_mut()
            {
                critique.visual = p.critique(
                    &json!({"perception":packet,"objective":critique,"instruction":redo}),
                    self.config.time_budget.saturating_sub(start.elapsed()),
                )?;
                if let Some(v) = &critique.visual {
                    let accepted = v["accepted"]
                        .as_bool()
                        .context("visual critique missing accepted")?;
                    let confidence = v["confidence"]
                        .as_f64()
                        .filter(|v| (0. ..=1.).contains(v))
                        .context("invalid visual confidence")?;
                    critique.accepted &= accepted;
                    critique.confidence = critique.confidence.min(confidence);
                }
            }
            report.accepted = critique.accepted;
            report.confidence = critique.confidence;
            report.critiques.push(critique);
            if start.elapsed() >= self.config.time_budget {
                report.accepted = false;
                report.confidence = 0.;
                report.stop_reason = "time budget".into();
                break;
            }
            if report.accepted {
                report.stop_reason = "accepted".into();
                break;
            }
            if planner.is_none() {
                report.stop_reason = "style-profile only; needs review".into();
                break;
            }
        }
        if !dry_run && Sidecar::paths(path).recipe.exists() {
            doc = document(path)?;
            doc.recipe.unknown.insert("tessera_agent_v1".into(),json!({"version":1,"provenance":"AI-assisted, non-generative edits","group":group,"report":report}));
            doc.record_write("agent", now())?;
            Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc)?;
        }
        Ok(report)
    }
    fn guard(&self, request: &ToolRequest, image: ImageId, scope: Option<&[String]>) -> Result<()> {
        ensure!(
            request
                .rationale
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty()),
            "each step requires a rationale"
        );
        let target = match &request.call {
            ToolCall::SetTone { image, update } => {
                if let Some(scope) = scope {
                    for (field, value) in serde_json::to_value(update)?.as_object().unwrap() {
                        ensure!(
                            value.is_null() || scope.contains(field),
                            "redo cannot change {field}"
                        );
                    }
                }
                *image
            }
            ToolCall::CreateMask { image, .. } | ToolCall::AdjustMask { image, .. }
                if self.config.allow_masks && scope.is_none() =>
            {
                *image
            }
            ToolCall::Crop { image, .. } if self.config.allow_crop && scope.is_none() => *image,
            ToolCall::RetouchSkin { image, .. }
                if self.config.allow_skin_retouch && scope.is_none() =>
            {
                *image
            }
            _ => bail!("guardrail blocked tool {}", request.call.name()),
        };
        ensure!(target == image, "cross-image tool blocked");
        Ok(())
    }
    fn critique(&self, m: &metrics::Metrics) -> Critique {
        let mut reasons = Vec::new();
        if m.highlight_clipping > 0.05 {
            reasons.push("highlight clipping exceeds 5%".into());
        }
        if (m.mean_luminance - self.config.target_luminance).abs() > self.config.luminance_tolerance
        {
            reasons.push("mean luminance outside target tolerance".into());
        }
        if m.noise > self.config.max_noise {
            reasons.push("noise exceeds allowance".into());
        }
        if let (Some(band), Some(deltas)) = (&self.config.skin_band, &m.skin_delta_e)
            && deltas.iter().any(|d| *d > band.max_delta_e)
        {
            reasons.push("skin Lab distance outside configured band".into());
        }
        let accepted = reasons.is_empty();
        Critique {
            metrics: m.clone(),
            accepted,
            confidence: if accepted {
                (0.9 - (m.mean_luminance - self.config.target_luminance).abs() - m.noise * 0.2)
                    .clamp(0., 1.)
            } else {
                0.1
            },
            reasons,
            visual: None,
        }
    }
}
fn jpeg(rgb: &image::RgbImage) -> Result<String> {
    let mut bytes = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 75).encode_image(rgb)?;
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}
pub fn document(path: &Path) -> Result<RecipeDocument> {
    let paths = Sidecar::paths(path);
    if paths.recipe.try_exists()? {
        return Ok(Sidecar::read_recipe(paths.recipe)?);
    }
    let xmp = if paths.xmp.try_exists()? {
        paths.xmp
    } else {
        path.with_extension("xmp")
    };
    if xmp.try_exists()? {
        return Ok(RecipeDocument {
            recipe: Sidecar::read_xmp(xmp)?.to_recipe()?.recipe,
            ..Default::default()
        });
    }
    Ok(RecipeDocument::default())
}
fn request(call: ToolCall, rationale: &str) -> ToolRequest {
    ToolRequest {
        call,
        rationale: Some(rationale.into()),
        group: None,
        expect_recipe: None,
    }
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn redo_scope(instruction: &str) -> Result<Vec<String>> {
    let text = instruction.to_lowercase();
    if ["warm", "cool", "white balance", "wb", "tint"]
        .iter()
        .any(|s| text.contains(s))
    {
        return Ok(vec!["temperature".into(), "tint".into()]);
    }
    if ["bright", "dark", "exposure"]
        .iter()
        .any(|s| text.contains(s))
    {
        return Ok(vec!["exposure".into()]);
    }
    if text.contains("contrast") {
        return Ok(vec!["contrast".into()]);
    }
    bail!("redo intent cannot be safely scoped; specify warmth, tint, exposure or contrast")
}
fn style_plan(
    packet: &PerceptionPacket,
    current: &DevelopSettings,
    redo: Option<&str>,
) -> Result<Vec<ToolRequest>> {
    if let Some(instruction) = redo {
        let text = instruction.to_lowercase();
        let scope = redo_scope(instruction)?;
        let update = if scope.contains(&"temperature".into())
            && (text.contains("warm") || text.contains("cool"))
        {
            ToneUpdate {
                temperature: Some(
                    (current.white_balance.temperature
                        + if text.contains("warm") { 300. } else { -300. })
                    .clamp(2000., 50000.),
                ),
                ..Default::default()
            }
        } else {
            bail!("this redo needs a configured planner");
        };
        return Ok(vec![request(
            ToolCall::SetTone {
                image: packet.image,
                update,
            },
            instruction,
        )]);
    }
    let s = &packet.style_settings;
    let update = ToneUpdate {
        exposure: Some(current.tone.exposure + s.tone.exposure),
        contrast: Some(current.tone.contrast + s.tone.contrast),
        highlights: Some(current.tone.highlights + s.tone.highlights),
        shadows: Some(current.tone.shadows + s.tone.shadows),
        whites: Some(current.tone.whites + s.tone.whites),
        blacks: Some(current.tone.blacks + s.tone.blacks),
        texture: Some(current.tone.texture + s.tone.texture),
        clarity: Some(current.tone.clarity + s.tone.clarity),
        dehaze: Some(current.tone.dehaze + s.tone.dehaze),
        vibrance: Some(current.color.vibrance + s.color.vibrance),
        saturation: Some(current.color.saturation + s.color.saturation),
        temperature: if s.white_balance.temperature
            != DevelopSettings::default().white_balance.temperature
        {
            Some(
                current.white_balance.temperature + s.white_balance.temperature
                    - DevelopSettings::default().white_balance.temperature,
            )
        } else {
            None
        },
        tint: if s.white_balance.tint != 0. {
            Some(current.white_balance.tint + s.white_balance.tint)
        } else {
            None
        },
    };
    Ok(vec![request(
        ToolCall::SetTone {
            image: packet.image,
            update,
        },
        &packet
            .style_rationale
            .iter()
            .map(|s| s.rationale.as_str())
            .collect::<Vec<_>>()
            .join("; "),
    )])
}
mod source;
