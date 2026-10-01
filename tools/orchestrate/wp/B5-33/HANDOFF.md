# B5-33 — Export Flat layout and background filter diagnostics

Status: partial progress; P16 whole-run-loop <8 ms and the historical P19 exception remain OPEN. Local branch `wp/B5-33`, base `68264c74`.

## Problem and baseline evidence

P16 requires main-thread busy spans below 8 ms during Export Flat. P19 previously crashed in
AppKit `layoutSubtreeIfNeeded` after the 4K resize. The historical crash excerpt contains an
AppKit exception termination stack, but does not include the exception reason.

The interrupted run already built release with `apps/mac/Support/make-app.sh release`, profiled
PID 46251 using `xctrace record --template 'Time Profiler'`, and committed the failing test as
`aba50d7e`. Its test log has two real assertion failures: adding export progress changes
`contentLayoutRect` from 1000×700 to 1000×664. Preserve this RED evidence; XCTest reports
`2 failures (0 unexpected)`, which is a failure.

Baseline unprofiled export-only run:

| Fixture | Export wall time | Main span p95 | Main span max |
|---|---:|---:|---:|
| 18 MP smart filter, 0 | 2.58 s | 1.67 ms | 103.13 ms |
| 18 MP smart filter, 1 | 1.99 s | 1.11 ms | 41.51 ms |
| Styled 14 MP, 0 | 125.47 s | 0.03 ms | 77.75 ms |
| Styled 14 MP, 1 | 122.62 s | 0.03 ms | 40.08 ms |

The styled fixture remains 14 MP because the B5-15 engine style-canvas limit prevents the
requested 20 MP styled case. This package does not change engine crates.

## Root cause and implementation

The Time Profiler capture summary has 850 main-thread samples, including 226 samples through
`layoutSubtreeIfNeeded`, 158 through `NSHostingView.layout`, and stacks evaluating
`FlatExportBar.body`. The titlebar accessory changes the content layout rectangle and triggers
viewport resizing plus SwiftUI layout. This is an observed source of main-thread work, not
proof that every outlier in the process is attributable to the export accessory.

`begin_export_flat` validates settings and clones an `Arc<DocState>` under the session lock;
it does not copy pixels. `DocFlatExport.run` bakes, composites, converts, encodes, and writes
on the detached Swift task. The profile contains 1,647 direct export-run worker stacks and zero
direct export-run main-thread stacks (sampling evidence, not proof that every operation is nonblocking). Progress and
completion publish on the main actor.

The fix uses a native manually laid-out progress overlay attached to the window
content view. Starting, progressing, cancelling, and finishing an export no longer adds or
removes titlebar height. Each concurrent export retains a progress row and Cancel button.
`DocumentFlatExport.run` enforces off-main execution with a precondition. Opt-in timing spans
cover setup, worker lifetime, progress publication, and completion. The self-test now flushes
`--timing-output` off main before terminating.

## Background resize

The baseline full nonactivating perf run on this base reached a 1920×1080 point viewport at
2× backing scale (3840×2160 device pixels), completed both drags, both fixture exports and
cancel checks, then reported `done, 0 failure(s)`. The earlier exception was **not reproduced**.
The original macOS crash report for PID 68248 was recovered. Its `asiBacktraces` shows
`NSWindow._postWindowNeedsUpdateConstraints` → view constraint invalidation → SwiftUI
`NSHostingView.setNeedsUpdate` / observation graph invalidation during `NSHostingView.layout`.
It contains no exception reason text, and the matching bounded unified-log query returned no rows.
No exception reason or speculative constraint fix is claimed. The unordered real SelfTestHost
resize regression verifies growing and shrinking without becoming key/main or visible. The
live harness now explicitly checks actual device-pixel viewport size instead of only logging it.

## After measurements (same release binary; export-only retry, no profiler overhead)

| Fixture | Export wall time | Main span p95 | Main span max |
|---|---:|---:|---:|
| 18 MP smart filter, 0 | 2.59 s | 0.79 ms | 29.59 ms |
| 18 MP smart filter, 1 | 2.27 s | 0.73 ms | 9.02 ms |
| Styled 14 MP, 0 | 161.43 s | 0.04 ms | 56.09 ms |
| Styled 14 MP, 1 | 154.77 s | 0.03 ms | 35.72 ms |

The existing run-loop observer still measures maxima above 8 ms. Do **not** accept P16 as closed.
The smaller maxima cannot be presented as a controlled speedup: this is a heavily contended shared
machine, with a load average of 559.74 at retry launch (earlier observed peak 983.92).

The opt-in `PerformanceTrace` file has zero dropped events. Its export-specific maxima:

| Named span | Count | Main thread? | Maximum |
|---|---:|---|---:|
| Setup (validation, snapshot, task/progress UI) | 5 | yes | 7.857 ms |
| Progress publication | 314 | yes | 5.174 ms (p95 1.842 ms) |
| Completion publication | 5 | yes | 2.986 ms |
| Worker lifetime | 5 | no | 161,410.080 ms |

All four exports succeeded. Cancellation at 31% finished in 7,121 ms, kept the prior destination,
and left no temporary file. The harness finished with `done, 0 failure(s)` and flushed its trace.
These named spans meet 8 ms in this run; they do not cover every later AppKit/SwiftUI layout or
run-loop operation. Remaining busy-span outliers are not attributed precisely by this trace.

## Final after-change Time Profiler capture

A subsequent release export-only run on the same source/binary completed successfully with the
profiler attached to PID 78115. Instruments saved a usable 285.58-second trace; the self-test
finished with zero failures. Whole-run-loop maxima were 104.70 / 13.05 ms for smart-filter exports
and 41.05 / 29.05 ms for styled exports (styled wall times 97.71 / 102.51 s). These are profiled,
contended observations, not a substitute for the unprofiled comparison above.

The trace summary has 1,009 main-thread samples, 1,880 direct export-run worker stacks and no
direct export-run main stacks. Main inclusive samples include `CA::Transaction::commit` (415),
`layoutSubtreeIfNeeded` (122), native view backing-layer display (93), export progress callback
(43), and `NSProgressIndicator.updateLayer` (17). These include document setup outside measured
exports and cannot assign each run-loop outlier to a particular function. Rendering/encoding/file
write remain on workers; residual main work is UI publication, AppKit/SwiftUI layout and drawing.

Named maxima with the profiler: setup **10.087 ms**, progress **3.759 ms**, completion **3.921 ms**.
The setup result also exceeds 8 ms under profiling; do not describe the named-span bound as a
universal guarantee. No further speculative engine or constraint change was made.

## Verification and disruptions

- Release packaging: `apps/mac/Support/make-app.sh release` completed, with release provenance
  verified. `evidence/build-provenance-summary.json` records archive, binary and changed-source hashes.
- RED: committed test `aba50d7e`, two geometry assertion failures (700 → 664 points).
- GREEN for the changed behavior: all eight `DocumentExportFlatTests`, all four `SelfTestHostTests`,
  and `ThemeLintTests` passed during the full gate. The timing test enforces <100 ms for setup,
  progress and completion on loaded CI, exact geometry preservation, and strict off-main worker entry.
- First required `build-ffi.sh` + `swift-gate.sh`: **FAILED**, 863 XCTest tests, 3 skipped,
  1 failure; 5 Swift Testing tests passed. Failure:
  `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`.
- Isolated rerun of that shell test: **FAILED**, two inspector probe diagnostics:
  `document-1280x800-stack-history-open historyBody overlaps historyHeader: {{900, 432}, {288, 168}} / {{992, 768}, {288, 32}}`
  and `document-1280x800-properties-history-open region historyBody not laid out`.
  This test does not start an export. No inspector implementation or shell test was changed, and
  no layout assertion was skipped or weakened. The unchanged-source second full gate **PASSED**:
  863 XCTest tests, 3 skipped, zero failures; 5 Swift Testing tests passed; **SWIFT GATE OK**, exit 0.
  Retain the earlier failures as evidence of intermittent shell-layout test behavior on this host.
- The interrupted run left a Swift test build alive. The continuation waited for it; that build
  rejected an input changed while compiling when completion instrumentation was added. The
  subsequent release package and full gate compiled the stable source successfully.
- The first after-change full background run verified the 3840×2160 resize and both filter drags,
  then failed an export with `No space left on device (os error 28)`. Its profiler reported a
  ktrace-stop error and exported no samples. After saving a stack sample, the invalid run was
  terminated (only this worktree's app and driver), and the export-only retry above succeeded.
  Disk space recovered to 69 GiB at retry launch. The full run's sample shows main waiting in
  the run loop and worker stacks in compositor style blur, not an AppKit exception.
- Automatic review rejected deletion of obsolete build directories (`rm -f`-style prohibition).
  No directories were removed; those local artifacts remain.

All builds used `PATH="$HOME/.cargo/bin:$PATH"` and
`CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-33"`, serially, without additional `-j` flags.
No Rust files changed, so Rust-specific gates do not apply. No engine crates, `Cargo.lock`, or
`board.json` changes. No push, foreground activation, or screen capture.

## Remaining work / Machine A asks

1. P16: repeat on a host without extreme contention and capture the residual whole-run-loop
   intervals in Time Profiler. Setup/progress/completion are now bounded in the observed named
   spans, but the broader <8 ms target remains unmet. Do not merge this as full P16 acceptance.
2. P19: the original AppKit exception remains unreproduced on this base; no exception-reason
   capture or constraint fix is claimed. Keep the real-host grow/shrink test and live device-pixel
   assertion. If it recurs, retain the full exception reason and exact hosting/window path.
3. Existing engine ask, B5-15 `NEEDS.md` item 5: `compositor/render/styles.rs` rejects style alpha
   canvases above 16,777,216 pixels including margin. Machine A must support >20 MP styled output
   (for example, bounded style planes over layer bounds plus margin with pixel-parity tests)
   before this harness can honestly report a 20 MP styled acceptance measurement. No engine edit
   was attempted by Machine B.

Reproduction: after the release build, run `tools/orchestrate/wp/B5-33/run-perf.sh after-export`
or `.../run-perf.sh after-full`; choose a fresh suffix for each run. `PROFILE=0` disables profiler
overhead for the comparison run. The script uses only `open -g -n ... --nonactivating`, attaches
Time Profiler to the exact worktree app PID, writes timing output, never raises a window, and
never captures the screen. Raw traces and generated catalogs/images remain locally under
`evidence/`; compact logs and profile summaries accompany this handoff. `analyze-profile.py`
can summarize xctrace time-profile XML (use `/usr/bin/python3` on this host; the Homebrew Python
expat extension failed to load).

## Local commits

- `aba50d7e` — failing geometry regression, worker/setup tracing, real background-host resize test.
- `09039ec5` — native progress overlay, off-main precondition, progress/completion timing checks,
  actual 4K viewport assertion and timing-output flush.
- The final docs commit carries this handoff and compact evidence. All commits include the requested
  co-author trailer and are local only.

## Machine A review follow-up (B1, 2026-10-01)

Commits are on top of reviewed `fa328da1`; no rebase. The earlier implementation and
measurements above describe the reviewed baseline, not a new performance measurement.

- RED commit `3850f72f`: added the tool-options geometry probe and reused `ShellHarness`
  at 960×600, 1280×800, 1440×900 and 1728×1117, with one and two concurrent exports.
  `swift test -c release -Xswiftc -enable-testing --filter 'ShellLayoutTests.testExportHUD'`
  exited 1: **2 tests, 30 assertion failures (0 unexpected)**. Representative RED lines:
  - `XCTAssertFalse failed - 960x600 export HUD overlaps inspector header`
    (also at all three larger sizes).
  - `XCTAssertTrue failed - HUD container must be exposed to AX`.
  - `XCTAssertNotNil failed - HUD belongs to the exporting document window`.
  - `XCTAssertNil failed ... HUD must not use another window`.
  - Cancel labels were `Cancel`, rather than `Cancel export of first.png` / `second.png`.
  The tests also assert no overlap with the tool-options bar, containment in the viewport,
  click ownership, and unchanged viewport/window content geometry.
- Chosen design: a native rounded lower-right viewport HUD, at most 400 pt wide with
  two-line 64 pt rows, above the transient zoom chip. It takes no layout space and consumes
  background mouse/scroll events; each row keeps its native Cancel button. Exports capture
  their own viewport when started and group progress by that host, so later main-window
  changes cannot move the overlay. An explicit unordered-test-window fallback remains for
  the original B5-33 geometry/worker regression; real document viewports take precedence.
- The container is an exposed AX group identified by `document-export-progress`; every
  Cancel button's AX label includes its export file name.
- Replaced the export worker's release-crashing precondition with a debug `assert`;
  retained the existing off-main execution test.
- Removed both tracked raw `spans.json` files (about 216 KB combined). Their compact
  `span-summary.json` files remain, as do the named-span maxima and counts above.

### Deferred measurement leads from A

Do not infer P16 acceptance from this layout fix. Re-measure on a quiet machine later:

1. Label-text changes on each progress tick can invalidate layout. Manual frame placement
   does not prove that label publication avoids later AppKit layout work.
2. Export setup takes the document-session lock for about 7.9–10 ms, so main can wait
   behind a render. Investigate that contention in a separate measurement/package.

Neither lead is addressed or re-profiled in this follow-up. P16 and the historical P19
exception remain open. No Rust, board.json or Cargo.lock changes, app launch, or push.

### Follow-up verification and commits

- `3850f72f` — test(B5-33): RED layout/AX/window-routing coverage.
- `f4e2f255` — fix(B5-33): viewport HUD and debug-only worker assertion.
- Selected GREEN: **11 tests, zero failures**, including both new HUD tests, all eight
  `DocumentExportFlatTests` (original geometry/off-main regression included), and theme lint.
- Required serial command executed with the prescribed PATH and B5-33 Cargo target:
  `cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh`.
  FFI build and Swift build passed. The full gate did **not** print `SWIFT GATE OK`.
- The gate recorded `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`
  failing at line 88: `XCTUnwrap failed: expected non-nil value of type CGImageSourceRef`.
  Its capture helper printed `could not create image from window`; the PNG did not exist.
  Both new HUD tests passed within this full run. The run later stopped making progress;
  a sample of this worktree's exact xctest PID showed main waiting in XCTest and the
  dispatch soft limit of 80 threads blocked in AppKit `NSAnimation._runBlocking`.
  After preserving the log/sample, only that test process was terminated. Gate exit was 1.
- Isolated unchanged `MasksPanelLayoutTests` retry also failed (1 test, 1 assertion failure)
  with the same capture error. Read-only environment checks then returned
  `CGPreflightScreenCaptureAccess() == true` and `CGSSessionScreenIsLocked == 1`.
  **The Mac must be unlocked before retrying the unchanged full gate.** No capture assertion
  was skipped, weakened, or replaced, and no screen unlock or GUI app launch was attempted.
- Compact RED/GREEN/gate/capture diagnostics are in `evidence/review-verification.log`.
  The gate remains unaccepted pending an unlocked-session run; this is not merge acceptance.
