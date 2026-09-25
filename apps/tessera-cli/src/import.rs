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

/// Persist a lossless import bundle, never sidecars beside source originals.
/// Recipes are keyed by catalog id to retain virtual copies and duplicate stems.
pub fn apply(source: &Path, dest: &Path) -> Result<Value> {
    ensure!(
        !dest.try_exists()?,
        "destination already exists: {}",
        dest.display()
    );
    let plan = import_lrcat::import(source)?;
    let parent = dest
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent)?;
    let staging = tempfile::tempdir_in(parent)?;
    plan.library.write(staging.path().join("library.json"))?;
    std::fs::write(
        staging.path().join("import-plan.json"),
        serde_json::to_vec_pretty(&plan)?,
    )?;
    let recipes = staging.path().join("recipes");
    std::fs::create_dir(&recipes)?;
    for image in &plan.images {
        sidecar::Sidecar::write_recipe(
            recipes.join(format!("{}.json", image.catalog_id)),
            &sidecar::RecipeDocument {
                recipe: image.recipe.clone(),
                ..Default::default()
            },
        )?;
    }
    // Exclusive reservation prevents replacing any pre-existing destination.
    std::fs::create_dir(dest).context("reserve import destination")?;
    if let Err(error) = std::fs::rename(staging.path(), dest) {
        let _ = std::fs::remove_dir(dest);
        return Err(error.into());
    }
    Ok(json!({"dest":dest,"images":plan.images.len(),"report":plan.report}))
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
