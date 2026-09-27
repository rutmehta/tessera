# P09 scheduler admission evidence

## Scope and integration

Read perf audit hotspot 8 / P09, existing scheduler implementation, scheduler tests and public API. Changed only `crates/jobs/src/lib.rs`, `crates/jobs/tests/scheduler.rs`, and this evidence file. No commit, app launch, foreground UI, or edits to other crates.

**Engine integration API:** `ThreadPoolScheduler::with_interactive_reservation(3)` replaces `ThreadPoolScheduler::new(3)` at the engine construction site (parent worker owns that edit). The new constructor requires at least two workers; zero/one panic. `new(n)` and `Default` retain existing priority/FIFO semantics, including single-worker operation.

Reserved mode keeps worker 0 exclusively eligible for Ui/Viewport. Other workers remain shared, selecting priority/FIFO normally; after eight ordinary admissions they select the oldest queued non-interactive job, if any, regardless of background priority. Thus finite/cooperative jobs make background progress under sustained interactive or higher-priority background submissions. This is a dispatch-count bound, not a real-time/preemption guarantee. A reserved worker intentionally stays idle rather than starting non-preemptible previews.

Submission and reprioritization notify all workers: waking only an ineligible reserved worker could strand background work, and promotion must wake the reserved worker while shared workers are busy. All transitions remain under the existing lock; execution/destruction stays outside it. Completion/error/panic/cancellation handling and pressure tracking are unchanged.

## RED: observed before implementation

All Cargo commands used:
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-57`.

1. Added channel-gated three-preview latency regression against existing `new(3)` before production edits.
   Command: `cargo test -p jobs three_slow_previews_leave_viewport_capacity -- --nocapture`
   Actual result: exit 101; `viewport blocked behind three previews: Timeout`; 0 passed, 1 failed, finished in 0.11s. No compilation/type failure.
2. Implemented opt-in reservation and switched that test to the new constructor.
   Actual result: pass, viewport admission latency **21.75µs**.
3. Added bounded-background-progress regression before fairness implementation. Hold the reserved worker, release only the shared worker with one export plus 32 UI jobs queued.
   Command: `cargo test -p jobs reserved_pool_background_progress -- --nocapture`
   Actual result: exit 101; `background starved: [true, ... (32 UI completions) ..., false]`.
4. Implemented bounded FIFO background service on shared workers only.

## GREEN: final verification

Commands:
- `rustfmt --edition 2024 --config skip_children=true crates/jobs/src/lib.rs crates/jobs/tests/scheduler.rs`
- `cargo test -p jobs -- --nocapture`
- `cargo clippy -p jobs --all-targets --no-deps -- -D warnings`
- `git diff --check -- crates/jobs`

Actual results:
- Pressure integration test: **1 passed**.
- Scheduler integration tests: **20 passed, 0 failed, 1 ignored** (existing manual throughput benchmark).
- Unit/doc test harnesses: successful, no tests.
- Final full-suite viewport admission latency: **21.375µs**, below **5ms**.
- Clippy: exit 0, no warnings.
- Diff whitespace check: exit 0.

Additional repeated execution of the final three-preview test: **20/20 passed**. Recorded latencies:
`34.666µs, 92.875µs, 33.292µs, 30.208µs, 89.292µs, 31.125µs, 30.125µs, 33.958µs, 31.417µs, 32.708µs, 31.75µs, 39.458µs, 31.958µs, 31.417µs, 36.875µs, 34.791µs, 30.375µs, 338.084µs, 33.167µs, 34.375µs`.

## Regression coverage

- Three deliberately slow channel-gated previews: two start, third must remain queued while viewport starts in <5ms; release all gates and verify all four successful completions. A 20ms observation window checks no third background admission. Work ordering is channel-controlled rather than relying on preview sleeps. Timing remains subject to host OS scheduling, not a hard real-time guarantee.
- Background export runs within eight preceding UI completions on the shared worker even while the reserved worker is blocked; every queued job completes.
- Promotion from Preview to Viewport wakes reserved capacity while the background worker is blocked.
- Reserved-mode concurrent randomized submit/cancel/reprioritize from four producers: 4,000 unique job IDs all reach success/cancelled, no lost completions.
- Single-worker reserved configuration rejected; existing single-worker ordering, cancellation, panic, drop and group tests remain green.

## Remaining integration boundary

Jobs-only work is complete. The engine must opt into the new constructor; legacy `new(3)` deliberately does not silently change scheduling policy. No app-level responsiveness claim is made from this scheduler-only deterministic workload.
