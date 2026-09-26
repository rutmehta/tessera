use engine_api::{
    id::JobId,
    jobs::{CancellationToken, Job, JobContext, Priority},
};
use ml_caption::{ImageUnderstanding, UnderstandingJob};
use std::sync::{Arc, Mutex};
struct Inference {
    batches: Arc<Mutex<Vec<usize>>>,
}
impl ImageUnderstanding for Inference {
    fn model_version(&self) -> String {
        "test/v1".into()
    }
    fn analyze_batch(
        &mut self,
        images: &[image::RgbImage],
    ) -> anyhow::Result<Vec<index::Understanding>> {
        self.batches.lock().unwrap().push(images.len());
        Ok(images
            .iter()
            .map(|_| index::Understanding {
                model_version: "test/v1".into(),
                keywords: vec![],
                caption: "an image".into(),
                alt_text: "image".into(),
                ocr: vec![],
            })
            .collect())
    }
}
#[test]
fn job_batches_persists_resumes_and_checks_cancellation() {
    let tmp = tempfile::tempdir().unwrap();
    let catalog = tmp.path().join("index.sqlite");
    let mut index = index::Index::open(&catalog).unwrap();
    for n in 0..9 {
        image::RgbImage::new(8, 8)
            .save(tmp.path().join(format!("{n}.jpg")))
            .unwrap();
    }
    index
        .scan(
            tmp.path(),
            &index::NoopSidecarReader,
            &index::NoopMetadataProvider,
        )
        .unwrap();
    let ids = index.search(&index::Query::default()).unwrap();
    let batches = Arc::new(Mutex::new(vec![]));
    let make = || {
        UnderstandingJob::new(
            catalog.clone(),
            tmp.path().join("previews"),
            ids.clone(),
            Box::new(Inference {
                batches: batches.clone(),
            }),
        )
    };
    assert_eq!(make().priority(), Priority::Score);
    jobs::blocking_run(Box::new(make())).unwrap();
    assert_eq!(*batches.lock().unwrap(), [4, 4, 1]);
    for id in &ids {
        assert_eq!(
            index.understanding(*id).unwrap().unwrap().caption,
            "an image"
        );
    }
    jobs::blocking_run(Box::new(make())).unwrap();
    assert_eq!(*batches.lock().unwrap(), [4, 4, 1]);
    let token = CancellationToken::new();
    token.cancel();
    let ctx = JobContext::new(JobId(1), token, None);
    assert!(Box::new(make()).run(&ctx).is_err());
    assert_eq!(*batches.lock().unwrap(), [4, 4, 1]);
    assert!(
        !std::fs::read_dir(tmp.path()).unwrap().any(|p| p
            .unwrap()
            .path()
            .extension()
            .is_some_and(|e| e == "xmp"))
    );
}
