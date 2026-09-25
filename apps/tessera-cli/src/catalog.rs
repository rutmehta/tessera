use anyhow::{Context, Result, ensure};
use engine_api::{EngineResult, id::ImageId, recipe::Recipe};
use index::{Index, SidecarData, SidecarReader};
use sidecar::{RecipeDocument, Sidecar};
use std::path::{Path, PathBuf};

pub fn document(path: &Path) -> EngineResult<RecipeDocument> {
    let paths = Sidecar::paths(path);
    if paths.recipe.try_exists()? {
        return Sidecar::read_recipe(paths.recipe);
    }
    let xmp = xmp_path(path)?;
    if xmp.try_exists()? {
        let imported = Sidecar::read_xmp(xmp)?.to_recipe()?;
        for warning in imported.warnings {
            eprintln!("warning: {warning}");
        }
        return Ok(RecipeDocument {
            recipe: imported.recipe,
            ..Default::default()
        });
    }
    Ok(RecipeDocument::default())
}
fn xmp_path(path: &Path) -> EngineResult<PathBuf> {
    let appended = Sidecar::paths(path).xmp;
    Ok(if appended.try_exists()? {
        appended
    } else {
        path.with_extension("xmp")
    })
}
pub struct Reader;
pub struct RawMetadata;
impl index::MetadataProvider for RawMetadata {
    fn read(&self, path: &Path) -> EngineResult<index::Metadata> {
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if matches!(
            ext.as_str(),
            "jpg" | "jpeg" | "png" | "tif" | "tiff" | "heic"
        ) {
            return Ok(index::Metadata::default());
        }
        let meta = raw_decode::RawSource::open(path)?.metadata();
        Ok(index::Metadata {
            camera: Some(format!("{} {}", meta.make, meta.model)),
            lens: meta.lens,
            capture_time: if meta.capture_time == 0 {
                None
            } else {
                chrono::DateTime::from_timestamp(meta.capture_time, 0)
                    .map(|date| date.format("%Y-%m-%d %H:%M:%S").to_string())
            },
            values: vec![
                ("iso".into(), meta.iso.to_string()),
                ("aperture".into(), meta.aperture.to_string()),
                ("shutter_s".into(), meta.shutter_s.to_string()),
                ("focal_mm".into(), meta.focal_mm.to_string()),
            ],
            ..Default::default()
        })
    }
}
impl SidecarReader for Reader {
    fn read(&self, path: &Path) -> EngineResult<SidecarData> {
        let mut data = SidecarData::default();
        let xmp = xmp_path(path)?;
        if xmp.try_exists()? {
            let metadata = Sidecar::read_xmp(xmp)?.metadata()?;
            data.caption = Some(metadata.description);
            data.keywords = metadata.keywords;
        }
        data.selection = Some(document(path)?.recipe.selection);
        Ok(data)
    }
}
pub fn resolve(index: &Index, image: &Path) -> Result<(ImageId, PathBuf)> {
    if let Some(text) = image.to_str()
        && let Ok(id) = text.parse::<ImageId>()
    {
        return Ok((id, index.image_info(id)?.path));
    }
    let path = image
        .canonicalize()
        .with_context(|| format!("image {}", image.display()))?;
    for id in index.search(&crate::all())? {
        if index.image_info(id)?.path == path {
            return Ok((id, path));
        }
    }
    anyhow::bail!("image is not indexed; run tessera index on its directory first")
}
pub fn check_identity(recipe: &Recipe, id: ImageId) -> Result<()> {
    ensure!(
        recipe.image_id.is_none_or(|stored| stored == id),
        "sidecar belongs to another image"
    );
    Ok(())
}
