# M5-35 verification and cache audit

## Result

PASS. Implemented only in `crates/compositor/src/render/smart_filters.rs`,
with regression coverage in `crates/compositor/tests/smart_filter_deadlock.rs`
and the concurrency contract in `crates/compositor/COMPOSITOR.md`.
No edits to the M5-31 collision files (`render/styles.rs`, `render/effects.rs`,
or resident styles). No commit or push performed.

## Fix

Source compositing and every filter/transform stage run outside the filter-cache
mutex. After a complete validated stack is computed, publication rechecks the
key while holding the mutex. A racing result reuses the winning entry without
inserting or accounting for the same retained bytes twice. Cache keys, budget
policy, pixel arithmetic, filter order, and post-stack mask placement are unchanged.

This is the brief's compute-outside-lock/double-check option, not blocking
single-flight. `OnceLock::get_or_init` or a condition-variable wait on a Rayon
worker can deadlock when the initializer steals a tile needing its pending key.
Concurrent cold computations can duplicate CPU work and temporary allocations;
only retained entries are budget-bounded. The public `filter_evaluations()`
counter counts stages of accepted complete results, excluding discarded racing
results and failed stacks. Uncached results still count. This preserves the
existing one-published-stack expectation in filters' compositor adapter test.

## Red / green evidence

Before changing production code, ran:

    cargo test -p compositor --release --test smart_filter_deadlock -- --nocapture

The custom evaluator rendered a different smart-filter document's tiles in
parallel using the SAME compositor and Rayon pool. The original implementation
failed after 15.01 seconds with:

    smart-filter stage re-entry timed out (cache/Rayon deadlock)
    test result: FAILED. 0 passed; 1 failed

The parent test killed and reaped the child, so Cargo returned exit 101 instead
of hanging. The initial fix passed the same test in 0.03 seconds.

The final regression additionally forces simultaneous same-key cold misses
with a two-worker barrier. It verifies two computations but one accepted result,
exact RGBA output across a tile boundary, and no reevaluation after clearing only
composite tiles. Nested stage re-entry runs with 1, 2, and 4 workers, each with
zero and nonzero cache budgets. All probes are inside the timeout-protected child.
The test's OnceLock only installs a Weak reference before rendering, never waits
for a cache entry, and avoids an Arc ownership cycle.

## Required gate

Ran the exact required chain successfully (exit 0):

    cargo test -p compositor -p filters --release && cargo clippy -p compositor -p filters --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-35` remained exported
for every Cargo command. No local target directory was used. Release tests had
no failures; explicitly ignored benchmark tests remained ignored. Existing
LibRaw C/C++ build warnings were emitted but did not fail the gate. The first
full run exposed duplicate evaluation-counter accounting, corrected at result
publication; Clippy then requested `as_chunks::<4>()` in the regression, corrected
before the final successful full chain.

## Cache-lock audit

- `render/smart_filters.rs`: fixed the mutex spanning evaluation. Lookups clone
  owned Arc entries before releasing the guard. Source rasterization, evaluator
  callbacks, transform Rayon work, result blending/validation, and mask work are
  all outside the lock. Only lookup, accounting, eviction, and insertion remain
  inside it. Errors cannot leave a pending entry or poison the cache via evaluator
  execution, because there is no pending-entry state or guard during that work.
- `render/cache.rs`: RenderCache guards only protect LRU map/order/byte bookkeeping
  and owned Tile clones. No computation callbacks other than `retain` predicates.
  All call sites use constant false or simple Part matching, not rendering/Rayon.
- `render/mod.rs`: `latest` guards cover only map lookup/insert/clear. Mips, smart
  source rendering, tile jobs, and parallel level rendering execute after these
  guards have dropped. Frame-cache clearing similarly does no rendering.
- `render/live.rs`: live raster and coverage caches use RenderCache and compute
  outside its guards. The prepared-geometry Memo guard spans synchronous shape
  validation/stroke outlining and text layout/outlining. Audited typography/vector
  source for Rayon use: none on these paths, and no compositor callbacks. The
  font-renderer guard protects its required mutable state. `set_text_renderer`
  releases it before `clear`, so it does not invert prepared/font lock order.
  No additional deadlock fix required here.
- `render`'s path-mounted `text_vector/damage.rs`: frame Memo gets return owned
  Arc snapshots; insertion clones/accounting occur under its short guard. Snapshot
  preparation and tile job compilation/execution occur outside that guard.
- `render/exec.rs`: group/root cache access goes through RenderCache get/put.
  `render/styles.rs`, `render/effects.rs`, and `render/pixel.rs` introduce no
  cache mutexes or blocking per-key initialization.
- `resident/filters.rs`: StackRuntime stores a plain HashMap used via exclusive
  `&mut ResidentRenderer`, with owned buffer handles returned by get. There is
  no cache mutex held across GPU stage submission or child rendering. Its CPU
  fallback calls `stack.cpu.filtered_source`, so it receives this same fix.
