# B5-47 — Layer-only flat-export progress

Originally implemented on `wp/B5-47`, based on `wp/B5-40` at `062a3927`.
The coordinator replayed the branch onto `e6c3e5da`; B5-47b adds commits on top
of `bea3e74d`, without rebasing.
Required gate: **SWIFT GATE OK**. The whole-main-thread **<8 ms target remains OPEN**:
one final after export reached 14.51 ms, and an earlier after set reached 15.45 ms.
All changes and commits are local; no push or merge.

## Changes

The B5-40 baseline was already an AppKit overlay. Its changing NSTextField and
NSProgressIndicator values could schedule downstream native/hosting layout work.

- Replaced both labels with CATextLayer and the progress indicator with a CALayer
  track/fill. Progress updates disable implicit animations and change layer contents
  and fill width directly. Fixed frames change only when the row size changes.
- FlatExportTask is no longer observable. A weak row callback receives task updates
  directly from FlatExportProgressPublisher delivery; delivery no longer rebuilds
  the workspace's HUD task list or visits/repositions every HUD.
- Kept the native Cancel button/cell, existing viewport placement, document ownership,
  multiple exports, cancellation, and prepared-row reuse.
- Virtual AX children expose title/status strings and the fractional progress value,
  0–1 bounds, rounded percentage description, and screen frames. The group retains
  `document-export-progress`; progress and Cancel labels retain the export file name.
  Backing-scale and appearance changes update layer scale/theme colors separately.
- The publisher's 100 ms interval, duplicate suppression, latest-wins pending value,
  and immediate finish behavior are unchanged.
- B5-47b restores the B5-33 setup/progress/completion <100 ms assertions by
  default. Named export-span <20 ms checks also run by default; only B5-40's
  whole-runloop `busy.max() <20 ms` bound requires `TESSERA_FILTER_PERF`.
  Control-baseline trace events are excluded from export timing assertions.

## Measurement method and limits

Host: Mac16,5, 16 logical CPUs, 2026-10-01. Other Codex jobs shared the machine;
load was not held constant and AFTER was generally less loaded. These results are
**descriptive, not a controlled causal speedup claim**. No other job was stopped.

BEFORE uses RED source `3f5b5c1e` (B5-40 behavior plus test instrumentation).
The layer implementation is `f8afa3d7`; final measurements use `7acded28`, which also
corrects the inherited diagnostic HUD lookup and asserts that the measured export
has a HUD attached to its captured host. Binary checksums are in `evidence/`.

Three independent release XCTest launches per set, without Instruments. Each uses
B5-40's 5212 × 3468 developed 16-bit RAW with Gaussian smart-filter radius 8, hosted
in the real **unordered SelfTestHost**. The window never becomes visible/key/main.
Each launch settles the document, measures idle, then a controller-driven Layers
opacity-slider drag (180 values, final commit), restores 100%, settles, and exports
one PNG with the HUD. Both controls use 17 ms ticks; export completion uses the
existing 10 ms polling helper. Actual durations are retained and normalized below.

This measures the hosted main runloop and actual publisher/HUD path, including its
setup and completion. It does **not** establish visible-window presentation cost,
retest the styled 14 MP fixture, or close the broader P16 target. No foreground app
launch or new screen capture was used for measurements. Existing gate tests ran
unchanged through their normal harnesses.

MainThreadSpans measures elapsed busy intervals, including scheduling delay; it is
not a CPU-utilization counter. The same observer and export named spans are used in
both sets. No run/outlier is dropped. Load values alongside each scenario below are
its start/end samples; continuous 1 Hz whole-launch samples are also retained.

## Raw main-thread results

Each cell is `busy.max() ms (load1 start–end range)`.

| Run | Idle | Opacity drag, no export | Export with HUD | Export wall s |
|---|---:|---:|---:|---:|
| before-1 | 1.03 (11.70–12.37) | 24.27 (11.70–13.16) | 28.92 (19.07–19.07) | 1.979 |
| before-2 | 2.05 (16.83–17.95) | 23.95 (15.88–15.88) | 27.19 (15.41–15.41) | 1.904 |
| before-3 | 1.10 (14.14–14.67) | 24.51 (13.65–14.14) | 25.76 (15.68–15.68) | 1.949 |
| after-1 | 0.35 (9.10–9.46) | 26.95 (9.10–9.18) | 14.51 (9.40–9.40) | 2.030 |
| after-2 | 0.33 (10.01–10.09) | 39.04 (10.14–10.32) | 5.29 (10.14–13.25) | 1.807 |
| after-3 | 0.28 (12.99–13.15) | 22.55 (12.74–13.15) | 5.20 (12.68–12.68) | 1.839 |

## Export minus baseline

These are diagnostic export-attributable **estimates**, not stack attribution.
`rate = sum(busy spans) / measured seconds` (ms of busy elapsed time per second).
For each run, subtract its control rate from export rate; the duration-adjusted
excess is `export total − control rate × export seconds`. Also subtract the control's
`busy.max()` from export's maximum, but a difference of maxima cannot identify the
cause of an individual interval. Negative export-minus-edit values mean the drag
control was busier; they do not mean negative export overhead.

All table entries are **medians of the three per-run values/differences**, rather
than differences of independently selected medians. Full p95, counts, totals,
durations and individual deltas are in `evidence/summary.json`.

| Metric | BEFORE | AFTER |
|---|---:|---:|
| Idle busy.max(), ms | 1.10 | 0.33 |
| Edit busy.max(), ms | 24.27 | 26.95 |
| Export busy.max(), ms | 27.19 | 5.29 |
| Export − idle maximum, ms | 25.14 | 4.97 |
| Export − edit maximum, ms | 3.24 | -17.35 |
| Export busy rate, ms/s | 22.23 | 8.43 |
| Export − idle rate, ms/s | 17.77 | 7.03 |
| Export − edit rate, ms/s | -523.85 | -503.90 |
| Export − duration-adjusted idle total, ms | 33.83 | 12.70 |
| Export − duration-adjusted edit total, ms | -1018.59 | -910.70 |

Named main-thread spans, median of the three launch maxima (ms). Their baseline
counts are zero: the idle/edit controls do not invoke export setup/publication.
These spans nest and must not be added together. HUD timings include membership
setup/removal as well as row updates.

| Named span | BEFORE | AFTER |
|---|---:|---:|
| export_flat_setup | 3.139 | 0.578 |
| export_flat_progress | 0.098 | 0.338 |
| export_flat_hud_update | 2.895 | 0.492 |
| export_flat_completion | 0.453 | 0.390 |

The synchronous progress span increased (0.098 → 0.338 ms median maximum); this
change does not claim every named method became faster. Whole-runloop results
include downstream work that those short method spans do not capture. No profile
was taken here, so remaining outliers are not assigned to particular call sites.

All six exports succeeded; each acquired exactly one snapshot off main. There were
zero dropped trace events. Minimum observed publication intervals: BEFORE 103.691 ms,
AFTER 100.722 ms; five publications per export. Final HUD dumps contain only
`FlatExportProgressView → Row → NSButton`; text/bar are layers, with AX checked by
the deterministic regression.

An earlier complete after set, before correcting the diagnostic lookup, is retained
in `evidence/exploratory-summary.json`: export maxima 15.452 / 9.546 / 7.271 ms;
export load1 ranges 11.23–11.30 / 10.75–10.77 / 10.69–10.69. All three passed the
opt-in 20 ms bounds. It is separate from the final set, not discarded for its timings.
Its inherited view dump captured the hosting tree rather than the HUD; this prompted
the final presence assertion and rerun. Raw exploratory logs/traces remain local.

## Validation and commits

- `3f5b5c1e` — `test(B5-47):` RED regression: one test, eight expected assertion
  failures against the native controls/observable task. Asserts publisher-driven
  layer text, unchanged view frames/layout, no observation invalidation, AX status
  and percentage, and cancellation. `evidence/red.log`.
- `53c071e3` — layer publication and AX implementation. Selected tests passed:
  17 tests, zero failures (export, new HUD, two existing B5-33 HUD layout/routing
  tests, theme lint). `evidence/green.log`.
- `f8afa3d7` — correct AX frame conversion's actor capture. An earlier gate failed
  compilation with `sending 'self' risks causing data races`; retained in
  `evidence/swift-gate-compile-failure.log`. No assertion was weakened to fix it.
- `7acded28` — assert actual measured HUD ownership and report measurement process
  exit codes. The script still gathers all three runs after a timing failure.
- Final required command, serial with the prescribed environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-47
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

Final exit **0**, **SWIFT GATE OK**: 895 XCTest tests, 3 skipped, zero failures;
5 Swift Testing tests passed. `evidence/swift-gate.log`. The preceding full gate
also passed before the diagnostic test correction (`swift-gate-initial-pass.log`).
Skips remain the existing opt-in generated-library and external Sony RAW cases.

- All three final `TESSERA_FILTER_PERF=1` runs exited 0. All three BEFORE runs
  failed the opt-in 20 ms bound because export busy maxima exceeded 20 ms; their
  old trace-bound loop also included edit-control events. The final loop correctly
  scopes export bounds to export events. That was the original B5-47 opt-in policy; B5-47b restores the default
  named-span timing checks as described above.
- No Rust, board.json, Cargo.lock, rebase, merge, push, foreground GUI launch, or
  installation. All commits carry the requested co-author trailer.

## Reproduction and evidence

After the required gate builds the release tests, run from the worktree root:

```sh
/usr/bin/python3 tools/orchestrate/wp/B5-47/measure.py after
/usr/bin/python3 tools/orchestrate/wp/B5-47/summarize.py
```

Use a fresh label to avoid overwriting existing evidence; the summary script includes
only `before-*` and `after-*` labels. `measure.py` enables `TESSERA_FILTER_PERF` and
`TESSERA_EXPORT_BASELINES`, records load1 each second, runs three serial launches,
and exits nonzero if any test process fails. It never orders or activates a window.

Compact gate/test logs, per-scenario measurements, load samples, summaries, host/
binary provenance and final HUD subtrees are tracked. Raw traces and complete FFI/
gate logs remain local and ignored. The FFI log added by the RED commit was removed
from tracking after validation; the local original is preserved. Generated fixture
catalogs/exports are removed by the existing XCTest teardown.


## B5-47b review follow-up

### Replay provenance

The earlier hashes in this document and retained measurement evidence identify the
original runs. The coordinator replayed those commits; use this mapping to locate
the equivalent commits in this branch's current ancestry.

| Original hash | Replayed hash |
|---|---|
| `3f5b5c1e` | `976a31d4` |
| `53c071e3` | `d16fabaf` |
| `f8afa3d7` | `42b593ab` |
| `7acded28` | `610de9b1` |
| `4e1d8795` | `bea3e74d` |

### Scope and regression coverage

- S1: Row is an AX group labeled `Export of <fileName>`, containing the three
  virtual layer elements and native Cancel button. HUD and row hit-testing route
  screen points to the correct child. Tests start at the unordered window's content
  view, normalize traversal through `NSAccessibility.unignoredDescendant` and
  `unignoredChildren`, and check group/HUD identity, labels, values, cancellation,
  and HUD hit-test results for every text/progress/button element. AppKit root
  hit-testing on an unordered window returns the window; this offscreen test
  therefore checks reachability through the root tree and screen-point routing
  directly through its attached HUD. Cancel's exposed native cell carries the
  same label as its view.
- S2: `resize(withOldSuperviewSize:)` re-runs viewport placement. The regression
  narrows a 700-point host to 260 points during export in both flipped and
  unflipped hosts, checks containment and row width, and requires no progress tick
  or export completion to repair placement.
- S3: The B5-33 <100 ms checks run by default again. Only the B5-40 whole-runloop
  `busy.max() <20 ms` bound remains opt-in.
- P16 whole-main-thread <8 ms attribution remains **OPEN**. Quiet-host runs and
  Time Profiler attribution are outside this follow-up; no new claim is made.

### Offscreen appearance evidence

`testOffscreenFixtureExportAppearance` starts a real 1600 × 1200 solid-fill fixture
PNG export through `DocumentWorkspace`, captures the attached HUD with
`bitmapImageRepForCachingDisplay` / `cacheDisplay`, and waits for successful export
and the resulting file. Both renders run synchronously on main before completion
can remove the row; the displayed fraction is fixed at 62.5% (`Encoding 63 %`) for
reproducibility. The window is never ordered or activated. Explicit `aqua` and
`darkAqua` appearances produce `evidence/hud-light.png` and `evidence/hud-dark.png`.
The test checks both text frames are inside the row, the name lies above the bar,
and both text layers explicitly handle the flipped row via `isGeometryFlipped`.

To regenerate the committed PNGs after building:

```sh
TESSERA_HUD_EVIDENCE="$PWD/tools/orchestrate/wp/B5-47/evidence" \
  swift test --package-path apps/mac -c release -Xswiftc -enable-testing \
  --filter FlatExportHUDTests/testOffscreenFixtureExportAppearance
```

Visual inspection of both PNGs: filename at the top left, Cancel at the top right,
and upright, readable `Encoding 63 %` alongside the lower bar. No mirrored or
upside-down glyphs were observed. A decoded-PNG pixel check (800 × 128 pixels)
found filename ink at y=20–41 and the accent bar at y=86–93 in both appearances,
confirming the top-origin text/bar placement independently of layer-frame assertions.

### B5-47b validation

- `f257b858` (`test(B5-47b):`): tests-first run produced five expected failures
  across the three HUD tests: absent AX row group, two spilled-host frames, and
  two text-layer orientation assertions. See `evidence/b5-47b-red.log`.
- `df4b5425` (`fix(B5-47b):`): all 16 selected export/HUD tests passed with
  `TESSERA_FILTER_PERF` unset; zero failures. See `evidence/b5-47b-green.log`.
  The debug build also passed without new HUD isolation warnings. AX callbacks
  assert main-actor execution and keep their non-Sendable results local to the
  synchronous call.
- Required serial FFI + Swift gate command (with the prescribed PATH and external
  `CARGO_TARGET_DIR`) exited **0** and printed **SWIFT GATE OK**: **903 XCTest
  tests, 3 skipped, zero failures**, plus **5 Swift Testing tests passed**.
  See `evidence/b5-47b-swift-gate.log`; full local output is retained at
  `/tmp/B5-47b-swift-gate.log`.
- Both PNGs and this handoff are committed with `docs(B5-47b):`. All three
  B5-47b commits carry the requested co-author trailer. Local commits only;
  no Rust, Cargo.lock, board.json, rebase, push, foreground window, or screen capture.


## B5-47c phase accessibility notification follow-up

Commits are on top of `1c27da8e` on `wp/B5-47`, without rebasing.

- `e7d279e1` (`test(B5-47c):`) adds an injectable notification-post closure,
  defaulting to `NSAccessibility.post`, and a deterministic regression recording
  notification targets/types without a live AX client. The unchanged publication
  behavior failed with 10 assertions in one test (`evidence/b5-47c-red.log`).
- `c5a5ad20` (`fix(B5-47c):`) posts `.valueChanged` on the phase element only
  when its AX string changes, and on the progress element only when its numeric
  value or percentage description changes. Notifications follow the layer transaction.
- Regression coverage includes a phase-only transition at 63%, the actual Cancel
  button action and callback, duplicate publication suppression, a numeric change
  within the same rounded percentage, row reuse, and a final phase-only transition
  at 100%. All four HUD tests passed (`evidence/b5-47c-green.log`).
- Required serial FFI + Swift gate command, using the prescribed PATH and
  `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-47`, exited **0** and printed
  **SWIFT GATE OK**: **904 XCTest tests, 3 skipped, zero failures**, plus
  **5 Swift Testing tests passed**. See `evidence/b5-47c-swift-gate.log`;
  full local output is `/tmp/B5-47c-swift-gate.log`.
- All three B5-47c commits carry the requested co-author trailer. Local commits
  only; no Rust, Cargo.lock, board.json, rebase, push, installation, or GUI launch.
  The existing P16 whole-main-thread <8 ms target remains OPEN.


## B5-47d export timing opt-in correction

Commits are on top of `fb4990d6` on `wp/B5-47`, without rebasing.
`844660cd` (`fix(B5-47d):`) restores the 18 MP smart-filter export test's
entire per-event <20 ms loop to the existing `TESSERA_FILTER_PERF` opt-in.
This supersedes B5-47b's default named-span policy documented above. Both named
export events and whole-runloop spans now require the opt-in; B5-33's setup,
progress, and completion <100 ms assertions remain always on. All deterministic
checks, including successful export, HUD presence, dimensions, snapshot worker
isolation, and progress coalescing, are unchanged. Export timing remains scoped
to export events, excluding earlier control-baseline events.

### Default and opt-in test evidence

The same release test binary was exercised in both modes, with
`TESSERA_EXPORT_BASELINES` unset. Each run executed one test with zero failures
and exited **0**:

```sh
cd apps/mac
env -u TESSERA_FILTER_PERF -u TESSERA_EXPORT_BASELINES \
  swift test -c release -Xswiftc -enable-testing \
  --filter DocumentExportFlatTests/testSmartFilterFixtureExportBoundsMainSpansAndCoalescesProgress
env -u TESSERA_EXPORT_BASELINES TESSERA_FILTER_PERF=1 \
  swift test -c release -Xswiftc -enable-testing --skip-build \
  --filter DocumentExportFlatTests/testSmartFilterFixtureExportBoundsMainSpansAndCoalescesProgress
```

- Default: `TESSERA_FILTER_PERF unset: skipped 20 ms export-event bound`.
  The branch containing the per-event assertions was not evaluated.
  See `evidence/b5-47d-default.log`.
- Opt-in: `TESSERA_FILTER_PERF: evaluated 20 ms export-event bound for 15 events`.
  The diagnostic is emitted after the assertion loop.
  See `evidence/b5-47d-opt-in.log`.

### Both required gates

With `TESSERA_FILTER_PERF` and `TESSERA_EXPORT_BASELINES` unset, the prescribed
serial FFI + Swift gate command was run from the worktree root:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-47
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

Exit **0**, **SWIFT GATE OK**: **904 XCTest tests, 3 skipped, zero failures**,
plus **5 Swift Testing tests passed**. See `evidence/b5-47d-swift-gate.log`.
The separate release product gate then ran from the worktree root:

```sh
cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

Exit **0**, `Build of product 'Tessera' complete! (593.98s)`.
See `evidence/b5-47d-strict-release.log`.
Full local logs are `/tmp/B5-47d-default.log`, `/tmp/B5-47d-opt-in.log`,
`/tmp/B5-47d-swift-gate.log`, and `/tmp/B5-47d-strict-release.log`.

The fix and docs commits both carry the requested Claude Opus 5.5 co-author
trailer. Local only; no Rust, board, Cargo.lock, rebase, push, installation, or
manual GUI work. The existing P16 whole-main-thread <8 ms target remains OPEN.
