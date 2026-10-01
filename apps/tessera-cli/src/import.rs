use anyhow::{Context, Result, ensure};
use serde_json::{Value, json};
use std::path::Path;

/// Compare at quarter scale. A full-size export is accepted only after checking
/// the exact developed, oriented full extent, never by inferring it from a
/// rounded quarter extent. This also catches differently cropped exports.
fn quarter_reference(
    reference: image::RgbImage,
    quarter: (u32, u32),
    full_dimensions: impl FnOnce() -> Result<(u32, u32)>,
) -> Result<image::RgbImage> {
    if reference.dimensions() == quarter {
        return Ok(reference);
    }
    let full = full_dimensions()?;
    ensure!(
        reference.dimensions() == full,
        "reference dimension mismatch: got {:?}, expected quarter {:?} or full {:?}; export matching crop/orientation",
        reference.dimensions(),
        quarter,
        full
    );
    Ok(image::imageops::resize(
        &reference,
        quarter.0,
        quarter.1,
        image::imageops::FilterType::Triangle,
    ))
}

/// Read-only audit of catalog recipes against explicitly supplied sRGB JPEGs.
/// Each virtual copy has its own reference and result; absent inputs have no score.
pub fn fidelity(source: &Path, reference_dir: &Path) -> Result<Value> {
    ensure!(
        reference_dir.is_dir(),
        "reference directory does not exist: {}",
        reference_dir.display()
    );
    let plan = import_lrcat::import(source)?;
    let mut rows = Vec::with_capacity(plan.images.len());
    let (mut compared, mut skipped, mut failed) = (0, 0, 0);
    for image in &plan.images {
        let reference = reference_dir.join(format!("{}.jpg", image.catalog_id));
        let mut row = json!({"catalog_id":image.catalog_id,"path":image.path,"reference":reference,"process_version":image.recipe.process_version});
        let reason = if !reference.is_file() {
            Some("missing reference export")
        } else if !image.path.is_file() {
            Some("missing original RAW")
        } else {
            None
        };
        if let Some(reason) = reason {
            row["status"] = json!("skipped");
            row["reason"] = json!(reason);
            skipped += 1;
        } else {
            let result = (|| -> Result<_> {
                let reference = image::open(&reference)
                    .context("decode reference JPEG")?
                    .into_rgb8();
                let pixels = crate::media::adobe_pixels(&image.path, 4, &image.recipe.settings)?;
                let reference = quarter_reference(reference, pixels.dimensions(), || {
                    // Only full-size/misaligned references require this second render.
                    Ok(
                        crate::media::adobe_pixels(&image.path, 1, &image.recipe.settings)?
                            .dimensions(),
                    )
                })?;
                pipeline_adobe::fidelity::compare(&pixels, &reference).map_err(anyhow::Error::msg)
            })();
            match result {
                Ok(stats) => {
                    row["status"] = json!("compared");
                    row["mean"] = json!(stats.mean);
                    row["p95"] = json!(stats.p95);
                    row["samples"] = json!(stats.samples);
                    compared += 1;
                }
                Err(error) => {
                    row["status"] = json!("failed");
                    row["reason"] = json!(format!("{error:#}"));
                    failed += 1;
                }
            }
        }
        rows.push(row);
    }
    Ok(
        json!({"scale":"1/4","total":rows.len(),"compared":compared,"skipped":skipped,"failed":failed,"images":rows}),
    )
}

/// One recipe file of the staged bundle: the bytes and checks of
/// `sidecar::Sidecar::write_recipe`, with a plain `fsync` (data handed to the
/// drive) instead of a per-file `F_FULLFSYNC`. The staging directory is
/// private until it is renamed into place, and [`apply`] issues one
/// `F_FULLFSYNC` (drive cache flush) after every file is written. Per-file
/// full flushes cost ~5 ms each (~100 s for 21k recipes).
fn write_staged_recipe(path: &Path, document: &sidecar::RecipeDocument) -> Result<()> {
    use std::{io::Write, os::fd::AsRawFd};
    document.recipe.to_json()?;
    document.recipe.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    let mut file = std::fs::File::create_new(path)?;
    file.write_all(&bytes)?;
    // SAFETY: fsync on a file descriptor this function owns and keeps open.
    if unsafe { libc::fsync(file.as_raw_fd()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

/// Persist a lossless import bundle, never sidecars beside source originals.
/// Recipes are keyed by catalog id to retain virtual copies and duplicate stems.
///
/// The catalog is streamed (B5-29c): translation runs on the importer's
/// threads, `import-plan.json` is written by one thread as images arrive and
/// recipe files by a small pool, so the whole plan is never in memory. Output
/// bytes are the same as serializing the full plan.
pub fn apply(source: &Path, dest: &Path) -> Result<Value> {
    use std::sync::{Mutex, mpsc};
    enum Msg {
        Begin(Box<import_lrcat::ImportPlan>),
        Image(Box<import_lrcat::ImportedImage>),
    }
    ensure!(
        !dest.try_exists()?,
        "destination already exists: {}",
        dest.display()
    );
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    let recipes = staging.path().join("recipes");
    std::fs::create_dir(&recipes)?;
    let plan_path = staging.path().join("import-plan.json");
    let plan_file = std::io::BufWriter::with_capacity(1 << 20, std::fs::File::create(&plan_path)?);
    // The first failure is kept; every queue still drains so no sender blocks.
    let failed: Mutex<Option<anyhow::Error>> = Mutex::new(None);
    let fail = |e: anyhow::Error| {
        failed.lock().expect("error slot").get_or_insert(e);
    };
    let (image_tx, image_rx) = mpsc::sync_channel::<Msg>(64);
    let (recipe_tx, recipe_rx) =
        mpsc::sync_channel::<(std::path::PathBuf, sidecar::RecipeDocument)>(256);
    let recipe_rx = Mutex::new(recipe_rx);
    let mut images = 0usize;
    let (plan, json) = std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| {
                loop {
                    let job = recipe_rx.lock().expect("queue").recv();
                    let Ok((path, document)) = job else { break };
                    if let Err(e) = write_staged_recipe(&path, &document) {
                        fail(e);
                    }
                }
            });
        }
        let writer = s.spawn(move || {
            let mut json = None;
            let mut plan_file = Some(plan_file);
            for msg in image_rx {
                let step = (|| -> Result<()> {
                    match msg {
                        Msg::Begin(plan) => {
                            json = Some(import_lrcat::PlanJson::begin(
                                plan_file.take().context("plan begun twice")?,
                                &plan,
                            )?);
                        }
                        Msg::Image(image) => {
                            json.as_mut().context("plan not begun")?.image(&image)?;
                            let path = recipes.join(format!("{}.json", image.catalog_id));
                            let document = sidecar::RecipeDocument {
                                recipe: image.recipe,
                                ..Default::default()
                            };
                            recipe_tx
                                .send((path, document))
                                .map_err(|_| anyhow::anyhow!("recipe writers stopped"))?;
                        }
                    }
                    Ok(())
                })();
                if let Err(e) = step {
                    fail(e);
                }
            }
            json
        });
        let stopped = || engine_api::error::EngineError::Conflict {
            message: "import plan writer stopped".into(),
        };
        let plan = import_lrcat::import_each(
            source,
            |plan| {
                image_tx
                    .send(Msg::Begin(Box::new(plan.clone())))
                    .map_err(|_| stopped())
            },
            |image| {
                images += 1;
                image_tx
                    .send(Msg::Image(Box::new(image)))
                    .map_err(|_| stopped())
            },
        );
        drop(image_tx);
        let json = writer
            .join()
            .unwrap_or_else(|e| std::panic::resume_unwind(e));
        (plan, json)
    });
    if let Some(error) = failed.into_inner().expect("error slot") {
        return Err(error);
    }
    let plan = plan?;
    plan.library.write(staging.path().join("library.json"))?;
    let json = json.context("catalog plan was not started")?;
    let file = json
        .finish(&plan)?
        .into_inner()
        .map_err(|e| e.into_error())?;
    // One drive-cache flush covers every file fsynced above.
    file.sync_all()?;
    drop(file);
    // Exclusive reservation prevents replacing any pre-existing destination.
    std::fs::create_dir(dest).context("reserve import destination")?;
    if let Err(error) = std::fs::rename(staging.path(), dest) {
        let _ = std::fs::remove_dir(dest);
        return Err(error.into());
    }
    Ok(json!({"dest":dest,"images":images,"report":plan.report}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reference_dimensions_are_validated_not_arbitrarily_resized() {
        let quarter = image::RgbImage::new(3, 2);
        assert_eq!(
            quarter_reference(quarter.clone(), (3, 2), || panic!("already quarter")).unwrap(),
            quarter
        );
        assert_eq!(
            quarter_reference(image::RgbImage::new(11, 7), (3, 2), || Ok((11, 7)))
                .unwrap()
                .dimensions(),
            (3, 2)
        );
        let error =
            quarter_reference(image::RgbImage::new(10, 7), (3, 2), || Ok((11, 7))).unwrap_err();
        assert!(error.to_string().contains("dimension mismatch"));
    }
}
