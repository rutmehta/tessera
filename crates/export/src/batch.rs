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
        crate::require_full_quality_source(&item.image.source)?;
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
            pipeline_cpu::RenderSource::Rgb(image)
            | pipeline_cpu::RenderSource::StoredRgb { image, .. } => {
                (image.width(), image.height())
            }
            pipeline_cpu::RenderSource::Cfa { metadata, .. } => (metadata.width, metadata.height),
            pipeline_cpu::RenderSource::CameraLinear(proxy) if proxy.is_external_dng() => {
                (proxy.pixels().width(), proxy.pixels().height())
            }
            pipeline_cpu::RenderSource::CameraLinear(_) => return Err(crate::original_required()),
        };
        let (ow, oh) = settings.resize.dimensions(w, h)?;
        max_output = max_output.max(u64::from(ow) * u64::from(oh));
        let pixels = (u64::from(w) * u64::from(h))
            .max(u64::from(ow) * u64::from(oh))
            .max(u64::from(ow) * u64::from(h));
        max_bytes = max_bytes.max(pixels.saturating_mul(64));
    }
    if jobs > 1 && std::env::var("TESSERA_EXPORT_BACKEND").as_deref() != Ok("cpu") {
        let renders = pipeline_renders(items, settings, max_output);
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

/// Share of physical memory two Adobe-process renders may hold together
/// (ENG-10): one eighth. Pairing a full-size 16 MP batch measured about 30%
/// faster than rendering one at a time, at about 1 GB more peak memory, so
/// machines with room pair and small ones do not.
const ADOBE_PAIR_MEMORY_SHARE: u64 = 8;
/// The pair budget when physical memory cannot be read.
const ADOBE_PAIR_FALLBACK_BYTES: u64 = 3 << 29;

/// Host working set two Adobe-process renders may hold at once.
fn adobe_pair_budget() -> u64 {
    physical_memory().map_or(ADOBE_PAIR_FALLBACK_BYTES, |m| m / ADOBE_PAIR_MEMORY_SHARE)
}

fn physical_memory() -> Option<u64> {
    // SAFETY: sysconf only reads system configuration values.
    let (pages, size) = unsafe {
        (
            libc::sysconf(libc::_SC_PHYS_PAGES),
            libc::sysconf(libc::_SC_PAGESIZE),
        )
    };
    (pages > 0 && size > 0).then(|| (pages as u64).saturating_mul(size as u64))
}

/// Peak host bytes of one Adobe-process render (Develop's renderer, see
/// `adobe_render`) at `render_scale`, from the 16 MP ARW measurements in
/// the ENG-10 HANDOFF: the renderer's own source copy (4 B per sensor
/// sample, 12 B per RGB or proxy pixel), about 96 MiB of demosaic chunk
/// scratch, and 41-52 B per developed level pixel for the renderer's level
/// frames plus the rendered output awaiting encode (56 B in all).
fn adobe_render_bytes(source: &pipeline_cpu::RenderSource<'_>, render_scale: u32) -> u64 {
    let (pixels, source_bytes) = match source {
        pipeline_cpu::RenderSource::Cfa { metadata, .. } => {
            let px = u64::from(metadata.width) * u64::from(metadata.height);
            (px, 4 * px)
        }
        pipeline_cpu::RenderSource::Rgb(image)
        | pipeline_cpu::RenderSource::StoredRgb { image, .. } => {
            let px = u64::from(image.width()) * u64::from(image.height());
            (px, 12 * px)
        }
        pipeline_cpu::RenderSource::CameraLinear(proxy) => {
            let px = u64::from(proxy.pixels().width()) * u64::from(proxy.pixels().height());
            (px, 12 * px)
        }
    };
    let scale = u64::from(render_scale.max(1));
    source_bytes + (96 << 20) + 56 * pixels / (scale * scale)
}

/// Renders [`export_pipeline`] runs at once. Two renders overlap one
/// image's CPU work (sensor copy, lens analysis) with the other's GPU bands
/// when outputs are small enough to hold three at once (two rendering, one
/// encoding). Adobe-process renders hold level-size float frames on the
/// host, so two of them must also fit [`adobe_pair_budget`] together.
fn pipeline_renders(items: &[ExportItem<'_>], settings: &ExportSettings, max_output: u64) -> usize {
    pipeline_renders_within(items, settings, max_output, adobe_pair_budget())
}

fn pipeline_renders_within(
    items: &[ExportItem<'_>],
    settings: &ExportSettings,
    max_output: u64,
    adobe_pair_budget: u64,
) -> usize {
    let adobe = items
        .iter()
        .filter(|item| crate::is_adobe(item.recipe))
        .map(|item| adobe_render_bytes(&item.image.source, settings.render_scale))
        .max()
        .unwrap_or(0);
    if max_output <= PIPELINE_PAIR_PIXELS && 2 * adobe <= adobe_pair_budget {
        2
    } else {
        1
    }
}

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
        crate::require_full_quality_source(&item.image.source)?;
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

    /// A RAW item whose metadata claims `w`x`h` (admission reads only the
    /// metadata; the sensor plane is a stand-in).
    fn admitted(
        w: u32,
        h: u32,
        process: engine_api::recipe::ProcessVersion,
        settings: &ExportSettings,
        memory: u64,
    ) -> usize {
        let cfa = raw_decode::CfaImage::from_linear(2, 2, vec![0.1; 4]).unwrap();
        let metadata = raw_decode::RawMetadata {
            make: "synthetic".into(),
            model: "camera".into(),
            lens: None,
            iso: 100.,
            shutter_s: 0.01,
            aperture: 4.,
            focal_mm: 50.,
            capture_time: 0,
            catalog_orientation: None,
            baseline_exposure: 0.,
            orientation: 1,
            width: w,
            height: h,
            cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [1, 2]]),
            black_levels: [0.; 4],
            white_level: 65535,
            as_shot_wb: [2., 1., 1.5, 1.],
            camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
            cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0.; 3]],
            rgb_cam: [[0.; 4]; 3],
            default_crop: [0, 0, w, h],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        };
        let recipe = Recipe {
            process_version: process,
            ..Default::default()
        };
        let items = [ExportItem {
            image: ExportImage {
                source: pipeline_cpu::RenderSource::Cfa {
                    image: &cfa,
                    metadata: &metadata,
                },
                name: "photo",
                sequence: 1,
                date: "",
                metadata: None,
            },
            recipe: &recipe,
        }];
        let (ow, oh) = settings.resize.dimensions(w, h).unwrap();
        pipeline_renders_within(
            &items,
            settings,
            u64::from(ow) * u64::from(oh),
            memory / ADOBE_PAIR_MEMORY_SHARE,
        )
    }

    /// ENG-10: an Adobe-process render holds several level-size float frames
    /// on the host (Develop's renderer), unlike the resident GPU bands of a
    /// Native render. Two run at once only while both fit an eighth of
    /// physical memory: on an 8 GiB machine full-size 16 and 24 MP Adobe
    /// exports render one at a time and Web-sized ones (render scale 2) still
    /// pair; on a 48 GiB machine full-size ones pair too. Native admission is
    /// unchanged on both.
    #[test]
    fn eng10_pipeline_pairs_adobe_renders_only_within_the_memory_budget() {
        use engine_api::recipe::ProcessVersion;
        const GIB: u64 = 1 << 30;
        let full = ExportSettings::default();
        let web = ExportSettings {
            resize: crate::Resize::LongEdge(2048),
            render_scale: 2,
            ..Default::default()
        };
        for (w, h) in [(4928, 3276), (6000, 4000)] {
            for (memory, full_pairs) in [(8 * GIB, 1), (48 * GIB, 2)] {
                assert_eq!(
                    admitted(w, h, ProcessVersion::adobe(6), &full, memory),
                    if u64::from(w) * u64::from(h) <= PIPELINE_PAIR_PIXELS {
                        full_pairs
                    } else {
                        1
                    },
                    "{w}x{h} full, {} GiB",
                    memory / GIB
                );
                assert_eq!(
                    admitted(w, h, ProcessVersion::adobe(6), &web, memory),
                    2,
                    "{w}x{h} web, {} GiB",
                    memory / GIB
                );
                // Native keeps its output-size rule (pairs up to 16 Mi pixels).
                assert_eq!(
                    admitted(w, h, ProcessVersion::NATIVE_CURRENT, &full, memory),
                    if u64::from(w) * u64::from(h) <= PIPELINE_PAIR_PIXELS {
                        2
                    } else {
                        1
                    },
                    "{w}x{h} native"
                );
                assert_eq!(
                    admitted(w, h, ProcessVersion::NATIVE_CURRENT, &web, memory),
                    2,
                    "{w}x{h} native web"
                );
            }
        }
        assert!(adobe_pair_budget() >= GIB / 8);
    }

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
