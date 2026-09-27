//! Opt-in passive CPU spans for isolated latency diagnostics; no GPU fences.
use std::{
    sync::{Mutex, OnceLock},
    time::Instant,
};
static ENABLED: OnceLock<bool> = OnceLock::new();
static ORIGIN: OnceLock<Instant> = OnceLock::new();
static EVENTS: Mutex<Vec<Event>> = Mutex::new(Vec::new());
struct Event {
    label: String,
    thread: std::thread::ThreadId,
    start_us: u128,
    end_us: u128,
}
/// A CPU interval recorded on drop, with bounded storage and deferred output.
pub struct Span<'a> {
    label: &'a str,
    start: Instant,
}
/// Disabled unless TESSERA_COLD_DIAGNOSTIC=1 at process start.
pub fn span(label: &str) -> Option<Span<'_>> {
    if !*ENABLED.get_or_init(|| std::env::var("TESSERA_COLD_DIAGNOSTIC").is_ok_and(|v| v == "1")) {
        return None;
    }
    ORIGIN.get_or_init(Instant::now);
    Some(Span {
        label,
        start: Instant::now(),
    })
}
impl Drop for Span<'_> {
    fn drop(&mut self) {
        let end = Instant::now();
        let origin = *ORIGIN.get().expect("diagnostic origin");
        if let Ok(mut events) = EVENTS.lock() {
            if events.len() < 4096 {
                events.push(Event {
                    label: self.label.to_owned(),
                    thread: std::thread::current().id(),
                    start_us: self.start.duration_since(origin).as_micros(),
                    end_us: end.duration_since(origin).as_micros(),
                });
            }
        }
    }
}
/// Emit only after the caller's measured interval. CPU spans overlap GPU work.
pub fn emit() {
    if let Ok(mut events) = EVENTS.lock() {
        events.sort_by_key(|event| event.start_us);
        for event in events.drain(..) {
            eprintln!(
                "COLD-SPAN thread={:?} start_us={} end_us={} duration_us={} label={}",
                event.thread,
                event.start_us,
                event.end_us,
                event.end_us - event.start_us,
                event.label
            );
        }
    }
}
