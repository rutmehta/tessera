//! Adapter for the index's scanner hooks and read-only schema-v3 metadata.
use engine_api::{EngineResult, id::ImageId, recipe::Recipe};
use index::{Metadata, MetadataProvider, SidecarData, SidecarReader};
use sidecar::{RecipeDocument, Sidecar, XmpPacket};
use std::path::{Path, PathBuf};

pub(crate) fn xmp_path(path: &Path) -> PathBuf {
    let appended = Sidecar::paths(path).xmp;
    if appended.exists() || !path.with_extension("xmp").exists() {
        appended
    } else {
        path.with_extension("xmp")
    }
}

pub(crate) fn document(path: &Path, id: ImageId) -> EngineResult<RecipeDocument> {
    let recipe = Sidecar::paths(path).recipe;
    if recipe.exists() {
        let document = Sidecar::read_recipe(recipe)?;
        if document.recipe.image_id != Some(id) {
            return Err(engine_api::EngineError::invalid(
                "image_id",
                "sidecar belongs to another image",
            ));
        }
        return Ok(document);
    }
    let mut recipe = Recipe::default();
    let xmp = xmp_path(path);
    if xmp.exists() {
        recipe = Sidecar::read_xmp(xmp)?.to_recipe()?.recipe;
    }
    recipe.image_id = Some(id);
    Ok(RecipeDocument {
        recipe,
        ..Default::default()
    })
}

pub(crate) struct Sidecars;
impl SidecarReader for Sidecars {
    fn read(&self, path: &Path) -> EngineResult<SidecarData> {
        let mut data = SidecarData::default();
        let xmp = xmp_path(path);
        if xmp.exists() {
            let packet = Sidecar::read_xmp(xmp)?;
            let meta = packet.metadata()?;
            data.caption = Some(meta.description);
            data.keywords = meta.keywords;
            data.selection = Some(packet.selection()?);
        }
        let recipe = Sidecar::paths(path).recipe;
        if recipe.exists() {
            let doc = Sidecar::read_recipe(recipe)?;
            data.recipe_hash = Some(doc.recipe.recipe_hash().0);
            data.selection = Some(doc.recipe.selection);
        } else {
            data.recipe_hash = Some(Recipe::default().recipe_hash().0);
        }
        Ok(data)
    }
}

pub(crate) fn selection_packet(path: &Path, doc: &RecipeDocument) -> EngineResult<XmpPacket> {
    let xmp = xmp_path(path);
    if xmp.exists() {
        let packet = Sidecar::read_xmp(xmp)?;
        packet.with_metadata(
            &doc.recipe.selection,
            &packet.metadata()?,
            &sidecar::MarkPreset::default(),
        )
    } else {
        Ok(XmpPacket::from_selection(
            &doc.recipe.selection,
            &sidecar::MarkPreset::default(),
        ))
    }
}

pub(crate) struct EmbeddedMetadata;
impl MetadataProvider for EmbeddedMetadata {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let ext = path
            .extension()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "tif" | "tiff") {
            let source = raw_decode::RawSource::open(path)?;
            let m = source.metadata();
            return Ok(Metadata {
                capture_time: Some(m.capture_time.to_string()),
                camera: Some(m.model),
                lens: m.lens,
                values: vec![("orientation".into(), m.orientation.to_string())],
                ..Default::default()
            });
        }
        let file = std::fs::File::open(path)?;
        let orientation = exif::Reader::new()
            .read_from_container(&mut std::io::BufReader::new(file))
            .ok()
            .and_then(|e| {
                e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                    .and_then(|f| f.value.get_uint(0))
            })
            .unwrap_or(1);
        Ok(Metadata {
            values: vec![("orientation".into(), orientation.to_string())],
            ..Default::default()
        })
    }
}
