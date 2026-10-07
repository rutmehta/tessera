//! Photo jobs over UniFFI. Callbacks run without catalog/status locks.
//! Cancellation is cooperative between decode/merge/inference/publication stages.
use crate::{Engine, Result, catalog, failure, parse_id};
use ::merge::{LinearImage, hdr, pano};
use engine_api::recipe::Recipe;
use std::{
    collections::HashSet,
    path::PathBuf,
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum MergeKind {
    #[default]
    Hdr,
    Panorama,
    HdrPanorama,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum MergeDeghost {
    None,
    Low,
    #[default]
    Medium,
    High,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum MergeProjection {
    #[default]
    Auto,
    Spherical,
    Cylindrical,
    Perspective,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct MergeOptions {
    pub kind: MergeKind,
    pub auto_align: bool,
    pub auto_tone: bool,
    pub deghost: MergeDeghost,
    pub projection: MergeProjection,
    pub boundary_warp: u8,
    pub fill_edges: bool,
    pub create_stack: bool,
    /// Optional calibrated focal length in source pixels, required for curved projections.
    pub focal_pixels: Option<f64>,
    /// HDR panorama: explicit sequential bracket lengths. Never infer groups from names.
    pub bracket_sizes: Vec<u32>,
    /// Optional positive sensor exposure values, in input order. Otherwise read EXIF.
    /// Useful for LinearRaw DNGs without exposure tags. No assumed equal exposures.
    pub exposure_values: Vec<f64>,
}
impl Default for MergeOptions {
    fn default() -> Self {
        Self {
            kind: MergeKind::Hdr,
            auto_align: true,
            auto_tone: true,
            deghost: MergeDeghost::Medium,
            projection: MergeProjection::Auto,
            boundary_warp: 0,
            fill_edges: false,
            create_stack: true,
            focal_pixels: None,
            bracket_sizes: vec![],
            exposure_values: vec![],
        }
    }
}
impl MergeOptions {
    fn validate(&self, count: usize) -> Result<()> {
        if !(2..=128).contains(&count) {
            return Err(failure("merge requires 2..128 distinct images"));
        }
        if self.kind == MergeKind::Hdr && count > 64 {
            return Err(failure("HDR supports at most 64 frames"));
        }
        if self.boundary_warp > 100 {
            return Err(failure("boundary warp must be 0..=100"));
        }

        if self.focal_pixels.is_some_and(|v| !v.is_finite() || v <= 0.) {
            return Err(failure("focal_pixels must be positive and finite"));
        }
        if self.kind != MergeKind::Hdr
            && matches!(
                self.projection,
                MergeProjection::Cylindrical | MergeProjection::Spherical
            )
            && self.focal_pixels.is_none()
        {
            return Err(failure("curved projection requires focal_pixels"));
        }
        if !self.exposure_values.is_empty()
            && (self.exposure_values.len() != count
                || self
                    .exposure_values
                    .iter()
                    .any(|v| !v.is_finite() || *v <= 0.))
        {
            return Err(failure(
                "exposure_values must contain one positive finite value per image",
            ));
        }
        if self.kind == MergeKind::HdrPanorama {
            if self.bracket_sizes.len() < 2
                || self.bracket_sizes.iter().any(|n| !(2..=64).contains(n))
                || self.bracket_sizes.iter().map(|n| *n as u64).sum::<u64>() != count as u64
            {
                return Err(failure(
                    "HDR panorama needs at least two explicit brackets of 2..64 frames, covering all image_ids",
                ));
            }
        } else if !self.bracket_sizes.is_empty() {
            return Err(failure("bracket_sizes only applies to HDR panorama"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct PhotoOutput {
    pub image_id: String,
    pub path: String,
    pub source_ids: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PhotoJobState {
    Running,
    Completed,
    Cancelled,
    Failed,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct PhotoJobError {
    pub stage: String,
    pub message: String,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct PhotoJobStatus {
    pub state: PhotoJobState,
    pub outputs: Vec<PhotoOutput>,
    pub error: Option<PhotoJobError>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct PhotoProgress {
    pub stage: String,
    pub done: u32,
    pub total: u32,
    pub status: PhotoJobStatus,
}
#[uniffi::export(with_foreign)]
pub trait PhotoJobListener: Send + Sync {
    fn on_progress(&self, progress: PhotoProgress);
}
#[derive(uniffi::Object)]
pub struct PhotoJob {
    cancel: AtomicBool,
    status: Mutex<PhotoJobStatus>,
    stage: Mutex<String>,
    finished: Condvar,
    listener: Arc<dyn PhotoJobListener>,
}
#[uniffi::export]
impl PhotoJob {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn status(&self) -> PhotoJobStatus {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
    /// Blocking; Swift must not call on the main actor or inside a job callback.
    pub fn wait(&self) -> PhotoJobStatus {
        let status = self.status.lock().unwrap_or_else(|p| p.into_inner());
        self.finished
            .wait_while(status, |s| s.state == PhotoJobState::Running)
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }
}
impl PhotoJob {
    pub(crate) fn spawn(
        listener: Arc<dyn PhotoJobListener>,
        work: impl FnOnce(&PhotoJob) -> Result<Vec<PhotoOutput>> + Send + 'static,
    ) -> Result<Arc<Self>> {
        let job = Arc::new(Self {
            cancel: AtomicBool::new(false),
            status: Mutex::new(PhotoJobStatus {
                state: PhotoJobState::Running,
                outputs: vec![],
                error: None,
            }),
            stage: Mutex::new("queued".into()),
            finished: Condvar::new(),
            listener,
        });
        let worker = job.clone();
        std::thread::Builder::new()
            .name("photo-job".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker.check()?;
                    work(&worker)
                }));
                let (state, error) = match result {
                    Ok(Ok(_)) => (PhotoJobState::Completed, None),
                    Ok(Err(e)) if worker.cancel.load(Ordering::Acquire) => {
                        (PhotoJobState::Cancelled, Some(e.to_string()))
                    }
                    Ok(Err(e)) => (PhotoJobState::Failed, Some(e.to_string())),
                    Err(_) => (PhotoJobState::Failed, Some("photo worker panicked".into())),
                };
                let stage = worker
                    .stage
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                {
                    let mut status = worker.status.lock().unwrap_or_else(|p| p.into_inner());
                    status.state = state;
                    status.error = error.map(|message| PhotoJobError { stage, message });
                }
                // Publish the terminal record before waking waiters; listener panics cannot strand wait().
                let label = match state {
                    PhotoJobState::Completed => "completed",
                    PhotoJobState::Cancelled => "cancelled",
                    _ => "failed",
                };
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    worker.progress(label, 1, 1)
                }));
                worker.finished.notify_all();
            })
            .map_err(failure)?;
        Ok(job)
    }
    pub(crate) fn check(&self) -> Result<()> {
        if self.cancel.load(Ordering::Acquire) {
            Err(failure("photo job cancelled"))
        } else {
            Ok(())
        }
    }
    pub(crate) fn progress(&self, stage: &str, done: u32, total: u32) {
        *self.stage.lock().unwrap_or_else(|p| p.into_inner()) = stage.into();
        self.listener.on_progress(PhotoProgress {
            stage: stage.into(),
            done,
            total,
            status: self.status(),
        });
    }
    fn published(&self, output: PhotoOutput) {
        self.status
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .outputs
            .push(output);
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct MergePreview {
    /// JPEG, at most 512 pixels per edge; absent if geometry could not be solved.
    pub bytes: Option<Vec<u8>>,
    pub width: u32,
    pub height: u32,
    pub warnings: Vec<String>,
}
pub(crate) struct PhotoSource {
    pub id: String,
    pub path: PathBuf,
    pub orientation: u16,
}

pub(crate) struct PhotoPublication<'a> {
    pub suffix: &'a str,
    pub source_ids: &'a [String],
    pub create_stack: bool,
}

impl Engine {
    pub(crate) fn photo_sources(&self, ids: &[String]) -> Result<Vec<PhotoSource>> {
        if ids.is_empty() || ids.len() > 128 {
            return Err(failure("select 1..128 images"));
        }
        let mut seen = HashSet::new();
        let c = self.lock()?;
        ids.iter().map(|id| {
            let canonical = parse_id(id)?.to_string();
            if !seen.insert(canonical.clone()) { return Err(failure("duplicate image_id")); }
            let path = Self::path(&c, &canonical)?.into();
            let orientation: String = c.reader.query_row("SELECT COALESCE((SELECT value FROM metadata WHERE image_id=? AND key='orientation'),'1')", [&canonical], |r| r.get(0))?;
            Ok(PhotoSource { id: canonical, path, orientation: orientation.parse().unwrap_or(1) })
        }).collect()
    }
    /// Atomic no-clobber file publication. Cancellation after this checkpoint may
    /// finish this one publication. Already published outputs survive later errors.
    pub(crate) fn publish_photo(
        &self,
        source: &PhotoSource,
        image: &LinearImage,
        recipe: &Recipe,
        publication: PhotoPublication<'_>,
        job: &PhotoJob,
    ) -> Result<PhotoOutput> {
        use std::io::Write;
        job.check()?;
        job.progress("write", 0, 1);
        let folder = if sidecar::Sidecar::is_lightroom_owned(&source.path) {
            self.photo_output_folder()?
        } else {
            source
                .path
                .parent()
                .ok_or_else(|| failure("source has no parent"))?
                .to_path_buf()
        };
        sidecar::Sidecar::ensure_destination(&folder, "export")?;
        std::fs::create_dir_all(&folder)?;
        let stem = source
            .path
            .file_stem()
            .ok_or_else(|| failure("source has no name"))?
            .to_string_lossy();
        let mut temp = tempfile::NamedTempFile::new_in(&folder)?;
        let mut recipe = recipe.clone();
        recipe.image_id = None;
        ::merge::write_dng(&mut temp, image, &recipe)?;
        temp.flush()?;
        temp.as_file().sync_all()?;
        job.check()?;
        let mut c = self.lock()?;
        // Recheck after acquiring a contended catalog lock, before committing.
        job.check()?;
        let mut n = 1u32;
        let suffix = publication.suffix;
        let path = loop {
            let name = if n == 1 {
                format!("{stem}{suffix}.dng")
            } else {
                format!("{stem}{suffix}-{n}.dng")
            };
            let path = folder.join(name);
            sidecar::Sidecar::ensure_destination(&path, "export")?;
            match temp.persist_noclobber(&path) {
                Ok(_) => break path,
                Err(e) if e.error.kind() == std::io::ErrorKind::AlreadyExists => {
                    temp = e.file;
                    n = n
                        .checked_add(1)
                        .ok_or_else(|| failure("too many output name conflicts"))?;
                }
                Err(e) => return Err(failure(e.error)),
            }
        };
        // Index IDs are derived from canonical paths. Do not duplicate that algorithm.
        let indexed = c
            .index
            .scan_file(&path, &catalog::Sidecars, &catalog::IndexedMetadata);
        if let Err(e) = indexed {
            return Err(failure(format!(
                "DNG saved at {}; catalog scan failed: {e}",
                path.display()
            )));
        }
        let path_string = path.to_string_lossy().into_owned();
        let id: String = c.reader.query_row(
            "SELECT i.id FROM image i JOIN file f ON f.id=i.file_id WHERE f.path=?",
            [&path_string],
            |r| r.get(0),
        )?;
        let output = PhotoOutput {
            image_id: id.clone(),
            path: path_string,
            source_ids: publication.source_ids.to_vec(),
        };
        job.published(output.clone());
        recipe.image_id = Some(parse_id(&id)?);
        let mut doc = sidecar::RecipeDocument {
            recipe,
            ..Default::default()
        };
        doc.record_write("tessera-photo-job", crate::now_ms())?;
        Self::persist(&mut c, &path, &doc)?;
        if publication.create_stack {
            catalog::stack_photos(&self.db, &id, publication.source_ids)?;
        }
        drop(c);
        self.notify_changes();
        Ok(output)
    }
}

#[uniffi::export]
impl Engine {
    pub fn photo_merge(
        self: Arc<Self>,
        image_ids: Vec<String>,
        options: MergeOptions,
        listener: Arc<dyn PhotoJobListener>,
    ) -> Result<Arc<PhotoJob>> {
        options.validate(image_ids.len())?;
        let sources = self.photo_sources(&image_ids)?;
        let stack_ids: Vec<_> = sources.iter().map(|s| s.id.clone()).collect();
        PhotoJob::spawn(listener, move |job| {
            let (image, recipe, _) = merge_images(&sources, &options, Some(job), false)?;
            job.check()?;
            let suffix = match options.kind {
                MergeKind::Hdr => "-HDR",
                MergeKind::Panorama => "-Pano",
                MergeKind::HdrPanorama => "-HDR-Pano",
            };
            Ok(vec![self.publish_photo(
                &sources[0],
                &image,
                &recipe,
                PhotoPublication {
                    suffix,
                    source_ids: &stack_ids,
                    create_stack: options.create_stack,
                },
                job,
            )?])
        })
    }
    /// Synchronous preview command, run off the main actor. Does not write files.
    pub fn merge_preview(
        &self,
        image_ids: Vec<String>,
        options: MergeOptions,
    ) -> Result<MergePreview> {
        options.validate(image_ids.len())?;
        let sources = self.photo_sources(&image_ids)?;
        match merge_images(&sources, &options, None, true) {
            Ok((image, recipe, mut warnings)) => {
                warnings.push(
                    "Preview is a camera-channel approximation, not the full develop renderer"
                        .into(),
                );
                let scale = 2f32.powf(recipe.settings.tone.exposure);
                let rgb =
                    image::RgbImage::from_fn(image.width as u32, image.height as u32, |x, y| {
                        let p = image.pixels[y as usize * image.width + x as usize];
                        image::Rgb(std::array::from_fn(|c| {
                            ((p[c] / image.as_shot_neutral[c] as f32 * scale)
                                .max(0.)
                                .powf(1. / 2.2)
                                .min(1.)
                                * 255.)
                                .round() as u8
                        }))
                    });
                use previews::Codec;
                Ok(MergePreview {
                    bytes: Some(previews::Jpeg.encode(&rgb).map_err(failure)?),
                    width: image.width as u32,
                    height: image.height as u32,
                    warnings,
                })
            }
            Err(e) => Ok(MergePreview {
                bytes: None,
                width: 0,
                height: 0,
                warnings: vec![e.to_string()],
            }),
        }
    }
    /// Derived image first, followed by sources; persistent across engine reopen.
    pub fn photo_stack(&self, image_id: String) -> Result<Vec<String>> {
        parse_id(&image_id)?;
        catalog::photo_stack(&self.db, &image_id)
    }
}

pub(crate) fn load_linear(source: &PhotoSource) -> Result<(LinearImage, Option<hdr::Exposure>)> {
    // Same gate as the two approved reader call sites: only a .dng is offered
    // to the LinearRaw reader, so every other original keeps its LibRaw path.
    if source
        .path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("dng"))
        && let Some(dng) = raw_decode::lossy_dng::read(&mut std::fs::File::open(&source.path)?)?
    {
        // Merge consumes unbalanced camera RGB. The Smart Preview is already
        // normalized and demosaiced; sending it to LibRaw loses JXL support.
        let wb = dng.metadata.as_shot_wb;
        let image = LinearImage {
            width: dng.width,
            height: dng.height,
            pixels: dng.pixels,
            color_matrix: std::array::from_fn(|i| dng.metadata.cam_xyz[i].map(f64::from)),
            as_shot_neutral: std::array::from_fn(|i| f64::from(wb[1] / wb[i])),
        };
        image.validate().map_err(failure)?;
        return Ok((
            orient_linear(image, source.orientation)?,
            Some(hdr::Exposure::from_metadata(&dng.metadata)),
        ));
    }
    if let Ok(dng) = raw_decode::linear_dng::read(&mut std::fs::File::open(&source.path)?) {
        return Ok((
            orient_linear(
                LinearImage {
                    width: dng.width,
                    height: dng.height,
                    pixels: dng.pixels,
                    color_matrix: dng.color_matrix,
                    as_shot_neutral: dng.as_shot_neutral,
                },
                source.orientation,
            )?,
            None,
        ));
    }
    let mut raw = raw_decode::RawSource::open(&source.path)?;
    let meta = raw.metadata();
    let image = ::merge::from_cfa(&raw.decode_cfa()?, &meta).map_err(failure)?;
    Ok((
        orient_linear(image, source.orientation)?,
        Some(hdr::Exposure::from_metadata(&meta)),
    ))
}
/// Consume EXIF orientation in camera space, before alignment, without
/// resampling or changing the calibration. Published DNGs are orientation 1.
fn orient_linear(mut image: LinearImage, orientation: u16) -> Result<LinearImage> {
    if !(1..=8).contains(&orientation) {
        return Err(failure("source orientation must be 1..=8"));
    }
    if orientation == 1 {
        return Ok(image);
    }
    let (w, h) = (image.width, image.height);
    let (out_w, out_h) = if orientation >= 5 { (h, w) } else { (w, h) };
    let mut pixels = vec![[0.; 3]; image.pixels.len()];
    for (i, pixel) in image.pixels.into_iter().enumerate() {
        let (x, y) = (i % w, i / w);
        let (x, y) = match orientation {
            2 => (w - 1 - x, y),
            3 => (w - 1 - x, h - 1 - y),
            4 => (x, h - 1 - y),
            5 => (y, x),
            6 => (h - 1 - y, x),
            7 => (h - 1 - y, w - 1 - x),
            8 => (y, w - 1 - x),
            _ => (x, y),
        };
        pixels[y * out_w + x] = pixel;
    }
    image.width = out_w;
    image.height = out_h;
    image.pixels = pixels;
    Ok(image)
}
/// Dependency inversion avoids filters -> compositor -> merge -> filters.
/// CAF sees linear float samples, preserving HDR/negative values and coverage.
fn content_aware_edges(
    image: &LinearImage,
    coverage: &[bool],
    job: Option<&PhotoJob>,
) -> ::merge::Result<Vec<[f32; 3]>> {
    use compositor::{Depth, Raster, Rect};
    use engine_api::tile::Extent;
    let (w, h) = (image.width, image.height);
    if let Some(job) = job {
        job.check().map_err(|e| e.to_string())?;
        job.progress("fill-edges", 0, 1);
    }
    let mut raster = Raster::new(Extent::new(w as u32, h as u32), 4, Depth::F32, 0.);
    raster
        .edit_region(Rect::new(0, 0, w as i64, h as i64), 1, |x, y, p| {
            let c = image.pixels[y as usize * w + x as usize];
            *p = [c[0], c[1], c[2], 1.];
        })
        .map_err(|e| e.to_string())?;
    let mask: Vec<_> = coverage.iter().map(|v| if *v { 0. } else { 1. }).collect();
    let uncancelled = AtomicBool::new(false);
    let result = filters::caf::fill(
        &raster,
        &mask,
        &Default::default(),
        job.map(|j| &j.cancel).unwrap_or(&uncancelled),
    )
    .map_err(|e| e.to_string())?;
    if let Some(job) = job {
        job.progress("fill-edges", 1, 1);
    }
    Ok((0..w * h)
        .map(|i| {
            let p = result.composite.pixel((i % w) as u32, (i / w) as u32);
            [p[0], p[1], p[2]]
        })
        .collect())
}

fn thumbnail(image: &LinearImage, limit: usize) -> LinearImage {
    let scale = (limit as f64 / image.width.max(image.height) as f64).min(1.);
    let w = (image.width as f64 * scale).round().max(1.) as usize;
    let h = (image.height as f64 * scale).round().max(1.) as usize;
    let mut output = image.clone();
    output.width = w;
    output.height = h;
    output.pixels = (0..w * h)
        .map(|i| {
            image
                .sample(
                    (i % w) as f64 * (image.width - 1) as f64 / (w - 1).max(1) as f64,
                    (i / w) as f64 * (image.height - 1) as f64 / (h - 1).max(1) as f64,
                )
                .unwrap()
        })
        .collect();
    output
}
fn merge_images(
    sources: &[PhotoSource],
    options: &MergeOptions,
    job: Option<&PhotoJob>,
    preview: bool,
) -> Result<(LinearImage, Recipe, Vec<String>)> {
    let mut frames = Vec::new();
    let mut warnings = Vec::new();
    let mut focal_scale = 1.;
    for (i, source) in sources.iter().enumerate() {
        if let Some(job) = job {
            job.check()?;
            job.progress("decode", i as u32, sources.len() as u32);
        }

        let (mut image, exposure) = load_linear(source)?;
        if preview {
            let resized = thumbnail(&image, 512);
            if i == 0 {
                focal_scale = resized.width as f64 / image.width as f64;
            }
            image = resized;
        }
        let exposure = if let Some(v) = options.exposure_values.get(i) {
            hdr::Exposure {
                shutter_s: *v,
                iso: 100.,
                aperture: 1.,
            }
        } else if options.kind == MergeKind::Panorama {
            exposure.unwrap_or(hdr::Exposure {
                shutter_s: 1.,
                iso: 100.,
                aperture: 1.,
            })
        } else {
            exposure.ok_or_else(|| {
                failure(
                    "missing exposure metadata; supply exposure_values for LinearRaw DNG brackets",
                )
            })?
        };
        frames.push(hdr::BracketFrame { image, exposure });
    }
    if let Some(job) = job {
        job.check()?;
        job.progress("merge", 0, 1);
    }
    let hdr_options = hdr::HdrOptions {
        auto_align: options.auto_align,
        deghost: match options.deghost {
            MergeDeghost::None => hdr::Deghost::None,
            MergeDeghost::Low => hdr::Deghost::Low,
            MergeDeghost::Medium => hdr::Deghost::Medium,
            MergeDeghost::High => hdr::Deghost::High,
        },
        ..Default::default()
    };
    let projection = match options.projection {
        MergeProjection::Auto => pano::Projection::Auto,
        MergeProjection::Perspective => pano::Projection::Perspective,
        MergeProjection::Spherical => pano::Projection::Spherical,
        MergeProjection::Cylindrical => pano::Projection::Cylindrical,
    };
    if options.projection == MergeProjection::Auto
        && options.kind != MergeKind::Hdr
        && options.focal_pixels.is_none()
    {
        warnings.push("Auto FOV estimated from a 60-degree first-view horizontal FOV; supply focal_pixels for calibrated selection".into());
    }
    let pano_options = pano::PanoramaOptions {
        projection,
        focal_pixels: options
            .focal_pixels
            .map(|v| v * focal_scale)
            .unwrap_or(frames[0].image.width as f64 / (2. * 30_f64.to_radians().tan())),
        auto_crop: false,
        boundary_warp: options.boundary_warp,
        fill_edges: options.fill_edges,
        ..Default::default()
    };
    let (mut image, mut recipe) = match options.kind {
        MergeKind::Hdr => {
            let result = hdr::hdr(&frames, &hdr_options).map_err(failure)?;
            (result.image, result.recipe)
        }
        MergeKind::Panorama => {
            let images: Vec<_> = frames.into_iter().map(|f| f.image).collect();
            let result = pano::panorama_with_fill(&images, &pano_options, |image, mask| {
                content_aware_edges(image, mask, job)
            })
            .map_err(failure)?;
            warnings.push(format!("Projection: {:?}", result.projection));
            if result.coverage.contains(&false) {
                warnings.push("Panorama contains uncovered or filled borders".into());
            }
            (result.image, result.recipe)
        }
        MergeKind::HdrPanorama => {
            let mut iter = frames.into_iter();
            let groups: Vec<Vec<_>> = options
                .bracket_sizes
                .iter()
                .map(|n| iter.by_ref().take(*n as usize).collect())
                .collect();
            let result = ::merge::hdr_panorama_with_fill(
                &groups,
                &hdr_options,
                &pano_options,
                |image, mask| content_aware_edges(image, mask, job),
            )
            .map_err(failure)?;
            warnings.push(format!("Projection: {:?}", result.panorama.projection));
            if result.panorama.coverage.contains(&false) {
                warnings.push("Panorama contains uncovered or filled borders".into());
            }
            (result.panorama.image, result.panorama.recipe)
        }
    };
    if !options.auto_tone {
        recipe = Recipe::default();
    }
    if preview {
        image = thumbnail(&image, 512);
    }
    Ok((image, recipe, warnings))
}

#[cfg(test)]
mod proxy_source_tests {
    #[test]
    fn lr13_merge_input_accepts_jxl_linearraw_and_catalog_orientation() {
        let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../raw-decode/tests/fixtures/linear-gradient-jxl.dng");
        let base = super::PhotoSource {
            id: String::new(),
            path: path.clone(),
            orientation: 1,
        };
        let (a, _) = super::load_linear(&base).unwrap();
        let rotated = super::PhotoSource {
            id: String::new(),
            path,
            orientation: 6,
        };
        let (b, _) = super::load_linear(&rotated).unwrap();
        assert_eq!((a.width, a.height), (b.height, b.width));
        assert_eq!(a.pixels.len(), b.pixels.len());
        a.validate().unwrap();
        b.validate().unwrap();
    }
}
