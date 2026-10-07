//! Lazy pixel work. Opening/regrouping only queues metadata; polling after open
//! starts one worker. At most 16 requests/results are retained, and a process-wide
//! gate permits one hash decode at a time even while a previous session retires.
use crate::{ImageId, PreviewProvider};
use engine_api::{EngineError, EngineResult};
use index::ImageInfo;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const WINDOW: usize = 16;
static PIXEL_GATE: Mutex<()> = Mutex::new(());
type Ticket = (ImageInfo, u64, bool);
type Reply = (ImageId, u64, EngineResult<Option<u64>>);
type ReadyHash = (ImageId, EngineResult<Option<u64>>);

#[derive(Default)]
pub(crate) struct BackgroundPreviews {
    queue: VecDeque<(ImageInfo, u64)>,
    versions: HashMap<ImageId, u64>,
    serial: u64,
    cancel: Arc<AtomicBool>,
    sender: Option<mpsc::SyncSender<Ticket>>,
    receiver: Option<mpsc::Receiver<Reply>>,
    in_flight: usize,
}
impl Drop for BackgroundPreviews {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Release);
    }
}
impl BackgroundPreviews {
    pub fn enqueue(&mut self, info: ImageInfo) {
        self.serial += 1;
        self.versions.insert(info.id, self.serial);
        self.queue.push_back((info, self.serial));
    }
    pub fn remove(&mut self, id: ImageId) {
        self.versions.remove(&id);
    }
    pub fn pending(&self) -> bool {
        !self.queue.is_empty() || self.in_flight != 0
    }
    pub fn poll(
        &mut self,
        provider: &PreviewProvider,
        notify: &Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> EngineResult<Vec<ReadyHash>> {
        if self.sender.is_none() && !self.queue.is_empty() {
            let (tx, rx) = mpsc::sync_channel::<Ticket>(WINDOW);
            let (done, results) = mpsc::sync_channel(WINDOW);
            let cancel = self.cancel.clone();
            let provider = provider.clone();
            let notify = notify.clone();
            thread::Builder::new()
                .name("cull-hashes".into())
                .spawn(move || {
                    let mut last_notice = std::time::Instant::now();
                    while let Ok((info, version, last)) = rx.recv() {
                        // Waiting for the global CPU budget must itself be cancellable.
                        let guard = loop {
                            if cancel.load(Ordering::Acquire) {
                                return;
                            }
                            match PIXEL_GATE.try_lock() {
                                Ok(guard) => break guard,
                                Err(std::sync::TryLockError::Poisoned(e)) => break e.into_inner(),
                                Err(std::sync::TryLockError::WouldBlock) => {
                                    thread::sleep(Duration::from_millis(5))
                                }
                            }
                        };
                        if cancel.load(Ordering::Acquire) {
                            return;
                        }
                        let hash = provider(&info);
                        drop(guard);
                        if cancel.load(Ordering::Acquire)
                            || done.send((info.id, version, hash)).is_err()
                        {
                            return;
                        }
                        if last || last_notice.elapsed() >= Duration::from_millis(200) {
                            if let Some(notify) = &notify {
                                notify();
                            }
                            last_notice = std::time::Instant::now();
                        }
                    }
                })
                .map_err(EngineError::from)?;
            self.sender = Some(tx);
            self.receiver = Some(results);
        }
        let mut ready = Vec::new();
        if let Some(receiver) = &self.receiver {
            for _ in 0..WINDOW {
                match receiver.try_recv() {
                    Ok((id, version, hash)) => {
                        self.in_flight -= 1;
                        if self.versions.get(&id) == Some(&version) {
                            ready.push((id, hash));
                        }
                    }
                    Err(mpsc::TryRecvError::Empty) => break,
                    Err(mpsc::TryRecvError::Disconnected) => {
                        return Err(EngineError::invalid("cull previews", "worker stopped"));
                    }
                }
            }
        }
        let mut batch = Vec::new();
        while batch.len() + self.in_flight < WINDOW {
            let Some((info, version)) = self.queue.pop_front() else {
                break;
            };
            if self.versions.get(&info.id) == Some(&version) {
                batch.push((info, version));
            }
        }
        let count = batch.len();
        for (n, (info, version)) in batch.into_iter().enumerate() {
            self.sender
                .as_ref()
                .expect("worker started")
                .send((info, version, n + 1 == count))
                .map_err(|_| EngineError::invalid("cull previews", "worker stopped"))?;
            self.in_flight += 1;
        }
        Ok(ready)
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    fn info() -> (tempfile::TempDir, ImageInfo) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("synthetic.dng"), b"synthetic").unwrap();
        let mut index = index::Index::open(dir.path().join("index.sqlite")).unwrap();
        index.scan(dir.path(), &index::NoopSidecarReader, &index::NoopMetadataProvider).unwrap();
        let id = index.search(&Default::default()).unwrap()[0];
        (dir, index.image_info(id).unwrap())
    }

    #[test]
    fn lr13d_refresh_metadata_is_coalesced_by_image() {
        let (_dir, image) = info();
        let mut work = BackgroundPreviews::default();
        for _ in 0..10_000 { work.enqueue(image.clone()); }
        assert_eq!(work.queue.len(), 1, "metadata grows with distinct images, not refresh count");
        work.remove(image.id);
        assert_eq!(work.queue.len(), 0, "removal releases queued metadata");
    }
}
