//! Import APPLY resource injection. This module never opens Adobe helper files.
use crate::{Result, failure};
use engine_api::{id::ImageId, recipe::Recipe};
use ml_segment::{MaskRaster, MaskStore};

pub(crate) const MAX_SLOTS: usize = 256;
const MAX_BYTES: u64 = 256 << 20;
const REGENERATED: &str = "regenerated: no Adobe mask raster; Tessera re-segments at render";

pub(crate) fn has_masks(recipe: &Recipe) -> bool {
    fn any(c: &engine_api::recipe::MaskComponent) -> bool {
        c.adobe_ai.is_some()
            || c.group
                .as_ref()
                .is_some_and(|children| children.iter().any(any))
    }
    recipe
        .settings
        .locals
        .adjustments
        .iter()
        .flat_map(|g| &g.components)
        .any(any)
}

fn owner_path(store: &MaskStore, id: ImageId) -> std::path::PathBuf {
    store.root().join("owners").join(id.to_string())
}
fn owner_keys(path: &std::path::Path) -> Result<Vec<[u8; 32]>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(e) => return Err(e.into()),
    };
    if bytes.len() % 32 != 0 {
        return Err(failure("invalid imported mask ownership record"));
    }
    Ok(bytes.as_chunks::<32>().0.to_vec())
}
fn write_owner(path: &std::path::Path, keys: &[[u8; 32]]) -> Result<()> {
    use std::io::Write;
    let root = path.parent().expect("owner directory");
    std::fs::create_dir_all(root)?;
    let mut tmp = tempfile::NamedTempFile::new_in(root)?;
    for key in keys {
        tmp.write_all(key)?;
    }
    tmp.as_file().sync_all()?;
    tmp.persist(path).map_err(|e| failure(e.error))?;
    Ok(())
}

fn decode(bytes: &[u8], extent: (u32, u32)) -> Option<MaskRaster> {
    use image::ImageDecoder;
    if bytes.len() as u64 > MAX_BYTES
        || extent.0 == 0
        || extent.1 == 0
        || u64::from(extent.0) * u64::from(extent.1) > (MAX_BYTES - 48) / 4
    {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Tiff) {
        return None;
    }
    let mut reader = image::ImageReader::with_format(std::io::Cursor::new(bytes), format);
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(MAX_BYTES);
    limits.max_image_width = Some(extent.0);
    limits.max_image_height = Some(extent.1);
    reader.limits(limits);
    let decoder = reader.into_decoder().ok()?;
    if decoder.dimensions() != extent
        || !matches!(
            decoder.color_type(),
            image::ColorType::L8 | image::ColorType::L16
        )
    {
        return None;
    }
    let decoded = image::DynamicImage::from_decoder(decoder).ok()?;
    MaskRaster::new(extent.0, extent.1, decoded.to_luma32f().into_raw()).ok()
}

/// At most 256 rasters and 256 MiB per apply. Immutable content keys keep
/// earlier recipes valid; owner records retain history references until pruning.
pub(crate) fn apply(
    recipe: &mut Recipe,
    id: ImageId,
    extent: (u32, u32),
    store: &MaskStore,
    mut resolve: impl FnMut(&str) -> Option<Vec<u8>>,
    publish: impl FnOnce(&Recipe) -> Result<()>,
) -> Result<()> {
    if !has_masks(recipe) {
        return publish(recipe);
    }
    let mut groups = recipe.settings.locals.adjustments.clone();
    let mut stack: Vec<_> = groups
        .iter_mut()
        .rev()
        .flat_map(|g| g.components.iter_mut().rev())
        .collect();
    let mut rasters = Vec::new();
    let mut bytes = 0;
    let mut unresolved = false;
    let mut any = false;
    while let Some(c) = stack.pop() {
        if let Some(children) = &mut c.group {
            stack.extend(children.iter_mut().rev());
            continue;
        }
        let Some(state) = &mut c.adobe_ai else {
            continue;
        };
        any = true;
        let raster = if rasters.len() < MAX_SLOTS {
            state
                .resource_id
                .as_deref()
                .and_then(&mut resolve)
                .and_then(|b| decode(&b, extent))
                .filter(|r| {
                    let size = r.data().len() as u64 * 2 + 48;
                    if bytes + size > MAX_BYTES {
                        false
                    } else {
                        bytes += size;
                        true
                    }
                })
        } else {
            None
        };
        state.mask_key = raster.as_ref().map(MaskRaster::content_key);
        state.regenerate = raster.is_none();
        unresolved |= state.regenerate;
        if let Some(raster) = raster {
            rasters.push(raster);
        }
    }
    let mut next = recipe.clone();
    if any {
        next.set_imported_masks(groups)?;
    }
    if unresolved {
        import_lrcat::diagnostics::push_approximate(
            &mut next,
            "MaskGroupBasedCorrections",
            "/settings/locals/adjustments",
            "LR-5",
            REGENERATED,
        );
    }
    // Write immutable blobs before publication. A crash can leave only orphans,
    // never different pixels under a key that an existing recipe already owns.
    let owner = owner_path(store, id);
    let prior = owner_keys(&owner)?;
    let mut keys = prior.clone();
    for raster in &rasters {
        let key = store.put_content_pinned(raster)?;
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
    if !keys.is_empty() {
        write_owner(&owner, &keys)?;
    }
    if let Err(original) = publish(&next) {
        // Cleanup must never mask the original publication error. Unreferenced
        // content remains safe and is reclaimed by prune_missing.
        if !keys.is_empty() {
            let _ = write_owner(&owner, &prior);
        }
        return Err(original);
    }
    *recipe = next;
    Ok(())
}

/// Drops the ownership records of removed images and collects their content
/// once for the batch. An image that never owned an imported raster costs one
/// failed unlink: no store is opened and no directory is listed.
pub(crate) fn remove_images(
    support: &std::path::Path,
    ids: impl IntoIterator<Item = ImageId>,
) -> Result<()> {
    let owners = support.join("imported-masks").join("owners");
    let mut removed = false;
    for id in ids {
        match std::fs::remove_file(owners.join(id.to_string())) {
            Ok(()) => removed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    if removed {
        prune_missing(support, |_| true)
    } else {
        Ok(())
    }
}

/// Explicit maintenance, not a per-pin write scan. Keeps shared and historical
/// keys of live owners; removes missing owners and crash-orphaned blobs.
pub(crate) fn prune_missing(
    support: &std::path::Path,
    mut alive: impl FnMut(ImageId) -> bool,
) -> Result<()> {
    let root = support.join("imported-masks");
    if !root.exists() {
        return Ok(());
    }
    let owners = root.join("owners");
    let pinned = root.join("pinned");
    for path in [&root, &owners, &pinned] {
        sidecar::Sidecar::ensure_destination(path, "prune imported masks")?;
    }
    let mut retained = std::collections::HashSet::new();
    if owners.exists() {
        for entry in std::fs::read_dir(&owners)? {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let Ok(id) = crate::parse_id(&name) else {
                continue;
            };
            if alive(id) {
                retained.extend(owner_keys(&entry.path())?);
            } else {
                std::fs::remove_file(entry.path())?;
            }
        }
    }
    if pinned.exists() {
        for entry in std::fs::read_dir(&pinned)? {
            let path = entry?.path();
            if path.extension().is_none_or(|extension| extension != "mask") {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
                continue;
            };
            let Ok(key) = blake3::Hash::from_hex(name) else {
                continue;
            };
            if !retained.contains(key.as_bytes()) {
                std::fs::remove_file(path)?;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        image::GrayImage::from_pixel(width, height, image::Luma([value]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }
    #[test]
    fn lr5_invalid_resource_is_pending_and_slots_are_compact() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(), 0).unwrap();
        let id = ImageId(700);
        let source = "s = { MaskGroupBasedCorrections = { { LocalExposure2012=1, CorrectionMasks={ { What='Mask/Image', MaskSubType=2, MaskDigest='missing' }, { What='Mask/Image', MaskSubType=1, MaskDigest='present' } } } } }";
        let (mut recipe, _) = import_lrcat::develop(700, source, "15.4").unwrap();
        apply(
            &mut recipe,
            id,
            (2, 1),
            &store,
            |resource| (resource == "present").then(|| png(2, 1, 128)),
            |_| Ok(()),
        )
        .unwrap();
        let components = &recipe.settings.locals.adjustments[0].components;
        assert!(components[0].adobe_ai.as_ref().unwrap().regenerate);
        assert_eq!(
            components[1].adobe_ai.as_ref().unwrap().mask_key,
            Some(decode(&png(2, 1, 128), (2, 1)).unwrap().content_key())
        );
        let stored = components[1].adobe_ai.as_ref().unwrap().mask_key.unwrap();
        assert!(store.get(&stored).is_some());
        assert!(
            import_lrcat::diagnostics::entries(&recipe)
                .values()
                .flatten()
                .any(|e| e.reason.starts_with("regenerated:"))
        );
        let (mut no_masks, _) =
            import_lrcat::develop(700, "s = { Exposure2012=1 }", "15.4").unwrap();
        apply(
            &mut no_masks,
            id,
            (2, 1),
            &store,
            |_| panic!("no resource request"),
            |_| Ok(()),
        )
        .unwrap();
        assert!(store.get(&stored).is_some());
        assert!(decode(&png(2, 1, 128), (1, 2)).is_none());
        assert!(decode(b"opaque proprietary blob", (2, 1)).is_none());
        assert!(decode(&png(2, 1, 128), (u32::MAX, u32::MAX)).is_none());
    }
}

#[cfg(test)]
mod bound_tests {
    use super::*;
    #[test]
    fn lr5_more_than_256_resources_keeps_excess_pending_and_reimport_shrinks() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(), 0).unwrap();
        let component = "{ What='Mask/Image',MaskSubType=1,MaskDigest='synthetic' }";
        let source = format!(
            "s={{ MaskGroupBasedCorrections={{{{CorrectionMasks={{{}}}}}}} }}",
            vec![component; 257].join(",")
        );
        let (mut recipe, _) = import_lrcat::develop(1, &source, "15.4").unwrap();
        assert_eq!(recipe.settings.locals.adjustments[0].components.len(), 257);
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::GrayImage::from_pixel(1, 1, image::Luma([255]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let bytes = bytes.into_inner();
        let mut calls = 0;
        apply(
            &mut recipe,
            ImageId(1),
            (1, 1),
            &store,
            |_| {
                calls += 1;
                Some(bytes.clone())
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(calls, 256);
        assert!(
            recipe.settings.locals.adjustments[0].components[256]
                .adobe_ai
                .as_ref()
                .unwrap()
                .regenerate
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("pinned"))
                .unwrap()
                .count(),
            1
        );
        apply(
            &mut recipe,
            ImageId(1),
            (1, 1),
            &store,
            |_| None,
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(
            std::fs::read_dir(dir.path().join("pinned"))
                .unwrap()
                .count(),
            1
        );
        assert!(
            recipe.settings.locals.adjustments[0]
                .components
                .iter()
                .all(|c| c.adobe_ai.as_ref().unwrap().regenerate)
        );
    }
}

#[cfg(test)]
mod lr5b_tests {
    use super::*;
    fn fixture() -> Recipe {
        import_lrcat::develop(1, "s={MaskGroupBasedCorrections={{LocalExposure2012=1,CorrectionMasks={{What='Mask/Image',MaskSubType=1,MaskDigest='synthetic'}}}}}", "15.4").unwrap().0
    }
    fn png(value: u8) -> Vec<u8> {
        let mut bytes = std::io::Cursor::new(Vec::new());
        image::GrayImage::from_pixel(8, 4, image::Luma([value]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    fn raster_key(r: &Recipe) -> [u8; 32] {
        r.settings.locals.adjustments[0].components[0]
            .adobe_ai
            .as_ref()
            .unwrap()
            .mask_key
            .unwrap()
    }
    #[test]
    fn lr5b_content_keys_preserve_previous_recipe_across_reimport_and_failed_publish() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(), 0).unwrap();
        let mut first = fixture();
        apply(
            &mut first,
            ImageId(1),
            (8, 4),
            &store,
            |_| Some(png(64)),
            |_| Ok(()),
        )
        .unwrap();
        let old = raster_key(&first);
        let mut second = fixture();
        apply(
            &mut second,
            ImageId(1),
            (8, 4),
            &store,
            |_| Some(png(192)),
            |_| Ok(()),
        )
        .unwrap();
        assert_ne!(old, raster_key(&second));
        assert!((store.get(&old).unwrap().data()[0] - 64. / 255.).abs() < 1e-5);
        let error = apply(
            &mut first,
            ImageId(1),
            (8, 4),
            &store,
            |_| Some(png(255)),
            |_| {
                // Force ownership rollback to fail too: the primary error must
                // still be returned, and previously referenced pixels survive.
                let owner = owner_path(&store, ImageId(1));
                std::fs::remove_file(&owner).unwrap();
                std::fs::create_dir(&owner).unwrap();
                Err(failure("original publication error"))
            },
        )
        .unwrap_err();
        assert!(error.to_string().contains("original publication error"));
        assert!((store.get(&old).unwrap().data()[0] - 64. / 255.).abs() < 1e-5);
        let mut other = fixture();
        apply(
            &mut other,
            ImageId(2),
            (8, 4),
            &store,
            |_| Some(png(192)),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(raster_key(&other), raster_key(&second));
    }
    /// M6: a raster superseded by a successful reimport is an orphan. Import
    /// never deletes or lists; explicit pruning reclaims it while the image
    /// is still alive, and keeps what the published recipe references.
    #[test]
    fn lr5b_superseded_rasters_are_orphans_reclaimed_by_explicit_prune() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path().join("imported-masks"), 0).unwrap();
        let mut recipe = fixture();
        let import = |recipe: &mut Recipe, value: Option<u8>| {
            apply(
                recipe,
                ImageId(1),
                (8, 4),
                &store,
                |_| value.map(png),
                |_| Ok(()),
            )
            .unwrap()
        };
        import(&mut recipe, Some(64));
        let old = raster_key(&recipe);
        import(&mut recipe, Some(192));
        let new = raster_key(&recipe);
        assert!(store.get(&old).is_some(), "import itself never deletes");
        prune_missing(dir.path(), |_| true).unwrap();
        assert!(store.get(&old).is_none(), "superseded raster is an orphan");
        assert!((store.get(&new).unwrap().data()[0] - 192. / 255.).abs() < 1e-5);
        import(&mut recipe, None);
        prune_missing(dir.path(), |_| true).unwrap();
        assert!(store.get(&new).is_none());
    }
    #[test]
    fn lr5b_no_ai_masks_do_not_access_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let store = MaskStore::new(&root, 0).unwrap();
        std::fs::remove_dir(&root).unwrap();
        std::fs::write(&root, b"inaccessible store").unwrap();
        let mut recipe = import_lrcat::develop(1, "s={Exposure2012=1}", "15.4")
            .unwrap()
            .0;
        apply(
            &mut recipe,
            ImageId(1),
            (8, 4),
            &store,
            |_| panic!("resolver called"),
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(std::fs::read(root).unwrap(), b"inaccessible store");
    }
    #[test]
    fn lr5b_imported_rasters_use_two_bytes_per_pixel() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(), 0).unwrap();
        let mut recipe = fixture();
        apply(
            &mut recipe,
            ImageId(1),
            (8, 4),
            &store,
            |_| Some(png(128)),
            |_| Ok(()),
        )
        .unwrap();
        let file = dir.path().join("pinned").join(format!(
            "{}.mask",
            blake3::Hash::from_bytes(raster_key(&recipe)).to_hex()
        ));
        assert_eq!(std::fs::metadata(file).unwrap().len(), 48 + 8 * 4 * 2);
        assert!((store.get(&raster_key(&recipe)).unwrap().data()[0] - 128. / 255.).abs() < 1e-5);
    }
}

#[cfg(test)]
mod lr5b_prune_tests {
    use super::*;
    #[test]
    fn lr5b_prune_missing_collects_orphans_and_preserves_shared_live_content() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path().join("imported-masks"), 0).unwrap();
        let shared = store
            .put_content_pinned(&MaskRaster::new(1, 1, vec![0.5]).unwrap())
            .unwrap();
        let gone = store
            .put_content_pinned(&MaskRaster::new(1, 1, vec![1.]).unwrap())
            .unwrap();
        let orphan = store
            .put_content_pinned(&MaskRaster::new(1, 1, vec![0.]).unwrap())
            .unwrap();
        write_owner(&owner_path(&store, ImageId(1)), &[shared, gone]).unwrap();
        write_owner(&owner_path(&store, ImageId(2)), &[shared]).unwrap();
        prune_missing(dir.path(), |id| id == ImageId(2)).unwrap();
        assert!(store.get(&shared).is_some());
        assert!(store.get(&gone).is_none());
        assert!(store.get(&orphan).is_none());
        assert!(!owner_path(&store, ImageId(1)).exists());
        assert!(owner_path(&store, ImageId(2)).exists());
    }
}

#[cfg(all(test, unix))]
mod lr5b_removal_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    /// M4: removing images that never owned an imported raster must not
    /// enumerate the store, even when other images have imported masks.
    #[test]
    fn lr5b_removing_images_without_ai_masks_does_not_scan_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path().join("imported-masks"), 0).unwrap();
        let key = store
            .put_content_pinned(&MaskRaster::new(1, 1, vec![0.5]).unwrap())
            .unwrap();
        write_owner(&owner_path(&store, ImageId(1)), &[key]).unwrap();
        let unreadable = [store.root().join("pinned"), store.root().join("owners")];
        for path in &unreadable {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o300)).unwrap();
        }
        let result = remove_images(dir.path(), [ImageId(2), ImageId(3)]);
        for path in &unreadable {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        assert!(
            result.is_ok(),
            "removal must not need to list the store: {:?}",
            result.err().map(|e| e.to_string())
        );
        // Removing the owner still collects its content, once for the batch.
        remove_images(dir.path(), [ImageId(1), ImageId(2)]).unwrap();
        assert!(store.get(&key).is_none());
        assert!(!owner_path(&store, ImageId(1)).exists());
    }
}
