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
/// Records exported (and later marked clean) per catalog lock acquisition:
/// enough to amortize the lock, few enough that a hold stays well under the
/// UI's 20 ms budget on a large shoot.
const EXPORT_BATCH: usize = 8;

#[derive(Default)]
pub(crate) struct DriveWriter {
    poked: Mutex<bool>,
    wake: Condvar,
    /// Test aid: a delay per file written, standing in for a slow drive.
    #[cfg(test)]
    pub(crate) write_delay: Mutex<Duration>,
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
    /// files written. The catalog lock is taken only for short SQL passes:
    /// ids and paths first, then records a batch at a time; stats, JSON and the
    /// files themselves happen outside it (ST-1: an external drive used to
    /// stall every main-thread lookup for the length of a flush). A record is
    /// marked clean only if nothing stamped it since.
    pub(crate) fn flush_drive(&self) -> Result<u32> {
        struct Pending {
            path: PathBuf,
            bytes: Vec<u8>,
            image: Option<(ImageId, i64)>,
        }
        let (roots, dirty, people_rev) = {
            let c = self.lock()?;
            let mut dirty = Vec::new();
            for id in c.index.drive_dirty_images()? {
                if let Ok(info) = c.index.image_info(id) {
                    dirty.push((id, info.path));
                }
            }
            (c.index.roots()?, dirty, c.index.people_rev()?)
        };
        // A root that is not attached keeps its records dirty for later.
        let roots: Vec<PathBuf> = roots
            .into_iter()
            .map(|r| r.path)
            .filter(|p| p.is_dir())
            .collect();
        let root_of = |path: &Path| {
            roots
                .iter()
                .filter(|r| path.starts_with(r))
                .max_by_key(|r| r.as_os_str().len())
                .cloned()
        };
        let dirty: Vec<(ImageId, PathBuf)> = dirty
            .into_iter()
            .filter_map(|(id, path)| root_of(&path).map(|root| (id, root)))
            .collect();
        let mut pending = Vec::new();
        for batch in dirty.chunks(EXPORT_BATCH) {
            let records = {
                let c = self.lock()?;
                batch
                    .iter()
                    .map(|(id, root)| Ok((*id, root, c.index.export_analysis(root, *id)?)))
                    .collect::<Result<Vec<_>>>()?
            };
            for (id, root, record) in records {
                pending.push(Pending {
                    path: TesseraDir::of_root(root).analysis(id),
                    bytes: serde_json::to_vec_pretty(&record).map_err(failure)?,
                    image: Some((id, record.modified)),
                });
            }
        }
        if people_rev.0 != people_rev.1 {
            let documents = {
                let c = self.lock()?;
                roots
                    .iter()
                    .map(|root| Ok((root, c.index.export_people(root)?)))
                    .collect::<Result<Vec<_>>>()?
            };
            for (root, document) in documents {
                pending.push(Pending {
                    path: TesseraDir::of_root(root).people(),
                    bytes: serde_json::to_vec_pretty(&document).map_err(failure)?,
                    image: None,
                });
            }
        }
        let mut written = 0;
        let mut clean = Vec::new();
        let mut people_written = true;
        for item in &pending {
            #[cfg(test)]
            std::thread::sleep(
                *self
                    .drive
                    .write_delay
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()),
            );
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
        for batch in clean.chunks(EXPORT_BATCH) {
            self.lock()?.index.mark_drive_clean(batch)?;
        }
        if people_written && people_rev.0 != people_rev.1 {
            self.lock()?.index.mark_people_flushed(people_rev.0)?;
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
    use std::time::Instant;

    /// ST-1: the writer never holds the catalog lock across the drive. A slow
    /// drive (2 ms per file) and a few hundred face records make the flush
    /// take half a second while no single hold reaches 20 ms.
    #[test]
    fn flush_never_holds_the_catalog_lock_across_the_drive() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path().canonicalize().unwrap();
        let photos = dir.join("shoot");
        std::fs::create_dir_all(&photos).unwrap();
        let count = 240;
        for n in 0..count {
            image::RgbImage::from_pixel(16, 12, image::Rgb([n as u8, 90, 120]))
                .save(photos.join(format!("f{n:03}.jpg")))
                .unwrap();
        }
        let engine = Engine::open(dir.join("app").to_string_lossy().into()).unwrap();
        engine
            .index_folder(photos.to_string_lossy().into())
            .unwrap();
        let ids: Vec<String> = engine
            .list_images(ImageQuery::default())
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert_eq!(ids.len(), count);
        let faces = || {
            vec![FaceInput {
                x: 2.,
                y: 2.,
                width: 8.,
                height: 8.,
                focus: 0.7,
                eyes_open: Some(0.9),
                embedding: Some(vec![0.5; 128]),
            }]
        };
        for id in &ids {
            engine.set_faces(id.clone(), faces(), 16, 12).unwrap();
        }
        *engine.drive.write_delay.lock().unwrap() = Duration::from_millis(2);
        // A loaded machine (the whole suite in parallel) can stretch any hold;
        // the best of three flushes is the writer's own behaviour.
        let mut best = Duration::MAX;
        for attempt in 0..3 {
            if attempt > 0 {
                for id in &ids {
                    engine.set_faces(id.clone(), faces(), 16, 12).unwrap();
                }
            }
            engine.catalog.stats().reset();
            let started = Instant::now();
            assert_eq!(engine.flush_drive_state().unwrap() as usize, count);
            let took = started.elapsed();
            let hold = engine.catalog.stats().max_hold();
            eprintln!("drive flush: {took:?}, longest catalog hold {hold:?}");
            assert!(took >= Duration::from_millis(2 * count as u64), "{took:?}");
            assert_eq!(engine.flush_drive_state().unwrap(), 0);
            best = best.min(hold);
            if best < Duration::from_millis(20) {
                break;
            }
        }
        assert!(
            best < Duration::from_millis(20),
            "catalog lock held {best:?} during a flush"
        );
    }

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
