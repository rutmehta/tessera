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
    let mut max_output = 0;
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
        max_output = max_output.max(u64::from(ow) * u64::from(oh));
        let pixels = (u64::from(w) * u64::from(h))
            .max(u64::from(ow) * u64::from(oh))
            .max(u64::from(ow) * u64::from(h));
        max_bytes = max_bytes.max(pixels.saturating_mul(64));
    }
    if jobs > 1 && std::env::var("TESSERA_EXPORT_BACKEND").as_deref() != Ok("cpu") {
        // Two renders overlap one image's CPU work (sensor copy, lens
        // analysis) with the other's GPU bands when outputs are small enough
        // to hold three at once (two rendering, one encoding).
        let renders = if max_output <= PIPELINE_PAIR_PIXELS {
            2
        } else {
            1
        };
        return export_pipeline(items, settings, progress, cancel, renders);
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

/// Largest output (pixels) for which two renders run at once: Web and
/// screen presets, not full-size float frames of large sensors.
const PIPELINE_PAIR_PIXELS: u64 = 16 << 20;

/// A rendezvous channel admits `renders` renders while the caller encodes the
/// prior frame (JPEG encoding itself is stripe-parallel). The zero-capacity
/// queue cannot accumulate full-resolution outputs.
fn export_pipeline(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    progress: impl Fn(Progress),
    cancel: &CancellationToken,
    renders: usize,
) -> EngineResult<BatchReport> {
    let mut report = BatchReport {
        results: vec![Err(EngineError::Cancelled); items.len()],
    };
    let (tx, rx) = mpsc::sync_channel(0);
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..renders.max(1) {
            let (tx, next) = (tx.clone(), &next);
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(item) = items.get(index) else {
                        break;
                    };
                    if cancel.is_cancelled() {
                        break;
                    }
                    let rendered = crate::render_one_cancellable(
                        &item.image,
                        item.recipe,
                        settings,
                        cancel,
                        None,
                        None,
                    );
                    if tx.send((index, rendered)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        let mut completed = 0;
        for (index, rendered) in rx {
            let result = rendered.and_then(|r| r.finish(cancel));
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

/// Serial SR batch sharing one loaded session. Cancellation is checked before
/// each image and before publication; an in-flight model invocation may finish.
/// Results/progress have the same semantics as `export_batch`.
pub fn export_batch_upscaled(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    progress: impl Fn(Progress),
    cancel: &CancellationToken,
    upscale: &mut ml_enhance::SuperResolution,
) -> EngineResult<BatchReport> {
    export_serial(items, settings, progress, cancel, |item| {
        crate::prepare_enhanced(&item.image, item.recipe, settings, cancel, Some(upscale))
    })
}

fn export_serial(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    progress: impl Fn(Progress),
    cancel: &CancellationToken,
    mut prepare_item: impl FnMut(&ExportItem<'_>) -> EngineResult<crate::PreparedExport>,
) -> EngineResult<BatchReport> {
    let mut report = BatchReport {
        results: vec![Err(EngineError::Cancelled); items.len()],
    };
    if items.is_empty() || cancel.is_cancelled() {
        return Ok(report);
    }
    settings.format.validate()?;
    let mut names = HashSet::new();
    for item in items {
        let name = filename(
            &settings.naming,
            item.image.name,
            item.image.sequence,
            item.image.date,
            settings.format.extension(),
        )?;
        if !names.insert(name.to_lowercase()) {
            return Err(EngineError::invalid("naming", "duplicate output names"));
        }
    }
    let mut completed = 0;
    for (index, item) in items.iter().enumerate() {
        if cancel.is_cancelled() {
            break;
        }
        let result = prepare_item(item).and_then(|prepared| prepared.commit(cancel));
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
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_cancellation_before_commit_discards_prepared_files() {
        let dir = tempfile::tempdir().unwrap();
        let settings = ExportSettings {
            output_dir: dir.path().into(),
            ..Default::default()
        };
        let pixels = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let recipe = Recipe::default();
        let items: Vec<_> = (1..=2)
            .map(|sequence| ExportItem {
                image: ExportImage {
                    source: pipeline_cpu::RenderSource::Rgb(&pixels),
                    name: "photo",
                    sequence,
                    date: "",
                    metadata: None,
                },
                recipe: &recipe,
            })
            .collect();
        let cancel = CancellationToken::new();
        let report = export_serial(
            &items,
            &settings,
            |_| panic!("no publication"),
            &cancel,
            |item| {
                let prepared = prepare(&item.image, item.recipe, &settings, &cancel)?;
                cancel.cancel();
                Ok(prepared)
            },
        )
        .unwrap();
        assert_eq!(report.remaining(), vec![0, 1]);
        assert!(
            report
                .results
                .iter()
                .all(|r| matches!(r, Err(EngineError::Cancelled)))
        );
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}
