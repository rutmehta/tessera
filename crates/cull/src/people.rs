//! People naming coordinated across the index and library document.
use engine_api::{EngineError, EngineResult, id::ImageId};
use index::Index;
use library::Library;
use sidecar::{FaceRegion, MarkPreset, Sidecar, XmpPacket};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

/// Export is disabled by default, including reads/probes of sidecar paths.
#[derive(Debug, Clone, Default)]
pub struct NamePersonOptions {
    pub write_sidecars: bool,
    /// Append names to dc:subject. Existing keywords are never removed.
    pub person_keywords: bool,
    /// Dimensions of the oriented analysis preview, NOT full-resolution RAW.
    /// Overrides `analysis_width` / `analysis_height` index scores per image.
    pub dimensions: HashMap<ImageId, (u32, u32)>,
}

/// Rename a stable cluster, synchronize library display names and optionally
/// replace all MWG Face entries in each affected image from indexed detections.
/// Unassigned/unnamed faces are exported with an empty name. No orientation
/// transform is applied. Callers must serialize edits to these documents.
/// Preflights every export before changing the name. Writes are atomic per file,
/// with compensation on ordinary failures (including the index name); this is
/// not crash-atomic across SQLite and files. Rollback failures are reported.
/// Existing foreign/non-face XML is preserved by the sidecar packet editor;
/// Face-entry extensions are replaced, and existing keywords are append-only.
pub fn name_person(
    index: &Index,
    library_path: impl AsRef<Path>,
    person_id: &str,
    name: Option<&str>,
    options: &NamePersonOptions,
) -> EngineResult<()> {
    let library_path = library_path.as_ref();
    let mut library = Library::read(library_path)?;
    let old_library = crate::persistence::optional_bytes(library_path)?;
    let old_name = index
        .people()?
        .into_iter()
        .find(|p| p.id == person_id)
        .ok_or_else(|| EngineError::not_found("person", person_id))?
        .name;
    let mut prepared = Vec::new();
    if options.write_sidecars {
        let mut destinations = HashSet::new();
        for id in index.images_with_person(person_id, false, i64::MAX as usize, 0)? {
            let (width, height) = if let Some(dimensions) = options.dimensions.get(&id) {
                *dimensions
            } else {
                let scores = index.scores(id)?;
                let dimension = |signal: &str| -> EngineResult<u32> {
                    let value = scores
                        .iter()
                        .find(|s| s.signal == signal)
                        .map(|s| s.value)
                        .filter(|v| {
                            v.is_finite()
                                && *v >= 1.
                                && *v <= f64::from(u32::MAX)
                                && v.fract() == 0.
                        })
                        .ok_or_else(|| {
                            EngineError::invalid(
                                "dimensions",
                                format!("missing or invalid {signal} for {id}"),
                            )
                        })?;
                    Ok(value as u32)
                };
                (dimension("analysis_width")?, dimension("analysis_height")?)
            };
            if width == 0 || height == 0 {
                return Err(EngineError::invalid("dimensions", "must be positive"));
            }
            let assignments = index.face_assignments(id)?;
            let faces = index
                .faces(id)?
                .into_iter()
                .map(|face| {
                    let assignment = assignments.iter().find(|a| a.face.ordinal == face.id);
                    let face_name = assignment
                        .and_then(|a| {
                            if a.person_id == person_id {
                                name
                            } else {
                                a.person_name.as_deref()
                            }
                        })
                        .unwrap_or_default();
                    let [x, y, w, h] = face.bbox.map(f64::from);
                    FaceRegion {
                        name: face_name.into(),
                        x: (x + w / 2.) / f64::from(width),
                        y: (y + h / 2.) / f64::from(height),
                        w: w / f64::from(width),
                        h: h / f64::from(height),
                    }
                })
                .collect::<Vec<_>>();
            let image = index.image_info(id)?.path;
            let mut path = Sidecar::paths(&image).xmp;
            let mut old = crate::persistence::optional_bytes(&path)?;
            if old.is_none() {
                let legacy = image.with_extension("xmp");
                if let Some(bytes) = crate::persistence::optional_bytes(&legacy)? {
                    let folder = image.parent().unwrap_or(Path::new("."));
                    for peer in index.search(&index::Query {
                        folder: Some(folder.to_string_lossy().into_owned()),
                        limit: i64::MAX as usize,
                        ..Default::default()
                    })? {
                        if peer != id
                            && index.image_info(peer)?.path.with_extension("xmp") == legacy
                        {
                            return Err(EngineError::invalid(
                                "sidecar",
                                "shared legacy destination collision",
                            ));
                        }
                    }
                    path = legacy;
                    old = Some(bytes);
                }
            }
            if !destinations.insert(path.clone()) {
                return Err(EngineError::invalid("sidecar", "destination collision"));
            }
            let packet = if old.is_some() {
                Sidecar::read_xmp(&path)?
            } else {
                XmpPacket::from_selection(
                    &index.selection(id)?.unwrap_or_default(),
                    &MarkPreset::default(),
                )
            };
            prepared.push((
                path,
                old,
                packet
                    .with_face_regions(&faces, options.person_keywords)?
                    .with_face_dimensions(width, height)?,
            ));
        }
    }
    index.name_person(person_id, name)?;
    let mut written = 0;
    let result = (|| {
        library.sync_people_names(index)?;
        for (path, _, packet) in &prepared {
            // A directory fsync can fail after the atomic rename. Include the
            // attempted destination in compensation even if write returns Err.
            written += 1;
            Sidecar::write_xmp(path, packet)?;
        }
        library.write(library_path)
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, old, _) in prepared[..written].iter().rev() {
            if let Err(e) = crate::persistence::restore(path, old) {
                failures.push(e.to_string());
            }
        }
        // Library::write may report a directory fsync error after its rename.
        if let Err(e) = crate::persistence::restore(library_path, &old_library) {
            failures.push(e.to_string());
        }
        if let Err(e) = index.name_person(person_id, old_name.as_deref()) {
            failures.push(e.to_string());
        }
        if !failures.is_empty() {
            return Err(EngineError::invalid(
                "people",
                format!("{error}; rollback failed: {}", failures.join(", ")),
            ));
        }
        return Err(error);
    }
    Ok(())
}
