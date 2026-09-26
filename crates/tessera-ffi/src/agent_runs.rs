//! Agentic base edits over UniFFI (docs/10): the style profile (status,
//! questionnaire, training), agent runs on one image or a batch with a chosen
//! planner (style profile only, Anthropic, OpenAI, Ollama, or a hidden
//! deterministic `FakePlanner` script), natural-language redo, the review
//! queue's accept / revert, and the provenance summary. Every agent step is an
//! ordinary recipe step in a named history group; nothing here generates pixels.
use crate::{
    CancelFlag, Engine, Result, catalog,
    develop::{disabled_steps, record_group_amount},
    failure, now_ms, parse_id,
};
use agent::{
    Agent, BatchInput, Config,
    providers::{AnthropicMessages, FakePlanner, Ollama, OpenAiResponses, Planner},
};
use engine_api::{
    id::ImageId,
    recipe::{DevelopSettings, Recipe, history::Author},
    tools::{ToneUpdate, ToolCall, ToolRequest},
};
use serde_json::{Value, json};
use sidecar::Sidecar;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use style_profile::{Profile, Questionnaire};

/// Recipe extension written by `crates/agent` (provenance + report).
const AGENT_EXTENSION: &str = "tessera_agent_v1";

// ───────────────────────────── style profile ─────────────────────────────

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StyleQuestionnaire {
    /// Signed preferences in [-1, 1].
    pub brightness: f64,
    pub contrast: f64,
    pub warmth: f64,
    pub saturation: f64,
    /// Priority in [0, 1].
    pub skin_tone_priority: f64,
}
impl From<&Questionnaire> for StyleQuestionnaire {
    fn from(q: &Questionnaire) -> Self {
        Self {
            brightness: q.brightness,
            contrast: q.contrast,
            warmth: q.warmth,
            saturation: q.saturation,
            skin_tone_priority: q.skin_tone_priority,
        }
    }
}
impl From<StyleQuestionnaire> for Questionnaire {
    fn from(q: StyleQuestionnaire) -> Self {
        Self {
            brightness: q.brightness,
            contrast: q.contrast,
            warmth: q.warmth,
            saturation: q.saturation,
            skin_tone_priority: q.skin_tone_priority,
        }
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StyleProfileStatus {
    pub library_id: String,
    /// A saved profile exists for this library.
    pub stored: bool,
    /// Learned samples (user-confirmed edits and accepted agent edits).
    pub samples: u32,
    pub questionnaire: StyleQuestionnaire,
}

// ─────────────────────────────── agent runs ───────────────────────────────

/// Who plans the base edit. Keys are passed per run (the host keeps them in
/// the Keychain); they are never stored, logged or included in errors.
#[derive(Clone, uniffi::Enum)]
pub enum AgentProvider {
    /// The style profile's prediction alone (no network).
    StyleProfile,
    Anthropic {
        api_key: String,
        model: String,
    },
    OpenAi {
        api_key: String,
        model: String,
    },
    /// Local Ollama; `host` is an origin such as `http://localhost:11434`.
    Ollama {
        host: String,
        model: String,
        vision: bool,
    },
    /// Hidden test aid: `FakePlanner` fed a deterministic script (no key, no network).
    Scripted,
}
impl std::fmt::Debug for AgentProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}
impl AgentProvider {
    fn name(&self) -> String {
        match self {
            Self::StyleProfile => "style profile".into(),
            Self::Anthropic { model, .. } => format!("Anthropic {model}"),
            Self::OpenAi { model, .. } => format!("OpenAI {model}"),
            Self::Ollama { model, .. } => format!("Ollama {model}"),
            Self::Scripted => "scripted planner".into(),
        }
    }
    fn planner(&self) -> Result<Option<Box<dyn Planner>>> {
        // Provider errors could echo configuration; keep them generic.
        let setup = |what: &str| failure(format!("{what} planner could not be configured"));
        Ok(match self {
            Self::StyleProfile => None,
            Self::Anthropic { api_key, model } => {
                if api_key.trim().is_empty() {
                    return Err(failure("add an Anthropic API key in Settings ▸ AI"));
                }
                Some(Box::new(
                    AnthropicMessages::new(api_key.trim(), model)
                        .map_err(|_| setup("Anthropic"))?,
                ))
            }
            Self::OpenAi { api_key, model } => {
                if api_key.trim().is_empty() {
                    return Err(failure("add an OpenAI API key in Settings ▸ AI"));
                }
                Some(Box::new(
                    OpenAiResponses::new(api_key.trim(), model).map_err(|_| setup("OpenAI"))?,
                ))
            }
            Self::Ollama {
                host,
                model,
                vision,
            } => {
                let mut p = Ollama::new(model).map_err(|_| setup("Ollama"))?;
                let host = if host.contains("://") {
                    host.clone()
                } else {
                    format!("http://{host}")
                };
                p.endpoint = format!("{}/api/chat", host.trim_end_matches('/'));
                p.vision = *vision;
                Some(Box::new(p))
            }
            Self::Scripted => Some(Box::new(Scripted(FakePlanner::default()))),
        })
    }
}

/// Guardrails (docs/10 §2 "Safety/scope"). No generative pixels in any case.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentGuardrails {
    pub allow_masks: bool,
    pub allow_crop: bool,
    /// Retouch stays blocked until the engine has a reversible operator.
    pub allow_skin_retouch: bool,
    /// Ask the planner's model to judge the rendered preview as well.
    pub visual_critic: bool,
    /// Plan → render → critique rounds per image (1...20).
    pub max_iterations: u32,
    /// Per-image time budget in seconds.
    pub time_budget_seconds: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentImageInput {
    pub image_id: String,
    /// Scene / burst key: images sharing one get the same tone and white balance.
    pub burst: Option<String>,
    /// Person identities: the same person gets the same exposure and skin treatment.
    pub people: Vec<String>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct AgentRunRequest {
    pub images: Vec<AgentImageInput>,
    /// Library folder (style profile identity).
    pub library_folder: String,
    pub provider: AgentProvider,
    pub guardrails: AgentGuardrails,
    /// Natural-language redo ("warmer, keep the sky"): scoped to the controls
    /// the instruction names; a new history group per image.
    pub instruction: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentRunProgress {
    pub done: u32,
    pub total: u32,
    /// File name being edited next (empty when finished).
    pub current: String,
    pub phase: String,
}

/// Called on the running thread before each image and after the last.
#[uniffi::export(with_foreign)]
pub trait AgentRunListener: Send + Sync {
    fn on_progress(&self, progress: AgentRunProgress);
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentStep {
    /// History entry id (for the per-step toggle).
    pub entry_id: u64,
    /// Controls the step sets, e.g. "Exposure, Contrast".
    pub title: String,
    pub rationale: String,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentReviewItem {
    pub image_id: String,
    pub name: String,
    /// The history group of the agent's steps.
    pub group_id: Option<u32>,
    /// The critic accepted the result.
    pub accepted: bool,
    /// Critic confidence in [0, 1]; the review queue shows the least sure first.
    pub confidence: f64,
    pub stop_reason: String,
    pub critic_reasons: Vec<String>,
    pub steps: Vec<AgentStep>,
    /// "needs review", "accepted" or "reverted".
    pub review_status: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentRunReport {
    /// Least confident first; failures before everything else.
    pub items: Vec<AgentReviewItem>,
    pub cancelled: bool,
    pub provider: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentProvenance {
    /// "AI-assisted, non-generative edits".
    pub provenance: String,
    pub item: AgentReviewItem,
    /// Number of agent history groups (base edit + redos).
    pub runs: u32,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AgentAcceptResult {
    /// The style profile learned from this image's final settings.
    pub feedback_recorded: bool,
    pub samples: u32,
    /// Why feedback was not recorded, if it was not.
    pub note: Option<String>,
}

fn reborrow(planner: &mut Option<Box<dyn Planner>>) -> Option<&mut dyn Planner> {
    match planner {
        Some(p) => Some(p.as_mut()),
        None => None,
    }
}

/// `FakePlanner` with a deterministic script built from the planning context:
/// a three-step base edit (exposure toward mid-grey, contrast/clarity,
/// vibrance) with rationales, and scoped single-step redos.
struct Scripted(FakePlanner);
impl Planner for Scripted {
    fn plan(&mut self, context: &Value, budget: Duration) -> anyhow::Result<Vec<ToolRequest>> {
        self.0.scripted.push_back(script(context)?);
        self.0.plan(context, budget)
    }
}
fn step(image: ImageId, update: ToneUpdate, rationale: String) -> ToolRequest {
    ToolRequest {
        call: ToolCall::SetTone { image, update },
        rationale: Some(rationale),
        group: None,
        expect_recipe: None,
    }
}
fn script(context: &Value) -> anyhow::Result<Vec<ToolRequest>> {
    let image: ImageId = serde_json::from_value(context["perception"]["image"].clone())?;
    if context["iteration"].as_u64().unwrap_or(0) > 0 {
        return Ok(Vec::new());
    }
    let current: DevelopSettings = serde_json::from_value(context["current_settings"].clone())?;
    if let Some(instruction) = context["redo"].as_str() {
        let text = instruction.to_lowercase();
        let allowed: Vec<String> =
            serde_json::from_value(context["allowed_tone_fields"].clone()).unwrap_or_default();
        let mut update = ToneUpdate::default();
        let why = if allowed.iter().any(|f| f == "temperature") {
            let delta = if text.contains("cool") { -400. } else { 400. };
            update.temperature =
                Some((current.white_balance.temperature + delta).clamp(2000., 50000.));
            format!("Redo “{instruction}”: temperature {delta:+.0} K; other controls untouched")
        } else if allowed.iter().any(|f| f == "exposure") {
            let delta = if text.contains("dark") { -0.3 } else { 0.3 };
            update.exposure = Some(current.tone.exposure + delta);
            format!("Redo “{instruction}”: exposure {delta:+.2} EV; other controls untouched")
        } else if allowed.iter().any(|f| f == "contrast") {
            let delta = if text.contains("less") || text.contains("flat") {
                -15.
            } else {
                15.
            };
            update.contrast = Some(current.tone.contrast + delta);
            format!("Redo “{instruction}”: contrast {delta:+.0}; other controls untouched")
        } else {
            anyhow::bail!("the scripted planner cannot scope this instruction");
        };
        return Ok(vec![step(image, update, why)]);
    }
    let mean = context["perception"]["metrics"]["mean_luminance"]
        .as_f64()
        .filter(|m| *m > 0.)
        .unwrap_or(0.18);
    let ev = ((0.18 / mean).log2() * 0.5).clamp(-1., 1.);
    let ev = (ev * 20.).round() / 20.;
    Ok(vec![
        step(
            image,
            ToneUpdate {
                exposure: Some(current.tone.exposure + ev as f32),
                ..Default::default()
            },
            format!(
                "Exposure {ev:+.2} EV because mean luminance measured {mean:.2} against a 0.18 mid-grey target"
            ),
        ),
        step(
            image,
            ToneUpdate {
                contrast: Some(current.tone.contrast + 10.),
                clarity: Some(current.tone.clarity + 8.),
                ..Default::default()
            },
            "Contrast +10 and clarity +8 to give the flat base some shape".into(),
        ),
        step(
            image,
            ToneUpdate {
                vibrance: Some(current.color.vibrance + 12.),
                ..Default::default()
            },
            "Vibrance +12: colours read muted at a neutral base; saturation left alone".into(),
        ),
    ])
}

/// The review item for the latest agent group of `recipe` (None without one).
fn review_item(image_id: &str, path: &Path, recipe: &Recipe) -> Option<AgentReviewItem> {
    let ext = recipe.unknown.get(AGENT_EXTENSION)?;
    let group = u32::try_from(ext["group"].as_u64()?).ok()?;
    let report = &ext["report"];
    let h = &recipe.history;
    let lineage = h.lineage(h.head).ok()?;
    let off = disabled_steps(&lineage);
    let steps = h
        .entries
        .iter()
        .filter(|e| e.meta.group.map(|g| g.0) == Some(group))
        .filter(|e| {
            crate::develop::amount_of(e).is_none() && crate::develop::toggle_of(e).is_none()
        })
        .map(|e| AgentStep {
            entry_id: e.id.0,
            title: if e.changes.is_empty()
                && e.meta
                    .rationale
                    .as_deref()
                    .is_some_and(|r| r.contains("consensus"))
            {
                "Held to the shoot's shared look".into()
            } else {
                crate::develop::changes_title(&e.changes)
            },
            rationale: e.meta.rationale.clone().unwrap_or_default(),
            enabled: !off.contains(&e.id.0) && lineage.iter().any(|l| l.id == e.id),
        })
        .collect();
    let critic_reasons = report["critiques"]
        .as_array()
        .and_then(|c| c.last())
        .and_then(|c| c["reasons"].as_array())
        .map(|r| {
            r.iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    Some(AgentReviewItem {
        image_id: image_id.to_owned(),
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        group_id: Some(group),
        accepted: report["accepted"].as_bool().unwrap_or(false),
        confidence: report["confidence"].as_f64().unwrap_or(0.),
        stop_reason: report["stop_reason"].as_str().unwrap_or("").to_owned(),
        critic_reasons,
        steps,
        review_status: ext["review_status"]
            .as_str()
            .unwrap_or("needs review")
            .to_owned(),
        error: None,
    })
}

impl Engine {
    fn library_id(folder: &str) -> Result<String> {
        let canonical = Path::new(folder).canonicalize()?;
        Ok(crate::assist::library_key(Some(&canonical)))
    }
    fn profile(&self, library: &str) -> Result<(Profile, bool)> {
        Ok(match Profile::open(self.support_dir()?, library)? {
            Some(p) => (p, true),
            None => (Profile::new(library, Questionnaire::default())?, false),
        })
    }
    fn profile_status(library: &str, profile: &Profile, stored: bool) -> StyleProfileStatus {
        StyleProfileStatus {
            library_id: library.to_owned(),
            stored,
            samples: profile.sample_count() as u32,
            questionnaire: profile.questionnaire().into(),
        }
    }
    /// An agent (its own Console catalog under app support, same stable ids).
    fn agent(&self, profile: Profile, config: Config) -> Result<Agent> {
        Agent::open(self.support_dir()?.join("agent"), profile, config).map_err(failure)
    }
    fn image_path(&self, image_id: &str) -> Result<PathBuf> {
        let c = self.lock()?;
        Ok(PathBuf::from(Self::path(&c, image_id)?))
    }
    /// After a writer outside the develop session changed the recipe: the XMP
    /// (develop values + selection) and the index (recipe hash, status).
    fn resync(&self, path: &Path, id: ImageId) -> Result<()> {
        let mut c = self.lock()?;
        let doc = catalog::document(path, id)?;
        let packet = catalog::selection_packet(path, &doc)?.with_recipe(&doc.recipe)?;
        Sidecar::write_xmp(catalog::xmp_path(path), &packet)?;
        c.index.scan(
            path.parent()
                .ok_or_else(|| failure("image has no folder"))?,
            &catalog::Sidecars,
            &catalog::EmbeddedMetadata,
        )?;
        drop(c);
        Ok(())
    }
    fn update_agent_extension(
        &self,
        path: &Path,
        id: ImageId,
        change: impl FnOnce(&mut Recipe) -> Result<()>,
    ) -> Result<Recipe> {
        let mut doc = catalog::document(path, id)?;
        change(&mut doc.recipe)?;
        doc.record_write("tessera-mac", now_ms())?;
        Sidecar::write_recipe(Sidecar::paths(path).recipe, &doc)?;
        self.resync(path, id)?;
        Ok(doc.recipe)
    }
}

#[uniffi::export]
impl Engine {
    /// The library's style profile (a default, untrained one when none is saved).
    pub fn style_profile_status(&self, library_folder: String) -> Result<StyleProfileStatus> {
        let library = Self::library_id(&library_folder)?;
        let (profile, stored) = self.profile(&library)?;
        Ok(Self::profile_status(&library, &profile, stored))
    }

    /// Cold-start answers (docs/10 §2); learned samples are kept.
    pub fn set_style_questionnaire(
        &self,
        library_folder: String,
        answers: StyleQuestionnaire,
    ) -> Result<StyleProfileStatus> {
        let library = Self::library_id(&library_folder)?;
        let (mut profile, _) = self.profile(&library)?;
        profile.set_questionnaire(answers.into())?;
        profile.save(self.support_dir()?)?;
        Ok(Self::profile_status(&library, &profile, true))
    }

    /// Learns from every photo in the folder whose history has user edits
    /// (their last user-confirmed state). Blocking; progress per photo.
    pub fn train_style_profile(
        &self,
        library_folder: String,
        cancel: Arc<CancelFlag>,
        listener: Option<Arc<dyn AgentRunListener>>,
    ) -> Result<StyleProfileStatus> {
        let library = Self::library_id(&library_folder)?;
        let folder = Path::new(&library_folder).canonicalize()?;
        let (mut profile, _) = self.profile(&library)?;
        let candidates = {
            let c = self.lock()?;
            let mut rows = Vec::new();
            for id in c.index.search(&index::Query {
                folder: Some(folder.to_string_lossy().into_owned()),
                limit: usize::MAX,
                ..Default::default()
            })? {
                let path = c.index.image_info(id)?.path;
                let recipe = catalog::document(&path, id)?.recipe;
                if recipe
                    .history
                    .lineage(recipe.history.head)?
                    .iter()
                    .any(|e| matches!(e.meta.author, Author::User))
                {
                    rows.push((id, path, recipe));
                }
            }
            rows
        };
        let total = candidates.len() as u32;
        let report = |done: u32, current: String| {
            if let Some(l) = &listener {
                l.on_progress(AgentRunProgress {
                    done,
                    total,
                    current,
                    phase: "Learning your edits".into(),
                });
            }
        };
        let mut agent = self.agent(
            Profile::new(&library, Questionnaire::default())?,
            Config::default(),
        )?;
        let mut rows = Vec::new();
        for (n, (id, path, recipe)) in candidates.into_iter().enumerate() {
            if cancel.is_cancelled() {
                return Err(failure("cancelled"));
            }
            report(
                n as u32,
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
            match agent.perceive(&path) {
                Ok(packet) => rows.push((id, packet.features, recipe)),
                Err(e) => eprintln!("style profile: {} skipped: {e}", path.display()),
            }
        }
        profile.collect(rows)?;
        profile.save(self.support_dir()?)?;
        report(total, String::new());
        Ok(Self::profile_status(&library, &profile, true))
    }

    /// Runs the agent's base edit (or an instruction redo) on each image and
    /// returns the review queue. Blocking: call off the main thread; cancel
    /// between images with `cancel`. Close develop sessions on these images
    /// first (their pending saves would overwrite the agent's steps).
    pub fn run_agent(
        &self,
        request: AgentRunRequest,
        cancel: Arc<CancelFlag>,
        listener: Option<Arc<dyn AgentRunListener>>,
    ) -> Result<AgentRunReport> {
        if request.images.is_empty() {
            return Err(failure("no images to edit"));
        }
        let g = &request.guardrails;
        if !(1..=20).contains(&g.max_iterations) || g.time_budget_seconds == 0 {
            return Err(failure(
                "iterations must be 1...20 and the time budget positive",
            ));
        }
        let instruction = request
            .instruction
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty());
        let library = Self::library_id(&request.library_folder)?;
        let (profile, _) = self.profile(&library)?;
        let config = Config {
            max_iterations: g.max_iterations as usize,
            time_budget: Duration::from_secs(u64::from(g.time_budget_seconds)),
            allow_masks: g.allow_masks,
            allow_crop: g.allow_crop,
            allow_skin_retouch: g.allow_skin_retouch,
            visual_critic: g.visual_critic,
            ..Config::default()
        };
        let mut agent = self.agent(profile, config)?;
        let mut planner = request.provider.planner()?;
        let images = request
            .images
            .iter()
            .map(|i| Ok((parse_id(&i.image_id)?, self.image_path(&i.image_id)?)))
            .collect::<Result<Vec<_>>>()?;
        let total = images.len() as u32;
        let name = |p: &Path| {
            p.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned()
        };
        let progress = |done: usize, current: String, phase: &str| {
            if let Some(l) = &listener {
                l.on_progress(AgentRunProgress {
                    done: done as u32,
                    total,
                    current,
                    phase: phase.into(),
                });
            }
        };
        let finish = |id: ImageId, path: &Path| -> Result<()> {
            if let Some(instruction) = instruction {
                // Name the redo's group after its instruction.
                self.update_agent_extension(path, id, |r| {
                    if let Some(group) = r
                        .unknown
                        .get(AGENT_EXTENSION)
                        .and_then(|e| e["group"].as_u64())
                        && let Ok(group) = u32::try_from(group)
                        && let Some(g) = r.history.groups.iter_mut().find(|g| g.id.0 == group)
                    {
                        let short: String = instruction.chars().take(40).collect();
                        g.name = format!("Agent redo: {short}");
                    }
                    Ok(())
                })?;
            } else {
                self.resync(path, id)?;
            }
            Ok(())
        };
        let mut errors: Vec<(usize, String)> = Vec::new();
        let mut cancelled = false;
        let batch = images.len() > 1
            && request
                .images
                .iter()
                .any(|i| i.burst.is_some() || !i.people.is_empty());
        if batch {
            let inputs: Vec<BatchInput> = request
                .images
                .iter()
                .zip(&images)
                .map(|(i, (_, path))| BatchInput {
                    path: path.clone(),
                    burst: i.burst.clone(),
                    people: i.people.clone(),
                })
                .collect();
            progress(0, name(&images[0].1), "Planning a consistent shoot");
            let result = agent.edit_batch_with_progress(
                &inputs,
                reborrow(&mut planner),
                instruction,
                false,
                &mut |i, _report| {
                    let (id, path) = &images[i];
                    finish(*id, path).map_err(|e| anyhow::anyhow!("{e}"))?;
                    progress(
                        i + 1,
                        images.get(i + 1).map(|(_, p)| name(p)).unwrap_or_default(),
                        "Editing",
                    );
                    if cancel.is_cancelled() {
                        anyhow::bail!("cancelled");
                    }
                    Ok(())
                },
            );
            if let Err(e) = result {
                if cancel.is_cancelled() {
                    cancelled = true;
                } else {
                    errors.push((usize::MAX, e.to_string()));
                }
            }
        } else {
            for (n, (id, path)) in images.iter().enumerate() {
                if cancel.is_cancelled() {
                    cancelled = true;
                    break;
                }
                progress(
                    n,
                    name(path),
                    if instruction.is_some() {
                        "Redoing"
                    } else {
                        "Editing"
                    },
                );
                match agent.edit(path, reborrow(&mut planner), instruction, false) {
                    Ok(_) => finish(*id, path)?,
                    Err(e) => {
                        errors.push((n, e.to_string()));
                        // Steps executed before a failure are real recipe steps.
                        if Sidecar::paths(path).recipe.exists() {
                            self.resync(path, *id)?;
                        }
                    }
                }
            }
        }
        progress(images.len(), String::new(), "Finished");
        let mut items = Vec::new();
        for (n, (id, path)) in images.iter().enumerate() {
            let error = errors
                .iter()
                .find(|(i, _)| *i == n || *i == usize::MAX)
                .map(|(_, e)| e.clone());
            let recipe = catalog::document(path, *id)?.recipe;
            let mut item =
                review_item(&id.to_string(), path, &recipe).unwrap_or_else(|| AgentReviewItem {
                    image_id: id.to_string(),
                    name: name(path),
                    group_id: None,
                    accepted: false,
                    confidence: 0.,
                    stop_reason: if cancelled {
                        "cancelled".into()
                    } else {
                        String::new()
                    },
                    critic_reasons: Vec::new(),
                    steps: Vec::new(),
                    review_status: "needs review".into(),
                    error: None,
                });
            if error.is_some() {
                item.error = error;
                item.confidence = 0.;
            }
            items.push(item);
        }
        items.sort_by(|a, b| {
            b.error
                .is_some()
                .cmp(&a.error.is_some())
                .then(a.confidence.total_cmp(&b.confidence))
                .then(a.name.cmp(&b.name))
        });
        Ok(AgentRunReport {
            items,
            cancelled,
            provider: request.provider.name(),
        })
    }

    /// The latest agent run's provenance and review item, if the image has one.
    pub fn agent_provenance(&self, image_id: String) -> Result<Option<AgentProvenance>> {
        let id = parse_id(&image_id)?;
        let path = self.image_path(&image_id)?;
        let recipe = catalog::document(&path, id)?.recipe;
        Ok(
            review_item(&image_id, &path, &recipe).map(|item| AgentProvenance {
                provenance: recipe.unknown[AGENT_EXTENSION]["provenance"]
                    .as_str()
                    .unwrap_or("AI-assisted, non-generative edits")
                    .to_owned(),
                runs: recipe
                    .history
                    .groups
                    .iter()
                    .filter(|g| g.name.starts_with("Agent"))
                    .count() as u32,
                item,
            }),
        )
    }

    /// Review queue "accept": marks the agent edit accepted and teaches the
    /// library's style profile the image's final settings (accepted changes
    /// included). The recipe is never altered by accepting.
    pub fn accept_agent_edit(
        &self,
        image_id: String,
        library_folder: String,
    ) -> Result<AgentAcceptResult> {
        let id = parse_id(&image_id)?;
        let path = self.image_path(&image_id)?;
        let recipe = self.update_agent_extension(&path, id, |r| {
            let ext = r
                .unknown
                .get_mut(AGENT_EXTENSION)
                .ok_or_else(|| failure("this photo has no agent edit"))?;
            ext["review_status"] = json!("accepted");
            Ok(())
        })?;
        let library = Self::library_id(&library_folder)?;
        let (mut profile, _) = self.profile(&library)?;
        let feedback = self
            .agent(
                Profile::new(&library, Questionnaire::default())?,
                Config::default(),
            )
            .and_then(|mut a| a.perceive(&path).map_err(failure))
            .and_then(|packet| {
                Ok(profile.record_feedback(id, &packet.features, &recipe.settings)?)
            })
            .and_then(|()| Ok(profile.save(self.support_dir()?)?));
        Ok(AgentAcceptResult {
            feedback_recorded: feedback.is_ok(),
            samples: profile.sample_count() as u32,
            note: feedback.err().map(|e| e.to_string()),
        })
    }

    /// Review queue "revert": the agent group at 0 % as one undoable step
    /// (later manual edits kept); the steps stay in history.
    pub fn revert_agent_edit(&self, image_id: String, group_id: u32) -> Result<()> {
        let id = parse_id(&image_id)?;
        let path = self.image_path(&image_id)?;
        self.update_agent_extension(&path, id, |r| {
            record_group_amount(r, group_id, 0.0, now_ms())?;
            if let Some(ext) = r.unknown.get_mut(AGENT_EXTENSION) {
                ext["review_status"] = json!("reverted");
            }
            Ok(())
        })?;
        Ok(())
    }
}
