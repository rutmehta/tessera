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
    /// Persistent catalog identity, independent of queue order.
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
    #[uniffi(default = false)]
    pub named: bool,
    /// Confirmed faces in the active queue (same scope as `faces`).
    #[uniffi(default = 0)]
    pub confirmed_count: u32,
    /// Alias of `faces` retained for existing consumers.
    #[uniffi(default = 0)]
    pub face_count: u32,
    /// Indexed medoid member, possibly outside the active queue. None when
    /// invalidated or no usable descriptor exists; never the sharpest fallback.
    #[uniffi(default = None)]
    pub medoid_face: Option<PersonFace>,
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
    people_job: ml_faces::people::PeopleJob,
    people_undo: Vec<PeopleEdit>,
    people_redo: Vec<PeopleEdit>,
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
            people_undo: Vec::new(),
            people_redo: Vec::new(),
            people_job:
                ml_faces::people::PeopleJob::new(ml_faces::people::PeopleOptions::default())
                    .expect("valid default people options"),
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

#[cfg(test)]
mod persistent_people_tests {
    use super::*;

    #[test]
    fn persistent_session_manual_operations_and_indexed_filters() {
        let dir = tempfile::tempdir().unwrap();
        let photos = dir.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        for n in 0..3 {
            image::RgbImage::from_pixel(100, 100, image::Rgb([n * 50, 30, 20]))
                .save(photos.join(format!("{n}.jpg")))
                .unwrap();
        }
        let engine =
            Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        let folder = engine
            .index_folder(photos.to_string_lossy().into_owned())
            .unwrap()
            .path;
        let ids: Vec<_> = engine
            .list_images(crate::ImageQuery::default())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        for (n, id) in ids.iter().enumerate() {
            let mut embedding = vec![0.; 128];
            embedding[0] = 1.;
            engine
                .set_faces(
                    id.clone(),
                    vec![FaceInput {
                        x: 10.,
                        y: 10.,
                        width: 50.,
                        height: 50.,
                        focus: 0.9,
                        eyes_open: Some(if n == 0 { 0.1 } else { 0.9 }),
                        embedding: Some(embedding),
                    }],
                    100,
                    100,
                )
                .unwrap();
        }
        let session = engine.open_cull_session(folder.clone()).unwrap();
        let original = session.people(true).unwrap();
        assert_eq!(original.len(), 1);
        let person = original[0].id.clone();
        session
            .name_person(
                person.clone(),
                Some("Ada".into()),
                PeopleNameOptions::default(),
            )
            .unwrap();
        assert_eq!(session.people(false).unwrap()[0].name, "Ada");
        assert_eq!(
            session
                .frames_with_person(person.clone(), Some(0.5))
                .unwrap(),
            vec![ids[0].clone()]
        );
        let key = PersonFace {
            image_id: ids[0].clone(),
            ordinal: 0,
        };
        session.confirm_person_face(key.clone(), true).unwrap();
        assert!(session.person_assignments(ids[0].clone()).unwrap()[0].confirmed);
        session.confirm_person_face(key.clone(), false).unwrap();
        assert!(!session.person_assignments(ids[0].clone()).unwrap()[0].confirmed);
        session
            .split_person(person.clone(), "manual-split".into(), vec![key.clone()])
            .unwrap();
        assert_eq!(
            session
                .frames_with_person("manual-split".into(), None)
                .unwrap(),
            vec![ids[0].clone()]
        );
        session
            .assign_person_face(key.clone(), person.clone())
            .unwrap();
        assert_eq!(
            session.person_assignments(ids[0].clone()).unwrap()[0].person_id,
            person
        );
        session
            .assign_person_face(key.clone(), "manual-split".into())
            .unwrap();
        session
            .merge_people(person.clone(), "manual-split".into())
            .unwrap();
        assert!(
            session
                .frames_with_person("manual-split".into(), None)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            session
                .frames_with_person(person.clone(), None)
                .unwrap()
                .len(),
            3
        );
        assert!(
            session
                .assign_person_face(PersonFace { ordinal: 99, ..key }, person.clone())
                .is_err()
        );
        assert!(session.people_name_suggestions(Some(f32::NAN)).is_err());
        assert!(session.people_name_suggestions(None).unwrap().is_empty());
        assert_eq!(session.refresh_people(true).unwrap().assigned, 0);
        let reopened = engine.open_cull_session(folder).unwrap();
        assert_eq!(reopened.people(false).unwrap()[0].id, person);
        assert_eq!(reopened.people(false).unwrap()[0].name, "Ada");
        let library = cull::Library::read(session.library_path().unwrap().unwrap()).unwrap();
        assert_eq!(library.people, vec!["Ada".to_string()]);
        for n in 0..3 {
            assert!(!photos.join(format!("{n}.jpg.xmp")).exists());
            assert!(!photos.join(format!("{n}.xmp")).exists());
        }
        // An indexed match outside the review queue must not leak into filters.
        let subset = vec![parse_id(&ids[1]).unwrap()];
        let s = session.lock().unwrap();
        assert_eq!(
            person_frames(s.core.index(), &subset, &person, None).unwrap(),
            subset
        );
        // Suggestions are read-only and carry the persisted target ID/name.
        let mut medoid = vec![0.; 128];
        medoid[0] = 1.;
        s.core
            .index()
            .create_person("named-example", Some("Grace"), Some(&medoid))
            .unwrap();
        s.core
            .index()
            .create_person("unnamed-example", None, Some(&medoid))
            .unwrap();
        drop(s);
        let suggestions = session.people_name_suggestions(Some(0.99)).unwrap();
        let suggestion = suggestions
            .iter()
            .find(|a| a.unnamed_id == "unnamed-example")
            .unwrap();
        assert_eq!(suggestion.named_id, "named-example");
        assert_eq!(suggestion.name, "Grace");
        assert!(
            session
                .lock()
                .unwrap()
                .core
                .index()
                .people()
                .unwrap()
                .iter()
                .find(|p| p.id == "unnamed-example")
                .unwrap()
                .name
                .is_none()
        );
        session
            .name_person(
                person.clone(),
                Some("Ada Lovelace".into()),
                PeopleNameOptions {
                    write_sidecars: true,
                    person_keywords: true,
                },
            )
            .unwrap();
        for n in 0..3 {
            let packet = sidecar::Sidecar::read_xmp(
                &sidecar::Sidecar::paths(photos.join(format!("{n}.jpg"))).xmp,
            )
            .unwrap();
            assert_eq!(packet.face_regions().unwrap()[0].name, "Ada Lovelace");
        }
        // The other session must resolve fresh joined names, not stale cache.
        assert_eq!(reopened.people(false).unwrap()[0].name, "Ada Lovelace");
        assert_eq!(
            session.face_strip(ids[0].clone()).unwrap()[0]
                .person_id
                .as_deref(),
            Some(person.as_str())
        );
    }

    #[test]
    fn stored_ids_and_names_survive_reorder_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["a.jpg", "b.jpg"] {
            std::fs::write(dir.path().join(name), b"jpeg").unwrap();
        }
        let db = dir.path().join("index.sqlite");
        let mut index = index::Index::open(&db).unwrap();
        index
            .scan(
                dir.path(),
                &index::NoopSidecarReader,
                &index::NoopMetadataProvider,
            )
            .unwrap();
        let mut ids = index.search(&index::Query::default()).unwrap();
        index
            .create_person("stable-ada", Some("Ada"), None)
            .unwrap();
        for &id in &ids {
            index
                .replace_faces(
                    id,
                    &[index::FaceRecord {
                        id: 0,
                        bbox: [0., 0., 40., 50.],
                        landmarks5: [[0.; 2]; 5],
                        confidence: 1.,
                        embedding: None,
                        sharpness: 0.8,
                        eyes_open: Some(0.9),
                    }],
                )
                .unwrap();
            index
                .assign_face(
                    index::FaceKey {
                        image_id: id,
                        ordinal: 0,
                    },
                    "stable-ada",
                )
                .unwrap();
        }
        let first = people(&index, &ids).unwrap();
        assert_eq!(first.list.len(), 1);
        assert_eq!(first.list[0].id, "stable-ada");
        assert_eq!(first.list[0].name, "Ada");
        drop(index);
        let index = index::Index::open(&db).unwrap();
        ids.reverse();
        let second = people(&index, &ids).unwrap();
        assert_eq!(second.list[0].id, first.list[0].id);
        assert_eq!(second.list[0].name, "Ada");
        assert_eq!(
            second.list[0].images,
            ids.iter().map(ToString::to_string).collect::<Vec<_>>()
        );
        assert_eq!(second.of_face.len(), 2);
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

/// An image-local detector ordinal, not an identity.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PersonFace {
    pub image_id: String,
    pub ordinal: u32,
}
impl PersonFace {
    fn key(&self) -> Result<index::FaceKey> {
        Ok(index::FaceKey {
            image_id: parse_id(&self.image_id)?,
            ordinal: self.ordinal,
        })
    }
}

/// Additional assignment metadata without changing the existing strip record.
#[derive(Clone, Debug, uniffi::Record)]
pub struct PersonAssignmentInfo {
    pub face: PersonFace,
    pub person_id: String,
    pub name: Option<String>,
    pub confirmed: bool,
}

#[derive(Clone, Copy, Debug, Default, uniffi::Record)]
pub struct PeopleNameOptions {
    /// Explicit opt-in; false does not even probe sidecar paths.
    pub write_sidecars: bool,
    /// Append names as keywords only when writing sidecars.
    pub person_keywords: bool,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PeopleJobResult {
    pub assigned: u64,
    pub reclustered: bool,
    pub approximate: bool,
    /// Actual fitted sample size, zero if this job had no eligible pending faces.
    #[uniffi(default = 0)]
    pub sample_size: u32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PeopleNameSuggestion {
    pub unnamed_id: String,
    pub named_id: String,
    pub name: String,
    pub similarity: f32,
}

fn person_frames(
    index: &index::Index,
    queue: &[ImageId],
    person_id: &str,
    eyes_closed_below: Option<f64>,
) -> Result<Vec<ImageId>> {
    // Do not cap at queue.len(): catalog matches outside the queue can precede
    // all its members in ID order. The index applies DISTINCT before LIMIT.
    let matches: HashSet<_> = index
        .images_with_person(person_id, false, i64::MAX as usize, 0)?
        .into_iter()
        .collect();
    let images: Vec<_> = queue
        .iter()
        .copied()
        .filter(|id| matches.contains(id))
        .collect();
    let Some(below) = eyes_closed_below else {
        return Ok(images);
    };
    let mut assignments = HashMap::new();
    for &image in &images {
        for assignment in index.face_assignments(image)? {
            assignments.insert((image, assignment.face.ordinal), assignment.person_id);
        }
    }
    ml_faces::frames_with_person_eyes_closed(index, &images, person_id, below, |image, face| {
        assignments.get(&(image, face.id)).cloned()
    })
    .map_err(failure)
}

/// Queue projection of persistent indexed identities.
#[derive(Clone, Default)]
pub(crate) struct People {
    of_face: HashMap<(ImageId, u32), String>,
    list: Vec<PersonInfo>,
}

fn people(index: &index::Index, images: &[ImageId]) -> Result<People> {
    let mut result = People::default();
    let medoids: HashMap<_, _> = index
        .people()?
        .into_iter()
        .map(|p| (p.id, p.medoid))
        .collect();
    let mut groups: HashMap<String, (PersonInfo, f64)> = HashMap::new();
    for &image in images {
        let faces: HashMap<_, _> = index.faces(image)?.into_iter().map(|f| (f.id, f)).collect();
        for assignment in index.face_assignments(image)? {
            let Some(face) = faces.get(&assignment.face.ordinal) else {
                continue;
            };
            let id = assignment.person_id;
            result.of_face.insert((image, face.id), id.clone());
            let (person, best) = groups.entry(id.clone()).or_insert_with(|| {
                (
                    PersonInfo {
                        named: assignment.person_name.is_some(),
                        confirmed_count: 0,
                        face_count: 0,
                        medoid_face: None,
                        name: assignment
                            .person_name
                            .unwrap_or_else(|| format!("Person {id}")),
                        id,
                        images: Vec::new(),
                        faces: 0,
                        cover_image: image.to_string(),
                        cover_ordinal: face.id,
                    },
                    f64::NEG_INFINITY,
                )
            });
            let image = image.to_string();
            if person.images.last() != Some(&image) {
                person.images.push(image.clone());
            }
            person.faces += 1;
            person.face_count += 1;
            person.confirmed_count += u32::from(assignment.confirmed);
            if face.sharpness > *best {
                *best = face.sharpness;
                person.cover_image = image;
                person.cover_ordinal = face.id;
            }
        }
    }
    result.list = groups.into_values().map(|(person, _)| person).collect();
    for person in &mut result.list {
        if let Some(Some(medoid)) = medoids.get(&person.id) {
            person.medoid_face = ml_faces::people::medoid_face(index, &person.id, medoid)
                .map_err(failure)?
                .map(|face| PersonFace {
                    image_id: face.image_id.to_string(),
                    ordinal: face.ordinal,
                });
        }
    }
    result
        .list
        .sort_by(|a, b| b.images.len().cmp(&a.images.len()).then(a.id.cmp(&b.id)));
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

/// History is separate from cull/recipe undo, scoped to the affected people and
/// bounded to the most recent 32 edits. File bytes are included only for naming.
struct PeopleEdit {
    description: &'static str,
    before: String,
    after: String,
    files_before: Vec<PeopleFile>,
    files_after: Vec<PeopleFile>,
}

#[derive(PartialEq)]
struct PeopleFile {
    path: PathBuf,
    bytes: Option<Vec<u8>>,
}

impl PeopleFile {
    fn read(path: PathBuf) -> Result<Self> {
        let bytes = match std::fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(failure(e)),
        };
        Ok(Self { path, bytes })
    }

    fn restore(&self) -> Result<()> {
        use std::io::Write;
        if let Some(bytes) = &self.bytes {
            let mut file = tempfile::NamedTempFile::new_in(
                self.path
                    .parent()
                    .ok_or_else(|| failure("missing parent"))?,
            )
            .map_err(failure)?;
            file.write_all(bytes).map_err(failure)?;
            file.as_file().sync_all().map_err(failure)?;
            file.persist(&self.path).map_err(failure)?;
        } else {
            match std::fs::remove_file(&self.path) {
                Ok(()) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(failure(e)),
            }
        }
        Ok(())
    }
}

impl Inner {
    fn edit_people(
        &mut self,
        description: &'static str,
        ids: &[String],
        faces: &[index::FaceKey],
        paths: Vec<PathBuf>,
        edit: impl FnOnce(&mut Self) -> Result<()>,
    ) -> Result<()> {
        let before = self.core.index().snapshot_people_edit(ids, faces)?;
        let files_before = paths
            .into_iter()
            .map(PeopleFile::read)
            .collect::<Result<Vec<_>>>()?;
        edit(self)?;
        let after = self.core.index().resnapshot_people_edit(&before)?;
        let files_after = files_before
            .iter()
            .map(|f| PeopleFile::read(f.path.clone()))
            .collect::<Result<Vec<_>>>()?;
        if before != after || files_before != files_after {
            self.assist.people_redo.clear();
            if self.assist.people_undo.len() == 32 {
                self.assist.people_undo.remove(0);
            }
            self.assist.people_undo.push(PeopleEdit {
                description,
                before,
                after,
                files_before,
                files_after,
            });
        }
        // A manual edit must not implicitly recluster on the next read. The
        // projection is rebuilt by people(); explicit refresh still works.
        self.assist.people = Some(People::default());
        Ok(())
    }

    fn replay_people_edit(&mut self, redo: bool) -> Result<Option<String>> {
        let stack = if redo {
            &self.assist.people_redo
        } else {
            &self.assist.people_undo
        };
        let Some(edit) = stack.last() else {
            return Ok(None);
        };
        let (expected, desired, old_files, new_files) = if redo {
            (
                &edit.before,
                &edit.after,
                &edit.files_before,
                &edit.files_after,
            )
        } else {
            (
                &edit.after,
                &edit.before,
                &edit.files_after,
                &edit.files_before,
            )
        };
        // Never overwrite intervening external edits. History is left intact on
        // failure so an ordinary I/O problem can be fixed and retried.
        for file in old_files {
            if PeopleFile::read(file.path.clone())? != *file {
                return Err(failure("people undo file changed since edit"));
            }
        }
        let mut written = 0;
        let result = (|| {
            for file in new_files {
                written += 1;
                file.restore()?;
            }
            self.core.index().restore_people_edit(expected, desired)?;
            Ok(())
        })();
        if let Err(error) = result {
            let mut failures = Vec::new();
            for file in old_files[..written].iter().rev() {
                if let Err(e) = file.restore() {
                    failures.push(e.to_string());
                }
            }
            return if failures.is_empty() {
                Err(error)
            } else {
                Err(failure(format!(
                    "{error}; rollback failed: {}",
                    failures.join(", ")
                )))
            };
        }
        let edit = if redo {
            self.assist.people_redo.pop()
        } else {
            self.assist.people_undo.pop()
        }
        .expect("history present");
        let description = edit.description.to_string();
        if redo {
            self.assist.people_undo.push(edit);
        } else {
            self.assist.people_redo.push(edit);
        }
        self.assist.people = Some(People::default());
        Ok(Some(description))
    }

    fn refresh_people_job(&mut self, force: bool) -> Result<PeopleJobResult> {
        // Refit over the catalog, not just this queue: persisted identities may
        // also have members outside the active folder/filter.
        let images = self.core.index().search(&index::Query {
            limit: i64::MAX as usize,
            ..Default::default()
        })?;
        let report = self
            .assist
            .people_job
            .run(self.core.index(), &images, force)
            .map_err(failure)?;
        self.assist.people = Some(people(self.core.index(), self.core.images())?);
        Ok(PeopleJobResult {
            assigned: report.assigned as u64,
            reclustered: report.reclustered,
            approximate: report.approximate,
            sample_size: report.sample_size as u32,
        })
    }

    fn people(&mut self) -> Result<&People> {
        if self.assist.people.is_none() {
            self.refresh_people_job(false)?;
        }
        // Resolve joins again: another session may have renamed/reassigned a face,
        // and a review reorder must be reflected without rerunning clustering.
        self.assist.people = Some(people(self.core.index(), self.core.images())?);
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
    /// Incremental ingestion with periodic refits, or an explicit forced refit.
    /// Blocking: invoke on the host worker queue, never the UI thread.
    pub fn refresh_people(&self, force: bool) -> Result<PeopleJobResult> {
        self.lock()?.refresh_people_job(force)
    }

    /// All catalog members in stable image-ID/ordinal order. No clustering or
    /// queue filtering; unknown/empty identities return an empty list.
    pub fn person_members(&self, person_id: String) -> Result<Vec<PersonFace>> {
        Ok(self
            .lock()?
            .core
            .index()
            .person_members(&person_id)?
            .into_iter()
            .map(|a| PersonFace {
                image_id: a.face.image_id.to_string(),
                ordinal: a.face.ordinal,
            })
            .collect())
    }

    /// Undo the last session-local people edit and return its menu description.
    /// None means empty history; conflicts/errors leave history and state intact.
    pub fn undo_people_edit(&self) -> Result<Option<String>> {
        self.lock()?.replay_people_edit(false)
    }

    /// Redo the last undone people edit. A new successful edit clears redo.
    pub fn redo_people_edit(&self) -> Result<Option<String>> {
        self.lock()?.replay_people_edit(true)
    }

    /// Read persisted names and confirmation state, including manual assignments
    /// to faces without usable descriptors. This never runs a clustering job.
    pub fn person_assignments(&self, image_id: String) -> Result<Vec<PersonAssignmentInfo>> {
        let image = parse_id(&image_id)?;
        let s = self.lock()?;
        Ok(s.core
            .index()
            .face_assignments(image)?
            .into_iter()
            .map(|a| PersonAssignmentInfo {
                face: PersonFace {
                    image_id: image_id.clone(),
                    ordinal: a.face.ordinal,
                },
                person_id: a.person_id,
                name: a.person_name,
                confirmed: a.confirmed,
            })
            .collect())
    }

    /// Assign an existing face to an existing persistent identity. Moving resets
    /// confirmation; confirm separately to protect it from automatic refits.
    pub fn assign_person_face(&self, face: PersonFace, person_id: String) -> Result<()> {
        let key = face.key()?;
        let mut s = self.lock()?;
        let mut ids = vec![person_id.clone()];
        ids.extend(
            s.core
                .index()
                .face_assignments(key.image_id)?
                .into_iter()
                .filter(|a| a.face == key)
                .map(|a| a.person_id),
        );
        s.edit_people("Assign face", &ids, &[key], vec![], |s| {
            Ok(s.core.index().assign_face(key, &person_id)?)
        })
    }

    /// Confirm (`true`) or unconfirm (`false`) an existing face assignment.
    pub fn confirm_person_face(&self, face: PersonFace, confirmed: bool) -> Result<()> {
        let key = face.key()?;
        let mut s = self.lock()?;
        let ids: Vec<_> = s
            .core
            .index()
            .face_assignments(key.image_id)?
            .into_iter()
            .filter(|a| a.face == key)
            .map(|a| a.person_id)
            .collect();
        s.edit_people(
            if confirmed {
                "Confirm face"
            } else {
                "Unconfirm face"
            },
            &ids,
            &[key],
            vec![],
            |s| Ok(s.core.index().confirm_face(key, confirmed)?),
        )
    }

    /// Explicit merge, target name wins. Does not export sidecars.
    pub fn merge_people(&self, target_id: String, source_id: String) -> Result<()> {
        let mut s = self.lock()?;
        s.edit_people(
            "Merge people",
            &[target_id.clone(), source_id.clone()],
            &[],
            vec![],
            |s| Ok(s.core.index().merge_people(&target_id, &source_id)?),
        )
    }

    /// Split selected members into a new unnamed identity (caller supplies a
    /// unique ID). Confirmations reset. Does not export sidecars.
    pub fn split_person(
        &self,
        source_id: String,
        new_id: String,
        faces: Vec<PersonFace>,
    ) -> Result<()> {
        let keys = faces
            .iter()
            .map(PersonFace::key)
            .collect::<Result<Vec<_>>>()?;
        let mut s = self.lock()?;
        s.edit_people(
            "Split person",
            &[source_id.clone(), new_id.clone()],
            &keys,
            vec![],
            |s| Ok(s.core.index().split_person(&source_id, &new_id, &keys)?),
        )
    }

    /// Rename/clear an identity and update the library's display names. Requires
    /// a library-backed session; XMP export is strictly opt-in. Coordinates use
    /// indexed analysis-preview dimensions, never RAW dimensions.
    pub fn name_person(
        &self,
        person_id: String,
        name: Option<String>,
        options: PeopleNameOptions,
    ) -> Result<()> {
        let mut s = self.lock()?;
        let path = s
            .core
            .library_path()
            .ok_or_else(|| failure("naming requires a library-backed session"))?
            .to_path_buf();
        let mut paths = vec![path.clone()];
        if options.write_sidecars {
            for image in
                s.core
                    .index()
                    .images_with_person(&person_id, false, i64::MAX as usize, 0)?
            {
                let image = s.core.index().image_info(image)?.path;
                let mut destination = sidecar::Sidecar::paths(&image).xmp;
                if !destination.try_exists().map_err(failure)?
                    && image.with_extension("xmp").try_exists().map_err(failure)?
                {
                    destination = image.with_extension("xmp");
                }
                paths.push(destination);
            }
        }
        paths.sort();
        paths.dedup();
        s.edit_people(
            "Name person",
            std::slice::from_ref(&person_id),
            &[],
            paths,
            |s| {
                cull::people::name_person(
                    s.core.index(),
                    &path,
                    &person_id,
                    name.as_deref(),
                    &cull::people::NamePersonOptions {
                        write_sidecars: options.write_sidecars,
                        person_keywords: options.person_keywords,
                        dimensions: HashMap::new(),
                    },
                )?;
                Ok(())
            },
        )
    }

    /// Read-only catalog suggestions. Accept explicitly with merge/assignment;
    /// neither names nor assignments are changed by requesting suggestions.
    pub fn people_name_suggestions(
        &self,
        threshold: Option<f32>,
    ) -> Result<Vec<PeopleNameSuggestion>> {
        let s = self.lock()?;
        Ok(
            ml_faces::people::name_suggestions(s.core.index(), threshold.unwrap_or(SAME_PERSON))
                .map_err(failure)?
                .into_iter()
                .map(|a| PeopleNameSuggestion {
                    unnamed_id: a.unnamed_id,
                    named_id: a.named_id,
                    name: a.name,
                    similarity: a.similarity,
                })
                .collect(),
        )
    }

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

    /// People in this queue, most frequent first. `refresh` ingests new faces
    /// and runs the job's periodic refit; use `refresh_people(true)` to force it.
    pub fn people(&self, refresh: bool) -> Result<Vec<PersonInfo>> {
        let mut s = self.lock()?;
        if refresh {
            s.refresh_people_job(false)?;
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
        if s.assist.people.is_none() {
            s.refresh_people_job(false)?;
        }
        let found = person_frames(
            s.core.index(),
            s.core.images(),
            &person_id,
            eyes_closed_below,
        )?;
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
