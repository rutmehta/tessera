//! Serialized catalog people edits, with persistent numeric ID adaptation.
use crate::Console;
use engine_api::{
    EngineError, EngineResult,
    people::{FaceRef, PeopleWriteOptions},
    tools::{LibraryToolCall, LibraryToolRequest},
};
use index::{FaceKey, Index};
use serde_json::{Value, json};
use std::collections::HashSet;

fn faces(index: &Index, refs: &[FaceRef], expected: Option<&str>) -> EngineResult<Vec<FaceKey>> {
    let mut seen = HashSet::new();
    if refs.is_empty() {
        return Err(EngineError::invalid(
            "faces",
            "nonempty unique faces required",
        ));
    }
    let mut keys = Vec::with_capacity(refs.len());
    for face in refs {
        if !seen.insert(*face) {
            return Err(EngineError::invalid("faces", "duplicate face reference"));
        }
        index.image_info(face.image_id)?;
        if !index
            .faces(face.image_id)?
            .iter()
            .any(|f| f.id == face.ordinal)
        {
            return Err(EngineError::not_found(
                "face",
                format!("{}:{}", face.image_id, face.ordinal),
            ));
        }
        let key = FaceKey {
            image_id: face.image_id,
            ordinal: face.ordinal,
        };
        if let Some(expected) = expected {
            let matches = index
                .face_assignments(face.image_id)?
                .iter()
                .any(|a| a.face == key && a.person_id == expected);
            if !matches {
                return Err(EngineError::invalid(
                    "faces",
                    "face is not assigned to the expected person",
                ));
            }
        }
        keys.push(key);
    }
    Ok(keys)
}

impl Console {
    // Reject missing analysis dimensions before changing membership. Export
    // itself delegates collision, geometry and XML validation to cull.
    fn preflight_people_writes(
        &self,
        keys: &[&str],
        faces: &[FaceKey],
        writes: PeopleWriteOptions,
    ) -> EngineResult<()> {
        if !writes.write_sidecars {
            return Ok(());
        }
        let mut images: HashSet<_> = faces.iter().map(|f| f.image_id).collect();
        for key in keys {
            images.extend(
                self.index
                    .images_with_person(key, false, i64::MAX as usize, 0)?,
            );
        }
        for image in images {
            let scores = self.index.scores(image)?;
            for signal in ["analysis_width", "analysis_height"] {
                if !scores.iter().any(|s| {
                    s.signal == signal
                        && s.value.is_finite()
                        && s.value >= 1.0
                        && s.value <= f64::from(u32::MAX)
                        && s.value.fract() == 0.0
                }) {
                    return Err(EngineError::invalid(
                        "dimensions",
                        format!("missing or invalid {signal} for {image}"),
                    ));
                }
            }
        }
        Ok(())
    }

    fn person_keywords(&self, key: &str, writes: PeopleWriteOptions) -> EngineResult<()> {
        if writes.person_keywords
            && let Some(name) = self
                .index
                .people()?
                .into_iter()
                .find(|p| p.id == key)
                .and_then(|p| p.name)
        {
            let path = self.app.join("library.json");
            let mut library = library::Library::read(&path)?;
            let name = name.trim();
            if library.keyword_path(name).is_none() {
                library.add_keyword(name, None)?;
            }
            let keyword_path = library.keyword_path(name).expect("inserted keyword");
            let parent = keyword_path
                .len()
                .checked_sub(2)
                .map(|i| keyword_path[i].as_str());
            self.index.add_keyword(name, parent)?;
            self.index
                .accept_keyword_names(
                    &self
                        .index
                        .images_with_person(key, false, i64::MAX as usize, 0)?,
                    &[name.to_owned()],
                )
                .map_err(|e| EngineError::invalid("keywords", e.to_string()))?;
            library.write(path)?;
        }
        Ok(())
    }

    fn export_people_edit(&self, key: &str, writes: PeopleWriteOptions) -> EngineResult<()> {
        let name = self
            .index
            .people()?
            .into_iter()
            .find(|p| p.id == key)
            .ok_or_else(|| EngineError::not_found("person", key))?
            .name;
        // Membership and files cannot be crash-atomic. Never hide an export
        // failure or imply that a catalog edit has been rolled back.
        (|| {
            cull::people::name_person(
                &self.index,
                self.app.join("library.json"),
                key,
                name.as_deref(),
                &cull::people::NamePersonOptions {
                    write_sidecars: writes.write_sidecars,
                    person_keywords: writes.person_keywords,
                    ..Default::default()
                },
            )?;
            self.person_keywords(key, writes)
        })()
        .map_err(|e| {
            EngineError::invalid(
                "people metadata",
                format!("catalog edit applied; metadata synchronization failed: {e}"),
            )
        })
    }

    /// Execute a library-scoped mutation, recording only successful calls.
    /// All references are checked before edits. These are not recipe history entries.
    pub fn run_library(&mut self, request: LibraryToolRequest) -> EngineResult<Value> {
        let output = self.edit_people(&request.call)?;
        if let Some(recorder) = &mut self.recorder {
            recorder.record_library(&request);
        }
        Ok(output)
    }

    fn edit_people(&mut self, call: &LibraryToolCall) -> EngineResult<Value> {
        match call {
            LibraryToolCall::AssignPerson {
                faces: refs,
                person_id,
                confirmed,
                writes,
            } => {
                let key = self.person_key(*person_id)?;
                let faces = faces(&self.index, refs, None)?;
                self.preflight_people_writes(&[&key], &faces, *writes)?;
                for face in faces {
                    self.index.assign_face(face, &key)?;
                    self.index.confirm_face(face, *confirmed)?;
                }
                self.export_people_edit(&key, *writes)?;
                Ok(json!({"person_id":person_id,"assigned":refs.len()}))
            }
            LibraryToolCall::NamePerson {
                person_id,
                name,
                writes,
            } => {
                let key = self.person_key(*person_id)?;
                cull::people::name_person(
                    &self.index,
                    self.app.join("library.json"),
                    &key,
                    name.as_deref(),
                    &cull::people::NamePersonOptions {
                        write_sidecars: writes.write_sidecars,
                        person_keywords: writes.person_keywords,
                        ..Default::default()
                    },
                )?;
                self.person_keywords(&key, *writes).map_err(|e| {
                    EngineError::invalid(
                        "people metadata",
                        format!("name applied; catalog keyword synchronization failed: {e}"),
                    )
                })?;
                Ok(json!({"person_id":person_id,"name":name}))
            }
            LibraryToolCall::ConfirmPerson {
                faces: refs,
                person_id,
                confirmed,
                writes,
            } => {
                let key = self.person_key(*person_id)?;
                let faces = faces(&self.index, refs, Some(&key))?;
                self.preflight_people_writes(&[&key], &faces, *writes)?;
                for face in faces {
                    self.index.confirm_face(face, *confirmed)?;
                }
                self.export_people_edit(&key, *writes)?;
                Ok(json!({"person_id":person_id,"confirmed":confirmed,"faces":refs.len()}))
            }
            LibraryToolCall::MergePeople {
                source,
                target,
                writes,
            } => {
                let source_key = self.person_key(*source)?;
                let target_key = self.person_key(*target)?;
                self.preflight_people_writes(&[&source_key, &target_key], &[], *writes)?;
                self.index.merge_people(&target_key, &source_key)?;
                // Reconcile the deleted source immediately, before any model
                // can recreate its deterministic opaque key.
                self.people()?;
                self.export_people_edit(&target_key, *writes)?;
                Ok(json!({"person_id":target,"merged":source}))
            }
            LibraryToolCall::SplitPerson {
                person_id,
                faces: refs,
                writes,
            } => {
                let source = self.person_key(*person_id)?;
                let faces = faces(&self.index, refs, Some(&source))?;
                self.preflight_people_writes(&[], &faces, *writes)?;
                let (next, key) = self.reserve_person()?;
                self.index.split_person(&source, &key, &faces)?;
                self.export_people_edit(&key, *writes)?;
                Ok(json!({"person_id":next,"source":person_id,"faces":refs.len()}))
            }
        }
    }
}
