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
/// private until it is renamed into place. After the writers finish, [`apply`]
/// synchronizes the plan with std `sync_all` (F_FULLFSYNC on Apple), then the
/// staging directories and, after publication, the destination parent. Avoiding
/// per-recipe full flushes saves ~5 ms each (~100 s for 21k recipes).
fn write_staged_recipe(path: &Path, document: &sidecar::RecipeDocument) -> Result<()> {
    use std::io::Write;
    document.recipe.to_json()?;
    document.recipe.validate()?;
    let bytes = serde_json::to_vec_pretty(document)?;
    let mut file = std::fs::File::create_new(path)?;
    file.write_all(&bytes)?;
    sync_plain(&file)?;
    Ok(())
}

/// Plain fsync on Apple (std sync_all uses F_FULLFSYNC); retry signals.
/// Other platforms use the standard library's synchronization implementation.
fn sync_plain(file: &std::fs::File) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    {
        use std::os::fd::AsRawFd;
        loop {
            // SAFETY: the descriptor is owned by the live File reference.
            if unsafe { libc::fsync(file.as_raw_fd()) } == 0 {
                return Ok(());
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    file.sync_all()
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    sync_plain(&std::fs::File::open(path)?)
}

/// Persist a lossless import bundle, never sidecars beside source originals.
/// Recipes are keyed by catalog id to retain virtual copies and duplicate stems.
///
/// The catalog is streamed (B5-29c): translation runs on the importer's
/// threads, `import-plan.json` is written by one thread as images arrive and
/// recipe files by a small pool, so the whole plan is never in memory. Output
/// bytes are the same as serializing the full plan.
pub fn apply(source: &Path, dest: &Path) -> Result<Value> {
    apply_with_publish(source, dest, |from, to| Ok(std::fs::rename(from, to)?))
}

fn apply_with_publish(
    source: &Path,
    dest: &Path,
    publish: impl FnOnce(&Path, &Path) -> Result<()>,
) -> Result<Value> {
    use std::sync::{Mutex, mpsc};
    enum Msg {
        Begin(Box<import_lrcat::ImportPlan>),
        Image(Box<import_lrcat::ImportedImage>),
    }
    if dest.try_exists()? {
        // Crash recovery: an empty directory is a reservation, not a bundle.
        // remove_dir atomically refuses nonempty destinations; never recurse.
        std::fs::remove_dir(dest).with_context(|| {
            format!(
                "destination already exists and is not an empty reservation: {}",
                dest.display()
            )
        })?;
    }
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
        let plan = import_lrcat::import_each_with_storage(
            source,
            Some(staging.path()),
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
    // The plan gets std sync_all (F_FULLFSYNC on Apple).
    file.sync_all()?;
    drop(file);
    sync_directory(&staging.path().join("recipes"))?;
    if staging.path().join("large").is_dir() {
        sync_directory(&staging.path().join("large"))?;
    }
    sync_directory(staging.path())?;
    // Exclusive reservation prevents replacing a concurrent destination.
    // A crash before rename leaves an empty reservation recovered above.
    std::fs::create_dir(dest).context("reserve import destination")?;
    if let Err(error) = publish(staging.path(), dest).and_then(|()| {
        ensure!(
            !staging.path().exists() && dest.join("import-plan.json").is_file(),
            "staging directory was not published"
        );
        Ok(())
    }) {
        let _ = std::fs::remove_dir(dest);
        return Err(error);
    }
    sync_directory(parent)?;
    Ok(json!({"dest":dest,"images":images,"report":plan.report}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrcat_empty_reservation_recovers_but_nonempty_destination_survives() {
        let temp = tempfile::tempdir().unwrap();
        let fixture = import_lrcat::fixture::write(&temp.path().join("fx")).unwrap();
        let dest = temp.path().join("bundle");
        std::fs::create_dir(&dest).unwrap();
        apply(&fixture.catalog, &dest).unwrap();
        let original = std::fs::read(dest.join("import-plan.json")).unwrap();
        assert!(apply(&fixture.catalog, &dest).is_err());
        assert_eq!(
            std::fs::read(dest.join("import-plan.json")).unwrap(),
            original
        );
    }

    #[test]
    fn lrcat_failed_publish_leaves_no_destination() {
        for skip in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let fixture = import_lrcat::fixture::write(&temp.path().join("fx")).unwrap();
            let dest = temp.path().join("bundle");
            let result = apply_with_publish(&fixture.catalog, &dest, |_, _| {
                if skip {
                    Ok(())
                } else {
                    anyhow::bail!("injected pre-rename error")
                }
            });
            assert!(result.is_err());
            assert!(!dest.exists());
            assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 1);
        }
    }

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
