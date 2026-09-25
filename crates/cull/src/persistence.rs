use crate::{Change, ImageId};
use engine_api::{EngineError, EngineResult, recipe::Recipe};
use index::Index;
use sidecar::{MarkPreset, RecipeDocument, Sidecar, XmpPacket};
use std::{fs, io::Write, path::Path};

pub(crate) fn optional_bytes(path: &Path) -> EngineResult<Option<Vec<u8>>> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(EngineError::io_at(path, &e)),
    }
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> EngineResult<()> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    fs::create_dir_all(parent).map_err(|e| EngineError::io_at(parent, &e))?;
    let mut temp =
        tempfile::NamedTempFile::new_in(parent).map_err(|e| EngineError::io_at(path, &e))?;
    temp.write_all(bytes)
        .map_err(|e| EngineError::io_at(path, &e))?;
    temp.as_file()
        .sync_all()
        .map_err(|e| EngineError::io_at(path, &e))?;
    temp.persist(path)
        .map_err(|e| EngineError::io_at(path, &e.error))?;
    Ok(())
}
fn restore(path: &Path, bytes: &Option<Vec<u8>>) -> EngineResult<()> {
    match bytes {
        Some(bytes) => atomic_write(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(EngineError::io_at(path, &e)),
        },
    }
}

pub(crate) fn load(index: &Index, id: ImageId) -> EngineResult<RecipeDocument> {
    let info = index.image_info(id)?;
    let paths = Sidecar::paths(&info.path);
    if optional_bytes(&paths.recipe)?.is_some() {
        let document = Sidecar::read_recipe(&paths.recipe)?;
        if document.recipe.image_id.is_some_and(|stored| stored != id) {
            return Err(EngineError::invalid(
                "image_id",
                "sidecar belongs to another image",
            ));
        }
        return Ok(document);
    }
    let mut recipe = Recipe::new(id);
    let xmp = if optional_bytes(&paths.xmp)?.is_some() {
        paths.xmp
    } else {
        info.path.with_extension("xmp")
    };
    recipe.selection = if optional_bytes(&xmp)?.is_some() {
        Sidecar::read_xmp(xmp)?.selection()?
    } else {
        index.selection(id)?.unwrap_or_default()
    };
    Ok(RecipeDocument {
        recipe,
        ..Default::default()
    })
}

/// Preflight every image before writing; compensate earlier writes on failure.
/// Atomic per file, not crash-atomic across files/SQLite. Recipe is authoritative
/// and opening a session reconciles the rebuildable index after interruption.
pub(crate) fn write_changes(index: &Index, changes: &[Change], forward: bool) -> EngineResult<()> {
    let mut prepared = Vec::new();
    let mut destinations = std::collections::HashSet::new();
    for change in changes {
        let info = index.image_info(change.id)?;
        let mut paths = Sidecar::paths(&info.path);
        // Search the index, not the filtered/paginated review queue.
        let folder = info.path.parent().unwrap_or(Path::new("."));
        for peer in index.search(&index::Query {
            folder: Some(folder.to_string_lossy().into_owned()),
            limit: i64::MAX as usize,
            ..Default::default()
        })? {
            if peer != change.id {
                let peer = index.image_info(peer)?;
                if peer.path.parent() == Some(folder)
                    && Sidecar::paths(&peer.path).recipe == paths.recipe
                {
                    return Err(EngineError::invalid(
                        "sidecar",
                        format!(
                            "destination collision: {} and {}",
                            info.path.display(),
                            peer.path.display()
                        ),
                    ));
                }
            }
        }
        if optional_bytes(&paths.xmp)?.is_none()
            && optional_bytes(&info.path.with_extension("xmp"))?.is_some()
        {
            paths.xmp = info.path.with_extension("xmp");
        }
        for destination in [&paths.recipe, &paths.xmp] {
            if !destinations.insert(destination.clone()) {
                return Err(EngineError::invalid(
                    "sidecar",
                    format!("batch destination collision: {}", destination.display()),
                ));
            }
        }
        let old_recipe = optional_bytes(&paths.recipe)?;
        let old_xmp = optional_bytes(&paths.xmp)?;
        let old_selection = index.selection(change.id)?.unwrap_or_default();
        let mut document = load(index, change.id)?;
        let expected = if forward {
            &change.before
        } else {
            &change.after
        };
        if &document.recipe.selection != expected {
            return Err(EngineError::invalid(
                "selection",
                "changed outside this undo stack; reopen session",
            ));
        }
        document.recipe.selection = if forward {
            change.after.clone()
        } else {
            change.before.clone()
        };
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .min(i64::MAX as u128) as i64;
        // Each process is a separate logical writer; no host identity/secret needed.
        document.record_write(&format!("cull-{}", std::process::id()), timestamp)?;
        let packet = if old_xmp.is_some() {
            let old = Sidecar::read_xmp(&paths.xmp)?;
            old.with_metadata(
                &document.recipe.selection,
                &old.metadata()?,
                &MarkPreset::default(),
            )?
        } else {
            XmpPacket::from_selection(&document.recipe.selection, &MarkPreset::default())
        };
        document.recipe.validate()?;
        document.recipe.to_json()?;
        prepared.push((
            change.id,
            paths,
            old_recipe,
            old_xmp,
            old_selection,
            document,
            packet,
        ));
    }
    for (n, (id, paths, _, _, _, document, packet)) in prepared.iter().enumerate() {
        let result = (|| {
            Sidecar::write_recipe(&paths.recipe, document)?;
            Sidecar::write_xmp(&paths.xmp, packet)?;
            index.set_selection(*id, &document.recipe.selection)
        })();
        if let Err(error) = result {
            let mut rollback_errors = Vec::new();
            for (id, paths, recipe, xmp, selection, _, _) in prepared[..=n].iter().rev() {
                for result in [
                    restore(&paths.recipe, recipe),
                    restore(&paths.xmp, xmp),
                    index.set_selection(*id, selection),
                ] {
                    if let Err(e) = result {
                        rollback_errors.push(e.to_string());
                    }
                }
            }
            if !rollback_errors.is_empty() {
                return Err(EngineError::invalid(
                    "persistence",
                    format!("{error}; rollback failed: {}", rollback_errors.join(", ")),
                ));
            }
            return Err(error);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use index::{NoopMetadataProvider, NoopSidecarReader, Query};

    #[test]
    fn late_same_stem_batch_collision_preserves_every_file_and_index_row() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["first.jpg", "image.jpg", "image.cr3"] {
            fs::write(dir.path().join(name), b"image").unwrap();
        }
        let mut index = Index::open(":memory:").unwrap();
        index
            .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        let ids = index.search(&Query::default()).unwrap();
        assert_eq!(ids.len(), 3);
        let mut changes: Vec<_> = ids
            .iter()
            .map(|id| {
                let before = load(&index, *id).unwrap().recipe.selection;
                let mut after = before.clone();
                after.set_decision(crate::Decision::Keep);
                Change {
                    id: *id,
                    before,
                    after,
                }
            })
            .collect();
        changes.sort_by_key(|change| index.image_info(change.id).unwrap().path);
        assert!(
            index
                .image_info(changes[0].id)
                .unwrap()
                .path
                .ends_with("first.jpg")
        );
        let old: Vec<_> = ids.iter().map(|id| index.selection(*id).unwrap()).collect();
        let error = write_changes(&index, &changes, true).unwrap_err();
        assert!(error.to_string().contains("collision"));
        assert!(!dir.path().join(".edits").exists());
        for (id, old) in ids.iter().zip(old) {
            assert_eq!(index.selection(*id).unwrap(), old);
            assert!(
                !Sidecar::paths(index.image_info(*id).unwrap().path)
                    .xmp
                    .exists()
            );
        }
    }

    #[test]
    fn duplicate_batch_destinations_fail_before_any_writes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("one.jpg"), b"image").unwrap();
        let mut index = Index::open(":memory:").unwrap();
        index
            .scan(dir.path(), &NoopSidecarReader, &NoopMetadataProvider)
            .unwrap();
        let id = index.search(&Query::default()).unwrap()[0];
        let before = load(&index, id).unwrap().recipe.selection;
        let mut after = before.clone();
        after.set_decision(crate::Decision::Keep);
        let change = Change { id, before, after };
        let old = index.selection(id).unwrap();
        let error = write_changes(&index, &[change.clone(), change], true).unwrap_err();
        assert!(error.to_string().contains("collision"));
        assert!(!dir.path().join(".edits").exists());
        assert!(!Sidecar::paths(dir.path().join("one.jpg")).xmp.exists());
        assert_eq!(index.selection(id).unwrap(), old);
    }
}
