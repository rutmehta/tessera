# WP M1-06 — Pipeline graph, stage memoization, and job scheduler (Opus)

Read docs/04 §3, docs/08 §2, crates/engine-api/CONTRACTS.md (stage, jobs, tile modules), and look at crates/pipeline-cpu (operators) and crates/raw-decode (CfaPyramid) as they exist on main. Implement two crates:

## crates/jobs
- `Scheduler` impl of `engine_api::jobs::Scheduler`: a thread pool (std threads or `rayon` is fine) with strict priority ordering (`Ui` before `Viewport` before … `Export`), cancellation via `CancellationToken`, job groups, and `reprioritize`. Invariants 10.x in CONTRACTS.md. Tests: a queued Export never runs before a queued Ui job when both are ready; cancelling a group cancels its children; a cancelled queued job never starts.

## crates/image-core (the pipeline graph lives here for now)
- `PipelineGraph`: fixed `StageId` order; for a `DevelopSettings` diff, compute the earliest dirty stage; per-stage tile cache keyed by `MemoKey` with an LRU byte budget; cached stage outputs stored as `F16Planar` when a stage is marked cacheable (post-demosaic, post-denoise placeholder), in-flight `F32Planar`.
- `Renderer::render_region(image, settings, level, rect) -> Vec<Tile>` that pulls tiles through the graph, reusing memoized upstream tiles, calling `pipeline-cpu` operators (trait `StageOp` with a CPU impl now, GPU impl later). Progressive: `render_progressive(viewport)` yields level 3 → 2 → 1 → 0 results through a callback and is cancellable between tiles.
- Tests: changing only a tone parameter re-runs no stage before `Tone` (count operator invocations); cache budget is respected; render of a full fixture at level 3 with default settings matches `pipeline-cpu`'s direct render within 1e-5; progressive render delivers coarse first.
- Bench (ignored test): tone-only change at level 2 on a fixture: report ms.

Constraints: do not modify engine-api (report if needed). Keep `pipeline-cpu`'s public API unchanged unless additive. `cargo test -p jobs -p image-core --release`, clippy -D warnings, fmt.
