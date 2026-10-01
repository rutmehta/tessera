# B5-40 — Export Flat main-thread spans

Status: implemented and measured; **the <8 ms whole-main-thread maximum target is NOT met**.
The required full Swift gate passes. Residual interval-matched profile evidence is below.
Branch `wp/B5-40`, local commits only. App/engine base `2164d3708e05ac06423b55554d6d0eacdb6d02ce`;
BEFORE app built at `01dd0b55`; AFTER source is `c9bc561c`.

## Changes and lifecycle semantics

- `EngineDocumentBackend.prepareExportFlat` reserves the native session using only short Swift
  bookkeeping. `DocumentFlatExportPreparation.snapshot` takes the potentially blocking native
  session snapshot on the export worker. Native rendering, encoding and file writing remain on
  that worker. The synchronous `beginExportFlat` API retains its original immediate-snapshot behavior.
- A reservation keeps the session alive if the document closes before the worker starts. Pending
  cancellation is forwarded once the snapshot exists; abandoning a reservation releases it.
  Native calls never occur while the new reservation/preparation locks are held. Two deterministic
  tests close the document before starting the worker, including cancellation with a preexisting destination.
- **Snapshot timing:** the menu export includes edits committed before the worker acquires its
  snapshot; later edits do not affect it. This differs from taking the snapshot synchronously at click time.
- Progress is latest-value coalesced, at most one pending main-queue delivery and at most 10 Hz,
  including phase changes. Identical displayed percent/phase pairs are skipped. Completion cancels
  pending publication and remains immediate. Existing fixed-width monospaced labels remain.
- HUD hosts are deduplicated; rows are rearranged only when membership or frame size changes.
  The first native row is prepared during document installation and reused across exports.
- Cold Time Profiler evidence found macOS 26's native rounded button invoking hosted SwiftUI
  sizing and text drawing. A themed `NSButtonCell` now draws the Cancel bezel and title directly;
  the native NSButton retains action, tracking, keyboard, and accessibility behavior.

## Quiet-host timeline and comparison limits

The previous docs-only attempt (`01dd0b55`) waited 05:56–06:26 EDT without a qualifying sample.
This continuation began its extended wait at 06:29 EDT on 2026-10-01, polling every 30 seconds.
At 07:34:32 and 07:35:03, load1 was 4.8828 and 4.2334 with no cargo/swift-build/xcodebuild.
The BEFORE release build then completed around 07:40. During the post-build wait, other builds
reappeared and no qualifying pair occurred before the overall 120-minute allowance ended at 08:29.
The authorized contended fallback was used. No other job was interrupted.

A past minimum-load window cannot be measured retroactively. The first fallback launch occurred
at load1 10.0376; subsequent launches waited for two samples below 10 with no build process and
started at 8.6836 and 8.3413. This is **not a clean BEFORE**. The styled third run encountered
load1 up to 354.772. Results must not be interpreted as a controlled speedup comparison.

Evidence: `evidence/quiet-before-continuation.jsonl`, `evidence/before-export-1/quiet.jsonl`,
and each measurement directory's timestamped `quiet.jsonl`, `host-load.txt`, and 1 Hz
`load-during.jsonl`. Both release builds use the isolated Cargo target
`$HOME/.cache/tessera-target/B5-40`, preceded by `$HOME/.cargo/bin` on PATH.

## Measurement method

The B5-33 fixture construction and export sequence are unchanged: 5212 × 3468 developed
16-bit RAW with smart Gaussian radius 8, and the 4608 × 3072 styled document with text,
shadow and glow. The harness attempts a gradient layer but the logged layer list contains
Caption and Layer 1; no fixture repair was introduced into this comparison.

Each of three independent unprofiled app launches exports **each fixture twice**. The primary
number is the median of the three launch-wise worst main-thread busy spans; all six underlying
values are retained. Wall time is the median of all six exports. `MainThreadSpans` measures the
entire main-runloop busy interval, not just instrumented export methods. Named spans and interval
traces supplement it. Host load is matched to each export using a mach_absolute_time/wall-clock
anchor. Profiling runs are separate from the three-run timing comparison.

## BEFORE / AFTER results

Primary statistic: median of three launch-wise maxima (two exports per fixture per launch).

| Fixture | BEFORE ms | AFTER ms | BEFORE load1 range | AFTER load1 range | <8 ms? |
|---|---:|---:|---|---|---|
| 18 MP smart filter | 13.88 | 13.18 | 8.20–14.99 | 7.89–8.03 | No |
| styled 14 MP | 93.61 | 25.75 | 8.20–354.77 | 7.84–120.20 | No |

| Fixture | BEFORE worst ms | AFTER worst ms | BEFORE wall median s | AFTER wall median s |
|---|---:|---:|---:|---:|
| 18 MP smart filter | 30.91 | 13.99 | 1.990 | 1.885 |
| styled 14 MP | 1027.57 | 122.19 | 71.895 | 65.745 |

Wall medians and worst spans use the same fixture-specific load ranges above.
All individual maxima and their measured load ranges follow; no outlier was discarded.

| Set/run | Fixture | Export 0 max ms (load1 range) | Export 1 max ms (load1 range) |
|---|---|---|---|
| BEFORE 1 | 18 MP smart filter | 30.91 (10.19–14.74) | 10.55 (14.74–14.74) |
| BEFORE 1 | styled 14 MP | 21.86 (18.29–96.34) | 93.61 (69.86–91.58) |
| BEFORE 2 | 18 MP smart filter | 13.88 (8.55–14.99) | 5.45 (14.99–14.99) |
| BEFORE 2 | styled 14 MP | 36.71 (14.27–44.18) | 63.87 (41.12–75.29) |
| BEFORE 3 | 18 MP smart filter | 13.50 (8.21–8.21) | 4.40 (8.20–8.21) |
| BEFORE 3 | styled 14 MP | 32.63 (8.20–317.55) | 1027.57 (283.55–354.77) |
| AFTER 1 | 18 MP smart filter | 13.18 (7.91–7.91) | 3.76 (7.91–7.91) |
| AFTER 1 | styled 14 MP | 15.44 (7.84–41.63) | 15.39 (38.44–49.33) |
| AFTER 2 | 18 MP smart filter | 13.99 (7.89–7.98) | 3.39 (7.98–7.98) |
| AFTER 2 | styled 14 MP | 14.36 (7.98–120.20) | 25.75 (88.08–111.21) |
| AFTER 3 | 18 MP smart filter | 11.99 (8.03–8.03) | 3.41 (8.03–8.03) |
| AFTER 3 | styled 14 MP | 122.19 (7.95–44.28) | 114.85 (43.14–89.92) |

AFTER launch loads were 7.3794, 7.2759 and 6.3501, each after two samples below 8.34
with no cargo/swift-build/xcodebuild. They were lower than all BEFORE launch loads.
The measured distributions were also lower in aggregate:

| Fixture | BEFORE load1 median / p95 / max | AFTER load1 median / p95 / max |
|---|---|---|
| 18 MP smart filter | 12.47 / 14.99 / 14.99 | 7.98 / 8.03 / 8.03 |
| styled 14 MP | 72.01 / 327.92 / 354.77 | 44.98 / 109.04 / 120.20 |

This satisfies the contended comparison admission condition and is at least as quiet in the
reported aggregate load measures. Individual runs are not load matched: AFTER run 2 styled
load exceeded BEFORE run 2. The results are descriptive, not a causal speedup claim.

Across the three AFTER launches (including each cancellation check), all 15 snapshot acquisitions
were off main, with no dropped trace events or self-test failures. Minimum progress intervals were
100.115 / 100.265 / 100.372 ms. Publication counts fell from 314 per launch to 183 / 185 / 185.
Main setup maxima were 1.235 / 1.392 / 2.833 ms (BEFORE 10.601 / 5.053 / 5.454 ms).
AFTER progress and completion maxima across the set were 5.436 ms and 1.428 ms.
These named spans share the full-run load recordings; they include cancellation work.

The 122.193 ms and 114.848 ms styled outliers in AFTER run 3 contain no named export setup,
progress, HUD-update or completion event. Exact stacks were not recorded in those unprofiled
intervals; do not assign them a call site from timing data alone.

## Remaining spans and call sites

A separate Time Profiler launch used the same final release app, starting at load1 6.6948
with no builds after the two-sample check. Its 170.753-second recording contains 759 main-thread
samples. All self-tests passed. Smart exports ran at load1 6.59–6.78; styled exports at 7.04–61.32.
These profiled timings are excluded from the comparison medians. The table matches each busy
interval to sampled stacks, allowing ±1 ms for TOC timestamp precision. Load is the nearest 1 Hz sample.

| Busy span ms | Fixture | Load1 | Main samples | Observed call sites within the interval |
|---:|---|---:|---:|---|
| 14.356 | Smart | 6.59 | 10 | `NSHostingView.beginTransaction` → `GraphHost.flushTransactions` → `AppKitPlatformViewHost.coreLayoutTraits`; `CA::Transaction::commit` and NSWindow layout (8/10 samples include CA commit). |
| 140.410 | Styled | 13.44 | 2 | `UC::DriverCore::continueProcessing` → `modeScheduling` → `NSDisplayTiming.displayTimingsForActiveScreens` → `SLSGetActiveDisplayList`; another sample is `NSUpdateCycleEmitUpdateSequenceDone` → `_os_log_impl_flatten_and_send`. |
| 9.190 | Styled | 17.49 | 1 | `AppDelegate.applicationDockMenu` → `GraphHost.environment.getter` → `AGGraphGetValue`. |
| 8.528 | Styled | 17.49 | 4 | AppKit/CA layout, including `NSTextFieldAppearanceBasedVisualProvider.layout`. |
| 23.511 | Styled | 57.93 | 8 | `FlatExportProgressPublisher.deliver` → `DocumentWorkspace.updateExportAccessory` → `FlatExportProgressView.Row.update` → `NSTextField.setStringValue`; six samples include CA commit. |
| 9.446 | Styled | 58.01 | 5 | NSView layout plus `NSTextFieldAppearanceBasedVisualProvider.updateLayer`. |
| 11.676 | Styled | 53.73 | 7 | CA commit → `NSWindow.layoutIfNeeded` / `NSView.layoutSubtreeIfNeeded`; includes NSTextField layout. |
| 9.095 | Styled | 52.87 | 6 | NSView auto-layout traversal / NSTextField layer update. |
| 10.814 | Styled | 51.44 | 2 | CA layer display → `NSViewBackingLayer.display`; includes `NSProgressIndicator.updateLayer`. |
| 11.752 | Styled | 55.10 | 10 | `NSHostingView.beginTransaction` → `GraphHost.flushTransactions` and `ViewGraphRootValueUpdater._sizeThatFits`, plus CA commit. |

The **140 ms elapsed interval has only two CPU samples**. These identify activity observed in
that interval; they do not explain its entire duration or prove a 140 ms CPU cost in either function.
Scheduling/off-CPU delay cannot be assigned precisely from this Time Profiler recording. Likewise,
no exact stack attribution is possible for the separate unprofiled 122/115 ms outliers.

The remaining export-specific source site is `DocumentWorkspace.swift:1233` (phase label assignment),
with progress-layer display at `:1234`. Coalescing limits how often they run, but native AppKit
still invalidates display/layout when the value changes. Shared SwiftUI/window update work also
remains. The target is therefore **open**, not waived because named export spans are small.

Full interval stacks and sample counts: `evidence/after-export-profile/attribution.json`.
Aggregate profile: `profile-analysis.json` in that directory. Recorded busy timestamps are in
`spans.json`; raw trace/XML remain local. Intermediate cold profiles documented why the custom
button cell was added: 26.646 ms with hosted SwiftUI layout, then 29.721 ms with default cell title
sizing; those are diagnostic unit-test runs, not the release comparison.

## Validation

- RED commit `9e16f3dd`: the new real 18 MP fixture test failed with main-thread snapshot acquisition,
  progress bursts (down to 0.03 ms apart), setup 22.16 ms, and whole busy interval 37.80 ms.
- Fix `13fdfb7e`: worker snapshot, close/cancel reservations, progress coalescing and HUD reuse.
- Follow-up fix `c9bc561c`: remove hosted Cancel-button layout; add interval-matched profile analysis.
- Final isolated cold fixture passed the unchanged 20 ms bound (maximum 17.252 ms; this alone does
  **not** meet the 8 ms target). Fifteen selected export/HUD/theme tests passed.
- Required `apps/mac/build-ffi.sh` followed by `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**.
  892 XCTest tests, 3 skipped, zero failures; 5 Swift Testing tests passed. Skips are the opt-in
  generated 20k library fixture and two external Sony RAW smart-preview tests. The gate's real exit status
  and marker were checked, not merely the unexpected-failure count.
- An earlier full gate failed this new performance assertion at 22.34 ms. It was not a screen-capture
  failure. The follow-up fix above resolved that gate failure; both logs are retained.
- No Rust source changes, so Rust test/clippy/fmt gates do not apply. No board.json or Cargo.lock
  edits, push, installation, foreground GUI launch, or screen capture.
- GUI measurement launches use only `open -g -n … --nonactivating`. Unit-test profiling launches
  the CLI xctest runner with an unordered test window. All builds and measurements are serial.

## Evidence retention

Raw photos, generated exports/catalogs, and Instruments trace bundles/XML remain local and are
ignored by the package evidence `.gitignore`. Compact measurement summaries, source/build
provenance hashes, load samples, test/gate logs, and attributed stacks are retained for review.
Historical filenames containing `green` are not success claims; `test-cold-start-green.log` records
an intermediate failure and the validation account above supersedes it.
