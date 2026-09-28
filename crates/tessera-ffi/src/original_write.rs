//! In-process admission for legacy operations which publish original sidecars.
//!
//! Acquire all stable image reservations before looking up destination gates.
//! Reservations, unlike held mutexes, may span agent progress callbacks. These
//! are not OS locks and cannot serialize another application or Console process.
use crate::{
    Result, failure,
    image_edit_admission::{self, EditSource, ImageEditLease},
    recipe_write,
    smart_preview_store::SmartPreviewJournal,
};
use engine_api::id::ImageId;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) struct OriginalWriteReservation {
    // Release destination protection first, then image admission.
    _destinations: Vec<recipe_write::DevelopLease>,
    _images: Vec<ImageEditLease>,
    authorized: Vec<(ImageId, PathBuf)>,
}
impl OriginalWriteReservation {
    pub(crate) fn acquire(support: &Path, images: &[(ImageId, PathBuf)]) -> Result<Self> {
        let mut images = images.to_vec();
        images.sort_by(|a, b| a.0.0.cmp(&b.0.0).then(a.1.cmp(&b.1)));
        images.dedup();
        if images.windows(2).any(|pair| pair[0].0 == pair[1].0) {
            return Err(failure(
                "conflict: one image resolved to multiple original paths",
            ));
        }
        let mut reservations = Vec::new();
        for (id, _) in &images {
            reservations.push(
                image_edit_admission::gate_for(*id)?.reserve_develop(EditSource::ExternalWriter)?,
            );
        }
        // Every image is admitted before any sidecar can be published. Dirty
        // journals fail locally, before destination_key canonicalizes a volume.
        for (id, path) in &images {
            require_clean_journal(support, *id)?;
            if !path.is_file() {
                return Err(failure(
                    "original unavailable: reconnect the original before changing its sidecars",
                ));
            }
        }
        let mut gates: Vec<Arc<recipe_write::GateState>> = Vec::new();
        let mut destinations = Vec::new();
        for (_, path) in &images {
            let gate = recipe_write::gate_for(path)?;
            if gates.iter().any(|previous| Arc::ptr_eq(previous, &gate)) {
                continue;
            }
            destinations.push(gate.reserve_external(path)?);
            gates.push(gate);
        }
        Ok(Self {
            _destinations: destinations,
            _images: reservations,
            authorized: images,
        })
    }
    pub(crate) fn validate(&self, id: ImageId, path: &Path) -> Result<()> {
        if !self
            .authorized
            .iter()
            .any(|(owner, locator)| *owner == id && locator == path)
        {
            return Err(failure(
                "original writer does not own this image destination",
            ));
        }
        if !path.is_file() {
            return Err(failure(
                "original unavailable: reconnect before publishing sidecars",
            ));
        }
        Ok(())
    }
}

pub(crate) fn require_clean_journal(support: &Path, id: ImageId) -> Result<()> {
    let journal = support
        .join("smart-previews")
        .join(id.to_string())
        .join("journal.json");
    if journal.try_exists()?
        && SmartPreviewJournal::open(support, id)
            .map_err(failure)?
            .1
            .dirty
    {
        return Err(failure(
            "Smart Preview needs sync: close the preview editor and synchronize local edits before changing original sidecars",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::Recipe;
    fn recipe(id: ImageId) -> Vec<u8> {
        serde_json::to_vec(&sidecar::RecipeDocument {
            recipe: Recipe::new(id),
            ..Default::default()
        })
        .unwrap()
    }
    #[test]
    fn dirty_offline_batch_rejects_before_creating_original_sidecars_and_releases_all_ids() {
        let local = tempfile::tempdir().unwrap();
        let first = ImageId(9211);
        let dirty = ImageId(9212);
        let original = local.path().join("original.raw");
        std::fs::write(&original, b"immutable").unwrap();
        let missing = local.path().join("offline").join("photo.raw");
        let mut journal =
            SmartPreviewJournal::create(local.path(), dirty, [1; 32], 9, recipe(dirty), None, None)
                .unwrap();
        journal.save_recipe(recipe(dirty)).unwrap();
        let result = OriginalWriteReservation::acquire(
            local.path(),
            &[(first, original.clone()), (dirty, missing.clone())],
        );
        assert!(result.err().unwrap().to_string().contains("needs sync"));
        assert!(!missing.parent().unwrap().exists());
        assert!(!sidecar::Sidecar::paths(&original).recipe.exists());
        assert_eq!(std::fs::read(&original).unwrap(), b"immutable");
        assert!(journal.snapshot().unwrap().dirty);
        assert!(
            image_edit_admission::gate_for(first)
                .unwrap()
                .reserve_develop(EditSource::Original)
                .is_ok()
        );
    }
    #[test]
    fn external_reservation_rejects_editors_and_selection_but_callbacks_can_read() {
        let local = tempfile::tempdir().unwrap();
        let id = ImageId(9213);
        let path = local.path().join("original.raw");
        std::fs::write(&path, b"original").unwrap();
        let reservation =
            OriginalWriteReservation::acquire(local.path(), &[(id, path.clone())]).unwrap();
        let gate = image_edit_admission::gate_for(id).unwrap();
        assert!(gate.reserve_develop(EditSource::Original).is_err());
        assert!(gate.reserve_develop(EditSource::SmartPreview).is_err());
        assert!(gate.begin_selection_write().is_err());
        drop(gate.begin_read().unwrap());
        let destination = recipe_write::gate_for(&path).unwrap();
        assert!(destination.begin_selection_write().is_err());
        drop(destination.begin_read().unwrap());
        assert!(reservation.validate(id, &path).is_ok());
        assert!(reservation.validate(ImageId(999), &path).is_err());
        drop(reservation);
        assert!(gate.begin_write().is_ok());
        assert!(destination.begin_selection_write().is_ok());
    }
    #[test]
    fn existing_proxy_editor_blocks_external_mutation_without_publishing() {
        let local = tempfile::tempdir().unwrap();
        let id = ImageId(9214);
        let path = local.path().join("original.raw");
        std::fs::write(&path, b"original").unwrap();
        let gate = image_edit_admission::gate_for(id).unwrap();
        let _preview = gate.reserve_develop(EditSource::SmartPreview).unwrap();
        assert!(OriginalWriteReservation::acquire(local.path(), &[(id, path.clone())]).is_err());
        assert!(!sidecar::Sidecar::paths(&path).recipe.exists());
    }
    #[test]
    fn missing_original_and_alias_destination_failures_release_reservations() {
        let local = tempfile::tempdir().unwrap();
        let id = ImageId(9215);
        let path = local.path().join("missing.raw");
        assert!(
            OriginalWriteReservation::acquire(local.path(), &[(id, path.clone())])
                .err()
                .unwrap()
                .to_string()
                .contains("original unavailable")
        );
        let gate = image_edit_admission::gate_for(id).unwrap();
        assert!(gate.begin_write().is_ok());
        std::fs::write(&path, b"original").unwrap();
        let destination = recipe_write::gate_for(&path).unwrap();
        let (lease, ()) = destination.reserve_develop(&path, || Ok(())).unwrap();
        assert!(OriginalWriteReservation::acquire(local.path(), &[(id, path.clone())]).is_err());
        drop(lease);
        assert!(gate.begin_write().is_ok());
        let alias = path.with_extension("dng");
        std::fs::write(&alias, b"alias").unwrap();
        assert!(
            OriginalWriteReservation::acquire(local.path(), &[(id, path), (ImageId(9216), alias)])
                .is_ok()
        );
    }
}

#[cfg(test)]
mod entrypoint_tests {
    use super::*;
    use crate::*;
    fn fixture() -> (tempfile::TempDir, Arc<Engine>, PathBuf, String) {
        let root = tempfile::tempdir().unwrap();
        let photos = root.path().join("photos");
        std::fs::create_dir(&photos).unwrap();
        let photo = photos.join("one.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([100, 80, 60]))
            .save(&photo)
            .unwrap();
        let engine = Engine::open(root.path().join("support").to_string_lossy().into()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into())
            .unwrap();
        let id = engine.list_images(ImageQuery::default()).unwrap()[0]
            .id
            .clone();
        (root, engine, photo, id)
    }
    fn dirty(engine: &Engine, id: &str) -> SmartPreviewJournal {
        let owner = crate::parse_id(id).unwrap();
        let recipe: engine_api::recipe::Recipe =
            serde_json::from_str(&engine.get_recipe(id.into()).unwrap()).unwrap();
        let bytes = serde_json::to_vec(&sidecar::RecipeDocument {
            recipe,
            ..Default::default()
        })
        .unwrap();
        let mut journal = SmartPreviewJournal::create(
            engine.support_dir().unwrap(),
            owner,
            [1; 32],
            1,
            bytes.clone(),
            None,
            None,
        )
        .unwrap();
        journal.save_recipe(bytes).unwrap();
        journal
    }
    #[test]
    fn metadata_and_agent_entrypoints_refuse_dirty_journal_before_original_publication() {
        let (_root, engine, photo, id) = fixture();
        let journal = dirty(&engine, &id);
        let paths = sidecar::Sidecar::paths(&photo);
        let library = engine
            .clone()
            .open_library(
                photo
                    .parent()
                    .unwrap()
                    .join("library.json")
                    .to_string_lossy()
                    .into(),
            )
            .unwrap();
        let result = library.set_iptc(
            vec![id.clone()],
            IptcEdit {
                title: Some("blocked".into()),
                ..Default::default()
            },
        );
        assert!(result.unwrap_err().to_string().contains("needs sync"));
        assert!(
            engine
                .accept_agent_edit(id.clone(), photo.parent().unwrap().to_string_lossy().into())
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        assert!(
            engine
                .revert_agent_edit(id.clone(), 1)
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        let request = AgentRunRequest {
            images: vec![AgentImageInput {
                image_id: id,
                burst: None,
                people: vec![],
            }],
            library_folder: photo.parent().unwrap().to_string_lossy().into(),
            provider: AgentProvider::Scripted,
            guardrails: AgentGuardrails {
                allow_masks: false,
                allow_crop: false,
                allow_skin_retouch: false,
                visual_critic: false,
                max_iterations: 1,
                time_budget_seconds: 1,
            },
            instruction: None,
        };
        assert!(
            engine
                .run_agent(request, CancelFlag::new(), None)
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        assert!(!paths.recipe.exists());
        assert!(!paths.xmp.exists());
        assert!(journal.snapshot().unwrap().dirty);
    }
    #[test]
    fn people_naming_and_historical_sidecar_undo_refuse_dirty_journal_without_losing_history() {
        let (_root, engine, photo, id) = fixture();
        let mut embedding = vec![0.; 128];
        embedding[0] = 1.;
        engine
            .set_faces(
                id.clone(),
                vec![FaceInput {
                    x: 2.,
                    y: 2.,
                    width: 12.,
                    height: 12.,
                    focus: 0.8,
                    eyes_open: None,
                    embedding: Some(embedding),
                }],
                32,
                24,
            )
            .unwrap();
        let session = engine
            .open_cull_session(photo.parent().unwrap().to_string_lossy().into())
            .unwrap();
        let person = session.people(false).unwrap()[0].id.clone();
        let options = PeopleNameOptions {
            write_sidecars: true,
            person_keywords: true,
        };
        session
            .name_person(person.clone(), Some("Alice".into()), options)
            .unwrap();
        let xmp = sidecar::Sidecar::paths(&photo).xmp;
        let before = std::fs::read(&xmp).unwrap();
        let mut journal = dirty(&engine, &id);
        assert!(
            session
                .name_person(person, Some("Bob".into()), options)
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        assert!(
            session
                .undo_people_edit()
                .unwrap_err()
                .to_string()
                .contains("needs sync")
        );
        assert_eq!(std::fs::read(&xmp).unwrap(), before);
        // Model an acknowledged clean baseline; the rejected operation must not
        // pop its history entry, and a later retry may restore the exact files.
        let bytes = journal.snapshot().unwrap().recipe;
        journal.mark_synced(bytes, None, Some(before)).unwrap();
        assert!(session.undo_people_edit().unwrap().is_some());
        assert!(session.redo_people_edit().unwrap().is_some());
    }
}
