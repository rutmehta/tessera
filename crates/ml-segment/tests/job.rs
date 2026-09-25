use engine_api::{
    EngineError,
    id::JobId,
    jobs::{CancellationToken, Job, JobContext, Priority},
};
use image::RgbImage;
use ml_segment::{PrecomputeJob, PrecomputeKind, Precomputer};
use std::sync::{Arc, Mutex};
struct Recorder(Arc<Mutex<Vec<PrecomputeKind>>>, bool);
impl Precomputer for Recorder {
    fn precompute(
        &mut self,
        _: &RgbImage,
        kind: PrecomputeKind,
        _: u8,
        ctx: &JobContext,
    ) -> anyhow::Result<()> {
        self.0.lock().unwrap().push(kind);
        if self.1 {
            ctx.cancellation.cancel();
        }
        Ok(())
    }
}
#[test]
fn score_job_precomputes_both_and_checks_cancellation() {
    for (before, during, count) in [(false, false, 2), (true, false, 0), (false, true, 1)] {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let job = PrecomputeJob::new(
            Box::new(Recorder(calls.clone(), during)),
            RgbImage::new(10, 10),
            1,
        );
        assert_eq!(job.priority(), Priority::Score);
        let token = CancellationToken::new();
        if before {
            token.cancel();
        }
        let result = Box::new(job).run(&JobContext::new(JobId(1), token, None));
        assert_eq!(calls.lock().unwrap().len(), count);
        if before || during {
            assert_eq!(result, Err(EngineError::Cancelled));
        } else {
            result.unwrap();
            assert_eq!(
                *calls.lock().unwrap(),
                vec![PrecomputeKind::Subject, PrecomputeKind::Sky]
            );
        }
    }
}
