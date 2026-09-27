//! Single-owner disk cache accounting. Readers never acquire the writer mutex.
//! Atomic rename publishes complete entries. Startup is the only reconciliation
//! walk, so a crash between rename and accounting cannot leak the byte budget.
use super::*;
use std::{
    collections::{BTreeSet, HashMap},
    io::Write,
    sync::mpsc::{Receiver, SyncSender, sync_channel},
    sync::{Arc, OnceLock, Weak},
};

const BATCH: usize = 64;

pub(super) struct Disk {
    writer: Mutex<Index>,
    touches: SyncSender<PathBuf>,
    // Cross-process ownership also prevents a second startup walk from deleting
    // the current owner's in-progress temporary file.
    _owner: fs::File,
    cap: u64,
}
struct Index {
    files: HashMap<PathBuf, (u64, u64)>, // size, monotonic access sequence
    order: BTreeSet<(u64, PathBuf)>,
    bytes: u64,
    sequence: u64,
    touches: Receiver<PathBuf>,
}
impl Index {
    fn record(&mut self, path: PathBuf, size: u64) {
        if let Some((old_size, stamp)) = self.files.remove(&path) {
            self.bytes -= old_size;
            self.order.remove(&(stamp, path.clone()));
        }
        self.sequence += 1;
        self.bytes += size;
        self.order.insert((self.sequence, path.clone()));
        self.files.insert(path, (size, self.sequence));
    }
    fn drain_touches(&mut self) {
        // Approximate LRU: bounded, lossy touches cannot grow with reader load.
        for _ in 0..BATCH {
            let Ok(path) = self.touches.try_recv() else {
                break;
            };
            if let Some(&(size, _)) = self.files.get(&path) {
                self.record(path, size);
            }
        }
    }
    fn evict_one(&mut self) -> Result<bool> {
        let Some((stamp, path)) = self.order.first().cloned() else {
            return Ok(false);
        };
        match fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        self.order.remove(&(stamp, path.clone()));
        if let Some((size, _)) = self.files.remove(&path) {
            self.bytes -= size;
        }
        // rmdir is constant work and fails harmlessly if siblings remain.
        let _ = fs::remove_dir(path.parent().unwrap());
        Ok(true)
    }
}
impl Disk {
    pub fn shared(root: &Path, cap: u64) -> Result<Arc<Self>> {
        static STORES: OnceLock<Mutex<HashMap<PathBuf, Weak<Disk>>>> = OnceLock::new();
        let root = fs::canonicalize(root)?;
        let mut stores = STORES
            .get_or_init(Mutex::default)
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        if let Some(store) = stores.get(&root).and_then(Weak::upgrade) {
            if store.cap != cap {
                return Err(std::io::Error::other("live preview cache has a different cap").into());
            }
            return Ok(store);
        }
        stores.retain(|_, store| store.strong_count() > 0);
        let store = Arc::new(Self::open(&root, cap)?);
        stores.insert(root, Arc::downgrade(&store));
        Ok(store)
    }
    pub fn open(root: &Path, cap: u64) -> Result<Self> {
        let owner = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(".preview.lock"))?;
        owner
            .try_lock()
            .map_err(|e| std::io::Error::other(format!("preview cache already in use: {e}")))?;
        let (tx, rx) = sync_channel(1024);
        let mut index = Index {
            files: HashMap::new(),
            order: BTreeSet::new(),
            bytes: 0,
            sequence: 0,
            touches: rx,
        };
        for dir in fs::read_dir(root)? {
            let dir = dir?;
            if !dir.file_type()?.is_dir() {
                continue;
            }
            for file in fs::read_dir(dir.path())? {
                let file = file?;
                if !file.file_type()?.is_file() {
                    continue;
                }
                let path = file.path();
                if path.extension().is_some_and(|s| s == "tmp") {
                    fs::remove_file(path)?;
                } else if path.extension().is_some_and(|s| s == "jpg") {
                    index.record(path, file.metadata()?.len());
                }
            }
            let _ = fs::remove_dir(dir.path());
        }
        // No readers exist yet. Restore the cap even if the process died mid-put
        // or the configured budget was reduced since the previous open.
        while index.bytes > cap && index.evict_one()? {}
        Ok(Self {
            writer: Mutex::new(index),
            touches: tx,
            _owner: owner,
            cap,
        })
    }
    pub fn get(&self, path: &Path) -> Option<Bytes> {
        // A racing eviction is a miss; a racing replacement is old OR new bytes,
        // never a partial file. Neither disk I/O nor maintenance locks readers.
        let bytes = fs::read(path).ok()?;
        let _ = self.touches.try_send(path.to_owned());
        Some(bytes)
    }
    pub fn put(&self, path: &Path, bytes: &[u8], cap: u64) -> Result<()> {
        self.put_inner(path, bytes, cap, true)
    }
    pub fn put_if_room(&self, path: &Path, bytes: &[u8], cap: u64) -> Result<()> {
        self.put_inner(path, bytes, cap, false)
    }
    fn put_inner(&self, path: &Path, bytes: &[u8], cap: u64, evict: bool) -> Result<()> {
        let size = bytes.len() as u64;
        if size > cap {
            return Ok(());
        }
        let mut index = loop {
            let mut index = self.writer.lock().unwrap_or_else(|e| e.into_inner());
            index.drain_touches();
            // Bound each maintenance batch, releasing even the writer-only
            // mutex between batches. Below-cap puts never enter eviction.
            for _ in 0..BATCH {
                let old_size = index.files.get(path).map_or(0, |v| v.0);
                if index.bytes - old_size <= cap - size {
                    break;
                }
                // Aliases are optional and must never evict the target image.
                if !evict {
                    return Ok(());
                }
                if !index.evict_one()? {
                    break;
                }
            }
            let old_size = index.files.get(path).map_or(0, |v| v.0);
            if index.bytes - old_size <= cap - size {
                break index;
            }
            drop(index);
            std::thread::yield_now();
        };
        fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("tmp");
        let result = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        result?;
        index.record(path.to_owned(), size);
        Ok(())
    }
    #[cfg(test)]
    pub fn accounted_bytes(&self) -> u64 {
        self.writer.lock().unwrap().bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cache_root_has_exactly_one_live_owner() {
        let dir = tempfile::tempdir().unwrap();
        let first = Disk::open(dir.path(), 100).unwrap();
        assert!(Disk::open(dir.path(), 100).is_err());
        drop(first);
        assert!(Disk::open(dir.path(), 100).is_ok());
    }

    #[test]
    fn multiple_store_handles_share_the_same_accounting() {
        let dir = tempfile::tempdir().unwrap();
        let first = Disk::shared(dir.path(), 100).unwrap();
        let second = Disk::shared(&dir.path().join("."), 100).unwrap();
        assert!(std::sync::Arc::ptr_eq(&first, &second));
        first
            .put(&dir.path().join("a/1.jpg"), &[1; 60], 100)
            .unwrap();
        second
            .put(&dir.path().join("b/1.jpg"), &[2; 60], 100)
            .unwrap();
        assert_eq!(first.accounted_bytes(), 60);
        assert!(Disk::shared(dir.path(), 20).is_err());
    }

    #[test]
    fn accounting_overwrite_eviction_and_missing_victim() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Disk::open(dir.path(), 100).unwrap();
        let a = dir.path().join("a/1.jpg");
        let b = dir.path().join("b/1.jpg");
        disk.put(&a, &[1; 40], 100).unwrap();
        disk.put(&a, &[2; 20], 100).unwrap();
        assert_eq!(disk.accounted_bytes(), 20);
        disk.put(&b, &[3; 50], 100).unwrap();
        assert_eq!(disk.accounted_bytes(), 70);
        fs::remove_file(&a).unwrap();
        disk.put(&b, &[4; 90], 100).unwrap();
        assert_eq!(disk.accounted_bytes(), 90);
        assert_eq!(disk.get(&b).unwrap(), [4; 90]);
        assert!(!a.parent().unwrap().exists());
        drop(disk);
        assert_eq!(Disk::open(dir.path(), 100).unwrap().accounted_bytes(), 90);
    }

    #[test]
    fn maintenance_never_locks_readers_even_with_full_touch_queue() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Disk::open(dir.path(), 100).unwrap();
        let a = dir.path().join("a/1.jpg");
        disk.put(&a, &[1; 40], 100).unwrap();
        let held = disk.writer.lock().unwrap();
        // Holding the actual maintenance mutex while reading would deadlock
        // with the old shared reader/writer lock. Also overflow the touch queue.
        for _ in 0..2048 {
            assert_eq!(disk.get(&a).unwrap(), [1; 40]);
        }
        drop(held);
    }

    #[test]
    fn bounded_admission_and_recovery_account_every_file() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Disk::open(dir.path(), 128).unwrap();
        for i in 0..128 {
            disk.put(&dir.path().join(format!("{i}/1.jpg")), &[1], 128)
                .unwrap();
        }
        let big = dir.path().join("big/1.jpg");
        disk.put(&big, &[2; 128], 128).unwrap();
        assert_eq!(disk.accounted_bytes(), 128);
        assert_eq!(disk.get(&big).unwrap(), [2; 128]);
    }

    #[test]
    fn concurrent_replacements_publish_complete_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let disk = Disk::open(dir.path(), 100_000).unwrap();
        let path = dir.path().join("a/1.jpg");
        disk.put(&path, &[0; 4096], 100_000).unwrap();
        std::thread::scope(|scope| {
            for value in 1..=4 {
                let disk = &disk;
                let path = &path;
                scope.spawn(move || {
                    for _ in 0..20 {
                        disk.put(path, &[value; 4096], 100_000).unwrap();
                    }
                });
            }
            for _ in 0..1000 {
                let bytes = disk.get(&path).unwrap();
                assert_eq!(bytes.len(), 4096);
                assert!(bytes.iter().all(|v| *v == bytes[0]));
            }
        });
        assert_eq!(disk.accounted_bytes(), 4096);
    }
}
