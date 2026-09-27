# M2-53 results

Status: PARTIAL IMPLEMENTATION, RESULT: FAIL. The exact build/test gate passes, P02 is verified, and the background first-grid trace verifies no loupe shader/resource initialization. Full P01 acceptance is blocked by the allowed-path boundary and absent actual presentation timestamps under background occlusion. Controlled cold/warm shader-cache comparisons remain unverified.

## Scope and hard blocker

Read `tools/orchestrate/audits/perf/REPORT.md` in full. Changes are confined to the M2-53 allowed source, support, tests, bench and result paths; no Document source edits.

The existing `crates/tessera-ffi/src/develop.rs:144-168` FrameInfo has generation/size/level/sink time, but no actual residency or dequeue timestamp. `render()` at 1033-1151 increments its private generation only on submission and can coalesce multiple Swift settings updates behind an in-flight render. `DevelopInfo.backend` (1794-1806) is session configuration, not proof of per-frame residency. Swift cannot honestly reconstruct these fields, or join an arbitrary input to the generation that rendered it. Subtracting `renderMs` from callback time is not a dequeue measurement.

Completing P01 needs permission for a narrow Rust/UniFFI change: carry input request identity into submitted/coalesced jobs and echo the rendered identity, actual backend route and dequeue timestamp in the callback. This requires `crates/tessera-ffi/src/develop.rs` and regenerated binding/header paths, which are outside this package's write allowlist. None were modified to simulate success. The timing script deliberately emits null input-to-present percentiles and exits 2 for the drag oracle.

## Implemented

- Opt-in `--nonactivating`: accessory policy, no explicit activate, ignores `--front`, no scheduled updater startup. Background runner uses `open -g -n` and checks foreground PID before/during/after. Timing mode hosts the real ContentView in a nonactivating NSPanel ordered behind other windows; it cannot become key/main. This is a diagnostic background host, not a foreground-drag equivalent. It does not replace personal recent-folder preferences with copied fixtures.
- Opt-in bounded Swift timing buffer: controller input sequence, flush/FFI start/end, callback enqueue/drain, drawable submission and actual drawable `presentedTime`. Session/generation/level/size travel unchanged from callback to presentation; unknown residency stays `unavailable`.
- Background grid/121-update exposure self-test, copied RAW fixture, separate state directory, one off-main JSON export, explicit missing/dropped-data handling. Checks inherited logging in both launcher and launched process.
- P02 fresh release packaging, commit/config/source/archive/bindings/compiler-command/binary hashes, signed-binary sidecar receipt and fail-closed verification. Explicit debug still works but cannot pass release verification. A fresh Swift scratch directory forces relinking the copied archive.
- P18 lazy worker-created, process-retained Metal resources. Empty grid loupe construction does not request resources. First use displays `Preparing loupe…`; failure has an explicit label. Latest frame/proof/overlay is replayed after preparation.
- Engine readouts and performance documentation say `engine sink`, record level/backend, distinguish unknown residency, and say `not app input-to-display`.
- Layout test no longer fronts a window during the required full suite.

## Before/after observations (not app latency claims)

The real original renderer from HEAD and the changed renderer were compiled and executed with the actual `LoupeFrame`/Metal code, without creating an application/window. Original source and temporary executables are under ignored `check-build/`; JSON evidence is retained alongside this file.

| Boundary | Original eager renderer | Lazy retained renderer |
|---|---:|---:|
| First resource initialization, measured call wall time | 80.116 ms, on main | 558.644 ms (process 1), 52.883 ms (process 2), off-main |
| Second resource request in the same process | 1.385 ms, a new renderer | 0.004041 / 0.002167 ms, same retained instance |
| Runtime shader/pipeline initialization on empty first-grid path | 1 eager initialization per view (source) | 0 requests in actual mounted 1k-item background grid trace |
| Packaging default | debug Swift | release Swift |
| Existing frame readout | ambiguous `render` | explicitly `engine sink`, not input-to-display |

These runs are not a controlled A/B benchmark. The changed renderer checks ran before the original-renderer check. Driver shader-cache state was uncontrolled and never purged, so these are NOT verified cold/warm shader-cache cases. No first-loupe speedup is asserted. The observed long cold-process wait is the reason for a loading state. At a live preflight the host had load averages 112.32 / 126.17 / 123.89 on a shared Apple Silicon workstation with other builds. Later load remained over 100. Swift toolchain: 6.3.3; no 6.2.4 execution claim.

## Final verification

- 15 Python tests pass (`python-tests.log`): trace-summary boundaries, percentile/dedup behavior, launch/source guards, provenance round trip and negative cases including -Onone/missing flags/archive/binding/stale binary/source changes. Measurement reports do not invalidate source receipts.
- Standalone Swift 6 `TraceChecks` passed: disabled collection, bounded drops, identity separation, monotonic span duration, 100 concurrent callback records.
- Actual Metal resource check passed twice: non-main initialization and retained identity. Original renderer check also completed.
- `git diff --check` passed.
- Required exact gate PASS after final changes (`gate-verified.log`, process exit 0): 286 XCTest tests, 0 failures, plus 5 Swift Testing tests, 0 failures. Command: `(cd apps/mac && ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing)`, with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-53`. New `LazyLoupeTests`, `PerformanceTraceTests`, background-safe `PeopleLayoutTests`, and the existing Develop readout test passed. Existing Swift concurrency and LibRaw deprecation warnings remain. The launch-order regression test first failed because a late loupe observer never processed the already-selected item (`late-loupe-red.log`); synchronizing selection at observer installation fixes it.
- The first gate (`gate.log`) exposed three pre-existing `ExportReport` test initializers missing `workflowErrors`. Updated only those test fixtures with `workflowErrors: []`, matching the existing generated initializer; no export implementation or generated binding was changed. `ExportWarningsTests` then passed.
- Fresh default-release packaging PASS (`package-verified.log`, exit 0), followed by independent `python3 apps/mac/Support/provenance.py verify --app apps/mac/build/Tessera.app` and `codesign --verify --deep --strict apps/mac/build/Tessera.app`, both exit 0.
- Earlier background launches had no windows despite loaded rows (`grid-final/app-stderr.log`), so those runs are failures, not latency evidence. Requiring `grid_appeared` exposed this. The audit NSPanel fix is covered by a non-key/main/style/content test. Its first size assertion included an incorrect assumption about titlebar height (`gate-window.log`); the corrected test accounts for OS-dependent full-size content chrome.
- Changed/untracked files were checked against the exact allowlist: no disallowed source changes, no Document source edits. No commit or push performed.

## Actual background app runs

Both final runs used `open -g -n ... --nonactivating`; provenance was verified before each launch. Foreground PID samples are retained in each run's `foreground.json`. Both report `foreground_unchanged: true`, `grid_appeared: true`, `selftest_complete: true`, no trace drops, and no enabled per-frame logging. The fixture was copied from the earlier local fixture into the new run directory before editing.

| Measurement | Before | Final release background run |
|---|---|---|
| Grid runtime loupe resource setup | Eager per-view initialization, source verified; original app not launched because it activates | 0 resource spans; 1k items, mounted real ContentView; runner exit 0 |
| Drag inputs | No safe comparable baseline | 121 |
| Callback generations / drawable submissions | No safe comparable baseline | 121 / 121, L2 1230 × 819 |
| Instrumented main-span p95 | Unavailable | 0.773458 ms |
| FFI main-span p95 | Unavailable | 0.587125 ms |
| Flush main-span p95 | Unavailable | 0.614667 ms |
| Callback-drain main-span p95 | Unavailable | 0.889292 ms |
| Drawable encode main-span p95 | Unavailable | 0.525166 ms |
| Engine sink p95 | Not remeasured on original app | 3.673875 ms; NOT input-to-display |
| Lazy first-loupe resource setup | See original 80.116 ms main-thread microcheck above | 1.343084 ms off-main in this app process; driver cache uncontrolled |
| Actual presented generations | Unavailable | 0; all 121 drawable callbacks had unavailable presentation timestamps |
| Input-to-present p50 / p95 | Unavailable | null / null, deliberately not inferred |

Evidence: `grid-verified/{run,trace,summary,foreground}.json`, `drag-verified/{run,trace,summary,foreground}.json`, corresponding app stdout/stderr, and `grid-verified.log` / `drag-verified.log`. The drag runner exits 2 for incomplete P01, not a pass. Instrumented main spans overlap and omit other main-thread work; their percentile is NOT whole-main-thread task occupancy. No 16 ms responsiveness or app latency speedup is claimed. Session backend was Metal (Apple M4), but actual per-frame residency is unavailable.

Pinned bundle base commit: `84fc6f23abeea5365394bb1dff0449c2ac85f33b`; uncommitted source digest: `f00141db836f69d5e04626c481b33df2d2c77c8c9ebada7e11c212efad29dff7`; release archive SHA-256: `826ff444b41cdc5af63b700aadf4ca92dfd8e512093ed1b8aadcca9e0709f8a6`. Exact binding digests are in both summary files and the bundle receipt. This is a dirty-source build pinned by content, not a claim that HEAD already contains this work.

Remaining acceptance: expose causal input/job/callback identity, dequeue and actual residency through an authorized FFI seam; obtain actual presented timestamps without violating foreground constraints; run separately controlled cold/warm shader-cache cases and Swift 6.2.4. The current implementation was built/tested with Swift 6.3.3.

## Retry verification

Re-read the full audit and confirmed the missing fields directly in the current Rust `FrameInfo`. No source change can truthfully supply actual job dequeue/residency from that callback within the given allowlist, so this retry preserves the implementation and its fail-closed oracle rather than inventing timestamps.

- Ran the exact required gate again with the specified external Cargo target. The first foreground tool call timed out at 420 seconds during tests (`gate-current.log`), so it is not counted as a pass. Re-ran the whole command as a tracked process: exit 0 (`gate-current-retry.log`).
- Python discovery: 25 tests passed. `git diff --check` passed.
- Existing release bundle passed current provenance verification and the runner's strict code-signature verification before launch.
- Ran `app_timing.py` again with `open -g` and `--nonactivating`. Grid run exited 0 (`grid-current/summary.json`), with no loupe resource spans. Drag run exited 2 (`drag-current/summary.json`), with 121 inputs, zero drops, unchanged foreground identity, and no valid presented generations.
- Current drag instrumented main-span p95: 0.763583 ms. Engine sink p95: 3.656084 ms. Input-to-present p50/p95 remain null. These are not comparable-before/after speedup claims; concurrent builds continued on the shared host.
- No new application source edits, no Document edits, no commit or push. The requested missing FFI instrumentation still needs an expanded write allowlist. No Kanban lifecycle transition is available in this runner because `HERMES_KANBAN_TASK` is unset.

## Latest independent recheck

- Re-read the entire audit and current Rust `FrameInfo` definition (lines 144–168). The required dequeue, causal input identity and actual residency fields are still absent. No out-of-scope source edits were made.
- Ran the exact required gate in this session with the existing external `CARGO_TARGET_DIR`: exit 0, 286 XCTest tests and 5 Swift Testing tests passed (`gate-recheck.log`). Toolchain reports Swift 6.3.3; Swift 6.2.4 was not available in this verification.
- Python discovery passed all 25 tests. Current bundle provenance verification and `git diff --check` passed. Programmatic validation of modified and untracked paths found no allowlist violations.
- Ran both background scripts again, each using `open -g -n` and `--nonactivating`. `grid-recheck` exited 0 and `drag-recheck` exited 2. Both mounted the grid, completed the self-test, preserved foreground identity and dropped zero trace records.
- The latest drag recorded 121 inputs. Instrumented main-span p95 was 1.557417 ms; engine sink p95 was 4.935292 ms. Actual presented generations remained zero, so input-to-present p50/p95 remain null, not inferred from GPU completion or sink timing. Evidence is in `grid-recheck/summary.json`, `drag-recheck/summary.json` and their trace/foreground/run records.
- The implementation remains partial. Completing P01 requires an authorized FFI instrumentation seam and a background-safe way to observe real presentation timestamps. Controlled cold/warm shader-cache cases also remain unverified. No additional application source changes, commit or push were made during this recheck.

RESULT: FAIL P01 cannot be fully instrumented within the write allowlist, and background presentation percentiles are unavailable.
