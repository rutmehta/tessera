use cull::CullSession;
use index::Index;
use std::{
    ops::Deref,
    time::{Duration, Instant},
};

/// Exercise the same lazy background work that a host polls after displaying
/// the initial queue. Existing grouping/error assertions remain unchanged.
pub fn finish_previews<I: Deref<Target = Index>>(session: &mut CullSession<I>) {
    let start = Instant::now();
    while session.previews_pending() {
        session.poll_previews().unwrap();
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "hash worker did not complete"
        );
        std::thread::yield_now();
    }
}
