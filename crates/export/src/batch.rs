use crate::{ExportImage, ExportSettings, encode_error, filename, prepare};
use engine_api::{EngineError, EngineResult, jobs::CancellationToken, recipe::Recipe};
use rayon::prelude::*;
use std::{collections::HashSet, path::PathBuf, sync::mpsc};

pub struct ExportItem<'a> {
    pub image: ExportImage<'a>,
    pub recipe: &'a Recipe,
}
#[derive(Clone, Debug)]
pub struct Progress {
    pub index: usize,
    /// Successfully committed images in this invocation (not attempted images).
    pub completed: usize,
    pub total: usize,
    pub result: EngineResult<PathBuf>,
}
#[derive(Debug)]
pub struct BatchReport {
    /// Input order. Cancelled/unstarted items are Err(Cancelled).
    pub results: Vec<EngineResult<PathBuf>>,
}
impl BatchReport {
    /// Resubmit these items with a fresh token and their original naming context.
    /// Existing files are never blindly treated as completed or overwritten.
    pub fn remaining(&self) -> Vec<usize> {
        self.results
            .iter()
            .enumerate()
            .filter_map(|(i, r)| r.is_err().then_some(i))
            .collect()
    }
}

/// Synchronous coordinator, intended for an Export-priority background job.
/// The callback runs on the calling thread, once per success/failure (not cancel).
/// Rendering/encoding is bounded by cores and a conservative 512 MiB estimate,
/// with a minimum of one image. No unbounded queue of pixel buffers is retained.
/// A cancelled run returns its partial report; remaining() is the resume cursor.
pub fn export_batch(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    progress: impl Fn(Progress),
    cancel: &CancellationToken,
) -> EngineResult<BatchReport> {
    export_batch_with_jobs(items, settings, progress, cancel, usize::MAX)
}

/// Like `export_batch`, but limits simultaneous export workers to `jobs`.
pub fn export_batch_with_jobs(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    progress: impl Fn(Progress),
    cancel: &CancellationToken,
    jobs: usize,
) -> EngineResult<BatchReport> {
    if jobs == 0 {
        return Err(EngineError::invalid("jobs", "must be positive"));
    }
    let mut report = BatchReport {
        results: vec![Err(EngineError::Cancelled); items.len()],
    };
    if items.is_empty() || cancel.is_cancelled() {
        return Ok(report);
    }
    settings.format.validate()?;
    let mut names = HashSet::new();
    let mut max_bytes = 1;
    for item in items {
        let name = filename(
            &settings.naming,
            item.image.name,
            item.image.sequence,
            item.image.date,
            settings.format.extension(),
        )?;
        // Conservative on case-sensitive filesystems, safe on macOS defaults.
        if !names.insert(name.to_lowercase()) {
            return Err(EngineError::invalid("naming", "duplicate output names"));
        }
        let (w, h) = match &item.image.source {
            pipeline_cpu::RenderSource::Rgb(image) => (image.width(), image.height()),
            pipeline_cpu::RenderSource::Cfa { metadata, .. } => (metadata.width, metadata.height),
        };
        let (ow, oh) = settings.resize.dimensions(w, h)?;
        let pixels = (u64::from(w) * u64::from(h))
            .max(u64::from(ow) * u64::from(oh))
            .max(u64::from(ow) * u64::from(h));
        max_bytes = max_bytes.max(pixels.saturating_mul(64));
    }
    let cores = std::thread::available_parallelism().map_or(1, usize::from);
    let workers = cores
        .min(jobs)
        .min(items.len())
        .min((512 * 1024 * 1024 / max_bytes).max(1) as usize);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .map_err(encode_error)?;
    let (tx, rx) = mpsc::sync_channel(workers);
    std::thread::scope(|scope| {
        scope.spawn(move || {
            // Bound image admission as well as OS threads: nested Rayon row
            // work can otherwise steal more images while retaining full buffers.
            for (wave, chunk) in items.chunks(workers).enumerate() {
                if cancel.is_cancelled() {
                    break;
                }
                pool.install(|| {
                    chunk
                        .par_iter()
                        .enumerate()
                        .for_each_with(tx.clone(), |tx, (offset, item)| {
                            let result = prepare(&item.image, item.recipe, settings, cancel);
                            let _ = tx.send((wave * workers + offset, result));
                        });
                });
            }
        });
        let mut completed = 0;
        for (index, prepared) in rx {
            let result = prepared.and_then(|p| p.commit(cancel));
            if result.is_ok() {
                completed += 1;
            }
            report.results[index] = result.clone();
            if !matches!(result, Err(EngineError::Cancelled)) {
                progress(Progress {
                    index,
                    completed,
                    total: items.len(),
                    result,
                });
            }
        }
    });
    Ok(report)
}
