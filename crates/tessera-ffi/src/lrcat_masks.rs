//! Import APPLY resource injection. This module never opens Adobe helper files.
use crate::{Result, failure};
use engine_api::{id::ImageId, recipe::Recipe};
use ml_segment::{MaskRaster, MaskStore};

pub(crate) const MAX_SLOTS: usize = 256;
const MAX_BYTES: u64 = 256 << 20;
const REGENERATED: &str = "regenerated: no Adobe mask raster; Tessera re-segments at render";

pub(crate) fn key(id: ImageId, slot: usize) -> [u8; 32] {
    let mut bytes = id.0.to_le_bytes().to_vec();
    bytes.extend((slot as u64).to_le_bytes());
    engine_api::id::Digest::derive("tessera imported AI mask image slot v1", &bytes).0
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

/// Image-owned slots: at most 256 rasters and 256 MiB total. Replacements are
/// staged in memory; publication failure restores every previous slot.
pub(crate) fn apply(
    recipe: &mut Recipe,
    id: ImageId,
    extent: (u32, u32),
    store: &MaskStore,
    mut resolve: impl FnMut(&str) -> Option<Vec<u8>>,
    publish: impl FnOnce(&Recipe) -> Result<()>,
) -> Result<()> {
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
                    let size = r.data().len() as u64 * 4 + 48;
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
        let slot = rasters.len();
        state.mask_key = raster.as_ref().map(|_| key(id, slot));
        state.regenerate = raster.is_none();
        unresolved |= state.regenerate;
        if let Some(raster) = raster {
            rasters.push((slot, raster));
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
    // Snapshot at most the same per-image durable bound for rollback.
    let mut prior = Vec::new();
    let mut prior_bytes = 0;
    for slot in 0..MAX_SLOTS {
        if let Some(raster) = store.get(&key(id, slot)) {
            prior_bytes += raster.data().len() as u64 * 4 + 48;
            if prior_bytes > MAX_BYTES {
                return Err(failure("existing imported masks exceed per-image bound"));
            }
            prior.push((slot, raster));
        }
    }
    let save = (|| {
        for (slot, raster) in &rasters {
            store.put_pinned(&key(id, *slot), raster)?;
        }
        for slot in rasters.len()..MAX_SLOTS {
            store.remove_pinned(&key(id, slot))?;
        }
        publish(&next)
    })();
    if save.is_err() {
        for slot in 0..MAX_SLOTS {
            if let Some((_, raster)) = prior.iter().find(|(s, _)| *s == slot) {
                store.put_pinned(&key(id, slot), raster)?;
            } else {
                store.remove_pinned(&key(id, slot))?;
            }
        }
    }
    save?;
    *recipe = next;
    Ok(())
}

pub(crate) fn remove_image(support: &std::path::Path, id: ImageId) -> Result<()> {
    let root = support.join("imported-masks");
    sidecar::Sidecar::ensure_destination(&root, "remove imported masks")?;
    sidecar::Sidecar::ensure_destination(root.join("pinned"), "remove imported masks")?;
    let store = MaskStore::new(root, 0).map_err(failure)?;
    for slot in 0..MAX_SLOTS {
        store.remove_pinned(&key(id, slot))?;
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
            Some(key(id, 0))
        );
        assert!(store.get(&key(id, 1)).is_none());
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
        assert!(store.get(&key(id, 0)).is_none());
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
            256
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
            0
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
        image::GrayImage::from_pixel(8, 4, image::Luma([value])).write_to(&mut bytes, image::ImageFormat::Png).unwrap();
        bytes.into_inner()
    }
    fn raster_key(r: &Recipe) -> [u8;32] {
        r.settings.locals.adjustments[0].components[0].adobe_ai.as_ref().unwrap().mask_key.unwrap()
    }
    #[test]
    fn lr5b_content_keys_preserve_previous_recipe_across_reimport_and_failed_publish() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(),0).unwrap();
        let mut first = fixture();
        apply(&mut first,ImageId(1),(8,4),&store,|_|Some(png(64)),|_|Ok(())).unwrap();
        let old = raster_key(&first);
        let mut second = fixture();
        apply(&mut second,ImageId(1),(8,4),&store,|_|Some(png(192)),|_|Ok(())).unwrap();
        assert_ne!(old,raster_key(&second));
        assert!((store.get(&old).unwrap().data()[0]-64./255.).abs()<1e-5);
        let error = apply(&mut first,ImageId(1),(8,4),&store,|_|Some(png(255)),|_|Err(failure("original publication error"))).unwrap_err();
        assert!(error.to_string().contains("original publication error"));
        assert!((store.get(&old).unwrap().data()[0]-64./255.).abs()<1e-5);
        let mut other = fixture();
        apply(&mut other,ImageId(2),(8,4),&store,|_|Some(png(192)),|_|Ok(())).unwrap();
        assert_eq!(raster_key(&other),raster_key(&second));
    }
    #[test]
    fn lr5b_no_ai_masks_do_not_access_store() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let store = MaskStore::new(&root,0).unwrap();
        std::fs::remove_dir(&root).unwrap();
        std::fs::write(&root,b"inaccessible store").unwrap();
        let mut recipe = import_lrcat::develop(1,"s={Exposure2012=1}","15.4").unwrap().0;
        apply(&mut recipe,ImageId(1),(8,4),&store,|_|panic!("resolver called"),|_|Ok(())).unwrap();
        assert_eq!(std::fs::read(root).unwrap(), b"inaccessible store");
    }
    #[test]
    fn lr5b_imported_rasters_use_two_bytes_per_pixel() {
        let dir = tempfile::tempdir().unwrap();
        let store = MaskStore::new(dir.path(),0).unwrap();
        let mut recipe = fixture();
        apply(&mut recipe,ImageId(1),(8,4),&store,|_|Some(png(128)),|_|Ok(())).unwrap();
        let file = dir.path().join("pinned").join(format!("{}.mask",blake3::Hash::from_bytes(raster_key(&recipe)).to_hex()));
        assert_eq!(std::fs::metadata(file).unwrap().len(),48+8*4*2);
        assert!((store.get(&raster_key(&recipe)).unwrap().data()[0]-128./255.).abs()<1e-5);
    }
}
