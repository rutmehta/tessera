//! Drive-resident state writer (CS-1, docs/05 §2.3). Catalog writes stamp
//! their image dirty (index migration 011); this thread wakes on every
//! `notify_changes`, waits a second for the burst to settle, and writes each
//! dirty image's `.tessera/analysis/<id>.json` and, when people changed, each
//! attached root's `.tessera/people.json`, skipping files whose bytes did not
//! change. Hosts call `flush_drive_state` before quitting.
use crate::{Engine, Result, failure};
use engine_api::id::ImageId;
use index::TesseraDir;
use std::{
    path::{Path, PathBuf},
    sync::{Condvar, Mutex, Weak},
    time::Duration,
};

/// How long a burst of writes is allowed to settle before the drive is written.
const DEBOUNCE: Duration = Duration::from_secs(1);
/// Idle writers still look for work this often (writes by other connections).
const IDLE: Duration = Duration::from_secs(10);

#[derive(Default)]
pub(crate) struct DriveWriter {
    poked: Mutex<bool>,
    wake: Condvar,
}

impl DriveWriter {
    /// Something changed: write after the debounce.
    pub(crate) fn poke(&self) {
        *self.poked.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.wake.notify_all();
    }
}

/// A stable per-machine id for last-writer-wins stamps, kept in app support.
pub(crate) fn machine_id(support: &Path) -> Result<String> {
    let path = support.join("machine-id");
    match std::fs::read_to_string(&path) {
        Ok(id) if !id.trim().is_empty() => return Ok(id.trim().to_owned()),
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(failure(e)),
    }
    let mut hasher = blake3::Hasher::new();
    hasher.update(
        std::env::var_os("HOSTNAME")
            .unwrap_or_default()
            .as_encoded_bytes(),
    );
    hasher.update(&std::process::id().to_le_bytes());
    hasher.update(&index::now_ms().to_le_bytes());
    hasher.update(&(&path as *const PathBuf as usize).to_le_bytes());
    let id = hasher.finalize().to_hex()[..16].to_owned();
    index::atomic_write(&path, id.as_bytes())?;
    Ok(id)
}

impl Engine {
    pub(crate) fn poke_drive(&self) {
        self.drive.poke();
    }

    /// Starts the writer thread once per engine (weak reference: it ends with
    /// the engine).
    pub(crate) fn start_drive_writer(&self) {
        let weak: Weak<Self> = self.this.clone();
        let spawned = std::thread::Builder::new()
            .name("tessera-drive-writer".into())
            .spawn(move || {
                loop {
                    let Some(engine) = weak.upgrade() else { return };
                    let poked = {
                        let guard = engine.drive.poked.lock().unwrap_or_else(|e| e.into_inner());
                        let (mut guard, _) = engine
                            .drive
                            .wake
                            .wait_timeout_while(guard, IDLE, |poked| !*poked)
                            .unwrap_or_else(|e| e.into_inner());
                        std::mem::take(&mut *guard)
                    };
                    drop(engine);
                    if poked {
                        std::thread::sleep(DEBOUNCE);
                    }
                    let Some(engine) = weak.upgrade() else { return };
                    if let Err(e) = engine.flush_drive() {
                        eprintln!("drive state: {e}");
                    }
                }
            });
        if let Err(e) = spawned {
            eprintln!("drive state: writer not started: {e}");
        }
    }

    /// Writes every dirty record whose root is attached. Returns the number of
    /// files written. Records are read under the catalog lock and written
    /// outside it; a record is marked clean only if nothing stamped it since.
    pub(crate) fn flush_drive(&self) -> Result<u32> {
        struct Pending {
            path: PathBuf,
            bytes: Vec<u8>,
            image: Option<(ImageId, i64)>,
        }
        let (pending, people_rev) = {
            let c = self.lock()?;
            let roots: Vec<_> = c
                .index
                .roots()?
                .into_iter()
                .filter(|r| r.path.is_dir())
                .collect();
            let root_of = |path: &Path| {
                roots
                    .iter()
                    .filter(|r| path.starts_with(&r.path))
                    .max_by_key(|r| r.path.as_os_str().len())
                    .map(|r| r.path.clone())
            };
            let mut pending = Vec::new();
            for id in c.index.drive_dirty_images()? {
                let Ok(info) = c.index.image_info(id) else {
                    continue;
                };
                // A root that is not attached keeps its records dirty for later.
                let Some(root) = root_of(&info.path) else {
                    continue;
                };
                let record = c.index.export_analysis(&root, id)?;
                pending.push(Pending {
                    path: TesseraDir::of_root(&root).analysis(id),
                    bytes: serde_json::to_vec_pretty(&record).map_err(failure)?,
                    image: Some((id, record.modified)),
                });
            }
            let people_rev = c.index.people_rev()?;
            if people_rev.0 != people_rev.1 {
                for root in &roots {
                    let document = c.index.export_people(&root.path)?;
                    pending.push(Pending {
                        path: TesseraDir::of_root(&root.path).people(),
                        bytes: serde_json::to_vec_pretty(&document).map_err(failure)?,
                        image: None,
                    });
                }
            }
            (pending, people_rev)
        };
        let mut written = 0;
        let mut clean = Vec::new();
        let mut people_written = true;
        for item in &pending {
            match index::write_if_changed(&item.path, &item.bytes) {
                Ok(changed) => {
                    written += u32::from(changed);
                    if let Some(image) = item.image {
                        clean.push(image);
                    }
                }
                Err(e) => {
                    eprintln!("drive state: {}: {e}", item.path.display());
                    if item.image.is_none() {
                        people_written = false;
                    }
                }
            }
        }
        let c = self.lock()?;
        c.index.mark_drive_clean(&clean)?;
        if people_written && people_rev.0 != people_rev.1 {
            c.index.mark_people_flushed(people_rev.0)?;
        }
        Ok(written)
    }
}

#[uniffi::export]
impl Engine {
    /// Writes pending drive-resident state now (people, analysis). Hosts call
    /// it when a folder closes or the app quits; the writer otherwise runs a
    /// second after the last change. Returns the number of files written.
    pub fn flush_drive_state(&self) -> Result<u32> {
        self.flush_drive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AnalysisOptions, Decision, FaceInput, ImageQuery, PeopleNameOptions, Selection};

    fn photos(dir: &Path) -> PathBuf {
        let photos = dir.join("shoot");
        std::fs::create_dir_all(&photos).unwrap();
        for (n, name) in ["a.jpg", "b.jpg", "c.jpg"].iter().enumerate() {
            image::RgbImage::from_pixel(64, 48, image::Rgb([40 + n as u8 * 60, 90, 120]))
                .save(photos.join(name))
                .unwrap();
        }
        photos
    }

    /// CS-1 round trip: one app dir writes `.tessera/`; a fresh app dir opens
    /// the same folder and sees the same people, decisions, albums and scores.
    #[test]
    fn drive_state_round_trips_between_app_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let photos = photos(&dir);
        let first = Engine::open(dir.join("one").to_string_lossy().into()).unwrap();
        assert_eq!(
            machine_id(&dir.join("one")).unwrap(),
            machine_id(&dir.join("one")).unwrap()
        );
        let folder = first
            .index_folder(photos.to_string_lossy().into())
            .unwrap()
            .path;
        let ids: Vec<String> = first
            .list_images(ImageQuery::default())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        // A decision (sidecar), an album (library.json), scores and a named person.
        first
            .set_selection(
                ids[0].clone(),
                Selection {
                    decision: Decision::Keep,
                    grade: Some(3),
                    mark: None,
                },
            )
            .unwrap();
        let store = first
            .clone()
            .open_library(library::Library::path_in(&photos).to_string_lossy().into())
            .unwrap();
        let album = store.create_album("Picks".into(), None).unwrap();
        store.add_to_album(album, vec![ids[1].clone()]).unwrap();
        first
            .analyze_image(
                ids[0].clone(),
                AnalysisOptions {
                    quality: true,
                    faces: false,
                    force: false,
                },
            )
            .unwrap();
        let mut embedding = vec![0.; 128];
        embedding[3] = 1.;
        for id in &ids[..2] {
            first
                .set_faces(
                    id.clone(),
                    vec![FaceInput {
                        x: 4.,
                        y: 4.,
                        width: 20.,
                        height: 20.,
                        focus: 0.8,
                        eyes_open: Some(0.9),
                        embedding: Some(embedding.clone()),
                    }],
                    64,
                    48,
                )
                .unwrap();
        }
        let session = first.open_cull_session(folder.clone()).unwrap();
        // Name every cluster the job made (the synthetic faces may cluster apart).
        let mut named: Vec<(String, String)> = session
            .people(true)
            .unwrap()
            .into_iter()
            .enumerate()
            .map(|(n, p)| (p.id, format!("Ada {n}")))
            .collect();
        named.sort();
        for (id, name) in &named {
            session
                .name_person(id.clone(), Some(name.clone()), PeopleNameOptions::default())
                .unwrap();
        }
        let person = named[0].0.clone();
        session.shutdown().unwrap();
        assert!(
            first.flush_drive_state().unwrap() >= 3,
            "analysis records and people.json"
        );
        assert_eq!(
            first.flush_drive_state().unwrap(),
            0,
            "unchanged records are not rewritten"
        );
        let tessera = TesseraDir::of_root(&photos);
        assert!(tessera.people().exists());
        assert!(tessera.analysis(crate::parse_id(&ids[0]).unwrap()).exists());
        assert!(library::Library::path_in(&photos).exists());
        drop(session);
        drop(first);

        // Another Mac: a fresh app dir, the same folder.
        let second = Engine::open(dir.join("two").to_string_lossy().into()).unwrap();
        let folder = second
            .index_folder(photos.to_string_lossy().into())
            .unwrap()
            .path;
        let rows = second.list_images(ImageQuery::default()).unwrap();
        assert_eq!(rows.len(), 3);
        let row = rows.iter().find(|r| r.id == ids[0]).unwrap();
        assert_eq!(row.selection.decision, Decision::Keep);
        assert_eq!(row.selection.grade, Some(3));
        let scores = second
            .lock()
            .unwrap()
            .index
            .scores(crate::parse_id(&ids[0]).unwrap())
            .unwrap();
        assert!(scores.iter().any(|s| s.signal == "quality"), "{scores:?}");
        let session = second.open_cull_session(folder).unwrap();
        let mut people: Vec<(String, String, u32)> = session
            .people(false)
            .unwrap()
            .into_iter()
            .map(|p| (p.id, p.name, p.faces))
            .collect();
        people.sort();
        assert_eq!(
            people
                .iter()
                .map(|(id, name, _)| (id.clone(), name.clone()))
                .collect::<Vec<_>>(),
            named
        );
        assert_eq!(people.iter().map(|(_, _, faces)| faces).sum::<u32>(), 2);
        assert_eq!(session.albums().unwrap()[0].name, "Picks");
        assert_eq!(session.albums().unwrap()[0].images, vec![ids[1].clone()]);
        // Nothing imported is dirty on the second machine; its own edits are.
        assert_eq!(second.flush_drive_state().unwrap(), 0);
        session
            .name_person(
                person.clone(),
                Some("Ada Lovelace".into()),
                PeopleNameOptions::default(),
            )
            .unwrap();
        assert_eq!(second.flush_drive_state().unwrap(), 1);
        session.shutdown().unwrap();
        drop(session);
        drop(second);
        // Back on the first Mac the rename wins (newer record).
        let first = Engine::open(dir.join("one").to_string_lossy().into()).unwrap();
        let folder = first
            .index_folder(photos.to_string_lossy().into())
            .unwrap()
            .path;
        let session = first.open_cull_session(folder).unwrap();
        let renamed = session
            .people(false)
            .unwrap()
            .into_iter()
            .find(|p| p.id == person)
            .unwrap();
        assert_eq!(renamed.name, "Ada Lovelace");
        session.shutdown().unwrap();
    }
}
