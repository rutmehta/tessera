use engine_api::{
    id::JobId,
    jobs::{CancellationToken, Job, JobContext, Priority},
};
use image::RgbImage;
use index::{Index, NoopMetadataProvider, NoopSidecarReader, Query};
use ml_embed::{EmbedFolderJob, ImageEmbedder, vector::SqliteVectorIndex};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
};

struct Fake {
    calls: Arc<Mutex<Vec<usize>>>,
    version: &'static str,
}
impl ImageEmbedder for Fake {
    fn model_version(&self) -> &str {
        self.version
    }
    fn dimension(&self) -> usize {
        2
    }
    fn embed_images(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>> {
        self.calls.lock().unwrap().push(images.len());
        Ok(images.iter().map(|_| vec![3., 4.]).collect())
    }
}
struct Fixture {
    _tmp: tempfile::TempDir,
    catalog: PathBuf,
    index: PathBuf,
    folder: PathBuf,
}
impl Fixture {
    fn new(count: usize) -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().canonicalize().unwrap();
        let folder = root.join("photos");
        std::fs::create_dir(&folder).unwrap();
        for i in 0..count {
            RgbImage::new(12, 8)
                .save(folder.join(format!("{i}.jpg")))
                .unwrap();
        }
        let catalog = root.join("catalog.sqlite");
        let mut db = Index::open(&catalog).unwrap();
        db.scan(&folder, &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        Self {
            _tmp: tmp,
            catalog,
            index: root.join("vectors"),
            folder,
        }
    }
    fn job(&self, calls: Arc<Mutex<Vec<usize>>>, version: &'static str) -> Box<dyn Job> {
        Box::new(EmbedFolderJob::new(
            self.catalog.clone(),
            self.index.clone(),
            self.folder.clone(),
            Box::new(Fake { calls, version }),
        ))
    }
    fn count(&self, version: &str) -> usize {
        SqliteVectorIndex::open(&self.index, version, 2)
            .unwrap()
            .rows()
            .unwrap()
            .len()
    }
}
fn context() -> JobContext {
    JobContext::new(JobId(1), CancellationToken::new(), None)
}
#[test]
fn job_batches_eight_and_persists_vectors() {
    let f = Fixture::new(19);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let job = f.job(calls.clone(), "v1");
    assert_eq!(job.priority(), Priority::Score);
    jobs::blocking_run(job).unwrap();
    assert_eq!(*calls.lock().unwrap(), [8, 8, 3]);
    assert_eq!(f.count("v1"), 19);
    for (_, v) in SqliteVectorIndex::open(&f.index, "v1", 2)
        .unwrap()
        .rows()
        .unwrap()
    {
        assert_eq!(v, [0.6, 0.8]);
    }
}

#[test]
fn persisted_current_version_skips_missing_previews_but_new_version_reembeds() {
    let f = Fixture::new(2);
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    jobs::blocking_run(f.job(calls.clone(), "v2")).unwrap();
    std::fs::remove_dir_all(&f.folder).unwrap();
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(*calls.lock().unwrap(), [2, 2]);
    assert_eq!(f.count("v1"), 2);
    assert_eq!(f.count("v2"), 2);
}

#[test]
fn folder_components_include_descendants_not_prefix_siblings() {
    let mut f = Fixture::new(0);
    let root = f.folder.parent().unwrap().to_path_buf();
    f.folder = root.join("a_[x]%");
    for folder in [
        &f.folder,
        &f.folder.join("nested"),
        &root.join("a_[x]%other"),
    ] {
        std::fs::create_dir_all(folder).unwrap();
        RgbImage::new(12, 8).save(folder.join("photo.jpg")).unwrap();
    }
    Index::open(&f.catalog)
        .unwrap()
        .scan(&root, &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(f.count("v1"), 2);
    assert_eq!(*calls.lock().unwrap(), [2]);
}
#[test]
fn empty_folder_does_not_infer() {
    let f = Fixture::new(0);
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(f.count("v1"), 0);
}

#[test]
fn folder_alias_is_resolved_before_catalog_matching() {
    let mut f = Fixture::new(1);
    f.folder = f.folder.join("..").join("photos");
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(f.count("v1"), 1);
    assert_eq!(*calls.lock().unwrap(), [1]);
}

#[test]
fn cancellation_before_run_does_no_io() {
    let f = Fixture::new(1);
    std::fs::remove_file(&f.catalog).unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ctx = context();
    ctx.cancellation.cancel();
    assert_eq!(
        f.job(calls.clone(), "v1").run(&ctx),
        Err(engine_api::EngineError::Cancelled)
    );
    assert!(!f.catalog.exists());
    assert!(!f.index.exists());
    assert!(calls.lock().unwrap().is_empty());
}
struct CancelInference {
    token: CancellationToken,
    calls: Arc<Mutex<Vec<usize>>>,
    on_call: usize,
}
impl ImageEmbedder for CancelInference {
    fn model_version(&self) -> &str {
        "v1"
    }
    fn dimension(&self) -> usize {
        2
    }
    fn embed_images(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>> {
        let mut calls = self.calls.lock().unwrap();
        calls.push(images.len());
        if calls.len() == self.on_call {
            self.token.cancel();
        }
        Ok(vec![vec![1., 0.]; images.len()])
    }
}
#[test]
fn cancellation_after_inference_discards_batch_preserves_prior_batches() {
    let f = Fixture::new(19);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let ctx = context();
    let job: Box<dyn Job> = Box::new(EmbedFolderJob::new(
        f.catalog.clone(),
        f.index.clone(),
        f.folder.clone(),
        Box::new(CancelInference {
            token: ctx.cancellation.clone(),
            calls: calls.clone(),
            on_call: 2,
        }),
    ));
    assert_eq!(job.run(&ctx), Err(engine_api::EngineError::Cancelled));
    assert_eq!(*calls.lock().unwrap(), [8, 8]);
    assert_eq!(f.count("v1"), 8);
}

struct ShapeEmbedder {
    expected: (u32, u32),
}
impl ImageEmbedder for ShapeEmbedder {
    fn model_version(&self) -> &str {
        "v1"
    }
    fn dimension(&self) -> usize {
        2
    }
    fn embed_images(&mut self, images: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>> {
        assert_eq!(images.len(), 1);
        assert_eq!(images[0].dimensions(), self.expected);
        Ok(vec![vec![1., 0.]])
    }
}
#[test]
fn jpeg_exif_orientation_is_applied_before_inference() {
    let f = Fixture::new(1);
    let path = f.folder.join("0.jpg");
    let jpeg = std::fs::read(&path).unwrap();
    // APP1: Exif, little-endian TIFF, one SHORT Orientation=6 entry.
    let exif: &[u8] = b"Exif\0\0II\x2a\0\x08\0\0\0\x01\0\x12\x01\x03\0\x01\0\0\0\x06\0\0\0\0\0\0\0";
    let mut oriented = jpeg[..2].to_vec();
    oriented.extend([0xff, 0xe1]);
    oriented.extend(((exif.len() + 2) as u16).to_be_bytes());
    oriented.extend(exif);
    oriented.extend(&jpeg[2..]);
    std::fs::write(path, oriented).unwrap();
    jobs::blocking_run(Box::new(EmbedFolderJob::new(
        f.catalog.clone(),
        f.index.clone(),
        f.folder.clone(),
        Box::new(ShapeEmbedder { expected: (8, 12) }),
    )))
    .unwrap();
    assert_eq!(f.count("v1"), 1);
}

#[test]
fn raw_preview_is_loaded_through_preview_store() {
    let f = Fixture::new(0);
    let raw = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/sony-arw.ARW");
    assert!(
        raw.is_file(),
        "RAW fixture required for preview integration test"
    );
    std::fs::copy(raw, f.folder.join("photo.ARW")).unwrap();
    Index::open(&f.catalog)
        .unwrap()
        .scan(&f.folder, &NoopSidecarReader, &NoopMetadataProvider)
        .unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(*calls.lock().unwrap(), [1]);
    assert_eq!(f.count("v1"), 1);
}

struct ShortOutput;
impl ImageEmbedder for ShortOutput {
    fn model_version(&self) -> &str {
        "v1"
    }
    fn dimension(&self) -> usize {
        2
    }
    fn embed_images(&mut self, _: &[RgbImage]) -> anyhow::Result<Vec<Vec<f32>>> {
        Ok(vec![vec![1., 0.]])
    }
}
#[test]
fn mismatched_inference_batch_is_an_error_not_silent_partial_success() {
    let f = Fixture::new(2);
    let result = jobs::blocking_run(Box::new(EmbedFolderJob::new(
        f.catalog.clone(),
        f.index.clone(),
        f.folder.clone(),
        Box::new(ShortOutput),
    )));
    assert!(result.is_err());
    assert_eq!(f.count("v1"), 0);
}
#[test]
fn corrupt_preview_returns_error_preserving_completed_batches() {
    let f = Fixture::new(9);
    let db = Index::open(&f.catalog).unwrap();
    let ids = db.search(&Query::default()).unwrap();
    let bad = db.image_info(*ids.last().unwrap()).unwrap().path;
    std::fs::write(&bad, b"corrupt").unwrap();
    let calls = Arc::new(Mutex::new(Vec::new()));
    assert!(jobs::blocking_run(f.job(calls.clone(), "v1")).is_err());
    assert_eq!(*calls.lock().unwrap(), [8]);
    assert_eq!(f.count("v1"), 8);
    RgbImage::new(12, 8).save(bad).unwrap();
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(*calls.lock().unwrap(), [8, 1]);
    assert_eq!(f.count("v1"), 9);
}
struct CancelProgress(CancellationToken);
impl engine_api::jobs::ProgressSink for CancelProgress {
    fn report(&self, fraction: f32, _: Option<&str>) {
        if fraction > 0.0 {
            self.0.cancel();
        }
    }
}
#[test]
fn cancellation_between_writes_preserves_only_written_rows() {
    let f = Fixture::new(9);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let token = CancellationToken::new();
    let ctx = JobContext::new(
        JobId(1),
        token.clone(),
        Some(Arc::new(CancelProgress(token))),
    );
    assert_eq!(
        f.job(calls.clone(), "v1").run(&ctx),
        Err(engine_api::EngineError::Cancelled)
    );
    assert_eq!(f.count("v1"), 1);
    assert_eq!(*calls.lock().unwrap(), [8]);
}

#[test]
fn folder_job_is_not_truncated_at_catalog_default_page_size() {
    let f = Fixture::new(113);
    let calls = Arc::new(Mutex::new(Vec::new()));
    jobs::blocking_run(f.job(calls.clone(), "v1")).unwrap();
    assert_eq!(f.count("v1"), 113);
    assert_eq!(calls.lock().unwrap().iter().sum::<usize>(), 113);
}
#[test]
fn scheduler_executes_job_and_reports_success() {
    use engine_api::jobs::{JobStatus, Scheduler};
    let f = Fixture::new(1);
    let pool = jobs::ThreadPoolScheduler::new(1);
    let calls = Arc::new(Mutex::new(Vec::new()));
    let handle = pool.submit(f.job(calls, "v1"), None);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    loop {
        match pool.status(handle.id) {
            JobStatus::Queued | JobStatus::Running { .. } => {
                assert!(std::time::Instant::now() < deadline);
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            status => {
                assert_eq!(status, JobStatus::Succeeded);
                break;
            }
        }
    }
    assert_eq!(f.count("v1"), 1);
}
