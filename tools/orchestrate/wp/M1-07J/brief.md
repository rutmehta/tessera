# WP M1-07J — Job scheduler (`jobs` crate)

Read docs/08 §2 and crates/engine-api/CONTRACTS.md (jobs module, invariants 10.x). Implement `crates/jobs` (workspace member stub):
- `ThreadPoolScheduler` implementing `engine_api::jobs::Scheduler`: N worker threads (default = physical cores), a priority queue keyed by `Priority` then FIFO, `submit` returning a `JobId`, `reprioritize`, `cancel` (by job or group, propagating through `CancellationToken`), `status`; a job that observes cancellation returns `EngineError::Cancelled` and its status becomes Cancelled.
- Priority inversion guard: a worker never picks a lower-priority job while a higher-priority job is ready; running jobs are not pre-empted, so document that long jobs must poll cancellation and yield per tile.
- `JobGroup` helper for "prefetch neighbours" style batches that can be cancelled as one.
- A `blocking_run(job)` for tests/CLI.
- Tests: ordering (Export never before ready Ui), group cancel, cancelled queued job never starts, reprioritize moves a queued job ahead, 10k tiny jobs complete without deadlock, a stress test with random submit/cancel from 4 threads. Bench (ignored): throughput of 100k no-op jobs.
`cargo test -p jobs --release`, clippy -D warnings, fmt. Do not modify engine-api.
