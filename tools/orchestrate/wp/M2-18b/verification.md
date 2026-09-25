# M2-18b verification

Implemented recipe-selected native/Adobe PV3–6 operators in image-core, explicit DCP byte injection, CameraProfile-boundary process/cache identity, CPU compatibility barriers over the selected native backend, session JSON process-version get/set with persisted undo/redo/checkout/snapshot transitions, and recipe-selected fidelity rendering.

Engine-api is unchanged. Native image-core golden/reference tests are unchanged and pass. All changed/untracked files were checked against the work package allowlist; no out-of-scope paths were found. No commits were created.

## Executed checks

CARGO_TARGET_DIR remained `/Users/rutmehta/.cache/tessera-target/M2-18b` for every cargo invocation.

The requested chained command completed with exit status 0:

```
cargo test -p image-core -p pipeline-adobe -p tessera-ffi --release && cargo clippy -p image-core -p pipeline-adobe -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check
```

Final run used `CI=1`, the existing suite's supported timeout/latency mode. Totals: 126 passed, 0 failed, 5 ignored (existing benchmarks). Clippy and formatting passed. `git diff --check` passed.

A preceding release-suite run without CI passed before the last two additional tests were added. Later non-CI runs hit the existing 3-second timeout in `tests/fallback.rs:56`, including an isolated retry. Host load averages at investigation were 32.04 / 19.35 / 14.70. No fallback test assertions or timeout values were changed. The final CI-mode run exercises callback behavior with the pre-existing 120-second limit and omits latency assertions guarded by CI. This verifies functionality, not a cold-render latency guarantee under load.

## New coverage

- Adobe operator invocations, different native/compat pixels, and finite scene-linear output.
- Shared demosaic cache across process switches; stage hashes diverge only from CameraProfile onward.
- Full-resolution comparison with the standalone compatibility renderer with and without an explicit DCP (including tint).
- Actual Metal backend dispatch through CPU compatibility barriers at level 2, measured readback counters, and CPU/GPU display agreement.
- Session process get/set, no-op and invalid-version handling, undo/redo, persistence and undo after reopening a real RAW session.
- JPEG fidelity selection agrees with the standalone Adobe path and differs from native.

## Integration notes

Develop edits are limited to process access/history and selecting immutable per-recipe render snapshots, including detail and persisted previews. The existing adaptive drag controller is retained; compatibility creative operators execute at the requested level. GPU resident and host tile caches remain separate, so first compatibility materialization can repeat upstream work despite matching upstream keys. Compatibility is approximate, not a claim of Adobe/Lightroom parity. See `crates/pipeline-adobe/ADOBE_COMPAT.md` for CPU readback boundaries and process-history representation.
