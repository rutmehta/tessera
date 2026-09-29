# B5-12b handoff (Machine B)

Branch `wp/B5-12b`, base **c15dee24** (on main). No rebase, no force-push: new commits sit on top of 43724d1b, the head Machine A reviewed.

## Commits

| Commit | What |
| --- | --- |
| cc397e01 | Brief |
| e2db7fd7 | Transform follow-ups (preview rows, status/options feedback, field Esc/Return, tests) |
| 5e0e3d45 | Follow-ups test runs parallel copies on two documents; the warm-up removal rides RES-01 |
| 43724d1b | Self-test evidence refresh (reviewed by A) |
| acb68bad | A review item 2: transform fields refuse non-finite input (tests first) |
| 48fdea2c | A review item 1: self-test counts every abort as a failure, runs with `--nonactivating`; evidence refreshed |

## A review fixes (43724d1b)

1. **Self-test abort reported "0 failure(s)".** Every early exit now records a FAIL check (`run aborted at TransformSelfTest.swift:<line>`). This covers a session that does not start, a document that does not open, and a missing text layer, reopen or 20 MP card. `began()` logs the begin time and tells a late start apart from a session that never starts.
   - **Warp on the text layer (396), and so 393 and 397–399.** In the 43724d1b log, the status after the 20 s wait already held the session's own hint ("Warp: … · Apply converts the layer to a smart object"). `started()` is the only place that sets that hint, so the engine begin did finish, but only after the wait had expired. The session did not fail to start.
   - Not a code regression: nothing on the begin path changed in B5-12b. `begin_advanced_transform` and `DocumentTransforms.begin/started` are identical to c15dee24.
   - On this head, text Warp began in 62–308 ms in the 3 timed runs (4 runs passed 396). Every Warp/Perspective/Puppet/CAS begin takes 33–308 ms (see `evidence/transform-selftest.log`, "began in").
   - A render-log run (`RENDERLOG=1`) shows no frame holding the session lock for more than about 0.3 s around 389–396. The 20 s stall did not reproduce, so it points to machine load during that run.
   - Related build hazard found and avoided: on this base, `build-ffi.sh` defaults `CARGO_TARGET_DIR` to the shared `~/.cache/tessera-target/mac-ffi`. It can pick up another checkout's build: a first build here generated B5-15's bindings (DocExportListener). Those bindings were kept out of every commit. All builds for these commits used `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-12b`. main's bab90835/4d9a3f17 fix this per checkout.
   - `--nonactivating`: the accessory launch creates no SwiftUI window. The test now hosts `ContentView` in a window that never becomes key or main and is ordered behind other apps. It has to be an `NSWindow`, not an `NSPanel`: AppKit does not count panels, so closing the Apply alert sheet looked like "last window closed" and quit the app at step 381.
2. **"nan" in W/H crashed the app** (`UInt32(Double.nan)`), and NaN or infinity also reached Rotate and Bend. `TransformNumberField.parsed()` now returns nil for non-finite values, and the field goes back to showing the accepted value. Tests: `testNonFiniteTextIsNotANumber` and `testTypedNaNInWidthOrBendCommitsNothingAndRestoresTheValue`. Fail-first log: `evidence/failfirst-swift-nan.log` (assertions plus the UInt32 trap).

## Gates (on 48fdea2c's tree)

- Rust unchanged since 43724d1b. `apps/mac/build-ffi.sh` was run with the per-worktree target dir, and the bindings match c15dee24.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (712 tests, 3 skipped, 0 failures).
- `Support/make-app.sh release`: provenance verified.
- `run-transform-selftest.sh`, background only (`open -g -n` plus `--nonactivating`): steps 380–399 all run, **61 checks, 0 failures**.
  - 382 preview median 351 ms, p95 538 ms.
  - 399: 20 MP begin 292 ms (draft level 3); drag median 345 ms, p95 562 ms; apply to exact frame 948 ms.
