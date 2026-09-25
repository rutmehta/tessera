use crate::Segmenter;
use engine_api::{
    EngineError, EngineResult,
    jobs::{Job, JobContext, Priority},
};
use image::RgbImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrecomputeKind {
    Subject,
    Sky,
}
/// Injectable worker boundary. Implementations must check cancellation before
/// publishing results. ORT calls themselves are not interruptible.
pub trait Precomputer: Send {
    fn precompute(
        &mut self,
        image: &RgbImage,
        kind: PrecomputeKind,
        level: u8,
        ctx: &JobContext,
    ) -> anyhow::Result<()>;
}
impl Precomputer for Segmenter {
    fn precompute(
        &mut self,
        image: &RgbImage,
        kind: PrecomputeKind,
        level: u8,
        ctx: &JobContext,
    ) -> anyhow::Result<()> {
        ctx.check_cancelled()?;
        self.cancellation = Some(ctx.cancellation.clone());
        let result = match kind {
            PrecomputeKind::Subject => self.subject(image, level),
            PrecomputeKind::Sky => self.sky(image, level),
        };
        self.cancellation = None;
        ctx.check_cancelled()?;
        result.map(|_| ())
    }
}
/// One oriented preview per job, avoiding an unbounded decoded folder batch.
/// Construction does no I/O; supply an already loaded Segmenter.
pub struct PrecomputeJob {
    worker: Box<dyn Precomputer>,
    image: RgbImage,
    level: u8,
}
impl PrecomputeJob {
    pub fn new(worker: Box<dyn Precomputer>, image: RgbImage, level: u8) -> Self {
        Self {
            worker,
            image,
            level,
        }
    }
}
impl Job for PrecomputeJob {
    fn label(&self) -> &str {
        "precompute subject and sky masks"
    }
    fn priority(&self) -> Priority {
        Priority::Score
    }
    fn run(mut self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        for (index, kind) in [PrecomputeKind::Subject, PrecomputeKind::Sky]
            .into_iter()
            .enumerate()
        {
            ctx.check_cancelled()?;
            let result = self.worker.precompute(&self.image, kind, self.level, ctx);
            ctx.check_cancelled()?;
            result.map_err(|e| EngineError::Model {
                model: "segment/u2net+sky-prior".into(),
                message: format!("{e:#}"),
            })?;
            ctx.report_progress((index + 1) as f32 / 2., Some(self.label()));
        }
        ctx.check_cancelled()
    }
}
