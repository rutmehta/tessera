//! Lazy pixel work. Opening/regrouping only queues metadata: O(distinct images),
//! coalesced by image ID. At most 16 hash tickets/results and one wake command
//! are outstanding per session. A process-wide gate permits one provider call.
//! Decoder tiles and codec allocations are additional to the reduced output.
use crate::{ImageId, PreviewProvider};
use engine_api::{EngineError, EngineResult};
use index::ImageInfo;
use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

const WINDOW: usize = 16;
static PIXEL_GATE: Mutex<()> = Mutex::new(());
type Notifier = Option<Arc<dyn Fn() + Send + Sync>>;
enum Ticket {
    Hash(ImageInfo, u64, bool, PreviewProvider),
    Wake,
}
type Reply = (ImageId, u64, EngineResult<Option<u64>>);
type ReadyHash = (ImageId, EngineResult<Option<u64>>);
type Completion = Arc<(Mutex<bool>, Condvar)>;

/// Completion barrier for retired preview work. Wait on a background thread,
/// never from inside this session's preview provider or notification callback.
/// Completion means the worker has been joined, including any already admitted
/// callback and provider/cache write, across every explicit regroup generation.
/// Dropping this barrier never detaches work.
#[derive(Clone)]
pub struct PreviewShutdown {
    current: Completion,
    previous: Arc<[Completion]>,
}
impl PreviewShutdown {
    fn pending(previous: Vec<Completion>) -> Self {
        Self {
            current: Arc::new((Mutex::new(false), Condvar::new())),
            previous: previous.into(),
        }
    }
    pub fn wait(self) {
        for completion in self.previous.iter().chain(std::iter::once(&self.current)) {
            let (done, signal) = &**completion;
            let mut done = done.lock().unwrap_or_else(|e| e.into_inner());
            while !*done {
                done = signal.wait(done).unwrap_or_else(|e| e.into_inner());
            }
        }
    }
    fn complete(&self) {
        finish_completion(&self.current);
    }
}
fn finish_completion(completion: &Completion) {
    let (done, signal) = &**completion;
    *done.lock().unwrap_or_else(|e| e.into_inner()) = true;
    signal.notify_all();
}

struct Retirement {
    worker: thread::JoinHandle<()>,
    // The join owner retains only this worker's flag, not older generations.
    completion: Completion,
}
fn retirements() -> &'static mpsc::Sender<Retirement> {
    static OWNER: OnceLock<mpsc::Sender<Retirement>> = OnceLock::new();
    OWNER.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Retirement>();
        thread::Builder::new()
            .name("cull-hash-joins".into())
            .spawn(move || {
                for retired in receiver {
                    let _ = retired.worker.join();
                    finish_completion(&retired.completion);
                }
            })
            .expect("start cull worker lifecycle owner");
        sender
    })
}

#[derive(Default)]
pub(crate) struct BackgroundPreviews {
    queue: VecDeque<ImageId>,
    queued: HashMap<ImageId, (ImageInfo, u64)>,
    versions: HashMap<ImageId, u64>,
    serial: u64,
    cancel: Arc<AtomicBool>,
    /// Notification admission generation. Retirement advances this under the
    /// same lock used to admit a callback. Already admitted callbacks finish
    /// before the join barrier completes; callbacks run without session locks.
    generation: Arc<Mutex<u64>>,
    wake_pending: Arc<AtomicBool>,
    sender: Option<mpsc::SyncSender<Ticket>>,
    receiver: Option<mpsc::Receiver<Reply>>,
    worker: Option<thread::JoinHandle<()>>,
    completion: Option<PreviewShutdown>,
    /// Still-retiring workers from explicit regroups of this same session.
    previous: Vec<Completion>,
    in_flight: usize,
}
impl Drop for BackgroundPreviews {
    fn drop(&mut self) {
        self.retire();
    }
}
impl BackgroundPreviews {
    /// Begin a fresh work generation without losing the session's join barrier.
    pub fn reset(&mut self) {
        let retired = self.retire();
        let previous = retired
            .previous
            .iter()
            .chain(std::iter::once(&retired.current))
            .filter(|completion| !*completion.0.lock().unwrap_or_else(|e| e.into_inner()))
            .cloned()
            .collect();
        *self = Self::default();
        self.previous = previous;
    }
    pub fn retire(&mut self) -> PreviewShutdown {
        if let Some(completion) = &self.completion {
            return completion.clone();
        }
        self.cancel.store(true, Ordering::Release);
        *self.generation.lock().unwrap_or_else(|e| e.into_inner()) += 1;
        self.sender.take();
        // Disconnect results too: a worker blocked delivering a result must
        // be able to exit before the lifecycle owner joins it.
        self.receiver.take();
        self.queue.clear();
        self.queued.clear();
        self.versions.clear();
        self.in_flight = 0;
        let completion = PreviewShutdown::pending(std::mem::take(&mut self.previous));
        if let Some(worker) = self.worker.take() {
            retirements()
                .send(Retirement {
                    worker,
                    completion: completion.current.clone(),
                })
                .unwrap_or_else(|_| panic!("cull worker lifecycle owner stopped"));
        } else {
            completion.complete();
        }
        self.completion = Some(completion.clone());
        completion
    }
    pub fn enqueue(&mut self, info: ImageInfo) {
        if self.cancel.load(Ordering::Acquire) {
            return;
        }
        self.serial += 1;
        self.versions.insert(info.id, self.serial);
        if self
            .queued
            .insert(info.id, (info.clone(), self.serial))
            .is_none()
        {
            self.queue.push_back(info.id);
        }
    }
    pub fn remove(&mut self, id: ImageId) {
        self.versions.remove(&id);
        self.queued.remove(&id);
        self.queue.retain(|queued| *queued != id);
    }
    pub fn pending(&self) -> bool {
        !self.queue.is_empty() || self.in_flight != 0
    }
    fn start(&mut self, notify: &Notifier) -> EngineResult<()> {
        if self.sender.is_some() || self.cancel.load(Ordering::Acquire) {
            return Ok(());
        }
        // Initialize the off-thread join owner before starting work.
        retirements();
        let (tx, rx) = mpsc::sync_channel::<Ticket>(WINDOW + 1);
        let (done, results) = mpsc::sync_channel(WINDOW);
        let cancel = self.cancel.clone();
        let generation = self.generation.clone();
        let expected = *generation.lock().unwrap_or_else(|e| e.into_inner());
        let wake_pending = self.wake_pending.clone();
        let notify = notify.clone();
        let worker = thread::Builder::new()
            .name("cull-hashes".into())
            .spawn(move || {
                let notify_current = || {
                    // Admission is linearized against retirement. Do not hold this
                    // lock across client code (callbacks can release the session).
                    let admitted =
                        *generation.lock().unwrap_or_else(|e| e.into_inner()) == expected;
                    if admitted && let Some(notify) = &notify {
                        notify();
                    }
                };
                let mut last_notice = std::time::Instant::now();
                while let Ok(ticket) = rx.recv() {
                    if cancel.load(Ordering::Acquire) {
                        return;
                    }
                    let Ticket::Hash(info, version, last, provider) = ticket else {
                        wake_pending.store(false, Ordering::Release);
                        notify_current();
                        continue;
                    };
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
                        notify_current();
                        last_notice = std::time::Instant::now();
                    }
                }
            })
            .map_err(EngineError::from)?;
        self.worker = Some(worker);
        self.sender = Some(tx);
        self.receiver = Some(results);
        Ok(())
    }
    /// Schedule another host poll for bounded grouping work, without running
    /// host callbacks synchronously under the caller's session lock.
    pub fn wake(&mut self, notify: &Notifier) -> EngineResult<()> {
        if self.cancel.load(Ordering::Acquire) || self.wake_pending.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        if let Err(error) = self.start(notify) {
            self.wake_pending.store(false, Ordering::Release);
            return Err(error);
        }
        self.sender
            .as_ref()
            .expect("worker started")
            .send(Ticket::Wake)
            .map_err(|_| EngineError::invalid("cull previews", "worker stopped"))
    }
    pub fn poll(
        &mut self,
        provider: &PreviewProvider,
        notify: &Notifier,
    ) -> EngineResult<Vec<ReadyHash>> {
        if self.cancel.load(Ordering::Acquire) {
            return Ok(Vec::new());
        }
        if !self.queue.is_empty() {
            self.start(notify)?;
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
            let Some(id) = self.queue.pop_front() else {
                break;
            };
            if let Some((info, version)) = self.queued.remove(&id) {
                batch.push((info, version));
            }
        }
        let count = batch.len();
        for (n, (info, version)) in batch.into_iter().enumerate() {
            self.sender
                .as_ref()
                .expect("worker started")
                .send(Ticket::Hash(
                    info,
                    version,
                    n + 1 == count,
                    provider.clone(),
                ))
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
        index
            .scan(
                dir.path(),
                &index::NoopSidecarReader,
                &index::NoopMetadataProvider,
            )
            .unwrap();
        let id = index.search(&Default::default()).unwrap()[0];
        (dir, index.image_info(id).unwrap())
    }

    #[test]
    fn lr13d_refresh_metadata_is_coalesced_by_image() {
        let (_dir, image) = info();
        let mut work = BackgroundPreviews::default();
        for revision in 0..10_000 {
            let mut refreshed = image.clone();
            refreshed.size = revision;
            work.enqueue(refreshed);
        }
        assert_eq!(
            work.queue.len(),
            1,
            "metadata grows with distinct images, not refresh count"
        );
        assert_eq!(
            work.queued[&image.id].0.size, 9_999,
            "latest metadata survives coalescing"
        );
        work.remove(image.id);
        assert_eq!(work.queue.len(), 0, "removal releases queued metadata");
        assert!(work.queued.is_empty());
    }
    #[test]
    fn lr13d_shutdown_joins_blocked_provider_and_suppresses_retired_notifications() {
        let (_dir, image) = info();
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let (destroyed, retired) = mpsc::channel();
        struct Retired(mpsc::Sender<()>);
        impl Drop for Retired {
            fn drop(&mut self) {
                let _ = self.0.send(());
            }
        }
        let lifetime = Retired(destroyed);
        let provider: PreviewProvider = Arc::new(move |_| {
            let _keep_alive = &lifetime;
            entered.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(Some(0))
        });
        let callbacks = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = callbacks.clone();
        let notify: Notifier = Some(Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
        }));
        let mut work = BackgroundPreviews::default();
        work.enqueue(image.clone());
        work.poll(&provider, &notify).unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(provider);
        let completion = work.retire();
        let again = work.retire();
        let (done, finished) = mpsc::channel();
        let waiter = thread::spawn(move || {
            completion.wait();
            done.send(()).unwrap();
        });
        assert_eq!(
            finished.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout),
            "shutdown cannot complete while provider remains blocked"
        );
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
        again.wait();
        assert_eq!(
            retired.try_recv(),
            Ok(()),
            "completion means provider has been released by joined worker"
        );
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
        assert!(work.worker.is_none());
        assert!(work.sender.is_none());
        assert!(!work.pending());
        work.enqueue(image);
        work.wake(&notify).unwrap();
        assert!(!work.pending(), "retired generation cannot restart");
        assert_eq!(callbacks.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn lr13d_shutdown_waits_for_already_admitted_callback() {
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let notify: Notifier = Some(Arc::new(move || {
            entered.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
        }));
        let mut work = BackgroundPreviews::default();
        work.wake(&notify).unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let completion = work.retire();
        let (done, finished) = mpsc::channel();
        let waiter = thread::spawn(move || {
            completion.wait();
            done.send(()).unwrap();
        });
        assert_eq!(
            finished.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        );
        release.send(()).unwrap();
        finished.recv_timeout(Duration::from_secs(5)).unwrap();
        waiter.join().unwrap();
    }
    #[test]
    fn lr13d_shutdown_includes_blocked_previous_generation_before_new_poll() {
        let (_dir, image) = info();
        let (entered, started) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let blocked = Mutex::new(blocked);
        let provider: PreviewProvider = Arc::new(move |_| {
            entered.send(()).unwrap();
            blocked.lock().unwrap().recv().unwrap();
            Ok(Some(0))
        });
        let mut work = BackgroundPreviews::default();
        work.enqueue(image.clone());
        work.poll(&provider, &None).unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..100 {
            work.reset();
            assert_eq!(
                work.previous.len(),
                1,
                "completed idle generations are pruned"
            );
        }
        // A separate idle session must not inherit another session's barrier.
        let unrelated = BackgroundPreviews::default().retire();
        let (idle_done, idle_finished) = mpsc::channel();
        let idle_waiter = thread::spawn(move || {
            unrelated.wait();
            idle_done.send(()).unwrap();
        });
        let idle_result = idle_finished.recv_timeout(Duration::from_secs(5));
        work.enqueue(image);
        // The new generation has metadata but no worker yet. Its shutdown must
        // still cover the provider retired by the previous explicit regroup.
        let completion = work.retire();
        let (done, finished) = mpsc::channel();
        let waiter = thread::spawn(move || {
            completion.wait();
            done.send(()).unwrap();
        });
        let early = finished.recv_timeout(Duration::from_millis(50));
        // Always release the provider, including when the regression is present.
        release.send(()).unwrap();
        if early.is_err() {
            finished.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        waiter.join().unwrap();
        idle_waiter.join().unwrap();
        assert_eq!(
            idle_result,
            Ok(()),
            "unrelated idle shutdown cannot wait on this provider"
        );
        assert_eq!(
            early,
            Err(mpsc::RecvTimeoutError::Timeout),
            "shutdown completed before the previous generation retired"
        );
        work.reset();
        assert!(work.previous.is_empty(), "joined generations are released");
    }
}
