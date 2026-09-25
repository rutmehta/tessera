use engine_api::{EngineError, EngineResult, id::ImageId};
use index::{Metadata, MetadataProvider, SidecarData, SidecarReader};
use sidecar::{RecipeDocument, Sidecar};
use std::path::Path;

pub(crate) struct Reader;
impl SidecarReader for Reader {
    fn read(&self, path: &Path) -> EngineResult<SidecarData> {
        Ok(SidecarData {
            selection: Some(document(path)?.recipe.selection),
            ..Default::default()
        })
    }
}
impl MetadataProvider for Reader {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        if crate::pixels::is_rgb(path) {
            return Ok(Metadata::default());
        }
        let meta = raw_decode::RawSource::open(path)?.metadata();
        Ok(Metadata {
            camera: Some(format!("{} {}", meta.make, meta.model)),
            lens: meta.lens,
            capture_time: (meta.capture_time != 0).then(|| meta.capture_time.to_string()),
            values: vec![
                ("iso".into(), meta.iso.to_string()),
                ("aperture".into(), meta.aperture.to_string()),
                ("shutter_s".into(), meta.shutter_s.to_string()),
            ],
            ..Default::default()
        })
    }
}
pub(crate) fn document(path: &Path) -> EngineResult<RecipeDocument> {
    let paths = Sidecar::paths(path);
    if paths.recipe.try_exists()? {
        return Sidecar::read_recipe(paths.recipe);
    }
    let xmp = if paths.xmp.try_exists()? {
        paths.xmp
    } else {
        path.with_extension("xmp")
    };
    if xmp.try_exists()? {
        return Ok(RecipeDocument {
            recipe: Sidecar::read_xmp(xmp)?.to_recipe()?.recipe,
            ..Default::default()
        });
    }
    Ok(RecipeDocument::default())
}
pub(crate) fn identify(doc: &mut RecipeDocument, id: ImageId) -> EngineResult<()> {
    if doc.recipe.image_id.is_some_and(|stored| stored != id) {
        return Err(EngineError::Conflict {
            message: "sidecar belongs to another image".into(),
        });
    }
    doc.recipe.image_id = Some(id);
    Ok(())
}
pub(crate) fn writable(path: &Path) -> EngineResult<()> {
    for entry in std::fs::read_dir(
        path.parent()
            .ok_or_else(|| EngineError::invalid("image", "no parent"))?,
    )? {
        let peer = entry?.path();
        if peer != path
            && peer.is_file()
            && peer.file_stem() == path.file_stem()
            && peer
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "jpg"
                            | "jpeg"
                            | "png"
                            | "tif"
                            | "tiff"
                            | "dng"
                            | "arw"
                            | "cr2"
                            | "cr3"
                            | "nef"
                            | "raf"
                    )
                })
        {
            return Err(EngineError::Conflict {
                message: format!("sidecar destination collision: {}", peer.display()),
            });
        }
    }
    Ok(())
}
