//! Keyword suggestions, captions / alt text and OCR over UniFFI (docs/09 §1,
//! §3; WP M3-15). All inference is local (`ml-caption`: SigLIP keywords,
//! Florence-2 captions and OCR) and runs as background jobs on a dedicated
//! one-worker scheduler, so a slow model never occupies the preview / viewport
//! workers. Results are cached in the index (`understanding` table, with
//! per-task provenance) and reused until the model version changes.
//!
//! Suggestions never change a photo on their own: only `accept_suggestions`
//! tags photos, through the library's keyword tree, and writes XMP only when
//! the "write suggested keywords to XMP" setting is on (otherwise the tag is
//! a local, index-only acceptance that survives rescans).
use crate::{Engine, Result, collections::LibraryStore, failure, parse_id};
use engine_api::{
    error::{EngineError, EngineResult},
    id::ImageId,
    jobs::{CancellationToken, Job, JobContext, Priority, Scheduler},
};
use image::RgbImage;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, HashSet},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicU64, Ordering},
    },
};

/// Suggestions kept per photo (ranked by confidence).
const TOP_KEYWORDS: usize = 20;
/// Long edge of the decoded preview the models see.
const INPUT_PX: u32 = 1536;
/// Images decoded per batch (bounds preview memory; SigLIP runs batched).
const BATCH: usize = 4;
/// Finished jobs kept for the status strip.
const FINISHED_KEPT: usize = 8;
const SETTINGS_FILE: &str = "ai-metadata.json";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, uniffi::Enum)]
pub enum UnderstandingTask {
    /// SigLIP keyword suggestions with confidence.
    Keywords,
    /// A one-sentence caption and a longer alt text (Florence-2).
    Caption,
    /// Text in the image (Florence-2 OCR with regions).
    Ocr,
}
impl UnderstandingTask {
    fn key(self) -> &'static str {
        match self {
            Self::Keywords => "keywords",
            Self::Caption => "caption",
            Self::Ocr => "ocr",
        }
    }
}

/// One suggested keyword, merged over the requested photos.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct KeywordSuggestionInfo {
    pub keyword: String,
    /// Highest confidence among the photos it was suggested for (0...1;
    /// independent sigmoid scores, not probabilities of exclusive labels).
    pub confidence: f32,
    /// Photos (of those requested) it is suggested for and not yet applied to.
    pub images: u32,
    /// Where accepting puts it in the keyword tree ("Places", "Beach").
    pub path: Vec<String>,
    /// The keyword (or a synonym of it) already exists in the tree at `path`;
    /// false: accepting creates it under "Suggested".
    pub existing: bool,
    /// More than one tree keyword matches by name or synonym: accepting fails
    /// until the tree is disambiguated.
    pub ambiguous: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct OcrRegionInfo {
    pub text: String,
    /// Normalized displayed-image box.
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Sequence-level decoder confidence shared by the regions of one image.
    pub confidence: f32,
}

/// Cached model output for one photo. Empty strings / lists: not generated
/// yet (see the `has_*` flags) or nothing found.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ImageUnderstandingInfo {
    pub image_id: String,
    pub caption: String,
    pub alt_text: String,
    pub ocr: Vec<OcrRegionInfo>,
    /// OCR regions joined in reading order, one per line.
    pub ocr_text: String,
    pub has_keywords: bool,
    pub has_caption: bool,
    pub has_ocr: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum UnderstandingJobState {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct UnderstandingJobInfo {
    pub id: u64,
    pub tasks: Vec<UnderstandingTask>,
    pub state: UnderstandingJobState,
    /// Photos finished (or failed) of `total`. Photos whose results were
    /// already cached for the current models are not counted.
    pub done: u32,
    pub total: u32,
    pub failed: u32,
    /// File name being analysed.
    pub current: String,
    /// Why the job failed, or the first per-photo error.
    pub error: Option<String>,
}

/// Settings ▸ AI ▸ Keywords and captions. Stored in the app support folder.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, uniffi::Record)]
#[serde(default)]
pub struct AiMetadataSettings {
    /// Suggest keywords for newly opened photos in the background.
    pub auto_suggest_on_import: bool,
    /// Accepted suggestions are also written to XMP sidecars (dc:subject and
    /// lr:hierarchicalSubject); off keeps them in Tessera's catalog only.
    pub write_suggested_keywords_to_xmp: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct UnderstandingModelStatus {
    /// Using the deterministic test models (`--fake-captioner`).
    pub test_models: bool,
    /// SigLIP tokenizer present (weights download on first use if missing).
    pub keywords_installed: bool,
    /// Florence tokenizer present (weights download on first use if missing).
    pub captions_installed: bool,
    /// Where tokenizers / weights are expected.
    pub cache: String,
}

// ───────────────────────────── models ─────────────────────────────

/// What the jobs need from a model; the real one is `LocalModels`, tests and
/// the `--fake-captioner` launch aid use `TestModels`.
pub(crate) trait Understander: Send {
    fn version(&self, task: UnderstandingTask) -> String;
    fn keywords(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<(String, f32)>>>;
    fn caption(&mut self, image: &RgbImage) -> anyhow::Result<ml_caption::Caption>;
    fn ocr(&mut self, image: &RgbImage) -> anyhow::Result<Vec<index::OcrRegion>>;
}

/// SigLIP + Florence, each loaded on first use from the app's model cache.
struct LocalModels {
    manifest: PathBuf,
    cache: PathBuf,
    labels: Vec<String>,
    keywords: Option<ml_caption::KeywordModel>,
    florence: Option<ml_caption::Florence>,
}
impl LocalModels {
    fn registry(&self) -> anyhow::Result<ml_runtime::ModelRegistry> {
        ml_runtime::ModelRegistry::open(&self.manifest, &self.cache)
    }
    fn tokenizer(&self, file: &str, script: &str) -> anyhow::Result<PathBuf> {
        let path = self.cache.join(file);
        anyhow::ensure!(
            path.is_file(),
            "model not installed: run python3 tools/{script} --cache \"{}\"",
            self.cache.display()
        );
        Ok(path)
    }
    fn keyword_model(&mut self) -> anyhow::Result<&mut ml_caption::KeywordModel> {
        if self.keywords.is_none() {
            let tokenizer = self.tokenizer("tokenizer.json", "fetch_siglip.py")?;
            let siglip = ml_embed::Siglip::load(
                &self.registry()?,
                &tokenizer,
                ml_runtime::SessionOptions::default(),
            )?;
            self.keywords = Some(ml_caption::KeywordModel::new(
                siglip,
                self.labels.clone(),
                ml_caption::Calibration::default(),
            )?);
        }
        Ok(self.keywords.as_mut().expect("loaded"))
    }
    fn florence(&mut self) -> anyhow::Result<&mut ml_caption::Florence> {
        if self.florence.is_none() {
            let tokenizer = self.tokenizer("florence-tokenizer.json", "fetch_florence.py")?;
            // CPU: the autoregressive decoder partitions poorly on CoreML
            // (tools/orchestrate/wp/M3-13/COREML.md).
            self.florence = Some(ml_caption::Florence::load(
                &self.registry()?,
                &tokenizer,
                ml_runtime::SessionOptions::cpu(),
            )?);
        }
        Ok(self.florence.as_mut().expect("loaded"))
    }
}
impl Understander for LocalModels {
    fn version(&self, task: UnderstandingTask) -> String {
        match task {
            UnderstandingTask::Keywords => {
                ml_caption::keyword_model_version(&self.labels, ml_caption::Calibration::default())
            }
            _ => ml_caption::FLORENCE_VERSION.into(),
        }
    }
    fn keywords(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<(String, f32)>>> {
        self.keyword_model()?.suggest_batch(images)
    }
    fn caption(&mut self, image: &RgbImage) -> anyhow::Result<ml_caption::Caption> {
        self.florence()?.caption(image)
    }
    fn ocr(&mut self, image: &RgbImage) -> anyhow::Result<Vec<index::OcrRegion>> {
        self.florence()?.ocr(image)
    }
}

/// Deterministic stand-in: names the dominant hue. Used by tests and the
/// hidden `--fake-captioner` launch aid (no weights, no network).
pub(crate) struct TestModels;
impl TestModels {
    fn hue(image: &RgbImage) -> &'static str {
        let n = (image.width() * image.height()).max(1) as f32;
        let [mut r, mut g, mut b] = [0f32; 3];
        for p in image.pixels() {
            r += f32::from(p[0]);
            g += f32::from(p[1]);
            b += f32::from(p[2]);
        }
        let (r, g, b) = (r / n / 255., g / n / 255., b / n / 255.);
        let (max, min) = (r.max(g).max(b), r.min(g).min(b));
        if max - min < 0.08 {
            return if max > 0.85 { "white" } else { "grey" };
        }
        let d = max - min;
        let h = if max == r {
            ((g - b) / d).rem_euclid(6.)
        } else if max == g {
            (b - r) / d + 2.
        } else {
            (r - g) / d + 4.
        } * 60.;
        match h {
            h if h < 15. => "red",
            h if h < 45. => "orange",
            h if h < 70. => "yellow",
            h if h < 160. => "green",
            h if h < 200. => "teal",
            h if h < 255. => "blue",
            h if h < 290. => "purple",
            h if h < 340. => "pink",
            _ => "red",
        }
    }
}
impl Understander for TestModels {
    fn version(&self, task: UnderstandingTask) -> String {
        format!("test-understanding-v1/{}", task.key())
    }
    fn keywords(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<(String, f32)>>> {
        Ok(images
            .iter()
            .map(|image| {
                vec![
                    ("photograph".into(), 0.93),
                    (Self::hue(image).into(), 0.81),
                    ("gradient".into(), 0.62),
                    ("abstract".into(), 0.41),
                    ("texture".into(), 0.22),
                ]
            })
            .collect())
    }
    fn caption(&mut self, image: &RgbImage) -> anyhow::Result<ml_caption::Caption> {
        let hue = Self::hue(image);
        Ok(ml_caption::Caption {
            caption: format!("A {hue} gradient photograph."),
            alt_text: format!("An abstract image of smooth {hue} tones with soft light."),
        })
    }
    fn ocr(&mut self, image: &RgbImage) -> anyhow::Result<Vec<index::OcrRegion>> {
        Ok(vec![index::OcrRegion {
            text: format!("TEST CARD {}", Self::hue(image).to_uppercase()),
            bbox: [0.1, 0.1, 0.6, 0.2],
            confidence: 0.9,
        }])
    }
}

// ───────────────────────────── jobs ─────────────────────────────

struct Progress {
    state: UnderstandingJobState,
    done: u32,
    total: u32,
    failed: u32,
    current: String,
    error: Option<String>,
}
struct JobRecord {
    id: u64,
    tasks: Vec<UnderstandingTask>,
    token: OnceLock<CancellationToken>,
    progress: Mutex<Progress>,
}
impl JobRecord {
    fn update(&self, f: impl FnOnce(&mut Progress)) {
        f(&mut self.progress.lock().unwrap_or_else(|e| e.into_inner()));
    }
    fn info(&self) -> UnderstandingJobInfo {
        let p = self.progress.lock().unwrap_or_else(|e| e.into_inner());
        let cancelled = self.token.get().is_some_and(|t| t.is_cancelled());
        UnderstandingJobInfo {
            id: self.id,
            tasks: self.tasks.clone(),
            // A job cancelled while queued never runs, so never reports.
            state: if cancelled && p.state == UnderstandingJobState::Queued {
                UnderstandingJobState::Cancelled
            } else {
                p.state
            },
            done: p.done,
            total: p.total,
            failed: p.failed,
            current: p.current.clone(),
            error: p.error.clone(),
        }
    }
    fn finished(&self) -> bool {
        !matches!(
            self.info().state,
            UnderstandingJobState::Queued | UnderstandingJobState::Running
        )
    }
}

type ModelSlot = Arc<Mutex<Option<Box<dyn Understander>>>>;

/// Engine-wide state: the loaded models, the job list and a one-worker
/// scheduler (jobs run one after another, in submission order).
#[derive(Default)]
pub(crate) struct UnderstandingState {
    model: ModelSlot,
    scheduler: OnceLock<jobs::ThreadPoolScheduler>,
    jobs: Mutex<Vec<Arc<JobRecord>>>,
    next: AtomicU64,
}

/// Per-task model versions, stored as a JSON object in `model_version` (the
/// CLI's format). A plain string is the combined `UnderstandingJob` version.
fn versions(value: &index::Understanding) -> BTreeMap<String, String> {
    match serde_json::from_str::<Value>(&value.model_version) {
        Ok(Value::Object(map)) => map
            .into_iter()
            .filter_map(|(k, v)| v.as_str().map(|v| (k, v.to_owned())))
            .collect(),
        _ if !value.model_version.is_empty() => ["keywords", "caption", "ocr"]
            .into_iter()
            .map(|k| (k.to_owned(), value.model_version.clone()))
            .collect(),
        _ => BTreeMap::new(),
    }
}
fn set_versions(value: &mut index::Understanding, map: &BTreeMap<String, String>) {
    value.model_version = json!(map).to_string();
}
fn empty() -> index::Understanding {
    index::Understanding {
        model_version: String::new(),
        keywords: vec![],
        caption: String::new(),
        alt_text: String::new(),
        ocr: vec![],
    }
}

struct UnderstandingRun {
    engine: Weak<Engine>,
    record: Arc<JobRecord>,
    images: Vec<ImageId>,
    tasks: Vec<UnderstandingTask>,
    force: bool,
    model: ModelSlot,
}
impl UnderstandingRun {
    fn fail(&self, message: String) -> EngineError {
        self.record.update(|p| {
            p.state = UnderstandingJobState::Failed;
            p.error = Some(message.clone());
            p.current.clear();
        });
        EngineError::Model {
            model: "ml-caption".into(),
            message,
        }
    }
    fn work(&self, ctx: &JobContext) -> EngineResult<()> {
        let engine = self.engine.upgrade().ok_or(EngineError::Cancelled)?;
        let mut slot = self.model.lock().unwrap_or_else(|e| e.into_inner());
        if slot.is_none() {
            let (manifest, cache) = engine.model_paths().map_err(|e| self.fail(e.to_string()))?;
            *slot = Some(Box::new(LocalModels {
                manifest,
                cache,
                labels: ml_caption::vocabulary(),
                keywords: None,
                florence: None,
            }));
        }
        let model = slot.as_mut().expect("model slot filled");
        let wanted: Vec<(UnderstandingTask, String)> =
            self.tasks.iter().map(|&t| (t, model.version(t))).collect();
        // Plan: the tasks each photo still needs for the current models.
        let mut plan = Vec::new();
        let mut seen = HashSet::new();
        for &id in &self.images {
            ctx.check_cancelled()?;
            if !seen.insert(id) {
                continue;
            }
            let stored = engine
                .lock()
                .map_err(|e| self.fail(e.to_string()))?
                .index
                .understanding(id)
                .map_err(|e| self.fail(format!("{e:#}")))?;
            let have = stored.as_ref().map(versions).unwrap_or_default();
            let todo: Vec<_> = wanted
                .iter()
                .filter(|(t, v)| self.force || have.get(t.key()) != Some(v))
                .cloned()
                .collect();
            if !todo.is_empty() {
                plan.push((id, todo));
            }
        }
        let total = plan.len() as u32;
        self.record.update(|p| {
            p.state = UnderstandingJobState::Running;
            p.total = total;
        });
        for chunk in plan.chunks(BATCH) {
            jobs::yield_to_interactive(
                &ctx.cancellation,
                std::time::Duration::from_millis(16),
                std::time::Duration::from_millis(250),
            )?;
            // Decode; a photo that cannot be read fails alone.
            let mut frames = Vec::new();
            for (id, todo) in chunk {
                ctx.check_cancelled()?;
                let key = id.to_string();
                let name = engine
                    .lock()
                    .ok()
                    .and_then(|c| Engine::path(&c, &key).ok())
                    .map(|p| {
                        Path::new(&p)
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or(p)
                    })
                    .unwrap_or_default();
                self.record.update(|p| p.current = name.clone());
                match engine.analysis_rgb(&key, INPUT_PX) {
                    Ok(image) => frames.push((*id, todo, image, name)),
                    Err(e) => self.photo_failed(&name, &e.to_string()),
                }
            }
            let needs_keywords: Vec<usize> = frames
                .iter()
                .enumerate()
                .filter(|(_, f)| f.1.iter().any(|(t, _)| *t == UnderstandingTask::Keywords))
                .map(|(i, _)| i)
                .collect();
            ctx.check_cancelled()?;
            let mut keywords: BTreeMap<usize, Vec<(String, f32)>> = BTreeMap::new();
            if !needs_keywords.is_empty() {
                let batch: Vec<RgbImage> = needs_keywords
                    .iter()
                    .map(|&i| frames[i].2.clone())
                    .collect();
                let ranked = model
                    .keywords(&batch)
                    .map_err(|e| self.fail(format!("keyword suggestions: {e:#}")))?;
                if ranked.len() != batch.len() {
                    return Err(self.fail("keyword batch cardinality mismatch".into()));
                }
                keywords.extend(needs_keywords.into_iter().zip(ranked));
            }
            for (i, (id, todo, image, name)) in frames.iter().enumerate() {
                ctx.check_cancelled()?;
                self.record.update(|p| p.current = name.clone());
                let mut caption = None;
                let mut ocr = None;
                let mut error = None;
                for (task, _) in todo.iter() {
                    match task {
                        UnderstandingTask::Caption => match model.caption(image) {
                            Ok(c) => caption = Some(c),
                            Err(e) => error = Some(format!("caption: {e:#}")),
                        },
                        UnderstandingTask::Ocr => match model.ocr(image) {
                            Ok(r) => ocr = Some(r),
                            Err(e) => error = Some(format!("text: {e:#}")),
                        },
                        UnderstandingTask::Keywords => {}
                    }
                    ctx.check_cancelled()?;
                }
                // Merge into the stored value, keeping other tasks' output and
                // provenance (and never touching accepted keywords).
                let c = engine.lock().map_err(|e| self.fail(e.to_string()))?;
                let mut value = c
                    .index
                    .understanding(*id)
                    .map_err(|e| self.fail(format!("{e:#}")))?
                    .unwrap_or_else(empty);
                let mut map = versions(&value);
                for (task, version) in todo.iter() {
                    let produced = match task {
                        UnderstandingTask::Keywords => {
                            let ranked = keywords.remove(&i).unwrap_or_default();
                            value.keywords = ranked
                                .into_iter()
                                .take(TOP_KEYWORDS)
                                .map(|(keyword, confidence)| index::KeywordSuggestion {
                                    keyword,
                                    confidence: confidence.clamp(0., 1.),
                                })
                                .collect();
                            true
                        }
                        UnderstandingTask::Caption => caption.take().is_some_and(|c| {
                            value.caption = c.caption;
                            value.alt_text = c.alt_text;
                            true
                        }),
                        UnderstandingTask::Ocr => ocr.take().is_some_and(|r| {
                            value.ocr = r;
                            true
                        }),
                    };
                    if produced {
                        map.insert(task.key().into(), version.clone());
                    }
                }
                set_versions(&mut value, &map);
                let stored = c.index.set_understanding(*id, &value);
                drop(c);
                match (stored, error) {
                    (Err(e), _) => self.photo_failed(name, &format!("{e:#}")),
                    (Ok(()), Some(e)) => self.photo_failed(name, &e),
                    (Ok(()), None) => self.record.update(|p| p.done += 1),
                }
                let done = self.record.info().done + self.record.info().failed;
                ctx.report_progress(done as f32 / total.max(1) as f32, None);
            }
        }
        self.record.update(|p| {
            p.state = UnderstandingJobState::Succeeded;
            p.current.clear();
        });
        Ok(())
    }
    fn photo_failed(&self, name: &str, message: &str) {
        self.record.update(|p| {
            p.failed += 1;
            p.error.get_or_insert_with(|| format!("{name}: {message}"));
        });
    }
}
impl Job for UnderstandingRun {
    fn label(&self) -> &str {
        "Keywords, captions and text"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        let result = self.work(ctx);
        if matches!(result, Err(EngineError::Cancelled)) {
            self.record.update(|p| {
                p.state = UnderstandingJobState::Cancelled;
                p.current.clear();
            });
        } else if let Err(e) = &result {
            // Errors not already reported (index, decoding pipeline).
            self.record.update(|p| {
                if p.state != UnderstandingJobState::Failed {
                    p.state = UnderstandingJobState::Failed;
                    p.error = Some(e.to_string());
                }
            });
        }
        result
    }
}

// ───────────────────────────── engine ─────────────────────────────

impl Engine {
    /// Manifest (installed from the bundled registry) and weight cache, shared
    /// with the face models. `TESSERA_MODEL_CACHE` overrides the cache.
    fn model_paths(&self) -> Result<(PathBuf, PathBuf)> {
        let dir = self.support_dir()?.join("models");
        std::fs::create_dir_all(&dir)?;
        let manifest = dir.join("models.toml");
        let text = include_str!("../../ml-runtime/models.toml");
        if std::fs::read_to_string(&manifest).ok().as_deref() != Some(text) {
            std::fs::write(&manifest, text)?;
        }
        let cache = std::env::var_os("TESSERA_MODEL_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("cache"));
        std::fs::create_dir_all(&cache)?;
        Ok((manifest, cache))
    }

    fn settings_path(&self) -> Result<PathBuf> {
        Ok(self.support_dir()?.join(SETTINGS_FILE))
    }

    fn submit_understanding(
        self: &Arc<Self>,
        image_ids: &[String],
        tasks: Vec<UnderstandingTask>,
        force: bool,
    ) -> Result<u64> {
        let images = image_ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<Vec<_>>>()?;
        let mut tasks = tasks;
        tasks.sort();
        tasks.dedup();
        if tasks.is_empty() {
            return Err(failure("choose keywords, captions or text"));
        }
        let state = &self.understanding;
        let record = Arc::new(JobRecord {
            id: state.next.fetch_add(1, Ordering::Relaxed) + 1,
            tasks: tasks.clone(),
            token: OnceLock::new(),
            progress: Mutex::new(Progress {
                state: UnderstandingJobState::Queued,
                done: 0,
                total: images.len() as u32,
                failed: 0,
                current: String::new(),
                error: None,
            }),
        });
        {
            let mut list = state.jobs.lock().map_err(failure)?;
            // Keep every active job and the most recent finished ones.
            let finished: Vec<u64> = list.iter().filter(|j| j.finished()).map(|j| j.id).collect();
            let drop_n = finished.len().saturating_sub(FINISHED_KEPT - 1);
            let old: HashSet<u64> = finished.into_iter().take(drop_n).collect();
            list.retain(|j| !old.contains(&j.id));
            list.push(record.clone());
        }
        let handle = state
            .scheduler
            .get_or_init(|| jobs::ThreadPoolScheduler::new(1))
            .submit(
                Box::new(UnderstandingRun {
                    engine: Arc::downgrade(self),
                    record: record.clone(),
                    images,
                    tasks,
                    force,
                    model: state.model.clone(),
                }),
                None,
            );
        let _ = record.token.set(handle.cancellation);
        Ok(record.id)
    }

    fn understanding_value(&self, id: ImageId) -> Result<index::Understanding> {
        Ok(self
            .lock()?
            .index
            .understanding(id)
            .map_err(|e| failure(format!("{e:#}")))?
            .unwrap_or_else(empty))
    }

    /// Keywords applied to a photo in the catalog (sidecar tags and local
    /// acceptances), lowercased.
    fn applied_keywords(&self, id: &str) -> Result<HashSet<String>> {
        let c = self.lock()?;
        let mut stmt = c.reader.prepare(
            "SELECT k.name FROM image_keyword ik JOIN keyword k ON k.id=ik.keyword_id WHERE ik.image_id=?",
        )?;
        let names = stmt
            .query_map([id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(names.into_iter().map(|n| n.to_lowercase()).collect())
    }
}

#[uniffi::export]
impl Engine {
    /// Suggests keywords for the photos in the background (cached results for
    /// the current model are reused). Returns the job id.
    pub fn suggest_keywords(self: Arc<Self>, image_ids: Vec<String>) -> Result<u64> {
        self.submit_understanding(&image_ids, vec![UnderstandingTask::Keywords], false)
    }
    /// Generates a caption and alt text for the photos in the background.
    pub fn caption(self: Arc<Self>, image_ids: Vec<String>) -> Result<u64> {
        self.submit_understanding(&image_ids, vec![UnderstandingTask::Caption], false)
    }
    /// Reads text in the photos in the background (searchable afterwards).
    pub fn ocr(self: Arc<Self>, image_ids: Vec<String>) -> Result<u64> {
        self.submit_understanding(&image_ids, vec![UnderstandingTask::Ocr], false)
    }
    /// Any combination of tasks; `force` recomputes cached results.
    pub fn analyze_understanding(
        self: Arc<Self>,
        image_ids: Vec<String>,
        tasks: Vec<UnderstandingTask>,
        force: bool,
    ) -> Result<u64> {
        self.submit_understanding(&image_ids, tasks, force)
    }
    /// When "auto-suggest on import" is on, starts a keyword job for the
    /// photos (only uncached ones do work). None when the setting is off.
    pub fn auto_suggest(self: Arc<Self>, image_ids: Vec<String>) -> Result<Option<u64>> {
        if !self.ai_metadata_settings()?.auto_suggest_on_import || image_ids.is_empty() {
            return Ok(None);
        }
        self.suggest_keywords(image_ids).map(Some)
    }
    /// Active jobs and the most recently finished ones, oldest first.
    pub fn understanding_jobs(&self) -> Result<Vec<UnderstandingJobInfo>> {
        Ok(self
            .understanding
            .jobs
            .lock()
            .map_err(failure)?
            .iter()
            .map(|j| j.info())
            .collect())
    }
    /// Cancels a queued or running job (results already stored stay).
    pub fn cancel_understanding_job(&self, id: u64) -> Result<()> {
        let list = self.understanding.jobs.lock().map_err(failure)?;
        let job = list
            .iter()
            .find(|j| j.id == id)
            .ok_or_else(|| failure(format!("no such job: {id}")))?;
        if let Some(token) = job.token.get() {
            token.cancel();
        }
        Ok(())
    }
    /// Hidden test aid (`--fake-captioner`): deterministic models that name
    /// the dominant colour. Replaces any loaded model.
    pub fn use_test_understanding(&self) -> Result<()> {
        *self.understanding.model.lock().map_err(failure)? = Some(Box::new(TestModels));
        Ok(())
    }
    pub fn understanding_model_status(&self) -> Result<UnderstandingModelStatus> {
        let (_, cache) = self.model_paths()?;
        let test_models = self
            .understanding
            .model
            .try_lock()
            .map(|m| {
                m.as_ref()
                    .is_some_and(|m| m.version(UnderstandingTask::Keywords).starts_with("test-"))
            })
            .unwrap_or(false);
        Ok(UnderstandingModelStatus {
            test_models,
            keywords_installed: cache.join("tokenizer.json").is_file(),
            captions_installed: cache.join("florence-tokenizer.json").is_file(),
            cache: cache.to_string_lossy().into_owned(),
        })
    }
    /// Cached caption, alt text and OCR of one photo.
    pub fn image_understanding(&self, image_id: String) -> Result<ImageUnderstandingInfo> {
        let value = self.understanding_value(parse_id(&image_id)?)?;
        let map = versions(&value);
        let mut regions = value.ocr.clone();
        // Reading order: top to bottom, then left to right.
        regions.sort_by(|a, b| {
            (a.bbox[1], a.bbox[0])
                .partial_cmp(&(b.bbox[1], b.bbox[0]))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Ok(ImageUnderstandingInfo {
            image_id,
            has_keywords: map.contains_key("keywords"),
            has_caption: map.contains_key("caption"),
            has_ocr: map.contains_key("ocr"),
            ocr_text: regions
                .iter()
                .map(|r| r.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            ocr: regions
                .into_iter()
                .map(|r| OcrRegionInfo {
                    text: r.text,
                    x0: r.bbox[0],
                    y0: r.bbox[1],
                    x1: r.bbox[2],
                    y1: r.bbox[3],
                    confidence: r.confidence,
                })
                .collect(),
            caption: value.caption,
            alt_text: value.alt_text,
        })
    }
    pub fn ai_metadata_settings(&self) -> Result<AiMetadataSettings> {
        match std::fs::read(self.settings_path()?) {
            Ok(bytes) => serde_json::from_slice(&bytes).map_err(failure),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Default::default()),
            Err(e) => Err(e.into()),
        }
    }
    pub fn set_ai_metadata_settings(&self, settings: AiMetadataSettings) -> Result<()> {
        let path = self.settings_path()?;
        let dir = path.parent().ok_or_else(|| failure("settings folder"))?;
        let mut file = tempfile::NamedTempFile::new_in(dir)?;
        serde_json::to_writer_pretty(&mut file, &settings).map_err(failure)?;
        file.persist(&path).map_err(|e| failure(e.error))?;
        Ok(())
    }
}

// ───────────────────────────── library ─────────────────────────────

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

#[uniffi::export]
impl LibraryStore {
    /// Suggested keywords over the photos, merged by name, excluding keywords
    /// a photo already has; highest confidence first.
    pub fn suggestions(&self, image_ids: Vec<String>) -> Result<Vec<KeywordSuggestionInfo>> {
        let library = self.read()?;
        let mut merged: Vec<KeywordSuggestionInfo> = Vec::new();
        for key in &image_ids {
            let value = self.engine.understanding_value(parse_id(key)?)?;
            if value.keywords.is_empty() {
                continue;
            }
            let applied = self.engine.applied_keywords(key)?;
            for s in value.keywords {
                if applied.contains(&s.keyword.trim().to_lowercase()) {
                    continue;
                }
                if let Some(m) = merged.iter_mut().find(|m| same(&m.keyword, &s.keyword)) {
                    m.images += 1;
                    m.confidence = m.confidence.max(s.confidence);
                    continue;
                }
                let (path, existing, ambiguous) =
                    match ml_caption::map_keyword(&library, &s.keyword) {
                        Ok(m) => {
                            // A mapped keyword applied under its tree name counts
                            // as applied too (synonyms).
                            if !m.proposed
                                && m.path
                                    .last()
                                    .is_some_and(|leaf| applied.contains(&leaf.to_lowercase()))
                            {
                                continue;
                            }
                            (m.path, !m.proposed, false)
                        }
                        Err(_) => (vec![s.keyword.clone()], false, true),
                    };
                merged.push(KeywordSuggestionInfo {
                    keyword: s.keyword,
                    confidence: s.confidence,
                    images: 1,
                    path,
                    existing,
                    ambiguous,
                });
            }
        }
        merged.sort_by(|a, b| {
            b.confidence
                .total_cmp(&a.confidence)
                .then_with(|| a.keyword.cmp(&b.keyword))
        });
        Ok(merged)
    }

    /// Accepts suggestions: each keyword is applied to those of `image_ids`
    /// it was suggested for, under its mapped tree path (created under
    /// "Suggested" when new). Writes XMP only with the XMP setting on. Returns
    /// the tree paths applied ("Places|Beach").
    pub fn accept_suggestions(
        &self,
        image_ids: Vec<String>,
        keywords: Vec<String>,
    ) -> Result<Vec<String>> {
        let library = self.read()?;
        let mut plan: Vec<(Vec<String>, Vec<String>)> = Vec::new(); // (path, image keys)
        for keyword in keywords.iter().filter(|k| !k.trim().is_empty()) {
            let mapped = ml_caption::map_keyword(&library, keyword).map_err(failure)?;
            let mut targets = Vec::new();
            for key in &image_ids {
                let value = self.engine.understanding_value(parse_id(key)?)?;
                if value.keywords.iter().any(|s| same(&s.keyword, keyword)) {
                    targets.push(key.clone());
                }
            }
            if !targets.is_empty() {
                plan.push((mapped.path, targets));
            }
        }
        if plan.is_empty() {
            return Ok(vec![]);
        }
        let library = self.edit(|l| {
            for (path, _) in &plan {
                let mut parent: Option<&str> = None;
                for name in path {
                    if l.keyword_path(name).is_none() {
                        l.add_keyword(name, parent)?;
                    }
                    parent = Some(name);
                }
            }
            Ok(l.clone())
        })?;
        self.engine
            .lock()?
            .index
            .sync_keyword_tree(&library.keyword_pairs())?;
        let xmp = self
            .engine
            .ai_metadata_settings()?
            .write_suggested_keywords_to_xmp;
        let mut applied = Vec::new();
        for (path, targets) in &plan {
            let name = path.last().expect("mapped paths are never empty").clone();
            // The tree may place an existing name elsewhere; its path wins.
            let joined = library
                .keyword_path(&name)
                .unwrap_or_else(|| path.clone())
                .join("|");
            if xmp {
                self.edit_xmp(targets, |meta| {
                    if !meta.keywords.contains(&name) {
                        meta.keywords.push(name.clone());
                    }
                    if !meta.hierarchical_keywords.contains(&joined) {
                        meta.hierarchical_keywords.push(joined.clone());
                    }
                    Ok(())
                })?;
            } else {
                let ids = targets
                    .iter()
                    .map(|k| parse_id(k))
                    .collect::<Result<Vec<_>>>()?;
                self.engine
                    .lock()?
                    .index
                    .accept_keyword_names(&ids, std::slice::from_ref(&name))
                    .map_err(|e| failure(format!("{e:#}")))?;
            }
            applied.push(joined);
        }
        Ok(applied)
    }

    /// Rejects suggestions for the photos: they are removed from the cached
    /// suggestions (until the photo is analysed again with a new model or
    /// `force`).
    pub fn reject_suggestions(&self, image_ids: Vec<String>, keywords: Vec<String>) -> Result<()> {
        for key in &image_ids {
            let id = parse_id(key)?;
            let c = self.engine.lock()?;
            let Some(mut value) = c
                .index
                .understanding(id)
                .map_err(|e| failure(format!("{e:#}")))?
            else {
                continue;
            };
            let before = value.keywords.len();
            value
                .keywords
                .retain(|s| !keywords.iter().any(|k| same(k, &s.keyword)));
            if value.keywords.len() != before {
                c.index
                    .set_understanding(id, &value)
                    .map_err(|e| failure(format!("{e:#}")))?;
            }
        }
        Ok(())
    }
}
