# B5-15 handoff (Machine B -> Machine A), refreshed 2026-09-29

Branch `wp/B5-15`, linear on origin/main **`c15dee24`** (merge base). Not merged; Machine A owns merge.
Earlier revisions of this file cited `aec3738e` / `7378ac16`: those were pre-rebase hashes and are stale.

* Code head reviewed by A (round 1): **`fa14c703`** (A verdict: code OK, WP not accepted, merge as partial progress).
* A's should-fix (late Cancel labelled "cancelled" although the file was written), on top, no rebase:
  * `bb7c2a3c` test(mac): RED, `DocumentExportFlatTests.testCancelAfterTheFileIsWrittenReportsExported` (engine) and
    `testStubCancelAfterTheFileIsWrittenReportsExported` (stub). Both block the main actor until the destination
    exists, then Cancel, so the cancel deterministically lands after the last checkpoint. Both failed with
    `.cancelled` before the fix.
  * `7da63b18` fix(mac): a successful run is always `.exported` (`DocumentWorkspace.startExportFlat`); the stub
    checks its cancel flag before its synchronous write instead of after it. Cancel seen at a checkpoint still
    throws and reports `.cancelled` with the destination untouched (existing tests unchanged and passing).
* New code head: **`7da63b18`** (the commit updating this file sits on top of it).

## Acceptance status: OPEN gaps (code may merge as partial progress; the WP is not accepted)

1. **P16 OPEN**: main-thread spans during a styled export measured max **35.9 ms / 58.0 ms** (p95 0.06 ms) against
   the **< 8 ms** target (CODEX-RECOVERY.md). Unmet.
2. **P19 OPEN**: the full (non export-only) filter self-test crashes with an AppKit layout exception
   (`+[NSApplication _crashOnException:]` in `layoutSubtreeIfNeeded`) right after it resizes the main window for
   the "100% 4K" drag (`FilterSelfTest.swift`, `w.setFrame(...)`). Not reproducible headlessly; reason unknown.
   Evidence: `recovery-evidence/background-filter-layout-crash.txt`.

## Rebase (historical: done 2026-09-28 against the then-tip; the hashes below are pre-rebase ids)

* The old merge commit `1389f6c` (main 503bc46 into wp/B5-15) was dropped by the linear rebase; its resolutions
  replayed without conflict.
* `ee4f81e` (stage-cache trim): conflict in `crates/tessera-ffi/src/document/render.rs` `present_frame`. Kept main's
  cancellable closure structure (path/requested bookkeeping, `cancel.check()`, `cpu_present(.., cancel)`) and
  re-inserted only the B5-15 `trim_filters` / `FILTER_CACHE_KEEP` block after `g.targets.retain`.
* `ff120c6` (worker busy flag, wait_idle race): main already sets `i.busy = true` in the same critical section
  that takes preview and bake jobs (with `RequestCancellation` / `ActiveBake`). Took main's version; the B5-15
  part of that hunk is dropped as equivalent. The rest of the commit (deterministic `read_presented_level`,
  export comparison ignoring the ICC timestamp, NEEDS.md) applied as is.
* `7378ac16` (now `fa14c703`): adapts B5-15 code to main's API: `Job::Bake(..)` now has two fields, and the GPU preview path
  cancels the running CPU job with `RequestCancellation::cancel()` instead of `AtomicBool::store`.
* Regenerated UniFFI bindings are identical to the merged ones (no binding diff).

## Gates

At `7da63b18` (2026-09-29, Machine B): `apps/mac/build-ffi.sh` + `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**
(710 XCTest executed, 3 skipped, 0 failures; 5 Swift Testing passed). The fix is Swift-only; the Rust gates below
were run on 2026-09-28 on the code now at `fa14c703` (then `7378ac16`) and were not re-run.

Earlier (2026-09-28, M4 Max, no concurrent builds):

* `cargo test --locked --release -p tessera-ffi --no-fail-fast`: 429 passed, 0 failed, 20 ignored (39 groups).
  One earlier run had `smart_preview_thumbnail::tests::hdr_saved_offline_recipe_keeps_policy_and_renders_sdr_thumbnail_without_mutation`
  fail once (main-owned test, not touched by B5-15); it passed alone and in the full rerun: flaky under load.
* `cargo clippy --locked [--release] -p tessera-ffi --all-targets -- -D warnings`: clean (dev and release).
* `cargo fmt --all -- --check`: clean.
* `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK (708 XCTest executed, 3 skipped, 0 failures; 5 Swift Testing passed).
* No engine crate, engine-api, Cargo.lock or board.json changes. filters/compositor are unchanged vs main, so their
  suites were not re-run here.

## Remaining gates for Machine A (GUI / perf; not attempted by Machine B)

1. **P16 main-thread spans < 8 ms** (OPEN, see above): last measured max 35.93 / 58.00 ms (p95 0.06 ms) in
   CODEX-RECOVERY.md; unmet. Also needs 20 MP fixtures (harness uses 14 MP styled because of the engine's
   16.7 MP style-canvas limit, NEEDS.md item 5, and 18 MP smart-filter).
2. **P19 full filter self-test** (OPEN, see above): the last full (non export-only) run crashed with an AppKit layout exception
   (`+[NSApplication _crashOnException:]` in `layoutSubtreeIfNeeded`) right after the self-test resized the main
   window for the "100% 4K" drag (`FilterSelfTest.swift`, `w.setFrame(...)`; the window came out 916 × 907 pt,
   not 4K). Needs an on-screen/background app rerun on this head to get the exception reason; not
   reproducible headlessly. Evidence: `recovery-evidence/background-filter-layout-crash.txt`.
3. **P19 acceptance numbers**: backend trace showing GPU Gaussian stages with no CPU fallback, publish latency and
   memory before/after for a filter drag on a 20 MP smart object, on this head (main's bounded smart-filter reuse
   and CPU cancellation landed underneath B5-15's GPU route).
4. **Export output parity / cancel** (including a late Cancel now reporting "Exported") in the running app on this head (unit tests pass: `document_export_flat.rs`,
   `DocumentExportFlatTests`), plus a quit/close-mid-export check.
5. Engine asks remain in NEEDS.md (stage-cache budget, smart-object re-keying, premultiplied GPU Gaussian,
   style canvas limit, level-aware previews).
