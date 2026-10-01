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
    fn additional_stamp_paths(&self, path: &Path) -> Vec<PathBuf> {
        if Sidecar::is_lightroom_owned(path) {
            let paths = Sidecar::paths(path);
            vec![paths.recipe, paths.xmp]
        } else {
            Vec::new()
        }
    }
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
        if !image_core::RgbSource::recognizes(path) {
            // Native float LinearRaw is mosaic-free and cannot use decode_cfa.
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dng"))
                && raw_decode::linear_dng::read(&mut std::fs::File::open(path)?).is_ok()
            {
                return Ok(Metadata {
                    values: vec![("orientation".into(), "1".into())],
                    ..Default::default()
                });
            }
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

/// FFI-owned persistent stacks; index has no stack model. Existing intersecting
/// groups are unioned, never destroyed when enhancing a member a second time.
pub(crate) fn stack_photos(db: &Path, derived: &str, sources: &[String]) -> crate::Result<()> {
    let mut conn = rusqlite::Connection::open(db)?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS photo_stack_member(
            image_id TEXT PRIMARY KEY REFERENCES image(id) ON DELETE CASCADE,
            stack_id TEXT NOT NULL, position INTEGER NOT NULL);
        CREATE INDEX IF NOT EXISTS photo_stack_group ON photo_stack_member(stack_id,position);",
    )?;
    let tx = conn.transaction()?;
    let mut members = vec![derived.to_owned()];
    for id in sources {
        if !members.contains(id) {
            members.push(id.clone());
        }
    }
    for id in sources {
        let mut stmt = tx.prepare("SELECT image_id FROM photo_stack_member WHERE stack_id=(SELECT stack_id FROM photo_stack_member WHERE image_id=?) ORDER BY position")?;
        for member in stmt.query_map([id], |r| r.get::<_, String>(0))? {
            let member = member?;
            if !members.contains(&member) {
                members.push(member);
            }
        }
    }
    for (position, id) in members.iter().enumerate() {
        tx.execute("INSERT INTO photo_stack_member(image_id,stack_id,position) VALUES(?,?,?) ON CONFLICT(image_id) DO UPDATE SET stack_id=excluded.stack_id,position=excluded.position", rusqlite::params![id, derived, position as i64])?;
        // Existing metadata change triggers make the source's grid row refresh.
        tx.execute("INSERT INTO metadata(image_id,key,value) VALUES(?,'photo_stack',?) ON CONFLICT(image_id,key) DO UPDATE SET value=excluded.value", rusqlite::params![id, derived])?;
    }
    tx.commit()?;
    Ok(())
}

pub(crate) fn photo_stack(db: &Path, id: &str) -> crate::Result<Vec<String>> {
    let conn =
        rusqlite::Connection::open_with_flags(db, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='photo_stack_member')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(vec![]);
    }
    let mut stmt = conn.prepare("SELECT image_id FROM photo_stack_member WHERE stack_id=(SELECT stack_id FROM photo_stack_member WHERE image_id=?) ORDER BY position")?;
    Ok(stmt
        .query_map([id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_metadata_does_not_use_raw_decoder() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rendered.png");
        image::RgbImage::from_pixel(32, 24, image::Rgb([100, 80, 40]))
            .save(&path)
            .unwrap();
        let metadata = EmbeddedMetadata.read(&path).unwrap();
        assert_eq!(metadata.values, vec![("orientation".into(), "1".into())]);
        assert!(metadata.camera.is_none());
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn heic_metadata_does_not_use_raw_decoder() {
        let path = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../image-core/tests/fixtures/rgb.heic"
        ));
        let metadata = EmbeddedMetadata.read(path).unwrap();
        assert_eq!(metadata.values, vec![("orientation".into(), "1".into())]);
        assert!(metadata.camera.is_none());
    }
}
