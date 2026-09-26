//! Assisted culling over UniFFI (docs/06 §3): real quality and face signals,
//! the face strip with per-person identities and filters, and the
//! library-local learner (assisted: predictions and a confidence-ordered queue;
//! automated: pre-filled decisions to confirm). Predictions never write a
//! Selection: only explicit decisions and `confirm_suggestions` do.
use crate::{CullSession, CullUpdate, Decision, Engine, Result, failure, parse_id, session::Inner};
use cull::{
    ImageId,
    learning::{Learner, ReviewContext, ReviewMode, ReviewPlan, SuggestedDecision},
};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};

/// ml-quality's model tag; analysis writes its rows plus the aggregates below.
const QUALITY_MODEL: &str = "classical-quality-v1";
const ANALYSIS_MODEL: &str = "tessera-analysis-v1";
/// Pixel size of the oriented preview the signals (and face boxes) were measured on.
pub(crate) const ANALYSIS_WIDTH: &str = "analysis_width";
pub(crate) const ANALYSIS_HEIGHT: &str = "analysis_height";
/// Long edge of the analysed preview.
const ANALYSIS_PX: u32 = 1024;
/// SFace cosine similarity above which two faces are the same person (OpenCV's
/// recommended threshold for SFace).
const SAME_PERSON: f32 = 0.363;
/// Complete-link clustering is cubic; larger shoots use greedy centroid linking.
const COMPLETE_LINK_MAX: usize = 400;

#[derive(Clone, Copy, Debug, uniffi::Record)]
pub struct AnalysisOptions {
    /// Classical quality (sharpness, blur, noise, exposure, clipping).
    pub quality: bool,
    /// Face detection + descriptors (YuNet/SFace; weights download on first use).
    pub faces: bool,
    /// Recompute signals that already exist.
    pub force: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AnalysisResult {
    pub image_id: String,
    /// Nothing requested was missing (and `force` was off).
    pub skipped: bool,
    pub sharpness: Option<f64>,
    pub highlight_clipping: Option<f64>,
    /// Faces found, when faces were analysed in this call.
    pub faces: Option<u32>,
}

/// A synthetic face for tests and acceptance aids, in analysis-preview pixels.
#[derive(Clone, Debug, uniffi::Record)]
pub struct FaceInput {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Crop sharpness in [0, 1].
    pub focus: f64,
    /// Eyes-open proxy in [0, 1] (not a verified blink signal).
    pub eyes_open: Option<f64>,
    /// 128 SFace components, for person clustering.
    pub embedding: Option<Vec<f32>>,
}

/// One face of the strip. The rectangle is normalized to the displayed
/// (oriented) preview: x, y, width, height in [0, 1].
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct FaceChipInfo {
    pub ordinal: u32,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    /// Normalized crop sharpness, higher is sharper.
    pub focus: f64,
    /// Weak geometric proxy; `None` when unknown.
    pub eyes_open: Option<f64>,
    /// Session-local identity from descriptor clustering (`person-N`).
    pub person_id: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PersonInfo {
    pub id: String,
    pub name: String,
    /// Images (queue order) in which this person appears.
    pub images: Vec<String>,
    pub faces: u32,
    /// A representative face (the sharpest).
    pub cover_image: String,
    pub cover_ordinal: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum AssistMode {
    /// No predictions, no learning.
    Off,
    /// Predictions, explanations and a confidence-ordered queue; decisions teach
    /// the learner. No pre-filled decisions.
    Assisted,
    /// As assisted, plus pre-filled (suggested) decisions outside the thresholds,
    /// shown for confirmation. Requires 0 <= reject_below < .5 < keep_above <= 1.
    Automated { reject_below: f64, keep_above: f64 },
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AssistStatus {
    pub mode: AssistMode,
    /// Confirmed Keep/Reject labels the learner has seen for this library.
    pub labels: u64,
    pub library_id: String,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct KeepContribution {
    pub feature: String,
    /// Signed additive contribution to the log-odds of Keep.
    pub contribution: f64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct KeepPrediction {
    pub image_id: String,
    pub p_keep: f64,
    /// Up to five terms, largest first.
    pub explanation: Vec<KeepContribution>,
    /// Pre-filled decision awaiting confirmation (automated mode, undecided
    /// frames only, not dismissed).
    pub suggested: Option<Decision>,
    pub technical_score: f64,
    /// Undecided with P(keep) < .5 (grouped at the end of the queue).
    pub likely_reject: bool,
}

/// Per-session assistance state, owned by the session's `Inner`.
pub(crate) struct AssistState {
    support: PathBuf,
    library: String,
    mode: AssistMode,
    learner: Option<Learner>,
    plan: Option<ReviewPlan>,
    dismissed: HashSet<ImageId>,
    people: Option<People>,
}

impl AssistState {
    pub(crate) fn new(support: PathBuf, library: String) -> Self {
        Self {
            support,
            library,
            mode: AssistMode::Off,
            learner: None,
            plan: None,
            dismissed: HashSet::new(),
            people: None,
        }
    }
    pub(crate) fn learning(&self) -> bool {
        self.mode != AssistMode::Off
    }
    fn learner(&mut self) -> Result<&mut Learner> {
        if self.learner.is_none() {
            self.learner = Some(Learner::open(&self.support, &self.library)?);
        }
        Ok(self.learner.as_mut().expect("learner loaded"))
    }
    fn save_learner(&self) -> Result<()> {
        if let Some(learner) = &self.learner {
            learner.save(&self.support, &self.library)?;
        }
        Ok(())
    }
    /// Decisions change the queue's before-states: any plan is stale.
    pub(crate) fn invalidate(&mut self) {
        self.plan = None;
    }
}

/// Stable, bounded learner/profile key for a library folder (or a query).
pub(crate) fn library_key(folder: Option<&Path>) -> String {
    match folder {
        Some(path) => {
            let hash = blake3::hash(path.to_string_lossy().as_bytes());
            format!("folder-{}", &hash.to_hex()[..32])
        }
        None => "query".into(),
    }
}

// ─────────────────────────────── analysis ───────────────────────────────

impl Engine {
    /// The oriented preview the app displays, at most `max_px` on the long
    /// edge: the camera JPEG fast path, or the RAW's embedded/rendered preview.
    pub(crate) fn analysis_rgb(&self, image_id: &str, max_px: u32) -> Result<image::RgbImage> {
        use previews::Codec;
        let (path, orientation) = {
            let c = self.lock()?;
            let path = Self::path(&c, image_id)?;
            let orientation: String = c.reader.query_row(
                "SELECT COALESCE((SELECT value FROM metadata WHERE image_id=? AND key='orientation'),'1')",
                [image_id],
                |r| r.get(0),
            )?;
            (path, orientation.parse::<u8>().unwrap_or(1))
        };
        let ext = Path::new(&path)
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if matches!(ext.as_str(), "jpg" | "jpeg") {
            // Decode and scale directly: the preview pyramid is the grid's business.
            let decoded = previews::Jpeg
                .decode(&std::fs::read(&path)?)
                .map_err(failure)?;
            let scaled = image::DynamicImage::ImageRgb8(decoded)
                .thumbnail(max_px, max_px)
                .to_rgb8();
            return Ok(orient(scaled, orientation));
        }
        let (key, _) = self
            .previews
            .from_raw(Path::new(&path), max_px)
            .map_err(failure)?;
        let bytes = self
            .previews
            .get(&key, previews::Level::Full)
            .ok_or_else(|| failure("preview was evicted"))?;
        previews::Jpeg.decode(&bytes).map_err(failure)
    }

    fn face_models(&self) -> Result<std::sync::MutexGuard<'_, Option<ml_faces::FaceModels>>> {
        let mut slot = self.faces.lock().map_err(failure)?;
        if slot.is_none() {
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
            let registry = ml_runtime::ModelRegistry::open(&manifest, &cache)
                .map_err(|e| failure(format!("face models: {e}")))?;
            let models =
                ml_faces::FaceModels::load(&registry, ml_runtime::SessionOptions::default())
                    .map_err(|e| failure(format!("face models unavailable: {e:#}")))?;
            *slot = Some(models);
        }
        Ok(slot)
    }
}

#[uniffi::export]
impl Engine {
    /// Computes real culling signals for one image from its displayed preview
    /// (≤ 1024 px): classical quality (`sharpness`, `motion_blur`, `noise`,
    /// per-channel exposure/clipping, `quality`, plus `highlight_clipping` = the
    /// worst channel) and, when asked, faces (`face_sharpness`, `eyes_open`
    /// aggregates and per-face rows with SFace descriptors). Blocking; call off
    /// the main thread and loop over a shoot (the host shows progress).
    pub fn analyze_image(
        &self,
        image_id: String,
        options: AnalysisOptions,
    ) -> Result<AnalysisResult> {
        let id = parse_id(&image_id)?;
        let (have_quality, have_faces) = {
            let c = self.lock()?;
            let scores = c.index.scores(id)?;
            (
                scores
                    .iter()
                    .any(|s| s.signal == "quality" && s.model == QUALITY_MODEL)
                    && scores.iter().any(|s| s.signal == ANALYSIS_WIDTH),
                scores.iter().any(|s| s.signal == "faces_analyzed"),
            )
        };
        let quality = options.quality && (options.force || !have_quality);
        let faces = options.faces && (options.force || !have_faces);
        let mut result = AnalysisResult {
            image_id: image_id.clone(),
            skipped: !quality && !faces,
            sharpness: None,
            highlight_clipping: None,
            faces: None,
        };
        if result.skipped {
            return Ok(result);
        }
        let rgb = self.analysis_rgb(&image_id, ANALYSIS_PX)?;
        let score = |signal: &str, value: f64| index::Score {
            signal: signal.into(),
            value,
            model: ANALYSIS_MODEL.into(),
        };
        if quality {
            let c = self.lock()?;
            let q = ml_quality::analyze_and_store(&c.index, id, &rgb)?;
            let highlights = q.highlight_clipping.iter().copied().fold(0., f64::max);
            c.index
                .set_score(id, &score("highlight_clipping", highlights))?;
            result.sharpness = Some(q.sharpness);
            result.highlight_clipping = Some(highlights);
        }
        if faces {
            // Detection and descriptors run outside the catalog lock.
            let records = {
                let mut slot = self.face_models()?;
                let models = slot.as_mut().expect("face models loaded");
                let mut records = Vec::new();
                for (ordinal, face) in models.detect(&rgb).map_err(failure)?.iter().enumerate() {
                    let signals = ml_faces::face_signals(&rgb, face).map_err(failure)?;
                    records.push(index::FaceRecord {
                        id: ordinal as u32,
                        bbox: face.bbox,
                        landmarks5: face.landmarks5,
                        confidence: face.score.clamp(0., 1.),
                        embedding: Some(models.embed(&rgb, face).map_err(failure)?.to_vec()),
                        sharpness: signals.sharpness,
                        eyes_open: signals.eyes_open,
                    });
                }
                records
            };
            let c = self.lock()?;
            c.index.replace_faces(id, &records)?;
            c.index.set_score(
                id,
                &index::Score {
                    signal: "faces_analyzed".into(),
                    value: 1.,
                    model: "yunet-sface-v1".into(),
                },
            )?;
            result.faces = Some(records.len() as u32);
        }
        let c = self.lock()?;
        c.index
            .set_score(id, &score(ANALYSIS_WIDTH, f64::from(rgb.width())))?;
        c.index
            .set_score(id, &score(ANALYSIS_HEIGHT, f64::from(rgb.height())))?;
        Ok(result)
    }

    /// Test aid (like `set_score`): replaces an image's faces with synthetic
    /// detections measured on a `width × height` preview. Landmarks are placed
    /// canonically inside each box. Producers are the face analysis above.
    pub fn set_faces(
        &self,
        image_id: String,
        faces: Vec<FaceInput>,
        width: u32,
        height: u32,
    ) -> Result<()> {
        let id = parse_id(&image_id)?;
        if width == 0 || height == 0 {
            return Err(failure("face coordinate space must be non-empty"));
        }
        let records = faces
            .iter()
            .enumerate()
            .map(|(n, f)| {
                let at = |u: f32, v: f32| [f.x + u * f.width, f.y + v * f.height];
                index::FaceRecord {
                    id: n as u32,
                    bbox: [f.x, f.y, f.width, f.height],
                    landmarks5: [
                        at(0.3, 0.4),
                        at(0.7, 0.4),
                        at(0.5, 0.6),
                        at(0.35, 0.8),
                        at(0.65, 0.8),
                    ],
                    confidence: 1.,
                    embedding: f.embedding.clone(),
                    sharpness: f.focus,
                    eyes_open: f.eyes_open,
                }
            })
            .collect::<Vec<_>>();
        let c = self.lock()?;
        c.index.replace_faces(id, &records)?;
        for (signal, value) in [
            ("faces_analyzed", 1.),
            (ANALYSIS_WIDTH, f64::from(width)),
            (ANALYSIS_HEIGHT, f64::from(height)),
        ] {
            c.index.set_score(
                id,
                &index::Score {
                    signal: signal.into(),
                    value,
                    model: "synthetic-faces/1".into(),
                },
            )?;
        }
        Ok(())
    }
}

/// EXIF orientation as the preview store applies it (so face boxes match the display).
fn orient(img: image::RgbImage, orientation: u8) -> image::RgbImage {
    use image::imageops::{flip_horizontal, flip_vertical, rotate90, rotate180, rotate270};
    match orientation {
        2 => flip_horizontal(&img),
        3 => rotate180(&img),
        4 => flip_vertical(&img),
        5 => rotate270(&flip_horizontal(&img)),
        6 => rotate90(&img),
        7 => rotate90(&flip_horizontal(&img)),
        8 => rotate270(&img),
        _ => img,
    }
}

// ─────────────────────────────── people ───────────────────────────────

/// Session-local identities from descriptor clustering.
#[derive(Clone, Default)]
pub(crate) struct People {
    of_face: HashMap<(ImageId, u32), String>,
    list: Vec<PersonInfo>,
}

fn cosine_unit(v: &[f32]) -> Option<[f32; 128]> {
    let norm = v.iter().map(|x| f64::from(*x).powi(2)).sum::<f64>().sqrt();
    if v.len() != 128 || !norm.is_finite() || norm == 0. {
        return None;
    }
    Some(std::array::from_fn(|i| (f64::from(v[i]) / norm) as f32))
}

fn cluster(embeddings: &[[f32; 128]]) -> Vec<Vec<usize>> {
    if embeddings.len() <= COMPLETE_LINK_MAX
        && let Ok(groups) = ml_faces::cluster(embeddings, SAME_PERSON)
    {
        return groups;
    }
    // Greedy: join the most similar running centroid above the threshold.
    let mut centroids: Vec<[f32; 128]> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for (i, e) in embeddings.iter().enumerate() {
        let best = centroids
            .iter()
            .enumerate()
            .map(|(g, c)| (g, c.iter().zip(e).map(|(a, b)| a * b).sum::<f32>()))
            .filter(|(_, s)| *s >= SAME_PERSON)
            .max_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((g, _)) => {
                groups[g].push(i);
                let n = groups[g].len() as f32;
                let mut c: [f32; 128] =
                    std::array::from_fn(|k| centroids[g][k] * (n - 1.) / n + e[k] / n);
                let norm = c.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-6);
                c.iter_mut().for_each(|x| *x /= norm);
                centroids[g] = c;
            }
            None => {
                centroids.push(*e);
                groups.push(vec![i]);
            }
        }
    }
    groups
}

fn people(index: &index::Index, images: &[ImageId]) -> Result<People> {
    let mut faces = Vec::new();
    let mut units = Vec::new();
    for &image in images {
        for face in index.faces(image)? {
            if let Some(unit) = face.embedding.as_deref().and_then(cosine_unit) {
                faces.push((image, face.id, face.sharpness));
                units.push(unit);
            }
        }
    }
    let mut groups = cluster(&units);
    let position: HashMap<ImageId, usize> =
        images.iter().enumerate().map(|(n, id)| (*id, n)).collect();
    let distinct = |g: &Vec<usize>| g.iter().map(|&i| faces[i].0).collect::<HashSet<_>>().len();
    groups.sort_by(|a, b| {
        distinct(b)
            .cmp(&distinct(a))
            .then_with(|| a.iter().min().cmp(&b.iter().min()))
    });
    let mut result = People::default();
    for (n, group) in groups.iter().enumerate() {
        let id = format!("person-{}", n + 1);
        let mut ids: Vec<ImageId> = group.iter().map(|&i| faces[i].0).collect();
        ids.sort_by_key(|id| position[id]);
        ids.dedup();
        let cover = group
            .iter()
            .copied()
            .max_by(|&a, &b| faces[a].2.total_cmp(&faces[b].2).then(b.cmp(&a)))
            .expect("non-empty cluster");
        for &i in group {
            result.of_face.insert((faces[i].0, faces[i].1), id.clone());
        }
        result.list.push(PersonInfo {
            name: format!("Person {}", n + 1),
            id,
            images: ids.iter().map(ToString::to_string).collect(),
            faces: group.len() as u32,
            cover_image: faces[cover].0.to_string(),
            cover_ordinal: faces[cover].1,
        });
    }
    Ok(result)
}

fn score(index: &index::Index, id: ImageId, signal: &str) -> Result<Option<f64>> {
    Ok(index
        .scores(id)?
        .into_iter()
        .find(|s| s.signal == signal)
        .map(|s| s.value))
}

fn analysis_space(index: &index::Index, id: ImageId) -> Result<Option<(u32, u32)>> {
    Ok(
        match (
            score(index, id, ANALYSIS_WIDTH)?,
            score(index, id, ANALYSIS_HEIGHT)?,
        ) {
            (Some(w), Some(h)) if w >= 1. && h >= 1. => Some((w as u32, h as u32)),
            _ => None,
        },
    )
}

impl Inner {
    fn people(&mut self) -> Result<&People> {
        if self.assist.people.is_none() {
            let found = people(self.core.index(), self.core.images())?;
            self.assist.people = Some(found);
        }
        Ok(self.assist.people.as_ref().expect("people computed"))
    }

    fn review_context(&self) -> Result<ReviewContext> {
        let mut context = ReviewContext::default();
        for &id in self.core.images() {
            if let Some(space) = analysis_space(self.core.index(), id)? {
                context.dimensions.insert(id, space);
            }
        }
        Ok(context)
    }

    /// A manual decision that also teaches the learner when assistance is on.
    /// Falls back to a plain decision when the frame has unusable signals.
    pub(crate) fn decide_learning(&mut self, decision: cull::Decision) -> Result<()> {
        self.assist.invalidate();
        if !self.assist.learning() {
            return Ok(self.core.decide(decision)?);
        }
        let context = self.review_context()?;
        let before = self
            .core
            .current()
            .map(|id| self.core.selection(id))
            .transpose()?;
        let learner = self.assist.learner()?;
        match self.core.decide_with_learning(decision, learner, &context) {
            Ok(()) => self.assist.save_learner(),
            Err(e) => {
                let after = self
                    .core
                    .current()
                    .map(|id| self.core.selection(id))
                    .transpose()?;
                if before == after {
                    // Nothing was written: the signals were unusable, decide plainly.
                    Ok(self.core.decide(decision)?)
                } else {
                    Err(e.into())
                }
            }
        }
    }
}

// ─────────────────────────────── session ───────────────────────────────

#[uniffi::export]
impl CullSession {
    /// Faces of one image, in detection order, with identities. Empty when the
    /// image has no analysed faces.
    pub fn face_strip(&self, image_id: String) -> Result<Vec<FaceChipInfo>> {
        let id = parse_id(&image_id)?;
        let mut s = self.lock()?;
        let Some((width, height)) = analysis_space(s.core.index(), id)? else {
            return Ok(Vec::new());
        };
        let of_face = s.people()?.of_face.clone();
        let index = s.core.index();
        let records = index.faces(id)?;
        let chips = ml_faces::face_strip_from_index(index, id, (width, height), |image, face| {
            of_face.get(&(image, face.id)).cloned()
        })
        .map_err(failure)?;
        Ok(records
            .iter()
            .zip(chips)
            .map(|(record, chip)| {
                let [x, y, w, h] = chip.crop_rect.map(f64::from);
                FaceChipInfo {
                    ordinal: record.id,
                    x: x / f64::from(width),
                    y: y / f64::from(height),
                    width: w / f64::from(width),
                    height: h / f64::from(height),
                    focus: chip.focus_score,
                    eyes_open: chip.eyes_open,
                    person_id: chip.person_id,
                }
            })
            .collect())
    }

    /// People in this queue, most frequent first. `refresh` re-clusters after
    /// new face analysis.
    pub fn people(&self, refresh: bool) -> Result<Vec<PersonInfo>> {
        let mut s = self.lock()?;
        if refresh {
            s.assist.people = None;
        }
        Ok(s.people()?.list.clone())
    }

    /// Per-person filter: frames where `person_id` appears; with
    /// `eyes_closed_below`, only frames where that person's eyes-open proxy is
    /// below the threshold (review only; never a decision).
    pub fn frames_with_person(
        &self,
        person_id: String,
        eyes_closed_below: Option<f64>,
    ) -> Result<Vec<String>> {
        let mut s = self.lock()?;
        let of_face = s.people()?.of_face.clone();
        let person =
            |image: ImageId, face: &index::FaceRecord| of_face.get(&(image, face.id)).cloned();
        let images = s.core.images().to_vec();
        let found = match eyes_closed_below {
            Some(below) => ml_faces::frames_with_person_eyes_closed(
                s.core.index(),
                &images,
                &person_id,
                below,
                person,
            )
            .map_err(failure)?,
            None => images
                .into_iter()
                .filter(|image| {
                    of_face
                        .iter()
                        .any(|((i, _), p)| i == image && *p == person_id)
                })
                .collect(),
        };
        Ok(found.iter().map(ToString::to_string).collect())
    }

    /// Switches assistance. The learner is library-local (app support
    /// `cull-learning/`) and survives sessions.
    pub fn set_assist_mode(&self, mode: AssistMode) -> Result<AssistStatus> {
        if let AssistMode::Automated {
            reject_below,
            keep_above,
        } = mode
            && !((0. ..0.5).contains(&reject_below) && keep_above > 0.5 && keep_above <= 1.)
        {
            return Err(failure(
                "thresholds must satisfy 0 <= reject < .5 < keep <= 1",
            ));
        }
        let mut s = self.lock()?;
        s.assist.mode = mode;
        s.assist.invalidate();
        s.assist.dismissed.clear();
        let labels = if mode == AssistMode::Off {
            s.assist.learner.as_ref().map_or(0, Learner::label_count)
        } else {
            s.assist.learner()?.label_count()
        };
        Ok(AssistStatus {
            mode,
            labels,
            library_id: s.assist.library.clone(),
        })
    }

    pub fn assist_status(&self) -> Result<AssistStatus> {
        let mut s = self.lock()?;
        let mode = s.assist.mode;
        let labels = s.assist.learner()?.label_count();
        Ok(AssistStatus {
            mode,
            labels,
            library_id: s.assist.library.clone(),
        })
    }

    /// Predicts every frame (read-only) in review order: likely keepers first,
    /// uncertain next, likely rejects last. Regenerate after new signals,
    /// decisions or confirmations.
    pub fn review(&self) -> Result<Vec<KeepPrediction>> {
        let mut s = self.lock()?;
        let mode = match s.assist.mode {
            AssistMode::Off | AssistMode::Assisted => ReviewMode::Assisted,
            AssistMode::Automated {
                reject_below,
                keep_above,
            } => ReviewMode::Automated {
                reject_below,
                keep_above,
            },
        };
        let context = s.review_context()?;
        let plan = {
            let inner = &mut *s;
            let learner = inner.assist.learner()?;
            inner.core.review(learner, &context, mode)?
        };
        let rejects: HashSet<ImageId> = plan.likely_rejects().iter().copied().collect();
        let out = plan
            .entries()
            .iter()
            .map(|e| KeepPrediction {
                image_id: e.image.to_string(),
                p_keep: e.prediction.p_keep,
                explanation: e
                    .prediction
                    .explanation
                    .iter()
                    .map(|c| KeepContribution {
                        feature: c.feature.clone(),
                        contribution: c.contribution,
                    })
                    .collect(),
                suggested: e
                    .suggested
                    .filter(|_| !s.assist.dismissed.contains(&e.image))
                    .map(|d| match d {
                        SuggestedDecision::Keep => Decision::Keep,
                        SuggestedDecision::Reject => Decision::Reject,
                    }),
                technical_score: e.technical_score,
                likely_reject: rejects.contains(&e.image),
            })
            .collect();
        s.assist.plan = Some(plan);
        Ok(out)
    }

    /// Reorders the review queue by the last `review` (navigation order only;
    /// cursor and undo positions are preserved). Returns the new order.
    pub fn reorder_queue(&self) -> Result<Vec<String>> {
        let mut s = self.lock()?;
        let inner = &mut *s;
        let plan = inner
            .assist
            .plan
            .as_ref()
            .ok_or_else(|| failure("call review() first"))?;
        inner.core.reorder_review(plan)?;
        Ok(inner
            .core
            .images()
            .iter()
            .map(ToString::to_string)
            .collect())
    }

    /// Applies the listed suggestions as one undo step and teaches the learner.
    /// Fails without writing anything when a suggestion is stale, unknown or
    /// was dismissed.
    pub fn confirm_suggestions(&self, image_ids: Vec<String>) -> Result<CullUpdate> {
        let ids = image_ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<Vec<_>>>()?;
        let mut s = self.lock()?;
        if ids.iter().any(|id| s.assist.dismissed.contains(id)) {
            return Err(failure("a dismissed suggestion cannot be confirmed"));
        }
        {
            let inner = &mut *s;
            let plan = inner
                .assist
                .plan
                .take()
                .ok_or_else(|| failure("call review() first"))?;
            inner.assist.learner()?;
            let learner = inner.assist.learner.as_mut().expect("learner loaded");
            inner.core.confirm_suggestions(&plan, &ids, learner)?;
        }
        // A model-save failure never undoes the confirmed decisions.
        if let Err(e) = s.assist.save_learner() {
            eprintln!("assist: learner not saved: {e}");
        }
        s.update(ids, false)
    }

    /// Rejects (dismisses) suggestions: they stop showing and cannot be
    /// confirmed until assistance is switched again. Nothing is written.
    pub fn dismiss_suggestions(&self, image_ids: Vec<String>) -> Result<()> {
        let ids = image_ids
            .iter()
            .map(|id| parse_id(id))
            .collect::<Result<Vec<_>>>()?;
        self.lock()?.assist.dismissed.extend(ids);
        Ok(())
    }
}
