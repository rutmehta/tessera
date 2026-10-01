//! Opt-in, instance-local diagnostics for integration tests. No cache policy changes.
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

#[derive(Default)]
pub(crate) struct MipProbe {
    enabled: AtomicBool,
    hits: AtomicU64,
    rebuilds: AtomicU64,
}

impl MipProbe {
    pub(crate) fn snapshot(&self) -> (u64, u64) {
        self.enabled.store(true, Ordering::Relaxed);
        (
            self.hits.load(Ordering::Relaxed),
            self.rebuilds.load(Ordering::Relaxed),
        )
    }

    pub(crate) fn hit(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub(crate) fn rebuild(&self) {
        if self.enabled.load(Ordering::Relaxed) {
            self.rebuilds.fetch_add(1, Ordering::Relaxed);
        }
    }
}
