//! Adapter for the index's scanner hooks and read-only schema-v3 metadata.
use engine_api::{EngineResult, id::ImageId, recipe::Recipe};
use index::{Metadata, MetadataProvider, SidecarData, SidecarReader};
use sidecar::{RecipeDocument, Sidecar, XmpPacket};
use std::path::{Path, PathBuf};

pub(crate) fn xmp_path(path: &Path) -> PathBuf {
    let appended = Sidecar::paths(path).xmp;
    if Sidecar::is_lightroom_owned(path)
        || appended.exists()
        || !path.with_extension("xmp").exists()
    {
        appended
    } else {
        path.with_extension("xmp")
    }
}

pub(crate) fn write_paths(path: &Path) -> EngineResult<sidecar::SidecarPaths> {
    let mut paths = Sidecar::paths(path);
    if !Sidecar::is_lightroom_owned(path)
        && !paths.xmp.exists()
        && path.with_extension("xmp").exists()
    {
        paths.xmp = path.with_extension("xmp");
    }
    Sidecar::ensure_writable_destination(&paths.recipe)?;
    Sidecar::ensure_writable_destination(&paths.xmp)?;
    Ok(paths)
}

pub(crate) fn document(path: &Path, id: ImageId) -> EngineResult<RecipeDocument> {
    let recipe = Sidecar::paths(path).recipe;
    if recipe.exists() {
        let mut document = Sidecar::read_recipe(recipe)?;
        // Content-addressed protected edits survive a path-derived index ID change.
        if Sidecar::is_lightroom_owned(path) {
            document.recipe.image_id = Some(id);
        }
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

/// The index's metadata provider: embedded metadata plus listing facts.
pub(crate) struct IndexedMetadata;

impl MetadataProvider for IndexedMetadata {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let mut metadata = EmbeddedMetadata.read(path)?;
        // An unreadable recipe records no facts: listing then reads it as before.
        let recipe = Sidecar::paths(path).recipe;
        if !recipe.exists() {
            metadata.values.extend(listing_facts(None));
        } else if let Ok(document) = Sidecar::read_recipe(recipe) {
            metadata
                .values
                .extend(listing_facts(Some(&document.recipe)));
        }
        Ok(metadata)
    }
}

impl MetadataProvider for EmbeddedMetadata {
    fn read(&self, path: &Path) -> EngineResult<Metadata> {
        let oriented = catalog_orientation(path).is_some();
        let presentation_orientation = |value: u16| if oriented { 1 } else { value };
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
            if let Ok(source) = raw_decode::RawSource::open(path) {
                let m = source.metadata();
                return Ok(Metadata {
                    capture_time: Some(m.capture_time.to_string()),
                    camera: Some(m.model),
                    lens: m.lens,
                    values: vec![(
                        "orientation".into(),
                        presentation_orientation(m.orientation).to_string(),
                    )],
                    ..Default::default()
                });
            }
            if path
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("dng"))
                && let Some(metadata) =
                    raw_decode::lossy_dng::read_metadata(&mut std::fs::File::open(path)?)?
            {
                return Ok(Metadata {
                    values: vec![(
                        "orientation".into(),
                        presentation_orientation(metadata.orientation).to_string(),
                    )],
                    ..Default::default()
                });
            }
            let source = raw_decode::RawSource::open(path)?;
            let m = source.metadata();
            return Ok(Metadata {
                capture_time: Some(m.capture_time.to_string()),
                camera: Some(m.model),
                lens: m.lens,
                values: vec![(
                    "orientation".into(),
                    presentation_orientation(m.orientation).to_string(),
                )],
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
            values: vec![(
                "orientation".into(),
                presentation_orientation(orientation as u16).to_string(),
            )],
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

/// Stable recipe owner stays at the proxy path; only the pixel source switches.
/// No index migration, sidecar move, or copy into Lightroom is involved.
#[cfg(test)]
thread_local! {
    static PROXY_RECIPE_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
/// Recipe reads made by listing projections on this thread (tests only).
#[cfg(test)]
pub(crate) fn proxy_recipe_reads() -> usize {
    PROXY_RECIPE_READS.with(std::cell::Cell::get)
}

pub(crate) fn lightroom_proxy(path: &Path) -> Option<serde_json::Value> {
    #[cfg(test)]
    PROXY_RECIPE_READS.with(|reads| reads.set(reads.get() + 1));
    let document = Sidecar::read_recipe(Sidecar::paths(path).recipe).ok()?;
    document
        .recipe
        .unknown
        .get("lightroom_smart_preview")
        .cloned()
}
/// Index metadata recorded when a file is indexed (A-LR8 M5): the recipe hash
/// the facts were derived from, and for imported Smart Previews the catalog's
/// original path (empty when the catalog had none). Listing uses them instead
/// of reading every recipe while that hash is still the image's current one.
pub(crate) const LISTING_RECIPE_KEY: &str = "tessera.listing.recipe";
pub(crate) const LISTING_ORIGINAL_KEY: &str = "tessera.listing.original";

/// Scan-time listing facts of one row, with the image's current recipe hash.
#[derive(Clone, Debug, Default)]
pub(crate) struct ScanFacts {
    pub recipe: Option<String>,
    pub original: Option<String>,
    pub current: String,
}

/// One listing row: the pixel source, whether it is an offline Smart Preview,
/// and the catalog file name of an imported proxy (user data: display and
/// export names only, never identifiers).
#[derive(Clone, Debug)]
pub(crate) struct ListingRow {
    pub source: String,
    pub offline: bool,
    pub display_name: Option<String>,
}

fn listing_facts(recipe: Option<&Recipe>) -> Vec<(String, String)> {
    let hash = recipe.map_or_else(
        || Recipe::default().recipe_hash(),
        |recipe| recipe.recipe_hash(),
    );
    let mut values = vec![(LISTING_RECIPE_KEY.into(), hash.to_string())];
    if let Some(proxy) = recipe.and_then(|r| r.unknown.get("lightroom_smart_preview")) {
        let original = proxy
            .get("original_path")
            .and_then(|p| p.as_str())
            .unwrap_or_default();
        values.push((LISTING_ORIGINAL_KEY.into(), original.into()));
    }
    values
}

/// Listing projection. With current scan-time facts no recipe is read; the
/// candidate original is still checked once, so a relinked original shows.
/// Pixel requests resolve independently so their source is never a stale snapshot.
pub(crate) fn project(path: &Path, facts: Option<&ScanFacts>) -> ListingRow {
    let proxy: Option<Option<PathBuf>> =
        match facts.filter(|f| f.recipe.as_deref() == Some(f.current.as_str())) {
            Some(facts) => facts
                .original
                .as_ref()
                .map(|o| (!o.is_empty()).then(|| PathBuf::from(o))),
            None => lightroom_proxy(path).map(|v| {
                v.get("original_path")
                    .and_then(|p| p.as_str())
                    .map(PathBuf::from)
            }),
        };
    let original = proxy.clone().flatten();
    let source = original
        .clone()
        .filter(|p| p.is_file())
        .unwrap_or_else(|| path.to_path_buf());
    ListingRow {
        offline: proxy.is_some() && source == path,
        display_name: original
            .as_deref()
            .and_then(Path::file_name)
            .map(|n| n.to_string_lossy().into_owned()),
        source: source.to_string_lossy().into_owned(),
    }
}

/// Catalog file name of an imported proxy, for export names (reads the recipe).
pub(crate) fn proxy_display_name(path: &Path) -> Option<String> {
    project(path, None).display_name
}

pub(crate) fn source_projection(path: &Path) -> (PathBuf, bool) {
    let row = project(path, None);
    (PathBuf::from(row.source), row.offline)
}
pub(crate) fn source_path(path: &Path) -> PathBuf {
    source_projection(path).0
}
#[cfg(test)]
pub(crate) fn is_offline_proxy(path: &Path) -> bool {
    source_projection(path).1
}

/// Catalog-change invalidated listing facts. A warm refresh performs no recipe
/// reads or original stats for unchanged rows; reopen takes a fresh snapshot.
#[derive(Default)]
pub(crate) struct ListingCache {
    sequence: u64,
    sources: std::collections::HashMap<ImageId, ListingRow>,
}
impl ListingCache {
    pub fn sync(&mut self, index: &index::Index) -> EngineResult<()> {
        let changes = index.changes_since(self.sequence)?;
        if changes.reset {
            self.sources.clear();
        }
        for change in changes.changes {
            if !matches!(change.kind, index::ChangeKind::Updated(fields)
                if !fields.intersects(index::ChangeFields::FILE | index::ChangeFields::RECIPE | index::ChangeFields::METADATA))
            {
                self.sources.remove(&change.id);
            }
        }
        self.sequence = changes.to;
        Ok(())
    }
    pub fn source(&mut self, id: ImageId, path: &Path, facts: Option<&ScanFacts>) -> ListingRow {
        self.sources
            .entry(id)
            .or_insert_with(|| project(path, facts))
            .clone()
    }
}

pub(crate) fn catalog_orientation(path: &Path) -> Option<u16> {
    Sidecar::read_recipe(Sidecar::paths(path).recipe)
        .ok()?
        .recipe
        .unknown
        .get("lightroom_orientation")?
        .as_u64()
        .filter(|o| (1..=8).contains(o))
        .map(|o| o as u16)
}

pub(crate) fn open_image(id: ImageId, owner_path: &Path) -> EngineResult<image_core::RawImage> {
    let source = source_path(owner_path);
    let render_id = if source != owner_path {
        crate::lrcat::app_image_id(&source).unwrap_or(id)
    } else {
        id
    };
    Ok(image_core::RawImage::open_with_catalog_orientation(
        render_id,
        source,
        catalog_orientation(owner_path),
    )?
    .with_recipe_owner(id))
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

#[cfg(test)]
#[path = "lrcat_orientation_tests.rs"]
mod lrcat_orientation_tests;
