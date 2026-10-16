//! Folder scans off the catalog lock (ST-4). The index crate's scan runs in
//! phases (see `index::ScanScope`): the walk, the stats, the EXIF and the
//! sidecar reads happen here with the catalog free, and each batch of
//! `index::SCAN_BATCH` files is checked and written under the lock in one
//! short hold, so `roots`, `list_images` and the UI's other lookups answer
//! between batches instead of queueing behind the whole folder. Each
//! committed batch is announced (`ScanProgress`, `LibraryChanged`), so the
//! grid fills as photos land rather than when the folder is done.
use crate::{Engine, EngineEvent, Result, catalog};
use std::path::Path;

impl Engine {
    /// Scans `root` in batches; returns how many originals were written.
    /// Legacy ids met on the way are re-keyed in the catalog and left in
    /// `Index::take_rekeyed` for the caller.
    pub(crate) fn scan_folder(&self, root: &Path) -> Result<usize> {
        let scope = self.lock()?.index.begin_scan(root)?;
        let mut walk = scope.walk(&catalog::Sidecars);
        let mut updated = 0;
        loop {
            let entries = walk.next_batch(index::SCAN_BATCH)?;
            if entries.is_empty() {
                break;
            }
            let pending = self.lock()?.index.scan_check(&scope, entries)?;
            let records = pending
                .into_iter()
                .map(|p| p.read(&catalog::Sidecars, &catalog::IndexedMetadata))
                .collect::<engine_api::EngineResult<Vec<_>>>()?;
            #[cfg(test)]
            self.delay_scan_read();
            let written = self.lock()?.index.scan_apply(&scope, &records)?;
            if written > 0 {
                updated += written;
                // The batch is committed: the UI can list it now, and the
                // change feed (M2-28) carries it to open sessions.
                self.emit(EngineEvent::ScanProgress {
                    path: root.to_string_lossy().into_owned(),
                    updated: updated as u64,
                    finished: false,
                });
                self.notify_changes();
            }
        }
        self.lock()?.index.finish_scan(updated)?;
        Ok(updated)
    }

    /// Tests stretch the read phase to watch the catalog while a scan runs.
    #[cfg(test)]
    fn delay_scan_read(&self) {
        let ms = self
            .scan_read_delay_ms
            .load(std::sync::atomic::Ordering::Relaxed);
        if ms > 0 {
            std::thread::sleep(std::time::Duration::from_millis(ms));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{BridgeError, Engine, EngineEvent, EngineEventListener, ImageQuery};
    use std::{
        path::PathBuf,
        sync::{
            Arc, Mutex,
            atomic::{AtomicBool, Ordering},
        },
        time::{Duration, Instant},
    };

    pub(crate) const COUNT: usize = 200;

    /// A folder of `COUNT` small JPEGs under `dir`.
    pub(crate) fn shoot(dir: &std::path::Path, name: &str) -> PathBuf {
        let photos = dir.join(name);
        std::fs::create_dir_all(photos.join("nested")).unwrap();
        for n in 0..COUNT {
            let folder = if n % 4 == 0 { "nested" } else { "" };
            image::RgbImage::from_pixel(24, 16, image::Rgb([n as u8, 80, 160]))
                .save(photos.join(folder).join(format!("p{n:03}.jpg")))
                .unwrap();
        }
        photos
    }

    /// What a UI thread saw while a scan ran: how long each lookup took,
    /// whether `roots` ever got through, and the image counts it listed.
    #[derive(Default)]
    struct Observed {
        longest: Duration,
        roots_ok: usize,
        roots_busy: usize,
        counts: Vec<usize>,
    }

    fn observe(engine: &Arc<Engine>, done: &AtomicBool) -> Observed {
        let mut seen = Observed::default();
        while !done.load(Ordering::Acquire) {
            let started = Instant::now();
            match engine.roots() {
                Ok(_) => seen.roots_ok += 1,
                Err(BridgeError::Busy { .. }) => seen.roots_busy += 1,
                Err(e) => panic!("{e}"),
            }
            seen.longest = seen.longest.max(started.elapsed());
            let started = Instant::now();
            let listed = engine.list_images(ImageQuery::default()).unwrap().len();
            seen.longest = seen.longest.max(started.elapsed());
            seen.counts.push(listed);
            std::thread::sleep(Duration::from_millis(5));
        }
        seen
    }

    /// ST-4: a scan of 200 files never holds the catalog longer than the
    /// tracing threshold, and lookups from another thread answer meanwhile.
    #[test]
    fn scan_holds_the_catalog_briefly_and_answers_lookups_meanwhile() {
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        // The read phase of each batch takes a while, as EXIF on a slow
        // volume does; the lock must be free for it.
        engine.scan_read_delay_ms.store(80, Ordering::Relaxed);
        // A loaded machine (the whole suite in parallel) can stretch any
        // hold; the best of three scans is the scanner's own behaviour.
        let mut best = Duration::MAX;
        let mut folders = 0;
        for attempt in 0..3 {
            let photos = shoot(dir.path(), &format!("photos{attempt}"));
            folders += 1;
            engine.catalog.stats().reset();
            let done = AtomicBool::new(false);
            let seen = std::thread::scope(|s| {
                let watcher = s.spawn(|| observe(&engine, &done));
                let folder = engine
                    .index_folder(photos.to_string_lossy().into_owned())
                    .unwrap();
                done.store(true, Ordering::Release);
                assert_eq!(folder.updated, COUNT as u64);
                watcher.join().unwrap()
            });
            let hold = engine.catalog.stats().max_hold();
            eprintln!(
                "scan: longest catalog hold {hold:?}, wait {:?}; lookups answered in at most {:?} ({} roots ok, {} busy, {} listings)",
                engine.catalog.stats().max_wait(),
                seen.longest,
                seen.roots_ok,
                seen.roots_busy,
                seen.counts.len()
            );
            // Four batches of reads at 80 ms each: the scan ran well over
            // 300 ms, and no lookup waited for it.
            assert!(seen.roots_ok > 0, "roots never got through");
            assert!(
                seen.longest < Duration::from_millis(200),
                "a lookup waited {:?} on the scan",
                seen.longest
            );
            best = best.min(hold);
            if best < crate::lock::WARN {
                break;
            }
        }
        assert!(
            best < crate::lock::WARN,
            "catalog lock held {best:?} during a scan"
        );
        assert_eq!(
            engine.list_images(ImageQuery::default()).unwrap().len(),
            COUNT * folders
        );
    }

    /// What the scan announced, in order: `Some(updated)` for a scan
    /// progress event (`None` once finished), or a library change.
    #[derive(Default)]
    struct Announced(Mutex<Vec<Announcement>>);
    #[derive(Debug, PartialEq, Eq)]
    enum Announcement {
        Progress(Option<u64>),
        Changed(u64),
    }
    impl EngineEventListener for Announced {
        fn on_event(&self, event: EngineEvent) {
            let item = match event {
                EngineEvent::ScanProgress {
                    updated, finished, ..
                } => Announcement::Progress((!finished).then_some(updated)),
                EngineEvent::LibraryChanged { sequence } => Announcement::Changed(sequence),
                _ => return,
            };
            self.0.lock().unwrap().push(item);
        }
    }

    /// ST-4: photos land in the UI batch by batch. Every committed batch is
    /// announced with the running count and a library change, and a listing
    /// from another thread sees the batches committed so far.
    #[test]
    fn scan_progress_lands_per_batch() {
        let dir = tempfile::tempdir().unwrap();
        let photos = shoot(dir.path(), "photos");
        let engine = Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let announced = Arc::new(Announced::default());
        engine.set_event_listener(Some(announced.clone()));
        engine.scan_read_delay_ms.store(40, Ordering::Relaxed);
        let done = AtomicBool::new(false);
        let seen = std::thread::scope(|s| {
            let watcher = s.spawn(|| observe(&engine, &done));
            engine
                .index_folder(photos.to_string_lossy().into_owned())
                .unwrap();
            done.store(true, Ordering::Release);
            watcher.join().unwrap()
        });
        let events = announced.0.lock().unwrap();
        // Four batches (64, 64, 64, 8), each announced as it committed, then
        // the finished event; the library changed after every batch.
        let progress: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Announcement::Progress(p) => Some(*p),
                Announcement::Changed(_) => None,
            })
            .collect();
        assert_eq!(
            progress,
            [Some(0), Some(64), Some(128), Some(192), Some(200), None]
        );
        let changes: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                Announcement::Changed(seq) => Some(*seq),
                Announcement::Progress(_) => None,
            })
            .collect();
        assert!(changes.len() >= 4, "{events:?}");
        assert!(changes.windows(2).all(|w| w[0] < w[1]), "{changes:?}");
        let last_progress = events
            .iter()
            .rposition(|e| matches!(e, Announcement::Progress(Some(_))))
            .unwrap();
        assert!(
            events[..last_progress]
                .iter()
                .filter(|e| matches!(e, Announcement::Changed(_)))
                .count()
                >= 3,
            "changes were announced only at the end: {events:?}"
        );
        // Listings mid-scan saw whole batches, never a partial one.
        let partial: Vec<_> = seen
            .counts
            .iter()
            .copied()
            .filter(|n| *n > 0 && *n < COUNT)
            .collect();
        assert!(
            !partial.is_empty(),
            "no listing ran mid-scan: {:?}",
            seen.counts
        );
        assert!(
            partial.iter().all(|n| n % 64 == 0),
            "a listing saw a batch half-written: {partial:?}"
        );
    }
}
